//! Desktop shell: windows, global hotkey and commands. No business rules here.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use openclicky_core::agent::runner::{self, AgentDeps, CancelToken};
use openclicky_core::agent::{Checkpoint, TaskRunner};
use openclicky_core::config::Config;
use openclicky_core::deps::Deps;
use openclicky_core::dictation::DictationEngine;
use openclicky_core::geom::PointerPlan;
use openclicky_core::hotkey::{Binding, Gesture, Modifier};
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
    dictation: Arc<DictationEngine>,
    /// Needed to replace a global shortcut when the config changes; a command
    /// only holds its own state.
    app: tauri::AppHandle,
    /// The dictation shortcut that is registered right now, so a save can take
    /// it back before installing the new one.
    dictation_shortcut: Arc<Mutex<Option<Shortcut>>>,
    /// Every problem `Config::validate` reports, recomputed on start-up and on a
    /// reload. Held here rather than derived per request because a file that no
    /// longer parses has to be able to say so, and that failure has no config to
    /// derive it from.
    issues: Arc<RwLock<Vec<String>>>,
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
    /// Which plan is on screen. A clear scheduled for one plan must not erase a
    /// newer one, so the plan that is current when the timer fires decides.
    current: Arc<Mutex<u64>>,
}

impl WindowOverlay {
    fn emit_clear(&self) {
        let _ = self.app.emit("pointer-clear", ());
    }
}

impl PointerOverlay for WindowOverlay {
    fn show(&self, plan: &PointerPlan) -> openclicky_core::Result<()> {
        *self.current.lock().unwrap() += 1;
        let payload = serde_json::to_value(plan).unwrap_or(serde_json::Value::Null);
        let _ = self.app.emit("pointer-plan", payload);
        Ok(())
    }
    fn clear(&self) -> openclicky_core::Result<()> {
        *self.current.lock().unwrap() += 1;
        self.emit_clear();
        Ok(())
    }
    /// The sleep runs on this thread rather than blocking the ask, which has
    /// already returned by the time it starts.
    fn clear_after(&self, ms: u64) -> openclicky_core::Result<()> {
        let mine = *self.current.lock().unwrap();
        let current = self.current.clone();
        let app = self.app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            if *current.lock().unwrap() == mine {
                let _ = app.emit("pointer-clear", ());
            }
        });
        Ok(())
    }
}

fn build_state(
    app: &tauri::AppHandle,
    paths: Paths,
) -> Result<AppState, Box<dyn std::error::Error>> {
    paths.ensure()?;
    let config = Config::load(&paths.config_file())?;
    // Reported, not enforced: a config with a missing model still starts, and the
    // banner is how the user finds out why nothing works.
    let issues = config_issues(&paths.config_file(), &config);
    let secrets = openclicky_platform::open_secrets(&paths.data_dir);
    let store = Arc::from(Store::open(&paths.db_file())?);
    let http: Arc<dyn HttpTransport> = Arc::from(UreqTransport::default());
    let memory = Arc::from(Memory::load(&paths.memory_dir())?);
    let config = Arc::from(RwLock::from(config));
    let tools: Arc<Registry> = Arc::from(Registry::default_set());

    // The banner is the one surface that says what the app cannot do, and the
    // engine has no way to reach it. A notice that needs the user to act is
    // therefore copied here as well as emitted to the front end.
    let issues: Arc<RwLock<Vec<String>>> = Arc::from(RwLock::from(issues));
    let banner = issues.clone();
    let handle = app.clone();
    let events = Arc::from(move |event: AppEvent| {
        if let AppEvent::Notice { code, message } = &event {
            if code.starts_with("E_INJECT_") {
                let mut lines = banner.write().unwrap();
                let line = format!("{message} ({code})");
                if !lines.contains(&line) {
                    lines.push(line);
                }
            }
        }
        let _ = handle.emit(
            "app-event",
            serde_json::to_value(&event).unwrap_or_default(),
        );
    });

    let deps = Deps::new(
        Arc::new(openclicky_core::clock::SystemClock),
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
        Arc::from(WindowOverlay {
            app: app.clone(),
            current: Arc::from(Mutex::new(0)),
        }),
        Arc::from(PlatformInjector),
        Arc::from(platform_permissions::SystemProbe),
        store.clone(),
        memory.clone(),
        config.clone(),
        events.clone(),
    );
    // The engine and the window must read the same lock, so take it from deps
    // before deps is handed to the dictation engine.
    let config = deps.config.clone();
    let engine = Arc::from(VoiceEngine::new(deps.clone()));
    let dictation = Arc::from(DictationEngine::new(deps));

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
        dictation,
        app: app.clone(),
        dictation_shortcut: Arc::from(Mutex::new(None)),
        issues,
    })
}

