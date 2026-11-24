//! Voice orchestration: the only place that turns a key press into an answer.
//!
//! Invariants: one session at a time; capture only through the gate; tags are
//! stripped before speaking; usage is charged once, in the same transaction as
//! the messages.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::annotations::{self, ParsedReply};
use crate::deps::Deps;
use crate::error::{Error, Result};
use crate::ports::{ChatMessage, ChatRequest, Role, SpeechOptions, TargetKind};
use crate::privacy::CaptureReason;
use crate::prompt::{self, PromptContext};
use crate::router::{self, RouteInput};
use crate::store::{MessageRow, Turn, UsageRow};
use crate::usage::{MeterInput, UsageKind};
use crate::AppEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum State {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Dictating,
    Driving,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Listening => "listening",
            State::Thinking => "thinking",
            State::Speaking => "speaking",
            State::Dictating => "dictating",
            State::Driving => "driving",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timings {
    pub capture_ms: u64,
    pub stt_ms: u64,
    pub first_delta_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug, Clone)]
pub struct AskOutcome {
    pub session_id: String,
    pub transcript: String,
    pub answer_text: String,
    pub parsed: ParsedReply,
    pub model: String,
    pub charged_units: f64,
    pub timings: Timings,
}

pub struct VoiceEngine {
    deps: Arc<Deps>,
    state: Mutex<State>,
    cancel: Arc<AtomicBool>,
    /// The utterance currently playing, so a new press can stop it.
    speech: Mutex<Option<Box<dyn crate::ports::SpeechHandle>>>,
}

impl VoiceEngine {
    pub fn new(deps: Arc<Deps>) -> Self {
        Self {
            deps,
            state: Mutex::new(State::Idle),
            cancel: Arc::new(AtomicBool::new(false)),
            speech: Mutex::new(None),
        }
    }

    pub fn usage_meter(&self) -> &Arc<crate::usage::UsageMeter> {
        &self.deps.usage
    }

    pub fn state(&self) -> State {
        *self.state.lock().unwrap()
    }

    fn set_state(&self, s: State) {
        *self.state.lock().unwrap() = s;
        self.deps.emit(AppEvent::State(s));
    }

    /// Start a session. Called from the global hotkey handler.
    pub fn on_hotkey_down(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if *state == State::Listening {
            return Ok(());
        }
        // Pressing the key while an answer is in flight interrupts it. The flag
        // stays set until the next request starts (`begin_request`), so the
        // in-flight stream actually observes the interruption instead of the
        // reset wiping it out in the same breath.
        if matches!(*state, State::Speaking | State::Thinking) {
            self.cancel.store(true, Ordering::SeqCst);
        }
        self.deps.audio.start()?;
        *state = State::Listening;
        drop(state);
        self.deps.emit(AppEvent::State(State::Listening));
        Ok(())
    }

    /// Reset the cancel flag for a brand new request. Safe because an interrupted
    /// previous stream had the whole key-hold to notice its own cancellation.
    fn begin_request(&self) {
        // A new press interrupts the previous answer, spoken or streaming.
        if let Some(handle) = self.speech.lock().unwrap().take() {
            handle.cancel();
        }
        self.cancel.store(false, Ordering::SeqCst);
    }

