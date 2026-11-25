//! L2 protocol contracts with fixed samples; no network.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use openclicky_core::config::{Config, ProviderKind};
use openclicky_core::ports::*;
use openclicky_core::providers::{anthropic, build, openai};

struct FakeHttp {
    response: Mutex<Option<HttpResponse>>,
    lines: Mutex<Vec<String>>,
    last_body: Mutex<Option<serde_json::Value>>,
}

impl FakeHttp {
    fn json(status: u16, body: &str) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            response: Mutex::new(Some(HttpResponse {
                status,
                headers: vec![],
                body: body.as_bytes().to_vec(),
            })),
            lines: Mutex::new(vec![]),
            last_body: Mutex::new(None),
        })
    }
    fn sse(lines: Vec<&str>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            response: Mutex::new(None),
            lines: Mutex::new(lines.into_iter().map(str::to_string).collect()),
            last_body: Mutex::new(None),
        })
    }
    fn body(&self) -> Option<serde_json::Value> {
        self.last_body.lock().unwrap().clone()
    }
}

struct Lines(Vec<String>, usize);
impl LineStream for Lines {
    fn next_line(&mut self) -> openclicky_core::Result<Option<String>> {
        if self.1 >= self.0.len() {
            return Ok(None);
        }
        let line = self.0[self.1].clone();
        self.1 += 1;
        Ok(Some(line))
    }
}

impl HttpTransport for FakeHttp {
    fn get(
        &self,
        _u: &str,
        _h: &[(String, String)],
        _t: Duration,
    ) -> openclicky_core::Result<HttpResponse> {
        Ok(self
            .response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(HttpResponse {
                status: 200,
                headers: vec![],
                body: b"{\"data\":[]}".to_vec(),
            }))
    }
    fn post_json(
        &self,
        _u: &str,
        _h: &[(String, String)],
        b: &serde_json::Value,
        _t: Duration,
    ) -> openclicky_core::Result<HttpResponse> {
        *self.last_body.lock().unwrap() = Some(b.clone());
        Ok(self.response.lock().unwrap().clone().unwrap())
    }
    fn post_multipart(
        &self,
        _u: &str,
        _h: &[(String, String)],
        _f: &MultipartForm,
        _t: Duration,
    ) -> openclicky_core::Result<HttpResponse> {
        Ok(self.response.lock().unwrap().clone().unwrap())
    }
    fn post_json_stream(
        &self,
        _u: &str,
        _h: &[(String, String)],
        b: &serde_json::Value,
        _t: Duration,
    ) -> openclicky_core::Result<Box<dyn LineStream>> {
        *self.last_body.lock().unwrap() = Some(b.clone());
        Ok(Box::new(Lines(self.lines.lock().unwrap().clone(), 0)))
    }
}

fn cfg(kind: ProviderKind) -> Config {
    let mut c = Config::load(Path::new("/nonexistent/openclicky.toml")).unwrap();
    c.provider.kind = kind;
    c.provider.chat_model = "test-model".to_string();
    c
}

fn key() -> SecretString {
    SecretString::new(["test", "-key"].concat())
}

#[test]
fn openai_parses_text_tools_and_usage() {
    let body = r#"{"choices":[{"message":{"content":"hello","tool_calls":[{"id":"c1","function":{"name":"fs_read","arguments":"{\"path\":\"a.txt\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#;
    let p = openai::OpenAi::new(FakeHttp::json(200, body), cfg(ProviderKind::Openai), key());
    let resp = p.complete(&ChatRequest::simple("m", "s", "u")).unwrap();
    assert_eq!(resp.text, "hello");
    assert_eq!(resp.tool_calls[0].name, "fs_read");
    assert_eq!(resp.tool_calls[0].arguments["path"], "a.txt");
    assert_eq!(resp.usage.output_tokens, 3);
    assert_eq!(resp.finish, FinishReason::ToolCalls);
}

#[test]
fn openai_stream_accumulates_deltas() {
    let http = FakeHttp::sse(vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}",
        "",
        "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}",
        "data: [DONE]",
    ]);
    let p = openai::OpenAi::new(http, cfg(ProviderKind::Openai), key());
    let mut got = String::new();
    let resp = p
        .complete_stream(&ChatRequest::simple("m", "s", "u"), &mut |d| {
            got.push_str(d)
        })
        .unwrap();
    assert_eq!(got, "hello");
    assert_eq!(resp.text, "hello");
}

