//! Desktop shell: windows, global hotkey and commands. No business rules here.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use openclicky_core::agent::runner::{AgentDeps, CancelToken};
use openclicky_core::agent::TaskRunner;
use openclicky_core::config::Config;
use openclicky_core::deps::Deps;
use openclicky_core::geom::PointerPlan;
use openclicky_core::memory::Memory;
use openclicky_core::ports::*;
use openclicky_core::privacy::CaptureReason;
use openclicky_core::providers;
use openclicky_core::store::Store;
use openclicky_core::tools::Registry;
use openclicky_core::usage::UsageMeter;
use openclicky_core::voice::VoiceEngine;
use openclicky_core::AppEvent;
use openclicky_platform::audio::CpalAudio;
use openclicky_platform::capture::XcapScreen;
use openclicky_platform::commands::SystemCommands;
use openclicky_platform::http::UreqTransport;
use openclicky_platform::input::PlatformInjector;
use openclicky_platform::paths::Paths;
use openclicky_platform::permissions;
use openclicky_platform::secrets::PlatformSecrets;
use openclicky_platform::speech::SystemSpeech;
use tauri::{Emitter, Manager, State as TauriState};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Everything the UI can reach. Built once at startup.
struct AppState {
    engine: Arc<VoiceEngine>,
    runner: TaskRunner,
    store: Arc<Store>,
    config: Arc<std::sync::RwLock<Config>>,
    paths: Paths,
    cancel: Mutex<CancelToken>,
}

/// Draws on the real screen through the overlay window.
struct WindowOverlay {
    app: tauri::AppHandle,
}

impl PointerOverlay for WindowOverlay {
    fn show(&self, plan: &PointerPlan) -> openclicky_core::Result<()> {
        let payload = serde_json::to_value(plan).unwrap_or(serde_json::Value::Null);
        let _ = self.app.emit("pointer-plan", payload);
        Ok(())
    }
    fn clear(&self) -> openclicky_core::Result<()> {
        let _ = self.app.emit("pointer-clear", ());
        Ok(())
    }
}

fn build_state(app: &tauri::AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
    let paths = Paths::resolve(None);
    paths.ensure()?;
    let config = Config::load(&paths.config_file())?;
    let secrets = Arc::new(PlatformSecrets::new(&paths.data_dir));
    let store = Arc::new(Store::open(&paths.db_file())?);
    let http: Arc<dyn HttpTransport> = Arc::new(UreqTransport::new());
    let memory = Arc::new(Memory::load(&paths.memory_dir())?);

    let chat = providers::build(&config, http.clone(), secrets.clone());
    let stt: Arc<dyn Transcriber> = match secrets.get(providers::KEY_NAME)? {
        Some(key) => Arc::new(providers::openai::OpenAiStt::new(
            http.clone(),
            config.clone(),
            key,
        )),
        None => Arc::new(NoStt),
    };
    let chat: Arc<dyn ChatProvider> = match chat {
        Ok(c) => c,
        Err(_) => Arc::new(NoChat),
    };

    let handle = app.clone();
    let events = Arc::new(move |event: AppEvent| {
        let _ = handle.emit(
            "app-event",
            serde_json::to_value(&event).unwrap_or_default(),
        );
    });

    let deps = Deps::new(
        Arc::new(XcapScreen::new()),
        Arc::new(CpalAudio::new()),
        stt,
        chat,
        Arc::new(SystemSpeech),
        Arc::new(WindowOverlay { app: app.clone() }),
        Arc::new(PlatformInjector::new()),
        Arc::new(permissions::SystemProbe),
        store.clone(),
        memory.clone(),
        config.clone(),
        events.clone(),
    );
    let engine = Arc::new(VoiceEngine::new(deps.clone()));

    let clock: Arc<dyn Clock> = Arc::new(openclicky_core::clock::SystemClock);
    let usage = Arc::new(UsageMeter::new(
        store.clone(),
        clock.clone(),
        config.budget.weights.clone(),
        config.budget.daily_units,
        config.budget.monthly_units,
    ));
    let provider = providers::build(&config, http.clone(), secrets.clone());
    let runner = TaskRunner::new(Arc::new(AgentDeps {
        store: store.clone(),
        chat: provider.unwrap_or_else(|_| Arc::new(NoChat)),
        tools: Arc::new(Registry::default_set()),
        clock,
        usage,
        memory,
        http,
        commands: Arc::new(SystemCommands),
        permissions: Arc::new(permissions::SystemProbe),
        workspace: paths.workspace_dir(),
        model: if config.provider.deep_model.is_empty() {
            config.provider.chat_model.clone()
        } else {
            config.provider.deep_model.clone()
        },
        max_steps: config.tools.max_steps,
        allow_private_network: config.tools.allow_private_network,
        shell_enabled: config.tools.shell,
        events: Some(events),
    }));

    Ok(AppState {
        engine,
        runner,
        store,
        config: Arc::new(std::sync::RwLock::new(config)),
        paths,
        cancel: Mutex::new(CancelToken::new()),
    })
}

