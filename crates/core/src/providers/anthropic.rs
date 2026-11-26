//! Anthropic Messages API: vision blocks, tool_use blocks, SSE deltas.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::ports::*;

use super::{classify_status, truncate};

pub struct Anthropic {
    http: Arc<dyn HttpTransport>,
    config: Config,
    key: SecretString,
}

impl Anthropic {
    pub fn new(http: Arc<dyn HttpTransport>, config: Config, key: SecretString) -> Self {
        Self { http, config, key }
    }

    fn base(&self) -> String {
        self.config
            .provider
            .base_url
            .trim_end_matches('/')
            .to_string()
    }

    fn headers(&self) -> Vec<(String, String)> {
        vec![
            ("Content-Type".into(), "application/json".into()),
            ("x-api-key".into(), self.key.expose().to_string()),
            ("anthropic-version".into(), "2023-06-01".into()),
        ]
    }

    fn body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = if m.role == Role::Assistant {
                    "assistant"
                } else {
                    "user"
                };
                json!({"role": role, "content": [{"type": "text", "text": m.content}]})
            })
            .collect();
        if !req.images.is_empty() {
            if let Some(last) = messages.last_mut() {
                if let Some(parts) = last["content"].as_array_mut() {
                    for img in &req.images {
                        parts.push(json!({
                            "type": "image",
                            "source": {"type": "base64", "media_type": "image/jpeg", "data": b64(&img.png)}
                        }));
                    }
                }
            }
        }
        let mut body = json!({
            "model": req.model,
            "system": req.system,
            "messages": messages,
            "max_tokens": req.max_tokens,
        });
        if stream {
            body["stream"] = json!(true);
        }
        if !req.tools.is_empty() {
            body["tools"] = Value::Array(
                req.tools
                    .iter()
                    .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}))
                    .collect(),
            );
        }
        body
    }
}

impl ChatProvider for Anthropic {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse> {
        super::with_retries(|| {
            let resp = self.http.post_json(
                &format!("{}/messages", self.base()),
                &self.headers(),
                &self.body(req, false),
                Duration::from_secs(180),
            )?;
            if resp.status >= 400 {
                return Err(classify_status(resp.status, &resp.text(), None));
            }
            parse_messages(&resp.text())
        })
    }

    fn complete_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse> {
        let mut stream = self.http.post_json_stream(
            &format!("{}/messages", self.base()),
            &self.headers(),
            &self.body(req, true),
            Duration::from_secs(180),
        )?;
        let mut text = String::new();
        while let Some(line) = stream.next_line()? {
            let line = line.trim();
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(payload) else {
                continue;
            };
            if v["type"] == "content_block_delta" {
                if let Some(t) = v["delta"]["text"].as_str() {
                    text.push_str(t);
                    on_delta(t);
                }
            }
            if v["type"] == "message_stop" {
                break;
            }
        }
        Ok(ChatResponse {
            text,
            tool_calls: Vec::new(),
            usage: TokenUsage::default(),
            finish: FinishReason::Stop,
        })
    }

    fn list_models(&self) -> Result<Vec<String>> {
        let resp = self.http.get(
            &format!("{}/models", self.base()),
            &self.headers(),
            Duration::from_secs(20),
        )?;
        if resp.status >= 400 {
            return Err(classify_status(resp.status, &resp.text(), None));
        }
        let v: Value =
            serde_json::from_str(&resp.text()).map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }
}

pub fn parse_messages(body: &str) -> Result<ChatResponse> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| Error::Protocol(format!("invalid json: {e}")))?;
    if v["type"] == "error" {
        return Err(Error::BadRequest(truncate(
            &v["error"]["message"].to_string(),
            200,
        )));
    }
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = v["content"].as_array() {
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => text.push_str(b["text"].as_str().unwrap_or("")),
                Some("tool_use") => tool_calls.push(ToolCall {
                    id: b["id"].as_str().unwrap_or("call").to_string(),
                    name: b["name"].as_str().unwrap_or("").to_string(),
                    arguments: b["input"].clone(),
                }),
                _ => {}
            }
        }
    }
    Ok(ChatResponse {
        text,
        tool_calls,
        usage: TokenUsage {
            input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
        },
        finish: FinishReason::Stop,
    })
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