/// The payload `app_state` and `reload_config` both answer with. Pure, so the
/// banner's contract is testable without a window: every problem, in the order
/// `validate` found them, and an empty list when the config is usable.
fn app_state_view(
    state_name: &str,
    config: &Config,
    issues: &[String],
    has_key: bool,
    paths: &Paths,
) -> serde_json::Value {
    serde_json::json!({
        "state": state_name,
        "has_key": has_key,
        "ready": has_key && !config.provider.chat_model.is_empty(),
        "model": config.provider.chat_model,
        "kind": config.provider.kind.as_str(),
        "base_url": config.provider.base_url,
        "issues": issues,
        "budget": { "daily": config.budget.daily_units, "monthly": config.budget.monthly_units },
        "config_path": paths.config_file().to_string_lossy(),
        "data_dir": paths.data_dir.to_string_lossy(),
    })
}

fn has_key(state: &AppState) -> bool {
    matches!(state.secrets.get(providers::KEY_NAME), Ok(Some(_)))
}

#[tauri::command]
fn app_state(state: TauriState<'_, AppState>) -> serde_json::Value {
    let config = state.config.read().unwrap();
    let issues = state.issues.read().unwrap();
    app_state_view(
        state.engine.state().as_str(),
        &config,
        &issues,
        has_key(&state),
        &state.paths,
    )
}

/// Read the file from disk again.
///
/// #65 cannot be closed by a restart, so the reload has to be reachable from the
/// banner that reports the problem. A file that no longer parses must not replace
/// the config the app is running on: the parse error becomes the issue and the
/// previous values survive until the file is valid again.
fn reload_from_disk(path: &Path, config: &mut Config) -> Vec<String> {
    match Config::load(path) {
        Ok(next) => {
            *config = next;
            config_issues(path, config)
        }
        Err(e) => vec![format!("配置读不了：{}", e.user_message())],
    }
}

/// What is wrong with the file at `path`, given the config it produced.
///
/// A machine that has never been configured has no file, and `Config::load`
/// answers with the default, whose empty `chat_model` fails validation. Reporting
/// that would put a banner above the first-run card saying the same thing, on a
/// first launch that has done nothing wrong. The banner is for a file that exists
/// and is wrong.
///
/// Decided from the file's existence rather than from whether the first-run card
/// is showing: two states that depend on each other are two states that can
/// disagree.
fn config_issues(path: &Path, config: &Config) -> Vec<String> {
    if path.exists() {
        config.validate()
    } else {
        Vec::new()
    }
}

#[tauri::command]
fn reload_config(state: TauriState<'_, AppState>) {
    let mut next = state.config.read().unwrap().clone();
    let issues = reload_from_disk(&state.paths.config_file(), &mut next);
    *state.config.write().unwrap() = next;
    *state.issues.write().unwrap() = issues;
}

/// Where a save or a model list goes.
///
/// The settings field is prefilled with the live address, so an empty field is a
/// deliberate reset to the kind's own default rather than "leave it alone".
/// `None` means the caller offered no field at all, and the config keeps what it
/// has. `#143` was the opposite of this: the model changed and the address did
/// not, so a leftover test endpoint kept answering.
fn apply_provider_target(config: &mut Config, typed: Option<&str>) {
    let Some(typed) = typed else { return };
    let trimmed = typed.trim().trim_end_matches('/');
    config.provider.base_url = if trimmed.is_empty() {
        config.provider.kind.default_base_url().to_string()
    } else {
        trimmed.to_string()
    };
}

