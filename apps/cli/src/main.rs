//! `openclicky` — headless entry point and diagnostics. No business rules here.

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};

use openclicky_core::agent::runner::{AgentDeps, CancelToken};
use openclicky_core::agent::TaskRunner;
use openclicky_core::config::Config;
use openclicky_core::deps::Deps;
use openclicky_core::dictation::DictationEngine;
use openclicky_core::memory::Memory;
use openclicky_core::ports::*;
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
use openclicky_platform::singleton::Singleton;
use openclicky_platform::speech::SystemSpeech;

/// A smoke report has to name the build it ran against. The commit and the
/// build stamp come from `build.rs`, which reads git and falls back to
/// "unknown" in a source tarball.
const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (commit ",
    env!("OPENCLICKY_COMMIT"),
    ", built at unix ",
    env!("OPENCLICKY_BUILT_AT"),
    ")"
);

#[derive(Parser)]
#[command(
    name = "openclicky",
    version,
    long_version = LONG_VERSION,
    about = "an ai buddy that lives on your computer"
)]
struct Cli {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Doctor {
        #[arg(long)]
        online: bool,
    },
    Ask {
        #[arg(long)]
        text: String,
    },
    Voice {
        #[arg(long, default_value_t = 5)]
        seconds: u64,
    },
    Task {
        goal: String,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    Selftest {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Dictate {
        #[arg(long, default_value_t = 5)]
        seconds: u64,
    },
    SetKey {
        #[arg(long, default_value = "provider/default")]
        name: String,
        /// Store the key in this data directory's encrypted file instead of the
        /// system keychain. Needed where the keychain refuses the write, which
        /// otherwise leaves the key alive only until the process exits.
        #[arg(long)]
        remember_file: bool,
        /// Skip the live check. A key that cannot be verified is a key that will
        /// fail later, so this exists for offline setup and nothing else.
        #[arg(long)]
        no_check: bool,
    },
    Usage,
    /// Show what OpenClicky remembers about you.
    Memory,
    /// Remember a preference, verbatim.
    Remember {
        text: String,
    },
    /// Forget every remembered line containing this text.
    Forget {
        needle: String,
    },
    /// Permanently delete all local OpenClicky data (8 ordered steps).
    Delete {
        #[arg(long)]
        yes: bool,
    },
    /// Start the desktop shell.
    App {
        /// Keep the shell in the foreground so its output stays visible.
        #[arg(long)]
        wait: bool,
    },
}

struct PrintOverlay;

impl PointerOverlay for PrintOverlay {
    fn show(&self, plan: &openclicky_core::geom::PointerPlan) -> openclicky_core::Result<()> {
        for item in &plan.items {
            if let Some(t) = item.target {
                println!(
                    "point screen{} at ({:.0}, {:.0}) {}",
                    item.screen,
                    t.x,
                    t.y,
                    item.label.clone().unwrap_or_default()
                );
            }
            for s in &item.strokes {
                println!("draw {:?} with {} points", s.kind, s.points.len());
            }
        }
        Ok(())
    }
    fn clear_after(&self, ms: u64) -> openclicky_core::Result<()> {
        println!("pointer clears in {ms}ms");
        Ok(())
    }
    fn clear(&self) -> openclicky_core::Result<()> {
        Ok(())
    }
}

struct App {
    paths: Paths,
    config: Config,
    secrets: Arc<PlatformSecrets>,
    store: Arc<Store>,
    http: Arc<dyn HttpTransport>,
}

fn load(data_dir: Option<PathBuf>) -> anyhow::Result<App> {
    let paths = Paths::resolve(data_dir.as_deref());
    paths.ensure().context("creating data directory")?;
    let config = Config::load(&paths.config_file())?;
    let secrets = Arc::new(PlatformSecrets::new(&paths.data_dir));
    let store = Arc::new(Store::open(&paths.db_file())?);
    let http: Arc<dyn HttpTransport> = Arc::new(UreqTransport::new());
    Ok(App {
        paths,
        config,
        secrets,
        store,
        http,
    })
}

impl App {
    fn provider(&self) -> anyhow::Result<Arc<dyn ChatProvider>> {
        Ok(providers::build(
            &self.config,
            self.http.clone(),
            self.secrets.clone(),
        )?)
    }

    fn stt(&self) -> anyhow::Result<Arc<dyn Transcriber>> {
        let key = self
            .secrets
            .get(providers::KEY_NAME)?
            .ok_or_else(|| anyhow!("no api key; run: openclicky set-key"))?;
        Ok(Arc::new(providers::openai::OpenAiStt::new(
            self.http.clone(),
            self.config.clone(),
            key,
        )))
    }

    fn deps(&self) -> anyhow::Result<(Arc<Deps>, Arc<VoiceEngine>, Arc<Memory>)> {
        let memory = Arc::new(Memory::load(&self.paths.memory_dir())?);
        let stt = self.stt()?;
        let chat = self.provider()?;
        let events = Arc::new(|e: AppEvent| match e {
            AppEvent::Notice { code, message } => eprintln!("[{code}] {message}"),
            AppEvent::TaskUpdated { state, message, .. } => eprintln!("[{state:?}] {message}"),
            _ => {}
        });
        let deps = Deps::new(
            Arc::new(openclicky_core::clock::SystemClock),
            Arc::new(XcapScreen::new()),
            Arc::new(CpalAudio::new()),
            stt,
            chat,
            Arc::new(SystemSpeech),
            Arc::new(PrintOverlay),
            Arc::new(PlatformInjector::new()),
            Arc::new(permissions::SystemProbe),
            self.store.clone(),
            memory.clone(),
            Arc::new(std::sync::RwLock::new(self.config.clone())),
            events,
        );
        let engine = Arc::new(VoiceEngine::new(deps.clone()));
        Ok((deps, engine, memory))
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let app = load(cli.data_dir.clone())?;
    match cli.command {
        Command::Doctor { online } => doctor(&app, online),
        Command::Ask { text } => {
            let (_deps, engine, _mem) = app.deps()?;
            let outcome = engine.ask_typed(&text)?;
            println!("{}", outcome.answer_text);
            println!(
                "---\nmodel: {}   units: {:.1}",
                outcome.model, outcome.charged_units
            );
            Ok(())
        }
        Command::Voice { seconds } => {
            let (_deps, engine, _mem) = app.deps()?;
            println!("recording {seconds}s ...");
            engine.on_hotkey_down()?;
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            let outcome = engine.on_hotkey_up()?;
            println!("you: {}", outcome.transcript);
            println!("{}", outcome.answer_text);
            Ok(())
        }
        Command::Task { goal, dry_run } => task(&app, &goal, dry_run),
        Command::Memory => {
            let memory = Memory::load(&app.paths.memory_dir())?;
            let profile = memory.profile_text();
            let projects = memory.projects_text();
            println!("preferences:");
            println!(
                "{}",
                if profile.trim().is_empty() {
                    "  (nothing yet)"
                } else {
                    profile.trim()
                }
            );
            println!("ongoing:");
            println!(
                "{}",
                if projects.trim().is_empty() {
                    "  (nothing yet)"
                } else {
                    projects.trim()
                }
            );
            if memory.profile_over_budget() {
                println!("note: preferences are over budget and will be compacted");
            }
            Ok(())
        }
        Command::Remember { text } => {
            if text.trim().is_empty() {
                return Err(anyhow!("nothing to remember"));
            }
            let memory = Memory::load(&app.paths.memory_dir())?;
            memory.add_preference(text.trim())?;
            println!("remembered: {}", text.trim());
            Ok(())
        }
        Command::Forget { needle } => {
            let memory = Memory::load(&app.paths.memory_dir())?;
            let removed = memory.forget(&needle)?;
            println!("forgot {removed} line(s) matching {needle:?}");
            Ok(())
        }
        Command::Dictate { seconds } => {
            let (deps, _engine, _mem) = app.deps()?;
            let dict = DictationEngine::new(deps);
            dict.start()?;
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            let out = dict.finish()?;
            println!("{}", out.final_text);
            Ok(())
        }
        Command::Selftest { out } => selftest(&app, out),
        Command::SetKey {
            name,
            remember_file,
            no_check,
        } => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            let key = buf.trim();
            if key.is_empty() {
                return Err(anyhow!("empty key on stdin"));
            }
            if remember_file {
                app.secrets.allow_file_store()?;
            }
            let target = providers::describe_target(&app.config);
            if !no_check {
                let wanted = app.config.provider.chat_model.clone();
                match providers::check_save(
                    &app.config,
                    app.http.clone(),
                    Some(&key.into()),
                    &wanted,
                ) {
                    Ok(_) if wanted.trim().is_empty() => {
                        println!("key accepted by {target} (还没有配置模型)")
                    }
                    Ok(_) => println!("key accepted by {target}"),
                    Err(e) => {
                        return Err(anyhow!("{} ({}) — {}", e.user_message(), e.code(), target))
                    }
                }
            }
            app.secrets.set(&name, &key.into())?;
            if !app.secrets.is_durable() {
                // The reason, not just the outcome: "refused" was the same
                // sentence for a locked keychain, a refused ACL, and a missing
                // backend, and none of them are fixed the same way.
                let why = app
                    .secrets
                    .degraded_reason()
                    .unwrap_or_else(|| "no reason reported".to_string());
                return Err(anyhow!(
                    "the key was not stored ({why}). It would vanish when this process exits. Re-run with --remember-file to keep it in an encrypted file in the data directory."
                ));
            }
            println!("saved {name}");
            Ok(())
        }
        Command::App { wait } => open_app(wait),
        Command::Delete { yes } => {
            if !yes {
                println!("this removes all local data; re-run with --yes");
                return Ok(());
            }
            let paths = openclicky_core::deletion::DeletionPaths {
                memory_dir: app.paths.memory_dir(),
                workspace_dir: app.paths.workspace_dir(),
                log_dir: app.paths.data_dir.join("logs"),
                db_file: app.paths.db_file(),
            };
            let report =
                openclicky_core::deletion::execute(&paths, &app.store, app.secrets.as_ref());
            for step in &report.steps {
                println!(
                    "{} {} {}",
                    if step.ok { "ok  " } else { "FAIL" },
                    step.step,
                    step.detail
                );
            }
            for scope in &report.out_of_scope {
                println!("out of scope: {scope}");
            }
            if !report.all_ok() {
                return Err(anyhow!("some steps failed: {:?}", report.failures()));
            }
            Ok(())
        }
        Command::Usage => {
            let meter = UsageMeter::new(
                app.store.clone(),
                Arc::new(openclicky_core::clock::SystemClock),
                app.config.budget.weights.clone(),
                app.config.budget.daily_units,
                app.config.budget.monthly_units,
            );
            let s = meter.state(chrono::Utc::now())?;
            println!(
                "today {:.1}/{:.1} ({:.0}%)  month {:.1}/{:.1}",
                s.daily_used, s.daily_limit, s.percent, s.monthly_used, s.monthly_limit
            );
            Ok(())
        }
    }
}

fn doctor(app: &App, online: bool) -> anyhow::Result<()> {
    println!(
        "openclicky {} ({}) commit {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        env!("OPENCLICKY_COMMIT")
    );
    println!("config: {}", app.paths.config_file().display());
    println!("data:   {}", app.paths.data_dir.display());
    let issues = app.config.validate();
    if issues.is_empty() {
        println!("config: ok");
    } else {
        for i in &issues {
            println!("config: {i}");
        }
    }
    let key = app.secrets.get(providers::KEY_NAME)?;
    println!(
        "api key: {}",
        if key.is_some() { "present" } else { "missing" }
    );
    let fallback = app.paths.data_dir.join("secrets.enc");
    if app.secrets.is_degraded() {
        match app.secrets.degraded_reason() {
            Some(why) => println!("key store: memory only — {why}"),
            None => println!("key store: memory only — the system keychain refused it"),
        }
        println!("           the key is gone when the process exits, so set it again next launch");
        println!("           to keep it on disk: openclicky set-key --remember-file");
    } else if fallback.exists() {
        println!(
            "key store: encrypted file (0600) — the file key comes from a salt stored beside it"
        );
        println!("           this keeps the key off the command line, not off the disk");
    } else {
        println!("key store: system keychain");
    }
    let perms = permissions::probe();
    println!(
        "screen recording: {}",
        if perms.screen_recording {
            "ok"
        } else {
            "missing"
        }
    );
    println!(
        "accessibility:    {}",
        if perms.accessibility { "ok" } else { "missing" }
    );
    println!("microphone:       {}", perms.microphone.as_str());
    for hint in permissions::missing_guidance(perms) {
        println!("  -> {hint}");
    }
    match XcapScreen::new().displays() {
        Ok(ds) => {
            println!("displays: {}", ds.len());
            for d in ds {
                println!(
                    "  screen{} {}x{} @ {:.2} origin ({:.0},{:.0}){}",
                    d.index,
                    d.width_px(),
                    d.height_px(),
                    d.scale,
                    d.origin_x,
                    d.origin_y,
                    if d.is_primary { " primary" } else { "" }
                );
            }
        }
        Err(e) => println!("displays: {}", e.user_message()),
    }
    if online {
        match app.provider() {
            Ok(p) => match p.list_models() {
                Ok(models) => {
                    println!("models: {} available", models.len());
                    for m in models.iter().take(8) {
                        println!("  {m}");
                    }
                    if !models.contains(&app.config.provider.chat_model) {
                        println!(
                            "warning: configured model {} not in the list",
                            app.config.provider.chat_model
                        );
                    }
                }
                Err(e) => println!("models: {} ({})", e.user_message(), e.code()),
            },
            Err(e) => println!("provider: {e}"),
        }
    }
    Ok(())
}

fn task(app: &App, goal: &str, dry_run: bool) -> anyhow::Result<()> {
    // The task path never transcribes, so building the speech stack here would
    // demand an STT key a local-only or dry-run user does not have.
    let memory = Arc::new(Memory::load(&app.paths.memory_dir())?);
    let chat = app.provider()?;
    let clock: Arc<dyn Clock> = Arc::new(openclicky_core::clock::SystemClock);
    let usage = Arc::new(UsageMeter::new(
        app.store.clone(),
        clock.clone(),
        app.config.budget.weights.clone(),
        app.config.budget.daily_units,
        app.config.budget.monthly_units,
    ));
    let agent_deps = Arc::new(AgentDeps {
        store: app.store.clone(),
        chat,
        tools: Arc::new(Registry::default_set()),
        clock,
        usage,
        memory,
        http: app.http.clone(),
        commands: Arc::new(SystemCommands),
        permissions: Arc::new(permissions::SystemProbe),
        workspace: app.paths.workspace_dir(),
        protected: vec![
            app.paths.config_file(),
            app.paths.db_file(),
            app.paths.memory_dir(),
            app.paths.data_dir.join("secrets.enc"),
        ],
        model: if app.config.provider.deep_model.is_empty() {
            app.config.provider.chat_model.clone()
        } else {
            app.config.provider.deep_model.clone()
        },
        max_steps: app.config.tools.max_steps,
        allow_private_network: app.config.tools.allow_private_network,
        allow_domains: app.config.tools.allow_domains.clone(),
        shell_enabled: app.config.tools.shell,
        events: Some(Arc::new(|e: AppEvent| {
            if let AppEvent::TaskUpdated { state, message, .. } = e {
                eprintln!("[{state:?}] {message}");
            }
        })),
    });
    let runner = TaskRunner::new(agent_deps);
    let t = runner.create(goal)?;
    println!("task {} ({})", t.id, t.name);
    if dry_run {
        println!("dry run: created only");
        return Ok(());
    }
    let cancel = CancelToken::new();
    let done = runner.run(&t.id, &cancel)?;
    println!("state: {:?}", done.state);
    if let Some(r) = done.result {
        println!("result: {r}");
    }
    if let Some(e) = done.error_code {
        println!("code: {e}");
    }
    Ok(())
}

/// Where the desktop shell lands once installed, per platform.
fn find_desktop() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);

