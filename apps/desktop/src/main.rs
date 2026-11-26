//! Desktop shell: windows, global hotkey and commands. No business rules here.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use openclicky_core::agent::runner::{self, AgentDeps, CancelToken};
use openclicky_core::agent::{Checkpoint, TaskRunner};
use openclicky_core::config::Config;
use openclicky_core::deps::Deps;
use openclicky_core::geom::PointerPlan;
use openclicky_core::memory::Memory;
use openclicky_core::ports::*;
use openclicky_core::privacy::CaptureReason;
use openclicky_core::providers;
use openclicky_core::router;
use openclicky_core::store::{Store, TaskRow};
use openclicky_core::tools::{Registry, Risk};
use openclicky_core::usage::UsageMeter;
use openclicky_core::voice::VoiceEngine;
use openclicky_core::AppEvent;
use openclicky_platform::audio::CpalAudio;
use openclicky_platform::capture::XcapScreen;
use openclicky_platform::commands::SystemCommands;
use openclicky_platform::http::UreqTransport;
use openclicky_platform::input::PlatformInjector;
use openclicky_platform::paths::Paths;
use openclicky_platform::permissions as platform_permissions;
use openclicky_platform::singleton::Singleton;
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
    /// Same registry the runner drives, kept here so the approval card can name
    /// the risk of the call that is waiting.
    tools: Arc<Registry>,
    /// One cancel flag per live task run. Without this `cancel_task` has nothing
    /// to signal, because every run used to build its own token.
    cancels: Arc<Mutex<HashMap<String, Arc<CancelToken>>>>,
}

#[derive(serde::Serialize)]
struct TaskView {
    id: String,
    name: String,
    state: String,
    result: Option<String>,
    waiting_since: Option<String>,
    pending: Option<PendingView>,
}

/// What a task is blocked on. One object instead of two parallel `Option`s so
/// the UI branches on exactly one field.
#[derive(serde::Serialize)]
struct PendingView {
    kind: &'static str,
    tool: Option<String>,
    risk: Option<&'static str>,
    summary: Option<String>,
    undoable: bool,
    question: Option<String>,
}

fn task_view(row: &TaskRow, tools: &Registry) -> TaskView {
    let checkpoint = serde_json::from_str::<Checkpoint>(&row.checkpoint_json).unwrap_or_default();
    let approval = checkpoint.pending_call.as_ref().map(|call| {
        let risk = tools.risk_of(&call.name, &call.args);
        PendingView {
            kind: "approval",
            tool: Some(call.name.clone()),
            risk: Some(risk.as_str()),
            summary: Some(openclicky_core::security::log_safe(
                &call.args.to_string(),
                200,
            )),
            undoable: !matches!(risk, Risk::Destructive | Risk::External),
            question: None,
        }
    });
    let question = checkpoint.pending_question.as_ref().map(|q| PendingView {
        kind: "question",
        tool: None,
        risk: None,
        summary: None,
        undoable: false,
        question: Some(q.clone()),
    });
    TaskView {
        id: row.id.clone(),
        name: row.name.clone(),
        state: row.state.clone(),
        result: row.result.clone(),
        waiting_since: row.waiting_since.map(|t| t.to_rfc3339()),
        pending: approval.or(question),
    }
}

