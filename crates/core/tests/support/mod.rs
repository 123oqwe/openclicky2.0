//! Deterministic fakes for every port. No network, no filesystem outside
//! TempDir, no sleeping.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use openclicky_core::annotations::PointerPlan;
use openclicky_core::config::Config;
use openclicky_core::deps::Deps;
use openclicky_core::memory::Memory;
use openclicky_core::ports::*;
use openclicky_core::store::Store;
use openclicky_core::{AppEvent, Result};

// ---------------------------------------------------------------- screen

#[derive(Default)]
pub struct FakeScreen {
    pub captures: Mutex<u32>,
    pub displays: Mutex<Vec<DisplayInfo>>,
}

impl FakeScreen {
    pub fn new() -> Arc<Self> {
        let base = vec![
            DisplayInfo {
                index: 0,
                id: 1,
                name: "built-in".into(),
                origin_x: 0.0,
                origin_y: 0.0,
                width: 1440.0,
                height: 900.0,
                scale: 2.0,
                is_primary: true,
                is_cursor_display: true,
            },
            DisplayInfo {
                index: 1,
                id: 2,
                name: "external".into(),
                origin_x: -1920.0,
                origin_y: 0.0,
                width: 1920.0,
                height: 1080.0,
                scale: 1.0,
                is_primary: false,
                is_cursor_display: false,
            },
        ];
        Arc::new(Self {
            captures: Mutex::new(0),
            displays: Mutex::new(base),
        })
    }

    pub fn capture_count(&self) -> u32 {
        *self.captures.lock().unwrap()
    }
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G'];

impl ScreenSource for FakeScreen {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(self.displays.lock().unwrap().clone())
    }
    fn capture_all(
        &self,
        guard: &openclicky_core::privacy::CaptureGuard<'_>,
    ) -> Result<Vec<DisplayShot>> {
        let _ = guard.reason();
        *self.captures.lock().unwrap() += 1;
        Ok(self
            .displays
            .lock()
            .unwrap()
            .iter()
            .map(|d| DisplayShot {
                display: d.clone(),
                width_px: d.width_px(),
                height_px: d.height_px(),
                png: PNG.to_vec(),
            })
            .collect())
    }
}

// ---------------------------------------------------------------- audio

#[derive(Default)]
pub struct FakeAudio {
    recording: Mutex<bool>,
    pcm: Mutex<Vec<i16>>,
}

impl FakeAudio {
    pub fn new(ms: usize) -> Arc<Self> {
        let samples = 16_000 / 1000 * ms;
        Arc::new(Self {
            recording: Mutex::new(false),
            pcm: Mutex::new(vec![120i16; samples]),
        })
    }
}

impl AudioSource for FakeAudio {
    fn start(&self) -> Result<()> {
        *self.recording.lock().unwrap() = true;
        Ok(())
    }
    fn stop(&self) -> Result<AudioClip> {
        *self.recording.lock().unwrap() = false;
        Ok(AudioClip {
            pcm: self.pcm.lock().unwrap().clone(),
            sample_rate: 16_000,
            channels: 1,
        })
    }
    fn snapshot(&self) -> Result<AudioClip> {
        Ok(AudioClip {
            pcm: self.pcm.lock().unwrap().clone(),
            sample_rate: 16_000,
            channels: 1,
        })
    }
    fn is_recording(&self) -> bool {
        *self.recording.lock().unwrap()
    }
}

// ---------------------------------------------------------------- stt

pub struct FakeTranscriber {
    pub script: Mutex<Vec<Transcript>>,
    pub last: Mutex<String>,
    pub calls: Mutex<u32>,
}

impl FakeTranscriber {
    pub fn new(text: &str) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(vec![Transcript {
                text: text.into(),
                language: Some("zh".into()),
                is_partial: false,
            }]),
            last: Mutex::new(text.into()),
            calls: Mutex::new(0),
        })
    }
    pub fn with_script(items: Vec<&str>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(
                items
                    .into_iter()
                    .map(|t| Transcript {
                        text: t.into(),
                        language: None,
                        is_partial: false,
                    })
                    .collect(),
            ),
            last: Mutex::new(String::new()),
            calls: Mutex::new(0),
        })
    }
}

impl Transcriber for FakeTranscriber {
    fn transcribe(&self, _clip: &AudioClip, _prompt: Option<&str>) -> Result<Transcript> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        let mut script = self.script.lock().unwrap();
        if script.is_empty() {
            // Repeat the last answer so multi-question tests stay deterministic.
            Ok(Transcript {
                text: self.last.lock().unwrap().clone(),
                language: None,
                is_partial: false,
            })
        } else {
            let t = script.remove(0);
            *self.last.lock().unwrap() = t.text.clone();
            Ok(t)
        }
    }
}