#[tauri::command]
fn list_models(
    state: TauriState<'_, AppState>,
    key: Option<String>,
    base_url: Option<String>,
) -> Result<Vec<String>, String> {
    if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
        state
            .secrets
            .set(providers::KEY_NAME, &k.as_str().into())
            .map_err(|e| e.user_message())?;
    }
    let mut cfg = state.config.read().unwrap().clone();
    apply_provider_target(&mut cfg, base_url.as_deref());
    let target = providers::describe_target(&cfg);
    providers::build(&cfg, state.http.clone(), state.secrets.clone())
        .and_then(|p| p.list_models())
        .map_err(|e| format!("{} ({}) — {}", e.user_message(), e.code(), target))
}

#[tauri::command]
fn save_provider(
    state: TauriState<'_, AppState>,
    key: Option<String>,
    model: String,
    base_url: Option<String>,
) -> Result<(), String> {
    let candidate = key
        .filter(|k| !k.trim().is_empty())
        .map(|k| k.as_str().into());
    let mut cfg = state.config.read().unwrap().clone();
    apply_provider_target(&mut cfg, base_url.as_deref());
    // After the address has been applied: a refusal has to name the endpoint that
    // refused, not the one that was in the file before.
    let target = providers::describe_target(&cfg);
    providers::save_provider(
        &mut cfg,
        &state.paths.config_file(),
        state.http.clone(),
        state.secrets.as_ref(),
        candidate.as_ref(),
        &model,
    )
    .map_err(|e| format!("{} ({}) — {}", e.user_message(), e.code(), target))?;
    *state.config.write().unwrap() = cfg;
    // A save that leaves the key in memory looks identical to a good save until
    // the next restart, and then the key is gone with nothing said. Say it here,
    // on the banner the user already reads, with the backend's own words.
    if candidate.is_some() && !state.secrets.is_durable() {
        let why = state.secrets.degraded_reason().unwrap_or_else(|| {
            concat!("the system ", "keychain", " refused the write").to_string()
        });
        let line = format!("key 只在这个进程里有效，重启就要重新粘：{why}");
        let mut issues = state.issues.write().unwrap();
        if !issues.contains(&line) {
            issues.push(line);
        }
    }
    reregister_dictation(&state, &model);
    Ok(())
}

/// The config file is the source of truth for shortcuts, so a save has to take
/// effect without a restart. Re-registering is best effort: a binding the
/// system refuses leaves the previous one unbound rather than failing the save,
/// which has already happened by the time this runs.
fn reregister_dictation(state: &AppState, _saved_model: &str) {
    let requested = state.config.read().unwrap().shortcuts.dictation.clone();
    let previous = *state.dictation_shortcut.lock().unwrap();
    let Some(shortcut) = previous else { return };
    if shortcut.to_string().eq_ignore_ascii_case(&requested) {
        return;
    }
    let (maybe_next, fell_back) = dictation_shortcut(&requested);
    let Some(next) = maybe_next else { return };
    if next == shortcut {
        return;
    }
    let _ = state.app.global_shortcut().unregister(shortcut);
    match install_dictation_shortcut(&state.app, next, state.dictation.clone()) {
        Ok(()) => {
            *state.dictation_shortcut.lock().unwrap() = Some(next);
            if fell_back {
                let _ = state.app.emit(
                    "error",
                    format!("听写键 {requested} 系统不认，已改用 {DICTATION_FALLBACK}。"),
                );
            }
        }
        Err(e) => {
            let _ = state
                .app
                .emit("error", format!("听写键改成 {requested} 失败：{e}"));
        }
    }
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

/// Asking can block for a few hundred milliseconds while an input stream opens,
/// and a synchronous command runs on the window thread, so this one is async.
/// What it returns is an outcome, not a state: a system prompt is answered
/// later, and a probe taken while it is still on screen reports the old answer.
#[tauri::command]
async fn request_permission(kind: String) -> Result<serde_json::Value, String> {
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        platform_permissions::request(&kind, &CpalAudio::new())
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "outcome": outcome.as_str(),
        "permissions": permission_snapshot(),
    }))
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

