//! Dictation: incremental transcription, conservative cleanup, dictionary
//! learning and terminal safety (ADR-0013, ADR-0004).

use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::deps::Deps;
use crate::error::{Error, Result};
use crate::ports::{InjectOutcome, TargetInfo, TargetKind};
use crate::AppEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictEntry {
    pub heard: String,
    pub corrected: String,
}

/// Longest common prefix, then the new suffix. Never returns a negative suffix.
pub fn merge_partial(prev: &str, next: &str) -> (String, String) {
    let p: Vec<char> = prev.chars().collect();
    let n: Vec<char> = next.chars().collect();
    let mut k = 0;
    while k < p.len() && k < n.len() && p[k] == n[k] {
        k += 1;
    }
    let common: String = n.iter().take(k).collect();
    let suffix: String = n.iter().skip(k).collect();
    (common, suffix)
}

/// Replace words the user has corrected before.
pub fn apply_dictionary(text: &str, dict: &[(String, String)]) -> String {
    let mut out = text.to_string();
    // Longest keys first so "open clicky" wins over "clicky".
    let mut entries: Vec<&(String, String)> = dict.iter().collect();
    entries.sort_by_key(|(heard, _)| std::cmp::Reverse(heard.chars().count()));
    for (heard, corrected) in entries {
        if heard.is_empty() {
            continue;
        }
        out = out.replace(heard.as_str(), corrected);
    }
    out
}

/// Terminal safety: never inject anything that could run as a command.
pub fn terminal_safe(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Conservative cleanup: fix obvious spacing, never add facts or dashes.
pub fn cleanup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\u{2014}' || c == '\u{2013}' {
            out.push(' ');
            continue;
        }
        out.push(c);
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return collapsed;
    }
    let mut chars: Vec<char> = collapsed.chars().collect();
    if let Some(first) = chars.first_mut() {
        *first = first.to_uppercase().next().unwrap_or(*first);
    }
    chars.into_iter().collect()
}

pub fn looks_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    const WORDS: [&str; 8] = [
        "密码",
        "验证码",
        "卡号",
        "cvv",
        "password",
        "one-time",
        "otp",
        "card number",
    ];
    if WORDS.iter().any(|w| lower.contains(w)) {
        return true;
    }
    has_card_number(&lower) || has_otp(&lower)
}

/// Luhn check over any 13-19 digit run, so a dictation of a card number is
/// stopped even when nobody says the word "card".
fn has_card_number(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_digit() || c == ' ' || c == '-'))
        .map(|chunk| {
            chunk
                .chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
        })
        .any(|digits| (13..=19).contains(&digits.len()) && luhn(&digits))
}

fn luhn(digits: &str) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for c in digits.chars().rev() {
        let Some(d) = c.to_digit(10) else {
            return false;
        };
        let mut d = d;
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum.is_multiple_of(10)
}

/// A standalone six-to-eight digit token is almost always a one-time code.
/// Four and five digit tokens are excluded on purpose: years and prices would
/// trip them constantly.
fn has_otp(text: &str) -> bool {
    text.split_whitespace().any(|tok| {
        let t = tok.trim_matches(|c: char| !c.is_ascii_digit());
        (6..=8).contains(&t.len()) && t.chars().all(|c| c.is_ascii_digit())
    })
}

/// CJK text must not gain a space after it.
pub fn join_without_space(language: &str) -> bool {
    matches!(language, "zh" | "ja" | "th" | "ko")
}

pub struct DictationEngine {
    deps: Arc<Deps>,
    started: Mutex<Option<std::time::Instant>>,
    injected: Mutex<String>,
    learned: Mutex<Vec<DictEntry>>,
}

pub struct DictationResult {
    pub final_text: String,
    pub injected_text: String,
    pub outcome: Option<InjectOutcome>,
    pub charged_units: f64,
}

impl DictationEngine {
    pub fn new(deps: Arc<Deps>) -> Self {
        let learned = deps
            .store
            .dict()
            .unwrap_or_default()
            .into_iter()
            .map(|(h, c)| DictEntry {
                heard: h,
                corrected: c,
            })
            .collect();
        Self {
            deps,
            started: Mutex::new(None),
            injected: Mutex::new(String::new()),
            learned: Mutex::new(learned),
        }
    }

