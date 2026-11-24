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

pub struct ToolCtx {
    pub workspace: std::path::PathBuf,
    /// Set only after the user approved writing outside the workspace.
    pub allow_outside_write: bool,
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
    /// True when the call sends a local file to an external system.
    fn uploads_local_file(&self, _args: &Value) -> bool {
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
        let url = net::guard_url(url, ctx.allow_private_network)?;
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
        let url = net::guard_url(str_arg(args, "url")?, ctx.allow_private_network)?;
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
        let trashed = fs::trash(&ctx.workspace, str_arg(args, "path")?)?;
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
