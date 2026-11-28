//! Tool registry: parameters, risk classes, sandboxing and network guards.
//!
//! Authorization is decided here, never by the model (ADR-0007).

pub mod fs;
pub mod html;
pub mod net;
pub mod policy;

use std::sync::Arc;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::ports::{Clock, CommandRunner, HttpTransport, ToolSpec};

pub use policy::{Approval, Approvals, Policy, Risk};

#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    Ok { summary: String, data: Value },
    Denied { reason: String },
    Failed { code: String, message: String },
}

impl ToolOutcome {
    /// What the model gets to read. `data` carries the payload a tool actually
    /// produced; `summary` is a digest for the step log and the UI, and handing
    /// only the digest to the model makes every read and fetch useless.
    ///
    /// Bounded below the fence's own limit so the truncation marker survives.
    pub fn model_text(&self) -> String {
        const LIMIT: usize = 7000;
        let raw = match self {
            // A payload that carries more than the text (a url, a path) must not
            // be reduced to the text alone, or the model cannot say what it read.
            Self::Ok { data, summary } => {
                let single_text = data
                    .as_object()
                    .is_some_and(|m| m.len() == 1 && m.contains_key("text"));
                match (single_text, data.get("text").and_then(|v| v.as_str())) {
                    (true, Some(text)) => text.to_string(),
                    (_, None) if data.is_null() => summary.clone(),
                    // Cut the long values first, then serialise. Slicing the
                    // serialised string instead would drop whichever key sorts
                    // last, and that key is the url or the path.
                    _ => short_values(data, LIMIT.saturating_sub(512)).to_string(),
                }
            }
            Self::Denied { reason } => reason.clone(),
            Self::Failed { message, .. } => message.clone(),
        };
        let count = raw.chars().count();
        if count <= LIMIT {
            return raw;
        }
        let kept: String = raw.chars().take(LIMIT).collect();
        format!("{kept}\n[truncated {} chars]", count - LIMIT)
    }
}

/// Bound every string inside a JSON value, so the identifying fields survive
/// when the payload is long.
fn short_values(value: &serde_json::Value, cap: usize) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(text) => {
            let count = text.chars().count();
            if count <= cap {
                return value.clone();
            }
            let kept: String = text.chars().take(cap).collect();
            Value::String(format!("{kept}\n[truncated {} chars]", count - cap))
        }
        Value::Array(items) => Value::Array(items.iter().map(|v| short_values(v, cap)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), short_values(v, cap)))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub struct ToolCtx {
    pub workspace: std::path::PathBuf,
    /// Set only after the user approved writing outside the workspace.
    pub allow_outside_write: bool,
    /// Config and data files a tool may never write, approved or not.
    pub protected: Vec<std::path::PathBuf>,
    pub task_id: String,
    pub http: Arc<dyn HttpTransport>,
    pub clock: Arc<dyn Clock>,
    pub commands: Arc<dyn CommandRunner>,
    pub allow_private_network: bool,
    pub shell_enabled: bool,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn risk(&self, args: &Value) -> Risk;
    /// True when the target path resolves outside the workspace.
    fn outside_workspace(&self, _args: &Value, _workspace: &std::path::Path) -> bool {
        false
    }
    fn spec(&self) -> ToolSpec;
    fn run(&self, ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome>;
}

pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    pub fn default_set() -> Self {
        Self {
            tools: vec![
                Box::new(FetchUrl),
                Box::new(HttpGetJson),
                Box::new(FsList),
                Box::new(FsRead),
                Box::new(FsWrite),
                Box::new(FsDelete),
                Box::new(Shell),
                Box::new(AskUser),
                Box::new(Finish),
            ],
        }
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|b| b.as_ref())
    }

    pub fn risk_of(&self, name: &str, args: &Value) -> Risk {
        self.get(name)
            .map(|t| t.risk(args))
            .unwrap_or(Risk::Destructive)
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::BadRequest(format!("missing string argument: {key}")))
}

fn spec(name: &str, description: &str, props: Value, required: &[&str]) -> ToolSpec {
    ToolSpec {
        name: name.to_string(),
        description: description.to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false
        }),
    }
}

// ------------------------------------------------------------------ network

pub struct FetchUrl;
impl Tool for FetchUrl {
    fn name(&self) -> &'static str {
        "fetch_url"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::External
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "fetch_url",
            "Fetch a public http(s) page and return its readable text.",
            serde_json::json!({ "url": { "type": "string" } }),
            &["url"],
        )
    }
    fn run(&self, ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        let url = str_arg(args, "url")?;
        let url = net::guard_resolved(url, ctx.allow_private_network)?;
        let resp = ctx.http.get(
            &url,
            &[("User-Agent".into(), "openclicky/2".into())],
            std::time::Duration::from_secs(20),
        )?;
        if resp.status >= 400 {
            return Ok(ToolOutcome::Failed {
                code: "http".into(),
                message: format!("status {}", resp.status),
            });
        }
        let body = html::to_text(&resp.text(), 200_000);
        Ok(ToolOutcome::Ok {
            summary: format!("{} chars from {}", body.chars().count(), url),
            data: serde_json::json!({ "url": url, "text": body }),
        })
    }
}