    pub fn start(&self) -> Result<()> {
        self.deps.audio.start()?;
        *self.started.lock().unwrap() = Some(std::time::Instant::now());
        self.injected.lock().unwrap().clear();
        self.deps
            .emit(AppEvent::State(crate::voice::State::Dictating));
        Ok(())
    }

    /// Called by the shell every ~700ms. Returns the newly typed suffix.
    pub fn tick(&self) -> Result<Option<String>> {
        let clip = self.deps.audio.snapshot()?;
        if clip.duration_ms() < 300 {
            return Ok(None);
        }
        let dict = self.learned.lock().unwrap().clone();
        let pairs: Vec<(String, String)> = dict
            .iter()
            .map(|d| (d.heard.clone(), d.corrected.clone()))
            .collect();
        let hint = if pairs.is_empty() {
            None
        } else {
            Some(
                pairs
                    .iter()
                    .map(|(h, _)| h.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        };
        if !self.deps.usage.can_start(self.deps.clock.now())? {
            self.deps.emit(AppEvent::Notice {
                code: "E_BUDGET".into(),
                message: "今天的额度用完了，听写已暂停。".into(),
            });
            return Ok(None);
        }
        let transcript = self.deps.stt.transcribe(&clip, hint.as_deref())?;
        let text = apply_dictionary(&transcript.text, &pairs);
        let mut injected = self.injected.lock().unwrap();
        let (_common, suffix) = merge_partial(&injected, &text);
        if suffix.trim().is_empty() {
            return Ok(None);
        }
        if looks_secret(&text) {
            self.deps.emit(AppEvent::Notice {
                code: "E_SECRET".into(),
                message: "这段话看起来包含密码或验证码，我不会代填。".into(),
            });
            return Ok(None);
        }
        let target = self.deps.injector.focused();
        let kind = target
            .as_ref()
            .map(|t| t.kind)
            .unwrap_or(TargetKind::Unknown);
        let payload = if kind == TargetKind::Terminal {
            terminal_safe(&suffix)
        } else {
            suffix.clone()
        };
        if let Some(t) = &target {
            self.deps.emit(AppEvent::Dictation {
                text: payload.clone(),
                partial: true,
            });
            let _ = self.deps.injector.inject(&payload, t);
        }
        injected.push_str(&suffix);
        drop(injected);
        Ok(Some(payload))
    }

    pub fn finish(&self) -> Result<DictationResult> {
        let clip = self.deps.audio.stop()?;
        *self.started.lock().unwrap() = None;
        if clip.duration_ms() < 200 {
            self.deps.emit(AppEvent::State(crate::voice::State::Idle));
            return Err(Error::TooShort);
        }
        let dict = self.learned.lock().unwrap().clone();
        let pairs: Vec<(String, String)> = dict
            .iter()
            .map(|d| (d.heard.clone(), d.corrected.clone()))
            .collect();
        if !self.deps.usage.can_start(self.deps.clock.now())? {
            self.deps.emit(AppEvent::State(crate::voice::State::Idle));
            return Err(Error::BudgetExhausted);
        }
        let transcript = self.deps.stt.transcribe(&clip, None)?;
        let raw = if transcript.text.trim().is_empty() {
            self.injected.lock().unwrap().clone()
        } else {
            transcript.text
        };

        // Tail correction: only when the edit is small, to avoid flicker.
        let injected = self.injected.lock().unwrap().clone();
        let mut final_text = apply_dictionary(&raw, &pairs);
        if !injected.is_empty() {
            let tail: String = injected
                .chars()
                .rev()
                .take(40)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            let candidate = &final_text;
            if edit_distance(&tail, candidate) > (candidate.chars().count() / 4).max(4) {
                final_text = injected.clone();
            }
        }

        let target = self.deps.injector.focused().unwrap_or(TargetInfo {
            app: "unknown".into(),
            kind: TargetKind::Unknown,
        });
        if looks_secret(&final_text) {
            self.deps.emit(AppEvent::Notice {
                code: "E_SECRET".into(),
                message: "这段话看起来包含密码或验证码，我不代填。请自己输入。".into(),
            });
            self.deps.emit(AppEvent::State(crate::voice::State::Idle));
            return Ok(DictationResult {
                final_text,
                injected_text: injected,
                outcome: None,
                charged_units: 0.0,
            });
        }

        let payload = if target.kind == TargetKind::Terminal {
            terminal_safe(&final_text)
        } else {
            cleanup(&final_text)
        };
        let outcome = self.deps.injector.inject(&payload, &target).ok();
        self.deps.emit(AppEvent::Dictation {
            text: payload.clone(),
            partial: false,
        });
        self.deps.emit(AppEvent::State(crate::voice::State::Idle));

        let units = self.deps.usage.meter(&crate::usage::MeterInput {
            kind: crate::usage::UsageKind::Dictation,
            task_id: None,
            session_id: None,
            started_at: Utc::now(),
            now: Utc::now(),
            outcome: crate::usage::UsageOutcome::Completed,
            task_already_charged: false,
        })?;

        Ok(DictationResult {
            final_text: payload,
            injected_text: injected,
            outcome,
            charged_units: units,
        })
    }

    /// Learn a correction the user made within the learning window.
    pub fn learn(&self, heard: &str, corrected: &str) -> Result<bool> {
        let Some(started) = *self.started.lock().unwrap() else {
            return Ok(false);
        };
        if started.elapsed().as_secs() > 20 {
            return Ok(false);
        }
        let heard = heard.trim();
        let corrected = corrected.trim();
        if heard.is_empty() || corrected.is_empty() || heard == corrected {
            return Ok(false);
        }
        self.deps.store.upsert_dict(heard, corrected, Utc::now())?;
        self.learned.lock().unwrap().push(DictEntry {
            heard: heard.into(),
            corrected: corrected.into(),
        });
        Ok(true)
    }

    pub fn cancel(&self) -> Result<()> {
        let _ = self.deps.audio.stop();
        *self.started.lock().unwrap() = None;
        self.injected.lock().unwrap().clear();
        self.deps.emit(AppEvent::State(crate::voice::State::Idle));
        Ok(())
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_grows_the_suffix() {
        let (common, suffix) = merge_partial("hello wor", "hello world");
        assert_eq!(common, "hello wor");
        assert_eq!(suffix, "ld");
    }

    #[test]
    fn merge_is_stable_when_nothing_new() {
        let (_, suffix) = merge_partial("same", "same");
        assert_eq!(suffix, "");
    }

    #[test]
    fn merge_handles_rewrite() {
        let (common, suffix) = merge_partial("I scream", "I screen");
        assert_eq!(common, "I scre");
        assert_eq!(suffix, "en");
    }

    #[test]
    fn terminal_safety_flattens_newlines() {
        assert_eq!(terminal_safe("rm -rf /\n echo hi"), "rm -rf / echo hi");
    }

    #[test]
    fn cleanup_removes_dashes_and_capitalises() {
        assert_eq!(cleanup("hello — world"), "Hello world");
    }

    #[test]
    fn dictionary_prefers_longest_key() {
        let dict = vec![
            ("clicky".to_string(), "Clicky".to_string()),
            ("hey clicky".to_string(), "OpenClicky".to_string()),
        ];
        assert_eq!(
            apply_dictionary("hey clicky is here", &dict),
            "OpenClicky is here"
        );
    }

    #[test]
    fn secrets_are_detected() {
        assert!(looks_secret("my password is hunter2"));
        assert!(looks_secret("验证码是 123456"));
        assert!(!looks_secret("write a short reply"));
    }

    #[test]
    fn card_numbers_are_detected_by_luhn_even_without_the_word() {
        assert!(looks_secret("4111 1111 1111 1111"));
        assert!(looks_secret("4111111111111111"));
        assert!(!looks_secret("4111 1111 1111 1112"));
    }

    #[test]
    fn one_time_codes_are_detected_but_years_are_not() {
        assert!(looks_secret("the code is 123456"));
        assert!(looks_secret("use 90817263 now"));
        assert!(!looks_secret("we ship in 2026"));
        assert!(!looks_secret("it costs 1234"));
    }

    #[test]
    fn edit_distance_basic() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
