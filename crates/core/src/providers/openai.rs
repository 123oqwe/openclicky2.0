//! OpenAI-compatible chat (vision + tools + SSE), transcription and model list.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::ports::*;

use super::{classify_status, retry_after, truncate, KEY_NAME};

// The table is the rule, so a later gpt-5.1 or o3-pro needs no edit here. The
// boundary after the prefix keeps gpt-50 and gpt-4o on the older dialect, and the
// last path segment covers an aggregator id such as openrouter/o3-mini.
const REASONING_FAMILIES: [&str; 4] = ["o1", "o3", "o4", "gpt-5"];

fn is_reasoning_model(model: &str) -> bool {
    let name = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .trim()
        .to_ascii_lowercase();
    REASONING_FAMILIES.iter().any(|family| {
        name.strip_prefix(family).is_some_and(|rest| {
            rest.is_empty()
                || rest.starts_with('-')
                || rest.starts_with('.')
                || rest.starts_with(':')
        })
    })
}

pub struct OpenAi {
    http: Arc<dyn HttpTransport>,
    config: Config,
    key: SecretString,
}

impl OpenAi {
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
            (
                "Authorization".into(),
                format!("Bearer {}", self.key.expose()),
            ),
        ]
    }

    fn body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut messages: Vec<Value> = vec![json!({"role": "system", "content": req.system})];
        for m in &req.messages {
            messages.push(json!({"role": m.role.as_str(), "content": m.content}));
        }
        if !req.images.is_empty() {
            if let Some(last) = messages.last_mut() {
                let mut parts =
                    vec![json!({"type": "text", "text": last["content"].as_str().unwrap_or("")})];
                for img in &req.images {
                    parts.push(json!({
                        "type": "image_url",
                        "image_url": {"url": format!("data:image/jpeg;base64,{}", b64(&img.png))}
                    }));
                }
                last["content"] = Value::Array(parts);
            }
        }
        let mut body = json!({
            "model": req.model,
            "messages": messages,
        });
        if is_reasoning_model(&req.model) {
            body["max_completion_tokens"] = json!(req.max_tokens);
        } else {
            body["max_tokens"] = json!(req.max_tokens);
            body["temperature"] = json!(req.temperature);
        }
        if stream {
            body["stream"] = json!(true);
        }
        if !req.tools.is_empty() {
            body["tools"] = Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({"type": "function",
                               "function": {"name": t.name, "description": t.description, "parameters": t.schema}})
                    })
                    .collect(),
            );
        }
        body
    }
}

impl ChatProvider for OpenAi {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse> {
        super::with_retries(|| {
            let resp = self.http.post_json(
                &format!("{}/chat/completions", self.base()),
                &self.headers(),
                &self.body(req, false),
                Duration::from_secs(180),
            )?;
            if resp.status >= 400 {
                return Err(classify_status(
                    resp.status,
                    &resp.text(),
                    retry_after(&resp.headers),
                ));
            }
            parse_chat(&resp.text())
        })
    }

    fn complete_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse> {
        let mut stream = self.http.post_json_stream(
            &format!("{}/chat/completions", self.base()),
            &self.headers(),
            &self.body(req, true),
            Duration::from_secs(180),
        )?;
        let mut text = String::new();
        let mut usage = TokenUsage::default();
        while let Some(line) = stream.next_line()? {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let payload = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
            if payload == "[DONE]" {
                break;
            }
            if !payload.starts_with('{') {
                continue;
            }
            let v: Value =
                serde_json::from_str(payload).map_err(|e| Error::Protocol(e.to_string()))?;
            if let Some(d) = v["choices"][0]["delta"]["content"].as_str() {
                text.push_str(d);
                on_delta(d);
            }
            if let Some(u) = v.get("usage") {
                usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0) as u32;
            }
        }
        Ok(ChatResponse {
            text,
            tool_calls: Vec::new(),
            usage,
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
        let mut out: Vec<String> = v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        Ok(out)
    }
}

pub fn parse_chat(body: &str) -> Result<ChatResponse> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| Error::Protocol(format!("invalid json: {e}")))?;
    if let Some(err) = v.get("error") {
        return Err(Error::BadRequest(truncate(&err.to_string(), 200)));
    }
    let message = &v["choices"][0]["message"];
    let text = match &message["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    };
    let mut tool_calls = Vec::new();
    if let Some(calls) = message["tool_calls"].as_array() {
        for c in calls {
            tool_calls.push(ToolCall {
                id: c["id"].as_str().unwrap_or("call").to_string(),
                name: c["function"]["name"].as_str().unwrap_or("").to_string(),
                arguments: c["function"]["arguments"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(Value::Null),
            });
        }
    }
    let finish = match v["choices"][0]["finish_reason"].as_str().unwrap_or("stop") {
        "tool_calls" => FinishReason::ToolCalls,
        "length" => FinishReason::Length,
        _ => FinishReason::Stop,
    };
    Ok(ChatResponse {
        text,
        tool_calls,
        usage: TokenUsage {
            input_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
        },
        finish,
    })
}

/// OpenAI-compatible transcription (`/audio/transcriptions`, multipart WAV).
pub struct OpenAiStt {
    http: Arc<dyn HttpTransport>,
    config: Config,
    key: SecretString,
}

impl OpenAiStt {
    pub fn new(http: Arc<dyn HttpTransport>, config: Config, key: SecretString) -> Self {
        Self { http, config, key }
    }
}

impl Transcriber for OpenAiStt {
    fn transcribe(&self, clip: &AudioClip, prompt: Option<&str>) -> Result<Transcript> {
        if clip.pcm.is_empty() {
            return Err(Error::EmptyAudio);
        }
        let mut form = MultipartForm::default();
        form.fields
            .push(("model".into(), self.config.provider.stt_model.clone()));
        if let Some(p) = prompt.filter(|p| !p.trim().is_empty()) {
            form.fields.push(("prompt".into(), truncate(p, 400)));
        }
        form.files.push(MultipartFile {
            field: "file".into(),
            filename: "audio.wav".into(),
            content_type: "audio/wav".into(),
            bytes: encode_wav(clip),
        });
        let base = if self.config.provider.stt_base_url.is_empty() {
            self.config
                .provider
                .base_url
                .trim_end_matches('/')
                .to_string()
        } else {
            self.config
                .provider
                .stt_base_url
                .trim_end_matches('/')
                .to_string()
        };
        let headers = vec![(
            "Authorization".into(),
            format!("Bearer {}", self.key.expose()),
        )];
        let resp = self.http.post_multipart(
            &format!("{base}/audio/transcriptions"),
            &headers,
            &form,
            Duration::from_secs(60),
        )?;
        if resp.status >= 400 {
            return Err(classify_status(resp.status, &resp.text(), None));
        }
        let v: Value =
            serde_json::from_str(&resp.text()).map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(Transcript {
            text: v["text"].as_str().unwrap_or("").trim().to_string(),
            language: v["language"].as_str().map(str::to_string),
            is_partial: false,
        })
    }
}

pub fn encode_wav(clip: &AudioClip) -> Vec<u8> {
    let data_len = (clip.pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&clip.channels.to_le_bytes());
    out.extend_from_slice(&clip.sample_rate.to_le_bytes());
    out.extend_from_slice(&(clip.sample_rate * clip.channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(clip.channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in &clip.pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn key_name() -> &'static str {
    KEY_NAME
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