// ---------------------------------------------------------------- clock

/// Deterministic clock. `now` only moves when a test moves it.
pub struct FakeClock {
    now: Mutex<chrono::DateTime<chrono::Utc>>,
    mono: Mutex<u64>,
}

impl FakeClock {
    pub fn new() -> Arc<Self> {
        Self::at(chrono::Utc::now())
    }
    pub fn at(now: chrono::DateTime<chrono::Utc>) -> Arc<Self> {
        Arc::new(Self {
            now: Mutex::new(now),
            mono: Mutex::new(0),
        })
    }
    pub fn advance(&self, by: chrono::Duration) {
        let mut now = self.now.lock().unwrap();
        *now += by;
    }
    pub fn set(&self, to: chrono::DateTime<chrono::Utc>) {
        *self.now.lock().unwrap() = to;
    }
    pub fn now_utc(&self) -> chrono::DateTime<chrono::Utc> {
        *self.now.lock().unwrap()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        *self.now.lock().unwrap()
    }
    fn monotonic_ms(&self) -> u64 {
        let mut m = self.mono.lock().unwrap();
        *m += 1;
        *m
    }
}

// ---------------------------------------------------------------- chat

/// Called once per streamed delta. Lets a test react mid-stream.
pub type DeltaHook = Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>;

pub struct ScriptedChat {
    pub responses: Mutex<Vec<ChatResponse>>,
    pub requests: Mutex<Vec<ChatRequest>>,
    /// Test hook: observes every streamed delta (used to cancel mid-stream).
    pub on_delta: DeltaHook,
}

impl ScriptedChat {
    pub fn new(responses: Vec<ChatResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(Vec::new()),
            on_delta: Mutex::new(None),
        })
    }
    pub fn text(text: &str) -> ChatResponse {
        ChatResponse {
            text: text.into(),
            tool_calls: Vec::new(),
            usage: TokenUsage::default(),
            finish: FinishReason::Stop,
        }
    }
    pub fn tools(calls: Vec<ToolCall>) -> ChatResponse {
        ChatResponse {
            text: String::new(),
            tool_calls: calls,
            usage: TokenUsage::default(),
            finish: FinishReason::ToolCalls,
        }
    }
    pub fn last_request(&self) -> Option<ChatRequest> {
        self.requests.lock().unwrap().last().cloned()
    }
}

impl ChatProvider for ScriptedChat {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse> {
        self.requests.lock().unwrap().push(req.clone());
        let mut r = self.responses.lock().unwrap();
        if r.is_empty() {
            return Ok(ChatResponse::default());
        }
        Ok(r.remove(0))
    }
    fn complete_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse> {
        self.requests.lock().unwrap().push(req.clone());
        let mut r = self.responses.lock().unwrap();
        if r.is_empty() {
            return Ok(ChatResponse::default());
        }
        let resp = r.remove(0);
        let hook = self.on_delta.lock().unwrap().clone();
        for chunk in resp.text.as_bytes().chunks(8) {
            let s = String::from_utf8_lossy(chunk).to_string();
            on_delta(&s);
            if let Some(h) = &hook {
                h(&s);
            }
        }
        Ok(resp)
    }
    fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["fake-model".into()])
    }
}

// ---------------------------------------------------------------- speech

pub struct FakeSpeech {
    pub spoken: Mutex<Vec<String>>,
    pub fail: bool,
}

impl FakeSpeech {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            spoken: Mutex::new(Vec::new()),
            fail: false,
        })
    }
    pub fn failing() -> Arc<Self> {
        Arc::new(Self {
            spoken: Mutex::new(Vec::new()),
            fail: true,
        })
    }
    pub fn said(&self) -> Vec<String> {
        self.spoken.lock().unwrap().clone()
    }
}

struct Done;
impl SpeechHandle for Done {
    fn cancel(&self) {}
    fn wait(&self) {}
}

impl SpeechOut for FakeSpeech {
    fn speak(&self, text: &str, _o: &SpeechOptions) -> Result<Box<dyn SpeechHandle>> {
        if self.fail {
            return Err(openclicky_core::Error::Internal("tts down".into()));
        }
        self.spoken.lock().unwrap().push(text.to_string());
        Ok(Box::new(Done))
    }
}

// ---------------------------------------------------------------- overlay