    /// The raw cancel flag, for callers and tests that need to interrupt a
    /// request from outside the engine.
    pub fn cancel_token(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Finish a session without a gesture.
    pub fn on_hotkey_up(&self) -> Result<AskOutcome> {
        self.on_hotkey_up_with_trail(None)
    }

    /// Finish a session: transcribe, capture, ask, point, speak.
    /// `trail` is the mouse path recorded while the key was held (SRC-PRD B4).
    pub fn on_hotkey_up_with_trail(&self, trail: Option<crate::geom::Trail>) -> Result<AskOutcome> {
        let started = self.deps.clock.monotonic_ms();
        let clip = self.deps.audio.stop()?;
        if clip.duration_ms() < 200 {
            self.set_state(State::Idle);
            return Err(Error::TooShort);
        }
        self.set_state(State::Thinking);
        // Budget is checked before anything expensive: no capture, no STT, no
        // request when the day is spent (ADR-0010).
        let now = self.deps.clock.now();
        if !self.deps.usage.can_start(now)? {
            self.set_state(State::Idle);
            return Err(Error::BudgetExhausted);
        }
        self.begin_request();

        let config = self.deps.config_snapshot();
        let session_id = self.deps.store.insert_session("ask", Utc::now())?;

        // Capture is authorised here and nowhere else (TR-02).
        if !self.deps.permissions.screen_recording() {
            self.set_state(State::Idle);
            return Err(Error::NoPermission(
                crate::error::Permission::ScreenRecording,
            ));
        }
        let shots = {
            let guard = self.deps.gate.authorize(CaptureReason::Hotkey);
            let result = self.deps.screen.capture_all(&guard);
            guard.close();
            result?
        };
        let capture_ms = self.deps.clock.monotonic_ms().saturating_sub(started);

        let stt_started = self.deps.clock.monotonic_ms();
        let dict_hint = None;
        let transcript = self.deps.stt.transcribe(&clip, dict_hint)?;
        let stt_ms = self.deps.clock.monotonic_ms().saturating_sub(stt_started);

        if transcript.text.trim().is_empty() {
            self.set_state(State::Idle);
            return Err(Error::EmptyAudio);
        }
        self.deps.emit(AppEvent::Transcript {
            text: transcript.text.clone(),
            partial: false,
        });

        let displays: Vec<_> = shots.iter().map(|s| s.display.clone()).collect();
        let cursor_display = displays
            .iter()
            .position(|d| d.is_cursor_display)
            .unwrap_or(0);
        let target = self.deps.injector.focused();
        let card = target.as_ref().and_then(|t| prompt::teaching_card(&t.app));

        let memory_profile = self.deps.memory.profile_text();
        let memory_projects = self.deps.memory.projects_text();
        let style_prefs: Vec<String> = memory_profile
            .lines()
            .filter(|l| {
                l.contains("短") || l.contains("简洁") || l.to_lowercase().contains("short")
            })
            .map(|l| l.to_string())
            .collect();

        let drawing_region = trail
            .as_ref()
            .filter(|t| t.is_meaningful())
            .and_then(|t| t.bbox())
            .map(|bbox| {
                let d = &displays[cursor_display.min(displays.len().saturating_sub(1))];
                let tl = crate::annotations::to_picture(
                    d,
                    crate::geom::Point {
                        x: bbox.x,
                        y: bbox.y,
                    },
                );
                let br = crate::annotations::to_picture(
                    d,
                    crate::geom::Point {
                        x: bbox.x + bbox.w,
                        y: bbox.y + bbox.h,
                    },
                );
                let r = crate::geom::Rect {
                    x: tl.x.min(br.x),
                    y: tl.y.min(br.y),
                    w: (br.x - tl.x).abs(),
                    h: (br.y - tl.y).abs(),
                };
                (d.index, r)
            });

        let ctx = PromptContext {
            app_name: target.as_ref().map(|t| t.app.as_str()),
            teaching_card: card,
            memory_profile: &memory_profile,
            memory_projects: &memory_projects,
            displays: &displays,
            cursor_display,
            language: if config.voice.language.is_empty() {
                None
            } else {
                Some(&config.voice.language)
            },
            style_prefs: &style_prefs,
            user_drawing: drawing_region.is_some(),
            drawing_region,
        };
        let system = prompt::system_prompt(&ctx);

        let tier = router::route(RouteInput {
            text: &transcript.text,
            has_screen: !shots.is_empty(),
            is_task: false,
        });
        let model = config.effective_model(tier);
        if model.is_empty() {
            self.set_state(State::Idle);
            return Err(Error::NoModel);
        }

        let labels: Vec<String> = displays
            .iter()
            .map(|d| format!("screen{} {}x{}", d.index, d.width_px(), d.height_px()))
            .collect();
        let images = shots
            .iter()
            .map(|s| crate::ports::ImageInput {
                screen_index: s.display.index,
                png: s.png.clone(),
                width_px: s.width_px,
                height_px: s.height_px,
            })
            .collect();

        let history = self
            .deps
            .store
            .recent_messages(&session_id, 10)
            .unwrap_or_default();
        let mut messages: Vec<ChatMessage> = history
            .iter()
            .map(|m| ChatMessage {
                role: if m.role == "assistant" {
                    Role::Assistant
                } else {
                    Role::User
                },
                content: m.content.clone(),
            })
            .collect();
        messages.push(ChatMessage::user(prompt::user_turn(
            &transcript.text,
            &labels,
        )));

        let request = ChatRequest {
            model: model.clone(),
            system,
            messages,
            images,
            tools: Vec::new(),
            max_tokens: config.voice.max_answer_tokens,
            temperature: 0.3,
        };

        let mut first_delta_at: Option<u64> = None;
        let mut emitted = 0usize;
        let stream_started = self.deps.clock.monotonic_ms();
        let response = {
            let deps = self.deps.clone();
            let cancel = self.cancel.clone();
            self.deps.chat.complete_stream(&request, &mut |delta| {
                // A second key press stops the stream where it is.
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                if first_delta_at.is_none() {
                    first_delta_at = Some(deps.clock.monotonic_ms());
                }
                emitted += delta.len();
                if config.voice.captions {
                    deps.emit(AppEvent::Subtitle {
                        text: delta.to_string(),
                    });
                }
                deps.emit(AppEvent::Delta {
                    text: delta.to_string(),
                });
            })?
        };

        if self.cancel.load(Ordering::SeqCst) {
            self.set_state(State::Idle);
            self.deps.overlay.clear().ok();
            return Err(Error::State("已取消".into()));
        }

        let parsed = annotations::parse_reply(&response.text, &displays, cursor_display);
        let plan_id = format!(
            "{}-{}",
            session_id,
            self.deps.clock.now().timestamp_millis()
        );
        let plan = annotations::plan_from(&parsed, &displays, &plan_id);

        self.set_state(State::Speaking);
        self.deps.emit(AppEvent::Answer {
            text: parsed.plain_text.clone(),
        });
        if let Some(p) = &plan {
            if let Err(e) = self.deps.overlay.show(p) {
                self.deps.emit(AppEvent::Notice {
                    code: "E_OVERLAY".into(),
                    message: e.user_message(),
                });
            }
            self.deps.emit(AppEvent::Annotations {
                plan_id: p.plan_id.clone(),
            });
        }

        let spoken = if parsed.plain_text.trim().is_empty() {
            response.text.clone()
        } else {
            parsed.plain_text.clone()
        };
        let mut speech_fallback = false;
        match self.deps.speech.speak(
            &spoken,
            &SpeechOptions {
                voice: config.provider.tts_voice.clone(),
                speed: config.voice.speech_speed,
                captions: config.voice.captions,
            },
        ) {
            Ok(handle) => {
                // Keep the handle so the next press can cut the audio off.
                *self.speech.lock().unwrap() = Some(handle);
                if let Some(h) = self.speech.lock().unwrap().as_ref() {
                    h.wait();
                }
                self.speech.lock().unwrap().take();
            }
            Err(e) => {
                speech_fallback = true;
                self.deps.emit(AppEvent::Notice {
                    code: "E_TTS".into(),
                    message: format!("{}（改为字幕与剪贴板）", e.user_message()),
                });
                let _ = self.deps.injector.inject(
                    &spoken,
                    &crate::ports::TargetInfo {
                        app: "clipboard".into(),
                        kind: TargetKind::Other,
                    },
                );
            }
        }

        // Expose the answer on the clipboard only when we could not speak.
        if speech_fallback {
            self.deps.emit(AppEvent::Subtitle {
                text: spoken.clone(),
            });
        }

        let (units, _waived) = self.deps.usage.quote(&MeterInput {
            kind: UsageKind::Voice,
            task_id: None,
            session_id: Some(session_id.clone()),
            started_at: Utc::now(),
            now: Utc::now(),
            task_already_charged: false,
        });

        let now = Utc::now();
        let user_id = uuid::Uuid::new_v4().to_string();
        let assistant_id = uuid::Uuid::new_v4().to_string();
        self.deps.store.commit_turn(&Turn {
            session_id: session_id.clone(),
            messages: vec![
                MessageRow {
                    id: user_id,
                    session_id: session_id.clone(),
                    role: "user".into(),
                    content: transcript.text.clone(),
                    model: None,
                    created_at: now,
                    meta_json: None,
                },
                MessageRow {
                    id: assistant_id,
                    session_id: session_id.clone(),
                    role: "assistant".into(),
                    content: parsed.plain_text.clone(),
                    model: Some(model.clone()),
                    created_at: now,
                    meta_json: None,
                },
            ],
            usage: Some(UsageRow {
                kind: "voice".into(),
                units,
                task_id: None,
                session_id: Some(session_id.clone()),
                waived: false,
                created_at: now,
                meta_json: None,
            }),
            task_update: None,
        })?;

        self.deps.overlay.clear().ok();
        self.set_state(State::Idle);

        let percent = self.deps.usage.state(now)?.percent;
        self.deps.emit(AppEvent::Usage { percent });
        let _ = emitted;

        Ok(AskOutcome {
            session_id,
            transcript: transcript.text,
            answer_text: parsed.plain_text.clone(),
            parsed,
            model,
            charged_units: units,
            timings: Timings {
                capture_ms,
                stt_ms,
                first_delta_ms: first_delta_at.map(|t| t - stream_started).unwrap_or(0),
                total_ms: self.deps.clock.monotonic_ms().saturating_sub(started),
            },
        })
    }

    pub fn cancel(&self) -> Result<()> {
        self.cancel.store(true, Ordering::SeqCst);
        self.deps.overlay.clear().ok();
        if self.state() != State::Idle {
            self.set_state(State::Idle);
        }
        Ok(())
    }

    /// Text question path (double-tap Control). No audio, no capture.
    pub fn ask_typed(&self, text: &str) -> Result<AskOutcome> {
        if text.trim().is_empty() {
            return Err(Error::BadRequest("问题为空".into()));
        }
        // Budget first: a blocked engine must not change state or write usage.
        let budget_now = self.deps.clock.now();
        if !self.deps.usage.can_start(budget_now)? {
            return Err(Error::BudgetExhausted);
        }
        let started = self.deps.clock.monotonic_ms();
        self.set_state(State::Thinking);
        let config = self.deps.config_snapshot();
        let session_id = self.deps.store.insert_session("typed", Utc::now())?;

        // A typed question may point at the screen too (SRC-PRD C4). Capture
        // still goes through the single gate; without permission we answer
        // without pointing instead of failing the question.
        let shots = if self.deps.permissions.screen_recording() {
            let guard = self.deps.gate.authorize(CaptureReason::TypedQuestion);
            let result = self.deps.screen.capture_all(&guard);
            guard.close();
            result.unwrap_or_default()
        } else {
            Vec::new()
        };
        let displays: Vec<_> = shots.iter().map(|s| s.display.clone()).collect();
        let cursor_display = displays
            .iter()
            .position(|d| d.is_cursor_display)
            .unwrap_or(0);
        let target = self.deps.injector.focused();
        let card = target.as_ref().and_then(|t| prompt::teaching_card(&t.app));
        let images = shots
            .iter()
            .map(|s| crate::ports::ImageInput {
                screen_index: s.display.index,
                png: s.png.clone(),
                width_px: s.width_px,
                height_px: s.height_px,
            })
            .collect::<Vec<_>>();

        let tier = router::route(RouteInput {
            text,
            has_screen: !shots.is_empty(),
            is_task: false,
        });
        let model = config.effective_model(tier);
        if model.is_empty() {
            self.set_state(State::Idle);
            return Err(Error::NoModel);
        }
        let ctx = PromptContext {
            app_name: target.as_ref().map(|t| t.app.as_str()),
            teaching_card: card,
            memory_profile: &self.deps.memory.profile_text(),
            memory_projects: &self.deps.memory.projects_text(),
            displays: &displays,
            cursor_display,
            language: if config.voice.language.is_empty() {
                None
            } else {
                Some(&config.voice.language)
            },
            style_prefs: &[],
            user_drawing: false,
            drawing_region: None,
        };
        let request = ChatRequest {
            model: model.clone(),
            system: prompt::system_prompt(&ctx),
            messages: vec![ChatMessage::user(text)],
            images,
            tools: Vec::new(),
            max_tokens: config.voice.max_answer_tokens,
            temperature: 0.3,
        };
        self.begin_request();
        let cancel = self.cancel.clone();
        let mut first_delta_at: Option<u64> = None;
        let stream_started = self.deps.clock.monotonic_ms();
        let mut cancelled = false;
        let response = {
            let deps = self.deps.clone();
            let cancel = cancel.clone();
            self.deps.chat.complete_stream(&request, &mut |delta| {
                if cancel.load(Ordering::SeqCst) {
                    cancelled = true;
                    return;
                }
                if first_delta_at.is_none() {
                    first_delta_at = Some(deps.clock.monotonic_ms());
                }
                if config.voice.captions {
                    deps.emit(AppEvent::Subtitle {
                        text: delta.to_string(),
                    });
                }
                deps.emit(AppEvent::Delta {
                    text: delta.to_string(),
                });
            })?
        };

        if cancelled || self.cancel.load(Ordering::SeqCst) {
            // Canceled mid-stream: no Answer event, no usage row, back to Idle.
            self.set_state(State::Idle);
            self.deps.overlay.clear().ok();
            return Err(Error::State("已取消".into()));
        }
        let parsed = annotations::parse_reply(&response.text, &displays, cursor_display);
        self.set_state(State::Speaking);
        self.deps.emit(AppEvent::Answer {
            text: parsed.plain_text.clone(),
        });
        if let Some(plan) = annotations::plan_from(
            &parsed,
            &displays,
            &format!("{session_id}-{}", self.deps.clock.now().timestamp_millis()),
        ) {
            if self.deps.overlay.show(&plan).is_ok() {
                self.deps.emit(AppEvent::Annotations {
                    plan_id: plan.plan_id.clone(),
                });
            }
        }
        let (units, _waived) = self.deps.usage.quote(&MeterInput {
            kind: UsageKind::Voice,
            task_id: None,
            session_id: Some(session_id.clone()),
            started_at: Utc::now(),
            now: Utc::now(),
            task_already_charged: false,
        });
        let now = Utc::now();
        self.deps.store.commit_turn(&Turn {
            session_id: session_id.clone(),
            messages: vec![
                MessageRow {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.clone(),
                    role: "user".into(),
                    content: text.to_string(),
                    model: None,
                    created_at: now,
                    meta_json: None,
                },
                MessageRow {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.clone(),
                    role: "assistant".into(),
                    content: parsed.plain_text.clone(),
                    model: Some(model.clone()),
                    created_at: now,
                    meta_json: None,
                },
            ],
            usage: Some(UsageRow {
                kind: "voice".into(),
                units,
                task_id: None,
                session_id: Some(session_id.clone()),
                waived: false,
                created_at: now,
                meta_json: None,
            }),
            task_update: None,
        })?;
        self.set_state(State::Idle);
        Ok(AskOutcome {
            session_id,
            transcript: text.to_string(),
            answer_text: parsed.plain_text.clone(),
            parsed,
            model,
            charged_units: units,
            timings: Timings {
                first_delta_ms: first_delta_at.map(|t| t - stream_started).unwrap_or(0),
                total_ms: self.deps.clock.monotonic_ms().saturating_sub(started),
                ..Default::default()
            },
        })
    }
}
