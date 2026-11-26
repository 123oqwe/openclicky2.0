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
fn a_reasoning_model_gets_max_completion_tokens_and_no_temperature() {
    for model in [
        "o1",
        "o1-preview",
        "o3-mini",
        "gpt-5",
        "gpt-5.1",
        "openrouter/o4-mini",
    ] {
        let http = FakeHttp::json(200, r#"{"choices":[{"message":{"content":"ok"}}]}"#);
        let p = openai::OpenAi::new(http.clone(), cfg(ProviderKind::Openai), key());
        p.complete(&ChatRequest::simple(model, "s", "u")).unwrap();
        let sent = http.body().unwrap();
        assert!(
            sent.get("temperature").is_none(),
            "{model} refuses temperature, so it must not be sent"
        );
        assert!(
            sent.get("max_tokens").is_none(),
            "{model} takes max_completion_tokens, not max_tokens"
        );
        assert_eq!(
            sent["max_completion_tokens"].as_u64(),
            Some(1024),
            "{model} carries the requested budget"
        );
        assert_eq!(
            sent["model"].as_str(),
            Some(model),
            "{model} is sent under the id the user picked"
        );
    }
}

#[test]
fn a_reasoning_model_is_streamed_with_the_same_fields() {
    let http = FakeHttp::sse(vec!["data: [DONE]"]);
    let p = openai::OpenAi::new(http.clone(), cfg(ProviderKind::Openai), key());
    p.complete_stream(&ChatRequest::simple("o3-mini", "s", "u"), &mut |_| {})
        .unwrap();
    let sent = http.body().unwrap();
    assert!(
        sent.get("temperature").is_none(),
        "a streamed o3-mini refuses temperature too"
    );
    assert!(
        sent.get("max_tokens").is_none(),
        "a streamed o3-mini takes max_completion_tokens"
    );
    assert_eq!(
        sent["max_completion_tokens"].as_u64(),
        Some(1024),
        "a streamed o3-mini carries the requested budget"
    );
}

#[test]
fn an_ordinary_model_keeps_temperature_and_max_tokens() {
    for model in ["gpt-4o", "gpt-4.1-mini", "gpt-50", "mock-chat"] {
        let http = FakeHttp::json(200, r#"{"choices":[{"message":{"content":"ok"}}]}"#);
        let p = openai::OpenAi::new(http.clone(), cfg(ProviderKind::Openai), key());
        p.complete(&ChatRequest::simple(model, "s", "u")).unwrap();
        let sent = http.body().unwrap();
        assert_eq!(
            sent["temperature"].as_f64(),
            Some(f64::from(0.3f32)),
            "{model} still takes a temperature"
        );
        assert_eq!(
            sent["max_tokens"].as_u64(),
            Some(1024),
            "{model} still takes max_tokens"
        );
        assert!(
            sent.get("max_completion_tokens").is_none(),
            "{model} must not be sent a field it may reject"
        );
    }
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

#[test]
fn an_unchosen_model_still_checks_the_key() {
    let config = Config::load(Path::new("/nonexistent/openclicky.toml")).unwrap();
    let key = SecretString::from("sk-good");
    let models = openclicky_core::providers::check_save(
        &config,
        FakeHttp::json(200, r#"{"data":[{"id":"gpt-real"}]}"#),
        Some(&key),
        "",
    )
    .expect("an empty model does not skip the check");
    assert_eq!(models, vec!["gpt-real".to_string()]);

    let refused = openclicky_core::providers::check_save(
        &config,
        FakeHttp::json(401, r#"{"error":{"message":"nope"}}"#),
        Some(&key),
        "",
    );
    assert_eq!(
        refused.unwrap_err().code(),
        "E_AUTH",
        "a dead key must fail even with no model chosen"
    );
}

#[test]
fn a_refused_key_fails_the_save_check() {
    let config = Config::load(Path::new("/nonexistent/openclicky.toml")).unwrap();
    let key = SecretString::from("sk-wrong");
    let denied = openclicky_core::providers::check_save(
        &config,
        FakeHttp::json(401, r#"{"error":{"message":"invalid api key"}}"#),
        Some(&key),
        "gpt-test",
    );
    assert_eq!(denied.unwrap_err().code(), "E_AUTH");
}

#[test]
fn a_model_outside_the_list_is_refused_before_it_is_saved() {
    let config = Config::load(Path::new("/nonexistent/openclicky.toml")).unwrap();
    let key = SecretString::from("sk-good");
    let err = openclicky_core::providers::check_save(
        &config,
        FakeHttp::json(200, r#"{"data":[{"id":"gpt-real"}]}"#),
        Some(&key),
        "gpt-imaginary",
    )
    .unwrap_err();
    assert!(
        err.user_message().contains("gpt-imaginary"),
        "the message must name the model that is missing: {}",
        err.user_message()
    );

    let ok = openclicky_core::providers::check_save(
        &config,
        FakeHttp::json(200, r#"{"data":[{"id":"gpt-real"}]}"#),
        Some(&key),
        "gpt-real",
    )
    .expect("a model in the list passes");
    assert_eq!(ok, vec!["gpt-real".to_string()]);
}

#[test]
fn the_target_is_named_so_a_refusal_points_at_one_endpoint() {
    let mut config = Config::load(Path::new("/nonexistent/openclicky.toml")).unwrap();
    config.provider.base_url = "https://example.test/v1".into();
    let target = openclicky_core::providers::describe_target(&config);
    assert!(target.contains("openai"), "provider missing from {target}");
    assert!(
        target.contains("https://example.test/v1"),
        "base_url missing from {target}"
    );
}

struct RememberingSecrets {
    stored: Mutex<Vec<String>>,
}

impl Secrets for RememberingSecrets {
    fn get(&self, _k: &str) -> openclicky_core::Result<Option<SecretString>> {
        Ok(None)
    }
    fn set(&self, _k: &str, v: &SecretString) -> openclicky_core::Result<()> {
        self.stored.lock().unwrap().push(v.expose().to_string());
        Ok(())
    }
    fn delete(&self, _k: &str) -> openclicky_core::Result<()> {
        Ok(())
    }
}

#[test]
fn a_refused_save_leaves_the_working_config_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut working = Config::default();
    working.provider.base_url = "https://api.openai.com/v1".into();
    working.provider.chat_model = "gpt-real".into();
    working.save(&path).unwrap();
    let before = std::fs::read(&path).unwrap();

    let secrets = RememberingSecrets {
        stored: Mutex::new(Vec::new()),
    };
    let mut attempted = working.clone();
    let err = openclicky_core::providers::save_provider(
        &mut attempted,
        &path,
        FakeHttp::json(401, r#"{"error":{"message":"invalid api key"}}"#),
        &secrets,
        Some(&SecretString::from("sk-wrong")),
        "gpt-real",
    )
    .unwrap_err();

    assert_eq!(err.code(), "E_AUTH");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "a refused save must not rewrite the file that still works"
    );
    assert!(
        secrets.stored.lock().unwrap().is_empty(),
        "a refused key must not reach the key store either"
    );
}

#[test]
fn an_accepted_save_writes_the_model_and_stores_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let secrets = RememberingSecrets {
        stored: Mutex::new(Vec::new()),
    };
    let mut cfg = Config::default();
    cfg.provider.base_url = "https://api.openai.com/v1".into();

    openclicky_core::providers::save_provider(
        &mut cfg,
        &path,
        FakeHttp::json(200, r#"{"data":[{"id":"gpt-real"}]}"#),
        &secrets,
        Some(&SecretString::from("sk-good")),
        "gpt-real",
    )
    .unwrap();

    let written = Config::load(&path).unwrap();
    assert_eq!(written.provider.chat_model, "gpt-real");
    assert_eq!(secrets.stored.lock().unwrap().len(), 1);
}

/// A transport that replays a script: each call takes the next entry, and the
/// last one repeats. Counts calls so a test can tell a retry from a single try.
struct ScriptedHttp {
    script: Mutex<Vec<openclicky_core::Result<HttpResponse>>>,
    calls: std::sync::atomic::AtomicUsize,
}

impl ScriptedHttp {
    fn new(script: Vec<openclicky_core::Result<HttpResponse>>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            script: Mutex::new(script),
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn next(&self) -> openclicky_core::Result<HttpResponse> {
        let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut script = self.script.lock().unwrap();
        if index + 1 < script.len() {
            script.remove(0)
        } else {
            script[0].clone()
        }
    }
}

impl HttpTransport for ScriptedHttp {
    fn get(
        &self,
        _u: &str,
        _h: &[(String, String)],
        _t: Duration,
    ) -> openclicky_core::Result<HttpResponse> {
        self.next()
    }
    fn post_json(
        &self,
        _u: &str,
        _h: &[(String, String)],
        _b: &serde_json::Value,
        _t: Duration,
    ) -> openclicky_core::Result<HttpResponse> {
        self.next()
    }
    fn post_multipart(
        &self,
        _u: &str,
        _h: &[(String, String)],
        _f: &MultipartForm,
        _t: Duration,
    ) -> openclicky_core::Result<HttpResponse> {
        self.next()
    }
    fn post_json_stream(
        &self,
        _u: &str,
        _h: &[(String, String)],
        _b: &serde_json::Value,
        _t: Duration,
    ) -> openclicky_core::Result<Box<dyn LineStream>> {
        Ok(Box::new(Lines(Vec::new(), 0)))
    }
}

fn chat_ok() -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![],
        body: br#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#.to_vec(),
    }
}

#[test]
fn a_transient_timeout_is_retried_and_then_succeeds() {
    let http = ScriptedHttp::new(vec![Err(openclicky_core::Error::Timeout), Ok(chat_ok())]);
    let provider = openai::OpenAi::new(http.clone(), cfg(ProviderKind::Openai), key());
    let reply = provider
        .complete(&ChatRequest::simple("m", "s", "u"))
        .unwrap();
    assert_eq!(reply.text, "ok");
    assert_eq!(http.calls(), 2, "one timeout, one retry");
}

#[test]
fn a_bad_key_is_not_retried() {
    let http = ScriptedHttp::new(vec![Ok(HttpResponse {
        status: 401,
        headers: vec![],
        body: br#"{"error":{"message":"invalid api key"}}"#.to_vec(),
    })]);
    let provider = openai::OpenAi::new(http.clone(), cfg(ProviderKind::Openai), key());
    let error = provider
        .complete(&ChatRequest::simple("m", "s", "u"))
        .unwrap_err();
    assert_eq!(error.code(), "E_AUTH");
    assert_eq!(http.calls(), 1, "a rejected key stays rejected");
}

#[test]
fn retries_stop_after_two_so_a_dead_provider_fails_in_seconds() {
    let http = ScriptedHttp::new(vec![Err(openclicky_core::Error::Outage)]);
    let provider = openai::OpenAi::new(http.clone(), cfg(ProviderKind::Openai), key());
    let error = provider
        .complete(&ChatRequest::simple("m", "s", "u"))
        .unwrap_err();
    assert_eq!(error.code(), "E_OUTAGE");
    assert_eq!(http.calls(), 3, "the first attempt plus two retries");
}