#[test]
fn openai_request_carries_images_and_tools() {
    let http = FakeHttp::json(200, r#"{"choices":[{"message":{"content":"ok"}}]}"#);
    let p = openai::OpenAi::new(http.clone(), cfg(ProviderKind::Openai), key());
    let mut req = ChatRequest::simple("m", "sys", "what is this");
    req.images.push(ImageInput {
        screen_index: 0,
        png: vec![1, 2, 3],
        width_px: 10,
        height_px: 10,
    });
    req.tools.push(ToolSpec {
        name: "finish".into(),
        description: "d".into(),
        schema: serde_json::json!({"type":"object"}),
    });
    p.complete(&req).unwrap();
    let text = http.body().unwrap().to_string();
    assert!(text.contains("data:image/jpeg;base64,"));
    assert!(text.contains("\"tools\""));
}

#[test]
fn anthropic_parses_tool_use() {
    let body = r#"{"content":[{"type":"text","text":"ok"},{"type":"tool_use","id":"t1","name":"fs_write","input":{"path":"x"}}],"usage":{"input_tokens":5,"output_tokens":2}}"#;
    let p = anthropic::Anthropic::new(
        FakeHttp::json(200, body),
        cfg(ProviderKind::Anthropic),
        key(),
    );
    let resp = p.complete(&ChatRequest::simple("m", "s", "u")).unwrap();
    assert_eq!(resp.text, "ok");
    assert_eq!(resp.tool_calls[0].name, "fs_write");
    assert_eq!(resp.usage.input_tokens, 5);
}

#[test]
fn anthropic_stream_reads_deltas() {
    let http = FakeHttp::sse(vec![
        "event: content_block_delta",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"你好\"}}",
        "data: {\"type\":\"message_stop\"}",
    ]);
    let p = anthropic::Anthropic::new(http, cfg(ProviderKind::Anthropic), key());
    let mut got = String::new();
    p.complete_stream(&ChatRequest::simple("m", "s", "u"), &mut |d| {
        got.push_str(d)
    })
    .unwrap();
    assert_eq!(got, "你好");
}

#[test]
fn error_statuses_map_to_stable_codes() {
    for (status, body, code) in [
        (401u16, "{\"error\":\"bad key\"}", "E_AUTH"),
        (403, "{\"error\":\"unsupported region\"}", "E_REGION"),
        (429, "{\"error\":\"slow down\"}", "E_RATE"),
        (500, "{\"error\":\"boom\"}", "E_OUTAGE"),
        (400, "{\"error\":\"bad\"}", "E_BAD_REQ"),
    ] {
        let p = openai::OpenAi::new(
            FakeHttp::json(status, body),
            cfg(ProviderKind::Openai),
            key(),
        );
        let err = p.complete(&ChatRequest::simple("m", "s", "u")).unwrap_err();
        assert_eq!(err.code(), code, "status {status}");
    }
}

#[test]
fn build_requires_a_key() {
    struct NoSecrets;
    impl Secrets for NoSecrets {
        fn get(&self, _k: &str) -> openclicky_core::Result<Option<SecretString>> {
            Ok(None)
        }
        fn set(&self, _k: &str, _v: &SecretString) -> openclicky_core::Result<()> {
            Ok(())
        }
        fn delete(&self, _k: &str) -> openclicky_core::Result<()> {
            Ok(())
        }
    }
    let http = FakeHttp::json(200, "{}");
    let config = Config::load(Path::new("/nonexistent/openclicky.toml")).unwrap();
    match build(&config, http, std::sync::Arc::new(NoSecrets)) {
        Ok(_) => panic!("must fail without a key"),
        Err(e) => assert_eq!(e.code(), "E_NO_KEY"),
    }
}

#[test]
fn wav_header_is_well_formed() {
    let clip = AudioClip {
        pcm: vec![0i16; 160],
        sample_rate: 16_000,
        channels: 1,
    };
    let wav = openai::encode_wav(&clip);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(wav.len(), 44 + 320);
}