/// Fallbacks so the UI still opens before a key is configured.
struct NoChat;
impl ChatProvider for NoChat {
    fn complete(&self, _r: &ChatRequest) -> openclicky_core::Result<ChatResponse> {
        Err(openclicky_core::Error::NoApiKey)
    }
    fn complete_stream(
        &self,
        _r: &ChatRequest,
        _d: &mut dyn FnMut(&str),
    ) -> openclicky_core::Result<ChatResponse> {
        Err(openclicky_core::Error::NoApiKey)
    }
    fn list_models(&self) -> openclicky_core::Result<Vec<String>> {
        Ok(vec![])
    }
}

struct NoStt;
impl Transcriber for NoStt {
    fn transcribe(
        &self,
        _clip: &AudioClip,
        _prompt: Option<&str>,
    ) -> openclicky_core::Result<Transcript> {
        Err(openclicky_core::Error::NoApiKey)
    }
}

#[tauri::command]
fn app_state(state: TauriState<'_, AppState>) -> serde_json::Value {
    let config = state.config.read().unwrap().clone();
    serde_json::json!({
        "state": state.engine.state().as_str(),
        "model": config.provider.chat_model,
        "budget": { "daily": config.budget.daily_units, "monthly": config.budget.monthly_units },
        "config_path": state.paths.config_file().to_string_lossy(),
        "data_dir": state.paths.data_dir.to_string_lossy(),
    })
}

#[tauri::command]
fn ask_text(state: TauriState<'_, AppState>, text: String) -> Result<String, String> {
    state
        .engine
        .ask_typed(&text)
        .map(|o| o.answer_text)
        .map_err(|e| format!("{} ({})", e.user_message(), e.code()))
}

#[tauri::command]
fn usage(state: TauriState<'_, AppState>) -> Result<serde_json::Value, String> {
    let meter = state.engine.usage_meter();
    let s = meter
        .state(chrono::Utc::now())
        .map_err(|e| e.user_message())?;
    Ok(serde_json::json!({
        "daily_used": s.daily_used, "daily_limit": s.daily_limit,
        "percent": s.percent, "blocked": s.blocked
    }))
}

#[tauri::command]
fn tasks(state: TauriState<'_, AppState>) -> Result<serde_json::Value, String> {
    let rows = state.store.list_tasks(None).map_err(|e| e.user_message())?;
    Ok(serde_json::json!(rows
        .into_iter()
        .map(|t| serde_json::json!({
            "id": t.id, "name": t.name, "state": t.state, "result": t.result
        }))
        .collect::<Vec<_>>()))
}

#[tauri::command]
fn cancel_voice(state: TauriState<'_, AppState>) -> Result<(), String> {
    let _ = &state.cancel;
    state.engine.cancel().map_err(|e| e.user_message())
}

#[tauri::command]
fn approve_task(state: TauriState<'_, AppState>, id: String) -> Result<(), String> {
    state.runner.approve(&id).map_err(|e| e.user_message())
}

#[tauri::command]
fn deny_task(state: TauriState<'_, AppState>, id: String) -> Result<(), String> {
    state.runner.deny(&id).map_err(|e| e.user_message())
}

#[tauri::command]
fn cancel_task(state: TauriState<'_, AppState>, id: String) -> Result<(), String> {
    state.runner.cancel(&id).map_err(|e| e.user_message())
}

#[tauri::command]
fn selftest(_state: TauriState<'_, AppState>) -> Result<serde_json::Value, String> {
    let gate = openclicky_core::privacy::CaptureGate::new();
    let guard = gate.authorize(CaptureReason::Calibration);
    let shots = XcapScreen::new()
        .capture_all(&guard)
        .map_err(|e| e.user_message())?;
    Ok(serde_json::json!(shots
        .iter()
        .map(|s| serde_json::json!({
            "screen": s.display.index, "width": s.width_px, "height": s.height_px,
            "scale": s.display.scale, "bytes": s.png.len()
        }))
        .collect::<Vec<_>>()))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            app_state,
            ask_text,
            usage,
            tasks,
            approve_task,
            deny_task,
            cancel_task,
            cancel_voice,
            selftest
        ])
        .setup(|app| {
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_ignore_cursor_events(true);
                let _ = overlay.set_always_on_top(true);
            }
            let state = build_state(&app.handle().clone()).map_err(|e| format!("startup: {e}"))?;
            let engine = state.engine.clone();
            app.manage(state);

            // Hold-to-talk: press starts, release asks.
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
            let release_engine = engine.clone();
            let handle = app.handle().clone();
            app.global_shortcut()
                .on_shortcut(shortcut, move |_app, _s, event| match event.state() {
                    ShortcutState::Pressed => {
                        let _ = release_engine.on_hotkey_down();
                    }
                    ShortcutState::Released => {
                        let engine = release_engine.clone();
                        let handle = handle.clone();
                        std::thread::spawn(move || match engine.on_hotkey_up() {
                            Ok(o) => {
                                let _ = handle.emit("answer", o.answer_text);
                            }
                            Err(e) => {
                                let _ = handle.emit("error", e.user_message());
                            }
                        });
                    }
                })?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run openclicky desktop");
}