/// Run a task on its own thread with a token the UI can reach later.
fn spawn_run(state: &AppState, id: String) {
    let token = Arc::new(CancelToken::new());
    state
        .cancels
        .lock()
        .unwrap()
        .insert(id.clone(), token.clone());
    let runner = state.runner.clone();
    let cancels = state.cancels.clone();
    std::thread::spawn(move || {
        let _ = runner.run(&id, &token);
        cancels.lock().unwrap().remove(&id);
    });
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

fn build_state(
    app: &tauri::AppHandle,
    paths: Paths,
) -> Result<AppState, Box<dyn std::error::Error>> {
    paths.ensure()?;
    let config = Config::load(&paths.config_file())?;
    let secrets = openclicky_platform::open_secrets(&paths.data_dir);
    let store = Arc::from(Store::open(&paths.db_file())?);
    let http: Arc<dyn HttpTransport> = Arc::from(UreqTransport::default());
    let memory = Arc::from(Memory::load(&paths.memory_dir())?);
    let config = Arc::from(RwLock::from(config));
    let tools: Arc<Registry> = Arc::from(Registry::default_set());

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
        Arc::from(platform_permissions::SystemProbe),
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
        tools: tools.clone(),
        clock,
        usage,
        memory,
        http: http.clone(),
        commands: Arc::from(SystemCommands),
        permissions: Arc::from(platform_permissions::SystemProbe),
        workspace: paths.workspace_dir(),
        model: if cfg.provider.deep_model.is_empty() {
            cfg.provider.chat_model.clone()
        } else {
            cfg.provider.deep_model.clone()
        },
        max_steps: cfg.tools.max_steps,
        allow_private_network: cfg.tools.allow_private_network,
        allow_domains: cfg.tools.allow_domains.clone(),
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
        tools,
        cancels: Arc::from(Mutex::new(HashMap::new())),
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
    let candidate = key.filter(|k| !k.trim().is_empty());
    let mut cfg = state.config.read().unwrap().clone();
    cfg.provider.chat_model = model.trim().to_string();
    let target = providers::describe_target(&cfg);

    // Prove the key and the model before either is written. A save that fails
    // must leave the previous, working configuration untouched, so nothing below
    // this point runs until the check passes.
    let checked = match candidate.as_deref() {
        Some(k) => providers::check_save(&cfg, state.http.clone(), Some(&k.into()), &model),
        None => match state.secrets.get(providers::KEY_NAME) {
            Ok(Some(stored)) => {
                providers::check_save(&cfg, state.http.clone(), Some(&stored), &model)
            }
            _ => Err(openclicky_core::Error::NoApiKey),
        },
    };
    checked.map_err(|e| format!("{} ({}) — {}", e.user_message(), e.code(), target))?;

    if let Some(k) = candidate {
        state
            .secrets
            .set(providers::KEY_NAME, &k.as_str().into())
            .map_err(|e| e.user_message())?;
        if !state.secrets.is_durable() {
            return Err(
                "这个系统没有接受写入钥匙串,key 会在这个进程结束时消失。点下面的按钮改为存到加密文件。(E_KEY_NOT_STORED)"
                    .to_string(),
            );
        }
    }
    cfg.save(&state.paths.config_file())
        .map_err(|e| e.user_message())?;
    *state.config.write().unwrap() = cfg;
    Ok(())
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
        .iter()
        .map(|row| task_view(row, &state.tools))
        .collect::<Vec<_>>()))
}

#[tauri::command]
fn task_detail(state: TauriState<'_, AppState>, id: String) -> Result<serde_json::Value, String> {
    let task = state.runner.get(&id).map_err(|e| e.user_message())?;
    let row = runner::to_row(&task);
    Ok(serde_json::json!(task_view(&row, &state.tools)))
}

#[tauri::command]
fn create_task(state: TauriState<'_, AppState>, goal: String) -> Result<String, String> {
    let task = state.runner.create(&goal).map_err(|e| e.user_message())?;
    spawn_run(&state, task.id.clone());
    Ok(task.id)
}

#[tauri::command]
fn approve_task(state: TauriState<'_, AppState>, id: String) -> Result<(), String> {
    state.runner.approve(&id).map_err(|e| e.user_message())?;
    spawn_run(&state, id);
    Ok(())
}

#[tauri::command]
fn deny_task(state: TauriState<'_, AppState>, id: String) -> Result<(), String> {
    state.runner.deny(&id).map_err(|e| e.user_message())
}

#[tauri::command]
fn answer_question(
    state: TauriState<'_, AppState>,
    id: String,
    answer: String,
) -> Result<(), String> {
    state
        .runner
        .answer_question(&id, &answer)
        .map_err(|e| e.user_message())?;
    spawn_run(&state, id);
    Ok(())
}

#[tauri::command]
fn cancel_task(state: TauriState<'_, AppState>, id: String) -> Result<(), String> {
    // Signal the loop first, then forget the token: `run` exits on the flag and
    // must not be able to miss it between these two steps.
    let mut cancels = state.cancels.lock().unwrap();
    if let Some(token) = cancels.get(&id) {
        token.cancel();
    }
    cancels.remove(&id);
    drop(cancels);
    state.runner.cancel(&id).map_err(|e| e.user_message())
}

#[tauri::command]
fn cancel_voice(state: TauriState<'_, AppState>) -> Result<(), String> {
    state.engine.cancel().map_err(|e| e.user_message())
}

fn permission_snapshot() -> serde_json::Value {
    let probe = platform_permissions::probe();
    serde_json::json!({
        "screen_recording": probe.screen_recording,
        "accessibility": probe.accessibility,
        "microphone": probe.microphone.as_str(),
        "guidance": platform_permissions::missing_guidance(probe),
    })
}

