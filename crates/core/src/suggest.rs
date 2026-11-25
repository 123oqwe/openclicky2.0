//! Interruption budget for suggestions and next-step chips (SRC-PRD M6, M9).

// UNWIRED: nothing emits or records suggestions yet, so the budget never runs. See #46.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SuggestBudget {
    pub ignored_mornings: u32,
    pub paused_until: Option<NaiveDate>,
    pub pause_days: u32,
    pub ignored_chips: u32,
    pub chips_paused_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Show,
    Suppress(String),
}

pub fn decide_morning(
    budget: &SuggestBudget,
    now_local: DateTime<chrono::Local>,
    in_call: bool,
    focus_mode: bool,
) -> Decision {
    if now_local.hour() < 6 {
        return Decision::Suppress("6 点前不打扰".into());
    }
    if in_call || focus_mode {
        return Decision::Suppress("通话或专注模式".into());
    }
    if let Some(until) = budget.paused_until {
        if now_local.date_naive() < until {
            return Decision::Suppress("之前连续忽略，休息中".into());
        }
    }
    Decision::Show
}

/// Returns the new pause length in days (0 = no pause yet).
pub fn on_morning_ignored(budget: &mut SuggestBudget, today: NaiveDate) -> u32 {
    budget.ignored_mornings += 1;
    if budget.ignored_mornings >= 3 {
        let days = if budget.pause_days == 0 {
            1
        } else {
            (budget.pause_days * 2).min(7)
        };
        budget.pause_days = days;
        budget.paused_until = Some(today + Duration::days(days as i64));
        budget.ignored_mornings = 0;
        days
    } else {
        0
    }
}

pub fn on_morning_shown(budget: &mut SuggestBudget) {
    budget.ignored_mornings = 0;
}

pub fn decide_chips(budget: &SuggestBudget, now: DateTime<Utc>) -> Decision {
    match budget.chips_paused_until {
        Some(until) if now < until => Decision::Suppress("连续没人点，暂停一小时".into()),
        _ => Decision::Show,
    }
}

pub fn on_chip_ignored(budget: &mut SuggestBudget, now: DateTime<Utc>) {
    budget.ignored_chips += 1;
    if budget.ignored_chips >= 8 {
        budget.chips_paused_until = Some(now + Duration::hours(1));
        budget.ignored_chips = 0;
    }
}

pub fn on_chip_clicked(budget: &mut SuggestBudget) {
    budget.ignored_chips = 0;
    budget.chips_paused_until = None;
}

pub fn reset_daily(budget: &mut SuggestBudget, today: NaiveDate) {
    if let Some(until) = budget.paused_until {
        if today >= until {
            budget.paused_until = None;
            budget.ignored_mornings = 0;
        }
    }
    if let Some(until) = budget.chips_paused_until {
        if Utc::now() >= until {
            budget.chips_paused_until = None;
        }
    }
    let _ = today.year();
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32) -> DateTime<chrono::Local> {
        chrono::Local
            .with_ymd_and_hms(2026, 11, 23, h, 0, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn no_suggestions_before_six() {
        let b = SuggestBudget::default();
        assert!(matches!(
            decide_morning(&b, at(5), false, false),
            Decision::Suppress(_)
        ));
        assert_eq!(decide_morning(&b, at(7), false, false), Decision::Show);
    }

    #[test]
    fn call_or_focus_suppresses() {
        let b = SuggestBudget::default();
        assert!(matches!(
            decide_morning(&b, at(9), true, false),
            Decision::Suppress(_)
        ));
        assert!(matches!(
            decide_morning(&b, at(9), false, true),
            Decision::Suppress(_)
        ));
    }

    #[test]
    fn three_ignored_mornings_pause_one_day_then_double() {
        let mut b = SuggestBudget::default();
        let today = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        assert_eq!(on_morning_ignored(&mut b, today), 0);
        assert_eq!(on_morning_ignored(&mut b, today), 0);
        assert_eq!(on_morning_ignored(&mut b, today), 1);
        assert_eq!(b.paused_until, Some(today + Duration::days(1)));
        for _ in 0..3 {
            on_morning_ignored(&mut b, today);
        }
        assert_eq!(b.pause_days, 2);
    }

    #[test]
    fn pause_is_capped_at_a_week() {
        let mut b = SuggestBudget::default();
        let today = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        for _ in 0..100 {
            for _ in 0..3 {
                on_morning_ignored(&mut b, today);
            }
        }
        assert!(b.pause_days <= 7);
    }

    #[test]
    fn eight_ignored_chips_pause_one_hour() {
        let mut b = SuggestBudget::default();
        let now = Utc::now();
        for _ in 0..8 {
            on_chip_ignored(&mut b, now);
        }
        assert!(matches!(decide_chips(&b, now), Decision::Suppress(_)));
        assert_eq!(
            decide_chips(&b, now + Duration::minutes(61)),
            Decision::Show
        );
    }
}
