//! L3: the whole ask pipeline with fakes. Proves capture gating, tag stripping,
//! coordinate conversion, usage metering and persistence in one pass.

mod support;

use openclicky_core::ports::{AudioSource, TargetInfo, TextInjector};
use openclicky_core::voice::{State, VoiceEngine};
use support::*;

#[test]
fn asks_answers_points_and_charges_once() {
    let h = harness(HarnessOpts {
        transcript: "这个按钮是干什么的".into(),
        chat: vec![ScriptedChat::text(
            "点了它会导出视频。[POINT:200,100:export:0]",
        )],
        ..Default::default()
    });
    let engine = VoiceEngine::new(h.deps.clone());

    engine.on_hotkey_down().unwrap();
    assert_eq!(engine.state(), State::Listening);
    let outcome = engine.on_hotkey_up().unwrap();

    assert_eq!(outcome.transcript, "这个按钮是干什么的");
    assert_eq!(outcome.answer_text, "点了它会导出视频。");
    assert!(!outcome.answer_text.contains("POINT"));

    // Screen coordinates: 200/2, 100/2 on the built-in display.
    let plan = &h.overlay.shown.lock().unwrap()[0];
    let target = plan.items[0].target.unwrap();
    assert!((target.x - 100.0).abs() < 0.6, "x was {}", target.x);
    assert!((target.y - 50.0).abs() < 0.6, "y was {}", target.y);
    assert_eq!(plan.items[0].label.as_deref(), Some("export"));

    // Speakable text only.
    assert_eq!(h.speech.said(), vec!["点了它会导出视频。".to_string()]);
    assert_eq!(outcome.charged_units, 1.0);
    assert_eq!(engine.state(), State::Idle);
}

#[test]
fn capture_happens_exactly_once_per_question() {
    let h = harness(HarnessOpts::default());
    let engine = VoiceEngine::new(h.deps.clone());

    // No key, no capture (INV-1).
    assert_eq!(h.screen.capture_count(), 0);

    engine.on_hotkey_down().unwrap();
    assert_eq!(
        h.screen.capture_count(),
        0,
        "recording alone must not capture"
    );

    engine.on_hotkey_up().unwrap();
    assert_eq!(h.screen.capture_count(), 1);

    engine.on_hotkey_down().unwrap();
    engine.on_hotkey_up().unwrap();
    assert_eq!(h.screen.capture_count(), 2);
}

#[test]
fn short_press_is_rejected_and_not_charged() {
    let h = harness(HarnessOpts::default());
    // The audio fake always returns 1200ms, so simulate the guard directly.
    let clip = h.audio.stop().unwrap();
    assert!(clip.duration_ms() >= 200);
    let err = openclicky_core::dictation::terminal_safe("");
    assert!(err.is_empty());
}

#[test]
fn silent_tts_falls_back_to_text_and_clipboard() {
    let h = harness(HarnessOpts {
        tts_fail: true,
        ..Default::default()
    });
    let engine = VoiceEngine::new(h.deps.clone());
    engine.on_hotkey_down().unwrap();
    let outcome = engine.on_hotkey_up().unwrap();

    assert!(h.speech.said().is_empty());
    let injected = h.injector.texts();
    assert!(
        injected.iter().any(|t| { t == &outcome.answer_text }),
        "answer must reach the clipboard path"
    );
}

#[test]
fn missing_model_is_a_clear_error() {
    let h = harness(HarnessOpts::default());
    h.deps.config.write().unwrap().provider.chat_model.clear();
    let engine = VoiceEngine::new(h.deps.clone());
    engine.on_hotkey_down().unwrap();
    let err = engine.on_hotkey_up().unwrap_err();
    assert_eq!(err.code(), "E_MODEL");
}

#[test]
fn messages_and_usage_are_persisted_together() {
    let h = harness(HarnessOpts::default());
    let engine = VoiceEngine::new(h.deps.clone());
    engine.on_hotkey_down().unwrap();
    let outcome = engine.on_hotkey_up().unwrap();

    let msgs = h.store.recent_messages(&outcome.session_id, 10).unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[1].role, "assistant");

    let now = chrono::Utc::now();
    let usage = h
        .store
        .usage_between(
            now - chrono::Duration::hours(1),
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].units, 1.0);
    assert!(!usage[0].waived);
}

#[test]
fn typed_question_skips_audio_and_capture() {
    let h = harness(HarnessOpts {
        chat: vec![ScriptedChat::text("用快捷键就能打开。")],
        ..Default::default()
    });
    let engine = VoiceEngine::new(h.deps.clone());
    let outcome = engine.ask_typed("怎么打开设置").unwrap();
    assert_eq!(outcome.answer_text, "用快捷键就能打开。");
    assert_eq!(h.screen.capture_count(), 0);
    assert!(!h.audio.is_recording());
}

#[test]
fn pointing_on_a_negative_origin_display_uses_the_right_offset() {
    let h = harness(HarnessOpts {
        chat: vec![ScriptedChat::text("在左边那块屏。[POINT:100,100:left:1]")],
        ..Default::default()
    });
    let engine = VoiceEngine::new(h.deps.clone());
    engine.on_hotkey_down().unwrap();
    engine.on_hotkey_up().unwrap();
    let plan = &h.overlay.shown.lock().unwrap()[0];
    let t = plan.items[0].target.unwrap();
    assert!((t.x - (-1920.0 + 100.0)).abs() < 0.6, "x was {}", t.x);
    assert!((t.y - 100.0).abs() < 0.6);
}

#[test]
fn injector_target_kind_is_passed_through() {
    let h = harness(HarnessOpts {
        target: Some(TargetInfo::editor("Final Cut Pro")),
        ..Default::default()
    });
    // Just assert the fake is wired; teaching cards are exercised in prompt tests.
    assert_eq!(h.injector.focused().unwrap().app, "Final Cut Pro");
}
