//! Central wiring. Tests build this with fakes; the shells build it with the
//! platform implementations.

use std::sync::{Arc, RwLock};

use crate::config::Config;
use crate::memory::Memory;
use crate::ports::*;
use crate::privacy::CaptureGate;
use crate::store::Store;
use crate::usage::UsageMeter;
use crate::AppEvent;

pub type EventSink = Arc<dyn Fn(AppEvent) + Send + Sync>;

pub struct Deps {
    pub clock: Arc<dyn Clock>,
    pub screen: Arc<dyn ScreenSource>,
    pub audio: Arc<dyn AudioSource>,
    pub stt: Arc<dyn Transcriber>,
    pub chat: Arc<dyn ChatProvider>,
    pub speech: Arc<dyn SpeechOut>,
    pub overlay: Arc<dyn PointerOverlay>,
    pub injector: Arc<dyn TextInjector>,
    pub permissions: Arc<dyn PermissionProbe>,
    pub store: Arc<Store>,
    pub usage: Arc<UsageMeter>,
    pub memory: Arc<Memory>,
    pub gate: Arc<CaptureGate>,
    pub config: Arc<RwLock<Config>>,
    pub events: EventSink,
}

impl Deps {
    /// The clock is a parameter rather than a `SystemClock` built here so a test
    /// can move time instead of sleeping through it.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        clock: Arc<dyn Clock>,
        screen: Arc<dyn ScreenSource>,
        audio: Arc<dyn AudioSource>,
        stt: Arc<dyn Transcriber>,
        chat: Arc<dyn ChatProvider>,
        speech: Arc<dyn SpeechOut>,
        overlay: Arc<dyn PointerOverlay>,
        injector: Arc<dyn TextInjector>,
        permissions: Arc<dyn PermissionProbe>,
        store: Arc<Store>,
        memory: Arc<Memory>,
        config: Config,
        events: EventSink,
    ) -> Arc<Self> {
        let usage = Arc::new(UsageMeter::new(
            store.clone(),
            clock.clone(),
            config.budget.weights.clone(),
            config.budget.daily_units,
            config.budget.monthly_units,
        ));
        Arc::new(Self {
            clock,
            screen,
            audio,
            stt,
            chat,
            speech,
            overlay,
            injector,
            permissions,
            store,
            usage,
            memory,
            gate: Arc::new(CaptureGate::new()),
            config: Arc::new(RwLock::new(config)),
            events,
        })
    }

    pub fn emit(&self, event: AppEvent) {
        (self.events)(event);
    }

    pub fn config_snapshot(&self) -> Config {
        self.config.read().unwrap().clone()
    }
}
