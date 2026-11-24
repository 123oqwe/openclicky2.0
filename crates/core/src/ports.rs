//! Ports: the only contract between the domain and the outside world.
//!
//! Production implementations live in `openclicky-platform`; fakes used by
//! tests live in `tests/support`. Nothing in this crate may reach the network,
//! the screen or the OS directly.

use chrono::{DateTime, Utc};
use std::fmt;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::geom::PointerPlan;

pub type Secs = f64;

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn monotonic_ms(&self) -> u64;
}

// ---------------------------------------------------------------- HTTP

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

/// Blocking HTTP. `stream` returns a line reader for SSE responses.
pub trait HttpTransport: Send + Sync {
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        timeout: Duration,
    ) -> Result<HttpResponse>;

    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<HttpResponse>;

    fn post_multipart(
        &self,
        url: &str,
        headers: &[(String, String)],
        form: &MultipartForm,
        timeout: Duration,
    ) -> Result<HttpResponse>;

    fn post_json_stream(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<Box<dyn LineStream>>;
}

pub trait LineStream: Send {
    /// Next line without the trailing newline; `None` at end of stream.
    fn next_line(&mut self) -> Result<Option<String>>;
}

#[derive(Debug, Clone, Default)]
pub struct MultipartForm {
    pub fields: Vec<(String, String)>,
    pub files: Vec<MultipartFile>,
}

#[derive(Debug, Clone)]
pub struct MultipartFile {
    pub field: String,
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

// ---------------------------------------------------------------- secrets

/// A secret that never prints itself.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***)")
    }
}

/// Re-checked before every tool call and before every capture (INV-7 / INV-D4).
pub trait PermissionProbe: Send + Sync {
    fn screen_recording(&self) -> bool;
    fn accessibility(&self) -> bool;
    fn microphone(&self) -> bool;
}

pub trait Secrets: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<SecretString>>;
    fn set(&self, key: &str, value: &SecretString) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

// ---------------------------------------------------------------- screen

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    pub index: usize,
    pub id: u32,
    pub name: String,
    /// Logical (points) origin in the global virtual desktop. Can be negative.
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
    /// physical pixels per logical point
    pub scale: f64,
    pub is_primary: bool,
    pub is_cursor_display: bool,
}

impl DisplayInfo {
    pub fn width_px(&self) -> u32 {
        (self.width * self.scale).round() as u32
    }
    pub fn height_px(&self) -> u32 {
        (self.height * self.scale).round() as u32
    }
}

#[derive(Debug, Clone)]
pub struct DisplayShot {
    pub display: DisplayInfo,
    pub width_px: u32,
    pub height_px: u32,
    pub png: Vec<u8>,
}

pub trait ScreenSource: Send + Sync {
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
    /// The guard parameter is the enforcement: without a `CaptureGuard`
    /// (constructible only through `CaptureGate::authorize`) this cannot be called.
    fn capture_all(&self, guard: &crate::privacy::CaptureGuard<'_>) -> Result<Vec<DisplayShot>>;
}

// ---------------------------------------------------------------- audio

#[derive(Debug, Clone, PartialEq)]
pub struct AudioClip {
    pub pcm: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioClip {
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0;
        }
        (self.pcm.len() as u64 * 1000) / (self.sample_rate as u64 * self.channels as u64)
    }
}

pub trait AudioSource: Send + Sync {
    fn start(&self) -> Result<()>;
    fn stop(&self) -> Result<AudioClip>;
    /// Copy of everything captured so far, without stopping. Used by
    /// incremental dictation.
    fn snapshot(&self) -> Result<AudioClip>;
    fn is_recording(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub is_partial: bool,
}

pub trait Transcriber: Send + Sync {
    fn transcribe(&self, clip: &AudioClip, prompt: Option<&str>) -> Result<Transcript>;
}

// ---------------------------------------------------------------- chat

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageInput {
    pub screen_index: usize,
    pub png: Vec<u8>,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub images: Vec<ImageInput>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl ChatRequest {
    pub fn simple(
        model: impl Into<String>,
        system: impl Into<String>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            system: system.into(),
            messages: vec![ChatMessage::user(user)],
            images: Vec::new(),
            tools: Vec::new(),
            max_tokens: 1024,
            temperature: 0.3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FinishReason {
    #[default]
    Stop,
    Length,
    ToolCalls,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
    pub finish: FinishReason,
}

pub trait ChatProvider: Send + Sync {
    /// Non-streaming; supports tool calls (used by the agent loop).
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse>;

    /// Streaming; every delta is delivered immediately (used by voice).
    fn complete_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse>;

    fn list_models(&self) -> Result<Vec<String>>;
}

// ---------------------------------------------------------------- speech

#[derive(Debug, Clone, PartialEq)]
pub struct SpeechOptions {
    pub voice: String,
    pub speed: f32,
    pub captions: bool,
}

impl Default for SpeechOptions {
    fn default() -> Self {
        Self {
            voice: "alloy".into(),
            speed: 1.0,
            captions: true,
        }
    }
}

pub trait SpeechHandle: Send + Sync {
    fn cancel(&self);
    /// Blocks until playback finished or was cancelled.
    fn wait(&self);
}

pub trait SpeechOut: Send + Sync {
    fn speak(&self, text: &str, opts: &SpeechOptions) -> Result<Box<dyn SpeechHandle>>;
}

// ---------------------------------------------------------------- overlay

pub trait PointerOverlay: Send + Sync {
    fn show(&self, plan: &PointerPlan) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

// ---------------------------------------------------------------- injection

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Terminal,
    Editor,
    Browser,
    Other,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInfo {
    pub app: String,
    pub kind: TargetKind,
}

impl TargetInfo {
    pub fn terminal(app: &str) -> Self {
        Self {
            app: app.into(),
            kind: TargetKind::Terminal,
        }
    }
    pub fn editor(app: &str) -> Self {
        Self {
            app: app.into(),
            kind: TargetKind::Editor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectOutcome {
    Typed,
    Pasted { clipboard_restored: bool },
    CopiedOnly,
}

pub trait TextInjector: Send + Sync {
    fn focused(&self) -> Option<TargetInfo>;
    fn inject(&self, text: &str, target: &TargetInfo) -> Result<InjectOutcome>;
}

// ---------------------------------------------------------------- processes

#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The only way the domain may run an external command. Implemented by the
/// platform layer so that `core` never touches `std::process`.
pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[String], cwd: &std::path::Path) -> Result<CommandOutput>;
}

// ---------------------------------------------------------------- helper

/// Convert a `Permission` style failure into a typed error.
pub fn permission_error(p: crate::error::Permission) -> Error {
    Error::NoPermission(p)
}