    #[cfg(target_os = "macos")]
    {
        let bundle = home
            .as_ref()?
            .join("Applications")
            .join("OpenClicky.app")
            .join("Contents")
            .join("MacOS")
            .join("openclicky-desktop");
        if bundle.exists() {
            return Some(bundle);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        let exe = local.join("OpenClicky").join("openclicky-desktop.exe");
        if exe.exists() {
            return Some(exe);
        }
    }

    let candidate = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("openclicky-desktop")));
    if let Some(p) = candidate {
        if p.exists() {
            return Some(p);
        }
    }
    if let Some(p) = home
        .as_ref()
        .map(|h| h.join(".local/bin/openclicky-desktop"))
    {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn open_app(wait: bool) -> anyhow::Result<()> {
    let Some(shell) = find_desktop() else {
        let hint = if cfg!(target_os = "windows") {
            "irm https://raw.githubusercontent.com/123oqwe/openclicky2.0/main/scripts/install.ps1 | iex"
        } else {
            "curl -fsSL https://raw.githubusercontent.com/123oqwe/openclicky2.0/main/scripts/install.sh | sh"
        };
        anyhow::bail!(
            "the desktop shell is not installed yet. Run this, then try again:\n  {hint}"
        );
    };
    println!("starting {}", shell.display());
    let mut cmd = std::process::Command::new(&shell);
    if wait {
        let status = cmd.status()?;
        anyhow::ensure!(status.success(), "the desktop shell exited with {status}");
    } else {
        // Detached means the caller's stdout is not held open by a child that
        // outlives it: an inherited pipe makes `openclicky app | cat` hang until
        // the window is closed.
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd.spawn()?;
    }
    Ok(())
}

fn selftest(app: &App, out: Option<PathBuf>) -> anyhow::Result<()> {
    let screen = XcapScreen::new();
    let gate = openclicky_core::privacy::CaptureGate::new();
    let guard = gate.authorize(openclicky_core::privacy::CaptureReason::Calibration);
    let shots = screen.capture_all(&guard)?;
    println!("captured {} display(s)", shots.len());
    for s in &shots {
        println!(
            "  screen{} {}x{} logical {:.0}x{:.0} scale {:.2} -> {} bytes",
            s.display.index,
            s.width_px,
            s.height_px,
            s.display.width,
            s.display.height,
            s.display.scale,
            s.png.len()
        );
    }
    if let Some(path) = out {
        let shot = shots.first().ok_or_else(|| {
            anyhow!("captured no display, so there is nothing to write (headless, or screen recording was refused)")
        })?;
        std::fs::write(&path, &shot.png)?;
        println!("wrote {}", path.display());
    }
    let _lock = Singleton::acquire(&app.paths.lock_file())?;
    println!("singleton: ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_names_the_build() {
        assert!(
            LONG_VERSION.starts_with(env!("CARGO_PKG_VERSION")),
            "{LONG_VERSION}"
        );
        assert!(LONG_VERSION.contains("commit "), "{LONG_VERSION}");
        assert!(
            !LONG_VERSION.contains("OPENCLICKY"),
            "an unexpanded env var leaked into the version: {LONG_VERSION}"
        );
    }
}