#[derive(Default)]
pub struct FakeOverlay {
    pub shown: Mutex<Vec<PointerPlan>>,
    pub clears: Mutex<u32>,
    pub delayed: Mutex<Vec<u64>>,
}

impl FakeOverlay {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    pub fn delayed(&self) -> Vec<u64> {
        self.delayed.lock().unwrap().clone()
    }
}

impl PointerOverlay for FakeOverlay {
    fn show(&self, plan: &PointerPlan) -> Result<()> {
        self.shown.lock().unwrap().push(plan.clone());
        Ok(())
    }
    fn clear(&self) -> Result<()> {
        *self.clears.lock().unwrap() += 1;
        Ok(())
    }
    fn clear_after(&self, ms: u64) -> Result<()> {
        self.delayed.lock().unwrap().push(ms);
        Ok(())
    }
}

// ---------------------------------------------------------------- injector

#[derive(Default)]
pub struct FakeInjector {
    pub injected: Mutex<Vec<(String, String)>>,
    pub copied: Mutex<Vec<String>>,
    pub target: Mutex<Option<TargetInfo>>,
}

impl FakeInjector {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    pub fn copied(&self) -> Vec<String> {
        self.copied.lock().unwrap().clone()
    }
    pub fn with_target(target: TargetInfo) -> Arc<Self> {
        Arc::new(Self {
            injected: Mutex::new(Vec::new()),
            copied: Mutex::new(Vec::new()),
            target: Mutex::new(Some(target)),
        })
    }
    pub fn texts(&self) -> Vec<String> {
        self.injected
            .lock()
            .unwrap()
            .iter()
            .map(|(t, _)| t.clone())
            .collect()
    }
}

impl TextInjector for FakeInjector {
    fn focused(&self) -> Option<TargetInfo> {
        self.target.lock().unwrap().clone()
    }
    fn inject(&self, text: &str, target: &TargetInfo) -> Result<InjectOutcome> {
        self.injected
            .lock()
            .unwrap()
            .push((text.to_string(), target.app.clone()));
        Ok(InjectOutcome::Typed)
    }
    fn copy(&self, text: &str) -> Result<InjectOutcome> {
        self.copied.lock().unwrap().push(text.to_string());
        Ok(InjectOutcome::CopiedOnly)
    }
}

// ---------------------------------------------------------------- http / commands

#[derive(Default)]
pub struct ScriptedHttp {
    pub responses: Mutex<Vec<HttpResponse>>,
    pub requests: Mutex<Vec<String>>,
}

impl ScriptedHttp {
    pub fn new(responses: Vec<HttpResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(Vec::new()),
        })
    }
    pub fn ok(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: vec![],
            body: body.as_bytes().to_vec(),
        }
    }
}

impl HttpTransport for ScriptedHttp {
    fn get(&self, url: &str, _h: &[(String, String)], _t: Duration) -> Result<HttpResponse> {
        self.requests.lock().unwrap().push(url.to_string());
        let mut r = self.responses.lock().unwrap();
        if r.is_empty() {
            return Ok(ScriptedHttp::ok("{}"));
        }
        Ok(r.remove(0))
    }
    fn post_json(
        &self,
        url: &str,
        _h: &[(String, String)],
        _b: &serde_json::Value,
        _t: Duration,
    ) -> Result<HttpResponse> {
        self.requests.lock().unwrap().push(url.to_string());
        let mut r = self.responses.lock().unwrap();
        if r.is_empty() {
            return Ok(ScriptedHttp::ok("{}"));
        }
        Ok(r.remove(0))
    }
    fn post_multipart(
        &self,
        _u: &str,
        _h: &[(String, String)],
        _f: &MultipartForm,
        _t: Duration,
    ) -> Result<HttpResponse> {
        let mut r = self.responses.lock().unwrap();
        if r.is_empty() {
            return Ok(ScriptedHttp::ok("{}"));
        }
        Ok(r.remove(0))
    }
    fn post_json_stream(
        &self,
        _u: &str,
        _h: &[(String, String)],
        _b: &serde_json::Value,
        _t: Duration,
    ) -> Result<Box<dyn LineStream>> {
        Err(openclicky_core::Error::Internal("not used in tests".into()))
    }
}

pub struct FakeProbe {
    pub accessibility: std::sync::atomic::AtomicBool,
    pub screen: std::sync::atomic::AtomicBool,
    pub microphone: std::sync::Mutex<openclicky_core::ports::MicrophoneAccess>,
}