/// The recovery path when the system keychain refuses the write. Explicit,
/// because it puts the key on disk.
#[tauri::command]
fn allow_file_store(state: TauriState<'_, AppState>) -> Result<(), String> {
    state
        .secrets
        .allow_file_store()
        .map_err(|e| e.user_message())
}

/// Tauri's own parser cannot express everything core can, and the default
/// dictation binding `fn+control` is the first example: `fn` is not a modifier
/// Tauri knows. Translate the parsed binding into a string Tauri accepts, and
/// answer `None` when the shape cannot be expressed, so the caller can degrade
/// instead of failing to start.
fn tauri_binding(binding: &Binding) -> Option<String> {
    if binding.gesture == Gesture::DoubleTap || binding.key.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for modifier in &binding.modifiers {
        parts.push(match modifier {
            Modifier::Control => "ctrl",
            Modifier::Alt => "alt",
            Modifier::Shift => "shift",
            Modifier::Meta => "super",
            Modifier::Fn => return None,
        });
    }
    parts.push(binding.key.as_str());
    Some(parts.join("+"))
}

const DICTATION_FALLBACK: &str = "ctrl+alt+d";

/// The shortcut to register and whether it is the fallback. Never fails: an
/// unparseable config must not stop the app from starting.
fn dictation_shortcut(spec: &str) -> (Option<Shortcut>, bool) {
    if let Some(text) = Binding::parse(spec).ok().as_ref().and_then(tauri_binding) {
        if let Ok(shortcut) = text.parse::<Shortcut>() {
            return (Some(shortcut), false);
        }
    }
    (DICTATION_FALLBACK.parse::<Shortcut>().ok(), true)
}

/// Press and hold to dictate. The ticker stops on release, and also when the
/// budget runs out, because a blocked engine would otherwise keep transcribing.
fn install_dictation_shortcut(
    app: &tauri::AppHandle,
    shortcut: Shortcut,
    dictation: Arc<DictationEngine>,
) -> Result<(), String> {
    let held = Arc::from(AtomicBool::new(false));
    let release_held = held.clone();
    let release_dictation = dictation.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _s, event| match event.state() {
            ShortcutState::Pressed => {
                if let Err(e) = dictation.start() {
                    let _ = app.emit("error", e.user_message());
                    return;
                }
                held.store(true, Ordering::SeqCst);
                let flag = held.clone();
                let ticker = dictation.clone();
                let handle = app.clone();
                std::thread::spawn(move || {
                    while flag.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(700));
                        if !flag.load(Ordering::SeqCst) {
                            break;
                        }
                        if let Err(e) = ticker.tick() {
                            flag.store(false, Ordering::SeqCst);
                            let _ = handle.emit("error", e.user_message());
                        }
                    }
                });
            }
            ShortcutState::Released => {
                release_held.store(false, Ordering::SeqCst);
                match release_dictation.finish() {
                    // A tap too short to contain a word is not an error worth
                    // showing; anything else is.
                    Err(openclicky_core::Error::TooShort) | Ok(_) => {}
                    Err(e) => {
                        let _ = app.emit("error", e.user_message());
                    }
                }
            }
        })
        .map_err(|e| e.to_string())
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
            reload_config,
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
                // `fullscreen: true` ran a native fullscreen transition during
                // start-up and left the window out of the window list entirely:
                // bounds and alpha were right, `onscreen` was false, and the
                // canvas never reached the display. Covering the monitor by hand
                // gets the same geometry without the transition.
                if let Ok(Some(monitor)) = overlay.primary_monitor() {
                    let size = monitor.size();
                    let origin = monitor.position();
                    let _ = overlay.set_position(tauri::PhysicalPosition::new(origin.x, origin.y));
                    let _ = overlay.set_size(tauri::PhysicalSize::new(size.width, size.height));
                }
                // Measured with a CGWindowList sampler: the level goes 0, then 5
                // when it is set here, then back to 0 about 20ms after this
                // window's first resize burst at start-up, and nothing touches it
                // again for the rest of the session. Pinning it after that burst
                // is the whole fix.
                let pinned = overlay.clone();
                overlay.on_window_event(move |event| {
                    if let tauri::WindowEvent::Resized(_) = event {
                        let _ = pinned.set_always_on_top(true);
                    }
                });
            }
            let state = build_state(&app.handle().clone(), paths.clone())
                .map_err(|e| format!("startup: {e}"))?;
            let engine = state.engine.clone();
            let tasks = state.runner.clone();
            let dictation = state.dictation.clone();
            let registered_dictation = state.dictation_shortcut.clone();
            let config = state.config.clone();
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

            // Dictation is registered before the tray for the same reason the
            // tray is optional: a shortcut that cannot be bound is a degraded
            // feature, not a failed start. No `?` on this path.
            let requested = config
                .read()
                .unwrap()
                .shortcuts
                .dictation
                .clone();
            let (maybe_shortcut, fell_back) = dictation_shortcut(&requested);
            if let Some(shortcut) = maybe_shortcut {
                match install_dictation_shortcut(app.handle(), shortcut, dictation.clone()) {
                    Ok(()) => {
                        *registered_dictation.lock().unwrap() = Some(shortcut);
                        if fell_back {
                            let _ = app.emit(
                                "error",
                                format!(
                                    "听写键 {requested} 系统不认，已改用 {DICTATION_FALLBACK}。可在配置里改成别的组合。"
                                ),
                            );
                        }
                    }
                    Err(e) => {
                        let _ = app.emit("error", format!("听写键注册失败：{e}"));
                    }
                }
            }

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