pub struct HttpGetJson;
impl Tool for HttpGetJson {
    fn name(&self) -> &'static str {
        "http_get_json"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::External
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "http_get_json",
            "GET a JSON endpoint and return the parsed body.",
            serde_json::json!({ "url": { "type": "string" } }),
            &["url"],
        )
    }
    fn run(&self, ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        let url = net::guard_resolved(str_arg(args, "url")?, ctx.allow_private_network)?;
        let resp = ctx
            .http
            .get(&url, &[], std::time::Duration::from_secs(20))?;
        if resp.status >= 400 {
            return Ok(ToolOutcome::Failed {
                code: "http".into(),
                message: format!("status {}", resp.status),
            });
        }
        if resp.body.len() > 1_000_000 {
            return Ok(ToolOutcome::Failed {
                code: "too_large".into(),
                message: "response over 1MB".into(),
            });
        }
        let value: Value = serde_json::from_slice(&resp.body)
            .map_err(|e| Error::Protocol(format!("invalid json: {e}")))?;
        Ok(ToolOutcome::Ok {
            summary: format!("json from {url}"),
            data: value,
        })
    }
}

// ------------------------------------------------------------------ files

pub struct FsList;
impl Tool for FsList {
    fn name(&self) -> &'static str {
        "fs_list"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::Read
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "fs_list",
            "List files inside the task workspace.",
            serde_json::json!({ "path": { "type": "string" } }),
            &["path"],
        )
    }
    fn run(&self, ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        let p = fs::resolve_in_workspace(&ctx.workspace, str_arg(args, "path")?)?;
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&p).map_err(|e| Error::Io(e.to_string()))? {
            let entry = entry.map_err(|e| Error::Io(e.to_string()))?;
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        names.sort();
        Ok(ToolOutcome::Ok {
            summary: format!("{} entries", names.len()),
            data: serde_json::json!(names),
        })
    }
}

pub struct FsRead;
impl Tool for FsRead {
    fn name(&self) -> &'static str {
        "fs_read"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::Read
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "fs_read",
            "Read a UTF-8 file inside the workspace (max 256KB).",
            serde_json::json!({ "path": { "type": "string" } }),
            &["path"],
        )
    }
    fn run(&self, ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        let text = fs::read_text(&ctx.workspace, str_arg(args, "path")?, 256 * 1024)?;
        Ok(ToolOutcome::Ok {
            summary: format!("{} chars", text.chars().count()),
            data: serde_json::json!({ "text": text }),
        })
    }
}

pub struct FsWrite;
impl Tool for FsWrite {
    fn name(&self) -> &'static str {
        "fs_write"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::Write
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "fs_write",
            "Write a UTF-8 file inside the workspace. Overwrites unless `mode=append`.",
            serde_json::json!({
                "path": { "type": "string" },
                "content": { "type": "string" },
                "mode": { "type": "string", "enum": ["overwrite", "append"] }
            }),
            &["path", "content"],
        )
    }
    fn outside_workspace(&self, args: &Value, workspace: &std::path::Path) -> bool {
        args.get("path")
            .and_then(|v| v.as_str())
            .map(|p| fs::escapes_workspace(workspace, p))
            .unwrap_or(false)
    }
    fn run(&self, ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("overwrite");
        let path = fs::write_text_with(
            &ctx.workspace,
            str_arg(args, "path")?,
            str_arg(args, "content")?,
            mode == "append",
            ctx.allow_outside_write,
            &ctx.protected,
        )?;
        Ok(ToolOutcome::Ok {
            summary: format!("wrote {}", path.display()),
            data: serde_json::json!({ "path": path.to_string_lossy() }),
        })
    }
}

pub struct FsDelete;
impl Tool for FsDelete {
    fn name(&self) -> &'static str {
        "fs_delete"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::Destructive
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "fs_delete",
            "Move a file inside the workspace to .trash.",
            serde_json::json!({ "path": { "type": "string" } }),
            &["path"],
        )
    }
    fn outside_workspace(&self, args: &Value, workspace: &std::path::Path) -> bool {
        args.get("path")
            .and_then(|v| v.as_str())
            .map(|p| fs::escapes_workspace(workspace, p))
            .unwrap_or(false)
    }
    fn run(&self, ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        let trashed = fs::trash(&ctx.workspace, str_arg(args, "path")?, &ctx.protected)?;
        Ok(ToolOutcome::Ok {
            summary: format!("moved to {}", trashed.display()),
            data: serde_json::json!({ "trashed": trashed.to_string_lossy() }),
        })
    }
}

