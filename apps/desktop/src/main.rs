//! Desktop shell: windows, global hotkey and commands. No business rules here.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

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
use openclicky_platform::speech::SystemSpeech;
use tauri::{Emitter, Manager, State as TauriState};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

struct ReloadableChat {
    http: Arc<dyn HttpTransport>,
    secrets: Arc<dyn Secrets>,
    config: Arc<RwLock<Config>>,
}

impl ChatProvider for ReloadableChat {
    fn complete(&self, req: &ChatRequest) -> openclicky_core::Result<ChatResponse> {
        let cfg = self.config.read().unwrap().clone();
        providers::build(&cfg, self.http.clone(), self.secrets.clone())?.complete(req)
    }
    fn complete_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> openclicky_core::Result<ChatResponse> {
        let cfg = self.config.read().unwrap().clone();
        providers::build(&cfg, self.http.clone(), self.secrets.clone())?
            .complete_stream(req, on_delta)
    }
    fn list_models(&self) -> openclicky_core::Result<Vec<String>> {
        let cfg = self.config.read().unwrap().clone();
        providers::build(&cfg, self.http.clone(), self.secrets.clone())?.list_models()
    }
}

struct ReloadableStt {
    http: Arc<dyn HttpTransport>,
    secrets: Arc<dyn Secrets>,
    config: Arc<RwLock<Config>>,
}

impl Transcriber for ReloadableStt {
    fn transcribe(
        &self,
        clip: &AudioClip,
        prompt: Option<&str>,
    ) -> openclicky_core::Result<Transcript> {
        let cfg = self.config.read().unwrap().clone();
        let key = self
            .secrets
            .get(providers::KEY_NAME)?
            .ok_or(openclicky_core::Error::NoApiKey)?;
        providers::openai::OpenAiStt::new(self.http.clone(), cfg, key).transcribe(clip, prompt)
    }
}

struct AppState {
    engine: Arc<VoiceEngine>,
    runner: Arc<TaskRunner>,
    store: Arc<Store>,
    config: Arc<RwLock<Config>>,
    secrets: Arc<dyn Secrets>,
    http: Arc<dyn HttpTransport>,
    paths: Paths,
}

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
    let secrets = openclicky_platform::open_secrets(&paths.data_dir);
    let store = Arc::from(Store::open(&paths.db_file())?);
    let http: Arc<dyn HttpTransport> = Arc::from(UreqTransport::default());
    let memory = Arc::from(Memory::load(&paths.memory_dir())?);
    let config = Arc::from(RwLock::from(config));

    let handle = app.clone();
    let events = Arc::from(move |event: AppEvent| {
        let _ = handle.emit(
            "app-event",
            serde_json::to_value(&event).unwrap_or_default(),
        );
    });

    let deps = Deps::new(
        Arc::from(XcapScreen),
        Arc::from(CpalAudio::default()),
        Arc::from(ReloadableStt {
            http: http.clone(),
            secrets: secrets.clone(),
            config: config.clone(),
        }),
        Arc::from(ReloadableChat {
            http: http.clone(),
            secrets: secrets.clone(),
            config: config.clone(),
        }),
        Arc::from(SystemSpeech),
        Arc::from(WindowOverlay { app: app.clone() }),
        Arc::from(PlatformInjector),
        Arc::from(permissions::SystemProbe),
        store.clone(),
        memory.clone(),
        config.read().unwrap().clone(),
        events.clone(),
    );
    let engine = Arc::from(VoiceEngine::new(deps));

    let clock: Arc<dyn Clock> = Arc::from(openclicky_core::clock::SystemClock);
    let cfg = config.read().unwrap().clone();
    let usage = Arc::from(UsageMeter::new(
        store.clone(),
        clock.clone(),
        cfg.budget.weights.clone(),
        cfg.budget.daily_units,
        cfg.budget.monthly_units,
    ));
    let runner = Arc::from(TaskRunner::new(Arc::from(AgentDeps {
        store: store.clone(),
        chat: Arc::from(ReloadableChat {
            http: http.clone(),
            secrets: secrets.clone(),
            config: config.clone(),
        }),
        tools: Arc::from(Registry::default_set()),
        clock,
        usage,
        memory,
        http: http.clone(),
        commands: Arc::from(SystemCommands),
        permissions: Arc::from(permissions::SystemProbe),
        workspace: paths.workspace_dir(),
        model: if cfg.provider.deep_model.is_empty() {
            cfg.provider.chat_model.clone()
        } else {
            cfg.provider.deep_model.clone()
        },
        max_steps: cfg.tools.max_steps,
        allow_private_network: cfg.tools.allow_private_network,
        shell_enabled: cfg.tools.shell,
        events: Some(events),
    })));

    Ok(AppState {
        engine,
        runner,
        store,
        config,
        secrets,
        http,
        paths,
    })
}

#[tauri::command]
fn app_state(state: TauriState<'_, AppState>) -> serde_json::Value {
    let config = state.config.read().unwrap().clone();
    let has_key = matches!(state.secrets.get(providers::KEY_NAME), Ok(Some(_)));
    serde_json::json!({
        "state": state.engine.state().as_str(),
        "has_key": has_key,
        "ready": has_key && !config.provider.chat_model.is_empty(),
        "model": config.provider.chat_model,
        "kind": config.provider.kind.as_str(),
        "base_url": config.provider.base_url,
        "budget": { "daily": config.budget.daily_units, "monthly": config.budget.monthly_units },
        "config_path": state.paths.config_file().to_string_lossy(),
        "data_dir": state.paths.data_dir.to_string_lossy(),
    })
}