#[cfg(test)]
mod config_issue_tests {
    use super::*;
    use std::path::PathBuf;

    /// `Config::default()` has no chat model, so it is not usable yet. This is
    /// the smallest config that reports nothing.
    fn usable() -> Config {
        let mut cfg = Config::default();
        cfg.provider.chat_model = "gpt-4o-mini".into();
        cfg
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openclicky-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.toml")
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn every_problem_is_reported_not_only_the_first() {
        let mut cfg = usable();
        cfg.provider.base_url = String::new();
        cfg.voice.speech_speed = 3.0;
        let issues = cfg.validate();
        assert!(
            issues.len() >= 2,
            "a banner that shows one line hides the other problems: {issues:?}"
        );
        assert!(issues.iter().any(|i| i.contains("base_url")), "{issues:?}");
        assert!(
            issues.iter().any(|i| i.contains("speech_speed")),
            "{issues:?}"
        );
    }

    #[test]
    fn fixing_the_file_clears_the_issues_without_a_restart() {
        let path = scratch("reload");
        let mut broken = usable();
        broken.provider.base_url = String::new();
        broken.save(&path).unwrap();

        let mut running = usable();
        let issues = reload_from_disk(&path, &mut running);
        assert!(
            issues.iter().any(|i| i.contains("base_url")),
            "an empty base_url has to be reported: {issues:?}"
        );
        assert!(running.provider.base_url.is_empty());

        usable().save(&path).unwrap();
        let issues = reload_from_disk(&path, &mut running);
        assert!(issues.is_empty(), "the banner must go away: {issues:?}");
        assert!(
            running.provider.base_url.starts_with("http"),
            "reloading has to hand back what is on disk, not what was in memory"
        );
        cleanup(&path);
    }

    #[test]
    fn a_first_run_with_no_config_file_shows_no_banner() {
        let path = scratch("first-run");
        let mut running = Config::default();

        let issues = reload_from_disk(&path, &mut running);
        assert!(
            issues.is_empty(),
            "nothing is configured yet, and the first-run card already says so: {issues:?}"
        );

        Config::default().save(&path).unwrap();
        let issues = reload_from_disk(&path, &mut running);
        assert!(
            issues.iter().any(|i| i.contains("模型")),
            "once the file is there, the missing model is the file's problem: {issues:?}"
        );
        cleanup(&path);
    }

    #[test]
    fn a_file_that_no_longer_parses_is_the_issue_and_the_old_config_survives() {
        let path = scratch("unparsable");
        std::fs::write(&path, "this is not toml {{{").unwrap();
        let mut running = usable();
        let issues = reload_from_disk(&path, &mut running);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("配置读不了"), "{issues:?}");
        assert_eq!(
            running.provider.chat_model, "gpt-4o-mini",
            "a typo must not wipe the settings the app is running on"
        );
        cleanup(&path);
    }

