//! Scheduled tasks. Time is injected; the module never sleeps.

use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Weekday};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    Interval {
        minutes: u32,
    },
    Weekly {
        days: Vec<Weekday>,
        hour: u8,
        minute: u8,
    },
    Daily {
        hour: u8,
        minute: u8,
    },
}

fn parse_day(s: &str) -> Option<Weekday> {
    Some(match s.trim().to_lowercase().as_str() {
        "mon" | "monday" | "周一" => Weekday::Mon,
        "tue" | "tuesday" | "周二" => Weekday::Tue,
        "wed" | "wednesday" | "周三" => Weekday::Wed,
        "thu" | "thursday" | "周四" => Weekday::Thu,
        "fri" | "friday" | "周五" => Weekday::Fri,
        "sat" | "saturday" | "周六" => Weekday::Sat,
        "sun" | "sunday" | "周日" | "周天" => Weekday::Sun,
        _ => return None,
    })
}

fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u8 = h.parse().ok()?;
    let m: u8 = m.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some((h, m))
}

pub fn parse(spec: &str) -> Result<Schedule> {
    let s = spec.trim().to_lowercase();
    if let Some(rest) = s.strip_prefix("every ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() == 2 {
            if let Ok(n) = parts[0].parse::<u32>() {
                let unit = parts[1].trim_end_matches('s');
                let minutes = match unit {
                    "minute" => n,
                    "hour" => n * 60,
                    _ => return Err(Error::BadRequest("只支持分钟或小时".into())),
                };
                if minutes == 0 {
                    return Err(Error::BadRequest("间隔必须大于 0".into()));
                }
                return Ok(Schedule::Interval { minutes });
            }
        }
    }
    if let Some(rest) = s.strip_prefix("every ") {
        if let Some((days, time)) = rest.split_once(" at ") {
            let days: Vec<Weekday> = days
                .split(',')
                .map(|d| {
                    parse_day(d).ok_or_else(|| Error::BadRequest(format!("不认识的星期：{d}")))
                })
                .collect::<Result<Vec<_>>>()?;
            let (h, m) =
                parse_hhmm(time).ok_or_else(|| Error::BadRequest("时间要写成 HH:MM".into()))?;
            return Ok(Schedule::Weekly {
                days,
                hour: h,
                minute: m,
            });
        }
    }
    if let Some(time) = s.strip_prefix("daily at ") {
        let (h, m) =
            parse_hhmm(time).ok_or_else(|| Error::BadRequest("时间要写成 HH:MM".into()))?;
        return Ok(Schedule::Daily { hour: h, minute: m });
    }
    Err(Error::BadRequest(format!("看不懂的时间表：{spec}")))
}

pub fn next_run(schedule: &Schedule, after: DateTime<Local>) -> DateTime<Local> {
    match schedule {
        Schedule::Interval { minutes } => after + Duration::minutes(*minutes as i64),
        Schedule::Daily { hour, minute } => {
            let mut candidate = Local
                .with_ymd_and_hms(
                    after.year(),
                    after.month(),
                    after.day(),
                    *hour as u32,
                    *minute as u32,
                    0,
                )
                .single()
                .unwrap_or(after);
            if candidate <= after {
                candidate += Duration::days(1);
            }
            candidate
        }
        Schedule::Weekly { days, hour, minute } => {
            for d in 0..8 {
                let day = after + Duration::days(d);
                if !days.contains(&day.weekday()) {
                    continue;
                }
                let candidate = Local
                    .with_ymd_and_hms(
                        day.year(),
                        day.month(),
                        day.day(),
                        *hour as u32,
                        *minute as u32,
                        0,
                    )
                    .single()
                    .unwrap_or(after);
                if candidate > after {
                    return candidate;
                }
            }
            after + Duration::days(7)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutineDecision {
    RunNow,
    Skip,
    Wait(String),
    Pause(String),
}

/// Decide what to do with a routine at `now`.
pub fn decide(
    routine: &crate::store::RoutineRow,
    now: DateTime<Local>,
    online: bool,
    awake: bool,
    budget_ok: bool,
) -> RoutineDecision {
    if !routine.enabled {
        return RoutineDecision::Skip;
    }
    if routine.failures >= 3 {
        return RoutineDecision::Pause("连续三次失败，已暂停".into());
    }
    if !awake {
        return RoutineDecision::Wait("电脑还没醒".into());
    }
    if !online {
        return RoutineDecision::Wait("等网络恢复".into());
    }
    if !budget_ok {
        return RoutineDecision::Wait("额度用完".into());
    }
    match routine.next_run_at {
        Some(next) if next.with_timezone(&Local) <= now => RoutineDecision::RunNow,
        None => RoutineDecision::RunNow,
        _ => RoutineDecision::Skip,
    }
}

/// After a wake-up, at most one missed run is caught up (SRC-PRD L4).
pub fn catch_up_once(schedule: &Schedule, now: DateTime<Local>) -> DateTime<Local> {
    next_run(schedule, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).single().unwrap()
    }

    #[test]
    fn parses_interval() {
        assert_eq!(
            parse("every 30 minutes").unwrap(),
            Schedule::Interval { minutes: 30 }
        );
        assert_eq!(
            parse("every 2 hours").unwrap(),
            Schedule::Interval { minutes: 120 }
        );
    }

    #[test]
    fn parses_weekly_and_daily() {
        assert_eq!(
            parse("every monday,thursday at 17:00").unwrap(),
            Schedule::Weekly {
                days: vec![Weekday::Mon, Weekday::Thu],
                hour: 17,
                minute: 0
            }
        );
        assert_eq!(
            parse("daily at 09:30").unwrap(),
            Schedule::Daily {
                hour: 9,
                minute: 30
            }
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("whenever i feel like it").is_err());
        assert!(parse("daily at 25:00").is_err());
    }

    #[test]
    fn daily_next_run_rolls_to_tomorrow() {
        let s = Schedule::Daily { hour: 9, minute: 0 };
        let now = at(2026, 11, 23, 10, 0);
        assert_eq!(next_run(&s, now), at(2026, 11, 24, 9, 0));
    }

    #[test]
    fn weekly_picks_the_next_matching_day() {
        let s = Schedule::Weekly {
            days: vec![Weekday::Mon],
            hour: 9,
            minute: 0,
        };
        // 2026-11-23 is a Monday.
        let now = at(2026, 11, 23, 10, 0);
        assert_eq!(next_run(&s, now), at(2026, 11, 30, 9, 0));
    }

    #[test]
    fn wakes_up_only_when_conditions_are_met() {
        let mut r = crate::store::RoutineRow {
            id: "r1".into(),
            name: "inbox".into(),
            schedule: "daily at 09:00".into(),
            enabled: true,
            next_run_at: Some(at(2026, 11, 23, 9, 0).with_timezone(&chrono::Utc)),
            run_count: 0,
            failures: 0,
            last_result: None,
            task_template: "check inbox".into(),
        };
        let now = at(2026, 11, 23, 10, 0);
        assert_eq!(
            decide(&r, now, false, true, true),
            RoutineDecision::Wait("等网络恢复".into())
        );
        assert_eq!(
            decide(&r, now, true, false, true),
            RoutineDecision::Wait("电脑还没醒".into())
        );
        assert_eq!(
            decide(&r, now, true, true, false),
            RoutineDecision::Wait("额度用完".into())
        );
        assert_eq!(decide(&r, now, true, true, true), RoutineDecision::RunNow);
        r.failures = 3;
        assert!(matches!(
            decide(&r, now, true, true, true),
            RoutineDecision::Pause(_)
        ));
    }
}