#[tauri::command]
fn permissions() -> serde_json::Value {
    permission_snapshot()
}

#[tauri::command]
fn request_permission(kind: String) -> serde_json::Value {
    match kind.as_str() {
        "screen_recording" => {
            platform_permissions::request_screen_recording();
        }
        "accessibility" => open_privacy_pane("Privacy_Accessibility"),
        // A refusal cannot be undone by asking again; only the settings row
        // can. A first run has never been asked, so the stream is what raises
        // the system prompt.
        "microphone" => match platform_permissions::probe().microphone {
            openclicky_core::ports::MicrophoneAccess::Denied => {
                open_privacy_pane("Privacy_Microphone")
            }
            openclicky_core::ports::MicrophoneAccess::NotDetermined => {
                platform_permissions::request_microphone(&CpalAudio::new());
            }
            openclicky_core::ports::MicrophoneAccess::Granted => {}
        },
        _ => {}
    }
    permission_snapshot()
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

#[cfg(target_os = "macos")]
fn open_privacy_pane(anchor: &str) {
    let _ = std::process::Command::new("open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.preference.security?{anchor}"
        ))
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn open_privacy_pane(_anchor: &str) {}

/// The recovery path when the system keychain refuses the write. Explicit,
/// because it puts the key on disk.
#[tauri::command]
fn allow_file_store(state: TauriState<'_, AppState>) -> Result<(), String> {
    state
        .secrets
        .allow_file_store()
        .map_err(|e| e.user_message())
}

fn reveal_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// The tray is the discoverable recall path; the global hotkey is the one that
/// always works, even where a tray cannot be shown.
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("OpenClicky")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal_main(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn main() {
    // A second instance would fight over the database, the tray and the global
    // hotkey. Exit silently instead of forking the state.
    let paths = Paths::resolve(None);
    let _instance = match Singleton::acquire(&paths.lock_file()) {
        Ok(lock) => lock,
        Err(_) => return,
    };
    // `withGlobalTauri` stays on in tauri.conf.json: ui/dist is a prebuilt bundle
    // that reaches Tauri through the injected `window.__TAURI__` global, not the
    // npm `@tauri-apps/api` import, so turning it off blanks both windows until
    // the frontend is rebuilt against the package.
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            app_state,
            list_models,
            save_provider,
            ask_text,
            usage,
            tasks,
            task_detail,
            answer_question,
            permissions,
            request_permission,
            allow_file_store,
            create_task,
            approve_task,
            deny_task,
            cancel_task,
            cancel_voice,
            selftest
        ])
        .setup(move |app| {
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_ignore_cursor_events(true);
                let _ = overlay.set_always_on_top(true);
            }
            let state = build_state(&app.handle().clone(), paths.clone())
                .map_err(|e| format!("startup: {e}"))?;
            let engine = state.engine.clone();
            let tasks = state.runner.clone();
            app.manage(state);

            // A task left `running` by a crash has no live heartbeat; a task that
            // has waited on a human for a day should release its queue slot.
            let now = chrono::Utc::now();
            let _ = tasks.recover_interrupted(now);
            let _ = tasks.expire_stale_waiting(now);
            let watchdog = tasks.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(600));
                let _ = watchdog.expire_stale_waiting(chrono::Utc::now());
            });

            // A missing tray cannot be fatal: the recall hotkey below works on
            // every platform and is the contract, the tray is the discoverable path.
            let _ = build_tray(app.handle());

            // Without a dock or tray the window must still be recallable.
            let recall: Shortcut = "ctrl+alt+o".parse()?;
            app.global_shortcut()
                .on_shortcut(recall, move |app, _s, event| match event.state() {
                    ShortcutState::Pressed => reveal_main(app),
                    ShortcutState::Released => {}
                })?;

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
                                // 「说一句就办」: an imperative becomes a task and the
                                // answer bubble stays empty, because a sentence like
                                // that has no answer to show.
                                Ok(o) if router::looks_like_task(&o.transcript) => {
                                    let state = handle.state::<AppState>();
                                    match state.runner.create(&o.transcript) {
                                        Ok(task) => spawn_run(&state, task.id.clone()),
                                        Err(e) => {
                                            let _ = handle.emit("error", e.user_message());
                                        }
                                    }
                                }
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
            // Reopen only exists on macOS; other platforms use the hotkey.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                reveal_main(app);
            }
            let _ = (app, event);
        });
}
