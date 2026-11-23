//! Usage metering and the local budget guard (ADR-0010).
//!
//! Rules live in `rules()` and are asserted by a table-driven test. Metering is
//! written through a single entry point so a task can never be charged twice
//! (INV-D2).

use chrono::{DateTime, Datelike, Local, TimeZone, Utc};
use std::sync::{Arc, Mutex};

use crate::clock::SystemClock;
use crate::config::Weights;
use crate::error::Result;
use crate::ports::Clock;
use crate::store::{Store, UsageRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageKind {
    Voice,
    Dictation,
    AgentStep,
    AgentTask,
}

impl UsageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UsageKind::Voice => "voice",
            UsageKind::Dictation => "dictation",
            UsageKind::AgentStep => "agent_step",
            UsageKind::AgentTask => "agent_task",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MeterInput {
    pub kind: UsageKind,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub now: DateTime<Utc>,
    /// True when the task already consumed its one task-level charge (long task rule).
    pub task_already_charged: bool,
}

impl MeterInput {
    pub fn simple(kind: UsageKind, now: DateTime<Utc>) -> Self {
        Self {
            kind,
            task_id: None,
            session_id: None,
            started_at: now,
            now,
            task_already_charged: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetState {
    pub daily_used: f64,
    pub daily_limit: f64,
    pub monthly_used: f64,
    pub monthly_limit: f64,
    pub percent: f64,
    pub blocked: bool,
    pub warned: bool,
}

pub struct UsageMeter {
    store: Arc<Store>,
    clock: Arc<dyn Clock>,
    weights: Weights,
    daily_limit: f64,
    monthly_limit: f64,
    boost: Mutex<Option<(chrono::NaiveDate, f64)>>,
}

impl UsageMeter {
    pub fn new(
        store: Arc<Store>,
        clock: Arc<dyn Clock>,
        weights: Weights,
        daily: f64,
        monthly: f64,
    ) -> Self {
        Self {
            store,
            clock,
            weights,
            daily_limit: daily,
            monthly_limit: monthly,
            boost: Mutex::new(None),
        }
    }

    pub fn with_system_clock(
        store: Arc<Store>,
        weights: Weights,
        daily: f64,
        monthly: f64,
    ) -> Self {
        Self::new(store, Arc::new(SystemClock), weights, daily, monthly)
    }

    /// Units for one action, before waiver rules.
    pub fn units_for(&self, kind: UsageKind) -> f64 {
        match kind {
            UsageKind::Voice => self.weights.voice,
            UsageKind::Dictation => self.weights.dictation,
            UsageKind::AgentStep => self.weights.agent_step,
            UsageKind::AgentTask => self.weights.agent_step.max(1.0),
        }
    }

    /// Waiver rules: 10 second cancel, long task continuation, approvals.
    pub fn is_waived(&self, input: &MeterInput) -> bool {
        match input.kind {
            UsageKind::AgentTask => {
                let elapsed = input
                    .now
                    .signed_duration_since(input.started_at)
                    .num_milliseconds();
                if (0..10_000).contains(&elapsed) {
                    return true; // SRC-PRD H3: stopped within 10s costs nothing
                }
                if input.task_already_charged {
                    return true; // long task keeps a single charge
                }
                false
            }
            _ => false,
        }
    }

    /// Pure computation: how many units this action costs, and whether it is
    /// waived. Callers that write inside a transaction use this directly.
    pub fn quote(&self, input: &MeterInput) -> (f64, bool) {
        let waived = self.is_waived(input);
        let units = if waived {
            0.0
        } else {
            self.units_for(input.kind)
        };
        (units, waived)
    }

    /// Compute and persist in one step (standalone actions such as agent steps).
    pub fn meter(&self, input: &MeterInput) -> Result<f64> {
        let (units, waived) = self.quote(input);
        let row = UsageRow {
            kind: input.kind.as_str().to_string(),
            units: if waived {
                self.units_for(input.kind)
            } else {
                units
            },
            task_id: input.task_id.clone(),
            session_id: input.session_id.clone(),
            waived,
            created_at: input.now,
            meta_json: None,
        };
        self.store.insert_usage(&row)?;
        Ok(units)
    }

    pub fn state(&self, now: DateTime<Utc>) -> Result<BudgetState> {
        let (day_from, day_to) = day_bounds(now);
        let (month_from, month_to) = month_bounds(now);
        let daily_used = unpaid_units(&self.store.usage_between(day_from, day_to)?);
        let monthly_used = unpaid_units(&self.store.usage_between(month_from, month_to)?);
        let daily_limit = self.current_daily_limit(now);
        let percent = if daily_limit > 0.0 {
            (daily_used / daily_limit * 100.0).clamp(0.0, 999.0)
        } else {
            100.0
        };
        Ok(BudgetState {
            daily_used,
            daily_limit,
            monthly_used,
            monthly_limit: self.monthly_limit,
            percent,
            blocked: daily_used >= daily_limit || monthly_used >= self.monthly_limit,
            warned: percent >= 80.0,
        })
    }

    pub fn can_start(&self, now: DateTime<Utc>) -> Result<bool> {
        Ok(!self.state(now)?.blocked)
    }

    /// One-off +50% for today (budget guard equivalent of "reset for $10").
    pub fn grant_temporary(&self, now: DateTime<Utc>, pct: f64) -> Result<f64> {
        let today = now.with_timezone(&Local).date_naive();
        let extra = self.daily_limit * (pct / 100.0).max(0.0);
        let mut boost = self.boost.lock().unwrap();
        *boost = Some((today, extra));
        Ok(extra)
    }

    fn current_daily_limit(&self, now: DateTime<Utc>) -> f64 {
        let today = now.with_timezone(&Local).date_naive();
        let boost = self.boost.lock().unwrap();
        match *boost {
            Some((d, extra)) if d == today => self.daily_limit + extra,
            _ => self.daily_limit,
        }
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }
}

pub fn unpaid_units(rows: &[UsageRow]) -> f64 {
    rows.iter().filter(|r| !r.waived).map(|r| r.units).sum()
}

pub fn day_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let local = now.with_timezone(&Local);
    let start_local = Local
        .with_ymd_and_hms(local.year(), local.month(), local.day(), 0, 0, 0)
        .single()
        .unwrap_or(local);
    let start = start_local.with_timezone(&Utc);
    (
        start,
        start + chrono::Duration::days(1) - chrono::Duration::milliseconds(1),
    )
}

pub fn month_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let local = now.with_timezone(&Local);
    let start_local = Local
        .with_ymd_and_hms(local.year(), local.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(local);
    let start = start_local.with_timezone(&Utc);
    let (ny, nm) = if local.month() == 12 {
        (local.year() + 1, 1)
    } else {
        (local.year(), local.month() + 1)
    };
    let next_local = Local
        .with_ymd_and_hms(ny, nm, 1, 0, 0, 0)
        .single()
        .unwrap_or(local);
    let next = next_local.with_timezone(&Utc);
    (start, next - chrono::Duration::milliseconds(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;

    fn meter() -> (Arc<Store>, Arc<TestClock>, UsageMeter) {
        let store = Arc::new(Store::open_memory().unwrap());
        let clock = Arc::new(TestClock::new());
        let m = UsageMeter::new(
            store.clone(),
            clock.clone(),
            Weights::default(),
            10.0,
            100.0,
        );
        (store, clock, m)
    }

    #[test]
    fn voice_costs_one_unit() {
        let (_s, clock, m) = meter();
        let u = m
            .meter(&MeterInput::simple(UsageKind::Voice, clock.now()))
            .unwrap();
        assert_eq!(u, 1.0);
    }

    #[test]
    fn task_stopped_within_ten_seconds_is_free() {
        let (store, clock, m) = meter();
        let started = clock.now();
        clock.advance_ms(8_000);
        let input = MeterInput {
            kind: UsageKind::AgentTask,
            task_id: Some("t1".into()),
            session_id: None,
            started_at: started,
            now: clock.now(),
            task_already_charged: false,
        };
        assert_eq!(m.meter(&input).unwrap(), 0.0);
        let rows = store
            .usage_between(
                started - chrono::Duration::hours(1),
                clock.now() + chrono::Duration::hours(1),
            )
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].waived);
        assert_eq!(unpaid_units(&rows), 0.0);
    }

    #[test]
    fn long_running_task_is_charged_once() {
        let (_s, clock, m) = meter();
        let started = clock.now();
        clock.advance_minutes(30);
        let input = MeterInput {
            kind: UsageKind::AgentTask,
            task_id: Some("t2".into()),
            session_id: None,
            started_at: started,
            now: clock.now(),
            task_already_charged: true,
        };
        assert_eq!(m.meter(&input).unwrap(), 0.0);
    }

    #[test]
    fn budget_blocks_at_limit_and_warns_at_80_percent() {
        let (_s, clock, m) = meter();
        for _ in 0..8 {
            m.meter(&MeterInput::simple(UsageKind::Voice, clock.now()))
                .unwrap();
        }
        let s = m.state(clock.now()).unwrap();
        assert!(s.warned);
        assert!(!s.blocked);
        for _ in 0..2 {
            m.meter(&MeterInput::simple(UsageKind::Voice, clock.now()))
                .unwrap();
        }
        assert!(m.state(clock.now()).unwrap().blocked);
    }

    #[test]
    fn daily_reset_clears_usage() {
        let (_s, clock, m) = meter();
        m.meter(&MeterInput::simple(UsageKind::Voice, clock.now()))
            .unwrap();
        assert_eq!(m.state(clock.now()).unwrap().daily_used, 1.0);
        clock.advance_minutes(60 * 24);
        assert_eq!(m.state(clock.now()).unwrap().daily_used, 0.0);
    }

    #[test]
    fn temporary_grant_unblocks_today() {
        let (_s, clock, m) = meter();
        for _ in 0..10 {
            m.meter(&MeterInput::simple(UsageKind::Voice, clock.now()))
                .unwrap();
        }
        assert!(m.state(clock.now()).unwrap().blocked);
        m.grant_temporary(clock.now(), 50.0).unwrap();
        assert!(!m.state(clock.now()).unwrap().blocked);
    }
}
