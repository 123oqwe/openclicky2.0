use chrono::{DateTime, Local, Utc};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use crate::ports::Clock;

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
    fn monotonic_ms(&self) -> u64 {
        static START: Mutex<Option<std::time::Instant>> = Mutex::new(None);
        let mut guard = START.lock().unwrap();
        let start = *guard.get_or_insert_with(std::time::Instant::now);
        start.elapsed().as_millis() as u64
    }
}

/// Deterministic clock for tests: time only moves when asked to.
#[derive(Debug)]
pub struct TestClock {
    now_ms: AtomicI64,
    mono_ms: AtomicI64,
}

impl TestClock {
    /// Starts at 2026-11-23T08:00:00Z by default.
    pub fn new() -> Self {
        Self::at(chrono::TimeZone::timestamp_opt(&Utc, 1_795_000_000, 0).unwrap())
    }

    pub fn at(t: DateTime<Utc>) -> Self {
        Self {
            now_ms: AtomicI64::new(t.timestamp_millis()),
            mono_ms: AtomicI64::new(0),
        }
    }

    pub fn advance_ms(&self, ms: i64) {
        self.now_ms.fetch_add(ms, Ordering::SeqCst);
        self.mono_ms.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn advance_minutes(&self, minutes: i64) {
        self.advance_ms(minutes * 60_000);
    }

    pub fn local(&self) -> DateTime<Local> {
        self.now().with_timezone(&Local)
    }
}

impl Default for TestClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        chrono::TimeZone::timestamp_millis_opt(&Utc, self.now_ms.load(Ordering::SeqCst)).unwrap()
    }
    fn monotonic_ms(&self) -> u64 {
        self.mono_ms.load(Ordering::SeqCst) as u64
    }
}
