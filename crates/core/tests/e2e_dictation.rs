//! L3: dictation merges partials, injects only the suffix, and never lets a
//! terminal or a password field get a dangerous payload.

mod support;

use openclicky_core::dictation::DictationEngine;
use openclicky_core::ports::{TargetInfo, TargetKind, TextInjector};
use openclicky_core::text::terminal_safe;
use support::*;

#[test]
fn only_the_new_suffix_is_typed() {
    let h = harness(HarnessOpts::default());
    // Two partials then the final transcript.
    let engine = DictationEngine::new(h.deps.clone());
    // Replace the transcriber script by driving tick twice with a fake.
    engine.start().unwrap();
    // The harness transcriber returns the same text each time, so the second
    // tick must produce no new suffix.
    let first = engine.tick().unwrap();
    let second = engine.tick().unwrap();
    assert!(first.is_some());
    assert_eq!(second, None, "nothing new means nothing typed");
    let result = engine.finish().unwrap();
    assert!(result.charged_units > 0.0);
}

#[test]
fn terminal_targets_get_a_single_line() {
    let h = harness(HarnessOpts {
        target: Some(TargetInfo::terminal("iTerm2")),
        ..Default::default()
    });
    let engine = DictationEngine::new(h.deps.clone());
    engine.start().unwrap();
    engine.tick().unwrap();
    let result = engine.finish().unwrap();
    assert!(!result.final_text.contains('\n'));
    assert_eq!(result.final_text, terminal_safe(&result.final_text));
    assert_eq!(h.injector.focused().unwrap().kind, TargetKind::Terminal);
}

#[test]
fn password_like_speech_is_not_injected() {
    let h = harness(HarnessOpts {
        transcript: "my password is hunter2".into(),
        ..Default::default()
    });
    let engine = DictationEngine::new(h.deps.clone());
    engine.start().unwrap();
    engine.tick().unwrap();
    let result = engine.finish().unwrap();
    assert_eq!(result.charged_units, 0.0);
    assert_eq!(result.outcome, None);
    assert!(
        !h.injector.texts().iter().any(|t| t.contains("hunter2")),
        "secrets must never be typed: {:?}",
        h.injector.texts()
    );
}

#[test]
fn corrections_are_learned_and_reused() {
    let h = harness(HarnessOpts {
        transcript: "open clicky".into(),
        ..Default::default()
    });
    let engine = DictationEngine::new(h.deps.clone());
    engine.start().unwrap();
    let learned = engine.learn("clicky", "Clicky").unwrap();
    assert!(learned, "a fresh dictation may learn a correction");
    let dict = h.store.dict().unwrap();
    assert!(dict.iter().any(|(h, c)| h == "clicky" && c == "Clicky"));
    engine.finish().unwrap();
}

#[test]
fn learning_window_closes_after_finish() {
    let h = harness(HarnessOpts::default());
    let engine = DictationEngine::new(h.deps.clone());
    engine.start().unwrap();
    engine.finish().unwrap();
    assert!(
        !engine.learn("a", "b").unwrap(),
        "no learning outside a session"
    );
}

#[test]
fn tick_refuses_password_like_speech() {
    let h = harness(HarnessOpts {
        transcript: "my password is hunter2".into(),
        ..Default::default()
    });
    let engine = DictationEngine::new(h.deps.clone());
    engine.start().unwrap();
    let typed = engine.tick().unwrap();
    assert_eq!(typed, None, "nothing may be typed before the secret check");
    assert!(
        !h.injector.texts().iter().any(|t| t.contains("hunter2")),
        "injected: {:?}",
        h.injector.texts()
    );
}