pub struct Shell;
impl Tool for Shell {
    fn name(&self) -> &'static str {
        "shell"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::Destructive
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "shell",
            "Run a shell command inside the workspace. Disabled unless explicitly enabled in settings.",
            serde_json::json!({ "command": { "type": "string" } }),
            &["command"],
        )
    }
    fn run(&self, ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        let command = str_arg(args, "command")?;
        if !ctx.shell_enabled {
            return Ok(ToolOutcome::Denied {
                reason: "shell is disabled in settings".into(),
            });
        }
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        let flag = if cfg!(windows) { "/C" } else { "-c" };
        let out = ctx.commands.run(
            program,
            &[flag.to_string(), command.to_string()],
            &ctx.workspace,
        )?;
        let mut text = out.stdout;
        text.push_str(&out.stderr);
        let text: String = text.chars().take(8000).collect();
        if out.status == 0 {
            Ok(ToolOutcome::Ok {
                summary: "command ok".into(),
                data: serde_json::json!({ "output": text }),
            })
        } else {
            Ok(ToolOutcome::Failed {
                code: "exit".into(),
                message: text,
            })
        }
    }
}

pub struct AskUser;
impl Tool for AskUser {
    fn name(&self) -> &'static str {
        "ask_user"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::Read
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "ask_user",
            "Ask the user one small question when you are blocked.",
            serde_json::json!({ "question": { "type": "string" } }),
            &["question"],
        )
    }
    fn run(&self, _ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        let q = str_arg(args, "question")?;
        Ok(ToolOutcome::Ok {
            summary: format!("asked: {q}"),
            data: serde_json::json!({ "question": q }),
        })
    }
}

pub struct Finish;
impl Tool for Finish {
    fn name(&self) -> &'static str {
        "finish"
    }
    fn risk(&self, _a: &Value) -> Risk {
        Risk::Read
    }
    fn spec(&self) -> ToolSpec {
        spec(
            "finish",
            "Finish the task. Include artifacts (workspace-relative paths) as evidence when you produced files.",
            serde_json::json!({
                "result": { "type": "string" },
                "artifacts": { "type": "array", "items": { "type": "string" } }
            }),
            &["result"],
        )
    }
    fn run(&self, _ctx: &ToolCtx, args: &Value) -> Result<ToolOutcome> {
        Ok(ToolOutcome::Ok {
            summary: "finished".into(),
            data: args.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_payload_is_what_the_model_reads() {
        let out = ToolOutcome::Ok {
            summary: "12 chars".into(),
            data: serde_json::json!({ "text": "the actual file body" }),
        };
        assert_eq!(out.model_text(), "the actual file body");
    }

    #[test]
    fn a_payload_with_a_url_keeps_the_url() {
        let out = ToolOutcome::Ok {
            summary: "120 chars from https://example.test/page".into(),
            data: serde_json::json!({ "url": "https://example.test/page", "text": "hello" }),
        };
        let text = out.model_text();
        assert!(text.contains("https://example.test/page"), "{text}");
        assert!(text.contains("hello"), "{text}");
    }

    #[test]
    fn a_long_page_still_says_where_it_came_from() {
        let out = ToolOutcome::Ok {
            summary: "9000 chars".into(),
            data: serde_json::json!({
                "url": "https://example.test/very/long/page",
                "text": "x".repeat(9000),
            }),
        };
        let text = out.model_text();
        assert!(
            text.contains("https://example.test/very/long/page"),
            "the url must survive"
        );
        assert!(text.contains("[truncated"), "the cut must be visible");
        assert!(
            text.chars().count() <= 7000 + 64,
            "the whole payload stays bounded"
        );
    }

    #[test]
    fn a_payload_without_a_text_field_still_arrives() {
        let out = ToolOutcome::Ok {
            summary: "2 entries".into(),
            data: serde_json::json!(["a.txt", "b.txt"]),
        };
        assert!(out.model_text().contains("a.txt"));
    }

    #[test]
    fn a_huge_payload_is_bounded_and_says_so() {
        let out = ToolOutcome::Ok {
            summary: "big".into(),
            data: serde_json::json!({ "text": "x".repeat(9000) }),
        };
        let text = out.model_text();
        assert!(text.chars().count() < 9000);
        assert!(text.contains("[truncated 2000 chars]"), "{text}");
    }

    #[test]
    fn a_denied_call_reads_as_its_reason() {
        let out = ToolOutcome::Denied {
            reason: "用户拒绝".into(),
        };
        assert_eq!(out.model_text(), "用户拒绝");
    }
}