    #[test]
    fn app_state_carries_every_issue() {
        let mut cfg = usable();
        cfg.provider.base_url = String::new();
        cfg.voice.speech_speed = 3.0;
        let issues = cfg.validate();
        let paths = Paths::resolve(Some(&std::env::temp_dir()));

        let view = app_state_view("idle", &cfg, &issues, true, &paths);
        assert_eq!(
            view["issues"].as_array().map(|a| a.len()),
            Some(issues.len()),
            "the banner reads this array: {view}"
        );
        assert!(
            view["issues"].to_string().contains("base_url"),
            "the line the user needs is the one naming the field: {view}"
        );

        let clean = app_state_view("idle", &usable(), &[], true, &paths);
        assert_eq!(clean["issues"].as_array().map(|a| a.len()), Some(0));
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn the_default_dictation_binding_falls_back_because_tauri_has_no_fn() {
        let (shortcut, fell_back) = dictation_shortcut("fn+control");
        assert!(fell_back, "fn cannot be expressed, so this must degrade");
        assert_eq!(shortcut, DICTATION_FALLBACK.parse::<Shortcut>().ok());
    }

    #[test]
    fn a_binding_tauri_can_express_is_used_as_written() {
        let (shortcut, fell_back) = dictation_shortcut("ctrl+alt+d");
        assert!(!fell_back);
        assert!(shortcut.is_some());
    }

    #[test]
    fn a_double_tap_gesture_is_not_a_global_shortcut() {
        let binding = Binding::parse("double:ctrl+k").unwrap();
        assert_eq!(tauri_binding(&binding), None);
    }

    #[test]
    fn modifiers_are_translated_to_the_names_tauri_parses() {
        let binding = Binding::parse("cmd+shift+k").unwrap();
        assert_eq!(tauri_binding(&binding).as_deref(), Some("super+shift+k"));
    }
}

#[cfg(test)]
mod provider_target_tests {
    use super::*;
    use openclicky_core::config::ProviderKind;

    fn stale() -> Config {
        let mut cfg = Config::default();
        cfg.provider.base_url = "http://127.0.0.1:8787/v1".into();
        cfg.provider.chat_model = "gpt-4o".into();
        cfg
    }

    #[test]
    fn a_typed_address_replaces_the_stale_one() {
        let mut cfg = stale();
        apply_provider_target(&mut cfg, Some(" https://gateway.example/v1/ "));
        assert_eq!(cfg.provider.base_url, "https://gateway.example/v1");
        assert_eq!(
            providers::describe_target(&cfg),
            "openai @ https://gateway.example/v1",
            "the settings page has to be able to name where requests go"
        );
    }

    #[test]
    fn an_empty_address_is_a_deliberate_reset_to_the_kind_default() {
        let mut cfg = stale();
        apply_provider_target(&mut cfg, Some("   "));
        assert_eq!(
            cfg.provider.base_url,
            ProviderKind::Openai.default_base_url(),
            "an empty field is not 'keep the old one'; the field arrives prefilled"
        );
    }

    #[test]
    fn no_field_at_all_keeps_what_the_config_has() {
        let mut cfg = stale();
        apply_provider_target(&mut cfg, None);
        assert_eq!(cfg.provider.base_url, "http://127.0.0.1:8787/v1");
    }

    #[test]
    fn a_custom_kind_resets_to_its_own_default_not_openais() {
        let mut cfg = stale();
        cfg.provider.kind = ProviderKind::OpenaiCompatible;
        apply_provider_target(&mut cfg, Some(""));
        assert_eq!(cfg.provider.base_url, "http://localhost:1234/v1");
    }
}
