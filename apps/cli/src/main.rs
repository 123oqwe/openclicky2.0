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

#[derive(Parser)]
#[command(
    name = "openclicky",
    version,
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
    },
    Usage,
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
            self.config.clone(),
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
        Command::SetKey { name } => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            let key = buf.trim();
            if key.is_empty() {
                return Err(anyhow!("empty key on stdin"));
            }
            app.secrets.set(&name, &SecretString::new(key))?;
            println!("saved {name}");
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
        "openclicky {} ({})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
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
    if fallback.exists() {
        println!("key store: encrypted file (0600) — keychain read-back failed or unavailable");
    } else {
        println!("key store: system keychain");
    }
    let _ = app.secrets.is_degraded();
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
    let (_deps, _engine, memory) = app.deps()?;
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
        model: if app.config.provider.deep_model.is_empty() {
            app.config.provider.chat_model.clone()
        } else {
            app.config.provider.deep_model.clone()
        },
        max_steps: app.config.tools.max_steps,
        allow_private_network: app.config.tools.allow_private_network,
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
        std::fs::write(&path, &shots[0].png)?;
        println!("wrote {}", path.display());
    }
    let _lock = Singleton::acquire(&app.paths.lock_file())?;
    println!("singleton: ok");
    Ok(())
}