#[tauri::command]
fn list_models(
    state: TauriState<'_, AppState>,
    key: Option<String>,
) -> Result<Vec<String>, String> {
    if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
        state
            .secrets
            .set(providers::KEY_NAME, &k.as_str().into())
            .map_err(|e| e.user_message())?;
    }
    let cfg = state.config.read().unwrap().clone();
    providers::build(&cfg, state.http.clone(), state.secrets.clone())
        .and_then(|p| p.list_models())
        .map_err(|e| format!("{} ({})", e.user_message(), e.code()))
}

#[tauri::command]
fn save_provider(
    state: TauriState<'_, AppState>,
    key: Option<String>,
    model: String,
) -> Result<(), String> {
    if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
        state
            .secrets
            .set(providers::KEY_NAME, &k.as_str().into())
            .map_err(|e| e.user_message())?;
    }
    let mut cfg = state.config.write().unwrap();
    cfg.provider.chat_model = model.trim().to_string();
    cfg.save(&state.paths.config_file())
        .map_err(|e| e.user_message())
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
    let s = state
        .engine
        .usage_meter()
        .state(chrono::Utc::now())
        .map_err(|e| e.user_message())?;
    let remaining = (s.daily_limit - s.daily_used).max(0.0);
    Ok(serde_json::json!({
        "daily_used": s.daily_used, "daily_limit": s.daily_limit,
        "percent": s.percent, "blocked": s.blocked,
        "about_questions": remaining.floor()
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
fn create_task(state: TauriState<'_, AppState>, goal: String) -> Result<String, String> {
    let task = state.runner.create(&goal).map_err(|e| e.user_message())?;
    let id = task.id.clone();
    let runner = state.runner.clone();
    let cancel = CancelToken::default();
    std::thread::spawn(move || {
        let _ = runner.run(&id, &cancel);
    });
    Ok(task.id)
}

#[tauri::command]
fn approve_task(state: TauriState<'_, AppState>, id: String) -> Result<(), String> {
    state.runner.approve(&id).map_err(|e| e.user_message())?;
    let runner = state.runner.clone();
    let cancel = CancelToken::default();
    std::thread::spawn(move || {
        let _ = runner.run(&id, &cancel);
    });
    Ok(())
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
fn cancel_voice(state: TauriState<'_, AppState>) -> Result<(), String> {
    state.engine.cancel().map_err(|e| e.user_message())
}

#[tauri::command]
fn selftest(_state: TauriState<'_, AppState>) -> Result<serde_json::Value, String> {
    let gate = openclicky_core::privacy::CaptureGate::default();
    let guard = gate.authorize(CaptureReason::Calibration);
    let shots = XcapScreen
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
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            app_state,
            list_models,
            save_provider,
            ask_text,
            usage,
            tasks,
            create_task,
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

            let shortcut: Shortcut = "ctrl+alt+space".parse()?;
            let release_engine = engine.clone();
            let handle = app.handle().clone();
            let recording = Arc::from(AtomicBool::default());
            let trail = Arc::from(Mutex::from(openclicky_core::geom::Trail::default()));
            app.global_shortcut()
                .on_shortcut(shortcut, move |app, _s, event| match event.state() {
                    ShortcutState::Pressed => {
                        trail.lock().unwrap().points.clear();
                        recording.store(true, Ordering::SeqCst);
                        let flag = recording.clone();
                        let points = trail.clone();
                        let app_handle = app.clone();
                        std::thread::spawn(move || {
                            while flag.load(Ordering::SeqCst) {
                                if let Ok(p) = app_handle.cursor_position() {
                                    let scale = app_handle
                                        .primary_monitor()
                                        .ok()
                                        .flatten()
                                        .map(|m| m.scale_factor())
                                        .unwrap_or(1.0);
                                    points.lock().unwrap().push(openclicky_core::geom::Point {
                                        x: p.x / scale,
                                        y: p.y / scale,
                                    });
                                }
                                std::thread::sleep(std::time::Duration::from_millis(30));
                            }
                        });
                        match release_engine.on_hotkey_down() {
                            Ok(()) => {
                                let _ = app.emit("recording", true);
                            }
                            Err(e) => {
                                recording.store(false, Ordering::SeqCst);
                                let _ = app.emit("recording", false);
                                let _ = app.emit("error", e.user_message());
                            }
                        }
                    }
                    ShortcutState::Released => {
                        recording.store(false, Ordering::SeqCst);
                        let _ = app.emit("recording", false);
                        let engine = release_engine.clone();
                        let handle = handle.clone();
                        let sampled = trail.lock().unwrap().clone();
                        std::thread::spawn(move || {
                            match engine.on_hotkey_up_with_trail(Some(sampled)) {
                                Ok(o) => {
                                    let _ = handle.emit("answer", o.answer_text);
                                }
                                Err(e) => {
                                    let _ = handle.emit("error", e.user_message());
                                }
                            }
                        });
                    }
                })?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides it; the process keeps running so the
            // hotkey and tasks survive (F1: "关窗可召回").
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build openclicky desktop")
        .run(|app, event| {
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
}