impl Default for FakeProbe {
    /// A fake that has not been told otherwise permits everything, so a test
    /// that forgets to configure permissions tests the feature, not the gate.
    fn default() -> Self {
        Self {
            accessibility: Default::default(),
            screen: Default::default(),
            microphone: std::sync::Mutex::new(openclicky_core::ports::MicrophoneAccess::Granted),
        }
    }
}

impl FakeProbe {
    pub fn allowing() -> std::sync::Arc<Self> {
        use std::sync::atomic::AtomicBool;
        std::sync::Arc::new(Self {
            accessibility: AtomicBool::new(true),
            screen: AtomicBool::new(true),
            microphone: std::sync::Mutex::new(openclicky_core::ports::MicrophoneAccess::Granted),
        })
    }
    pub fn deny_accessibility(&self) {
        self.accessibility
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl PermissionProbe for FakeProbe {
    fn screen_recording(&self) -> bool {
        self.screen.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn accessibility(&self) -> bool {
        self.accessibility.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn microphone(&self) -> openclicky_core::ports::MicrophoneAccess {
        *self.microphone.lock().unwrap()
    }
}

#[derive(Default)]
pub struct FakeCommands {
    pub calls: Mutex<Vec<String>>,
}

impl FakeCommands {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

impl CommandRunner for FakeCommands {
    fn run(&self, program: &str, args: &[String], _cwd: &std::path::Path) -> Result<CommandOutput> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{program} {}", args.join(" ")));
        Ok(CommandOutput {
            status: 0,
            stdout: "ok".into(),
            stderr: String::new(),
        })
    }
}

// ---------------------------------------------------------------- harness

pub struct Harness {
    pub deps: Arc<Deps>,
    pub clock: Arc<FakeClock>,
    pub screen: Arc<FakeScreen>,
    pub audio: Arc<FakeAudio>,
    pub chat: Arc<ScriptedChat>,
    pub speech: Arc<FakeSpeech>,
    pub overlay: Arc<FakeOverlay>,
    pub injector: Arc<FakeInjector>,
    pub http: Arc<ScriptedHttp>,
    pub commands: Arc<FakeCommands>,
    pub probe: Arc<FakeProbe>,
    pub store: Arc<Store>,
    pub events: Arc<Mutex<Vec<AppEvent>>>,
    pub _dir: tempfile::TempDir,
    pub workspace: std::path::PathBuf,
}

pub struct HarnessOpts {
    pub transcript: String,
    pub audio_ms: usize,
    pub chat: Vec<ChatResponse>,
    pub target: Option<TargetInfo>,
    pub tts_fail: bool,
    pub http: Vec<HttpResponse>,
}

impl Default for HarnessOpts {
    fn default() -> Self {
        Self {
            transcript: "这个按钮是干什么的".into(),
            audio_ms: 1200,
            chat: vec![ScriptedChat::text("点了它会导出。")],
            target: None,
            tts_fail: false,
            http: vec![],
        }
    }
}

pub fn harness(opts: HarnessOpts) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let screen = FakeScreen::new();
    let audio = FakeAudio::new(opts.audio_ms);
    let stt = FakeTranscriber::new(&opts.transcript);
    let chat = ScriptedChat::new(opts.chat);
    let speech = if opts.tts_fail {
        FakeSpeech::failing()
    } else {
        FakeSpeech::new()
    };
    let overlay = FakeOverlay::new();
    let injector = match opts.target {
        Some(t) => FakeInjector::with_target(t),
        None => FakeInjector::new(),
    };
    let http = ScriptedHttp::new(opts.http);
    let commands = FakeCommands::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let probe = FakeProbe::allowing();
    let sink = {
        let e = events.clone();
        Arc::new(move |ev: AppEvent| e.lock().unwrap().push(ev)) as openclicky_core::deps::EventSink
    };

    let mut config = Config::default();
    config.provider.chat_model = "fake-model".into();
    let memory = Arc::new(Memory::load(dir.path().join("memory").as_path()).unwrap());
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let clock = FakeClock::new();
    let deps = Deps::new(
        clock.clone(),
        screen.clone(),
        audio.clone(),
        stt,
        chat.clone(),
        speech.clone(),
        overlay.clone(),
        injector.clone(),
        probe.clone(),
        store.clone(),
        memory,
        config,
        sink,
    );

    Harness {
        deps,
        clock,
        screen,
        audio,
        chat,
        speech,
        overlay,
        injector,
        http,
        commands,
        probe,
        store,
        events,
        _dir: dir,
        workspace,
    }
}
