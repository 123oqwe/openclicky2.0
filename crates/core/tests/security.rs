//! L2/L3 security invariants: capture gating, redaction, deletion reporting.

mod support;

use openclicky_core::security::{contains_secret, redact};
use openclicky_core::store::Store;
use openclicky_core::voice::VoiceEngine;
use support::*;

#[test]
fn no_capture_without_a_gate() {
    let h = harness(HarnessOpts::default());
    assert_eq!(h.screen.capture_count(), 0);
    // Idle for "ten minutes": no timer, no capture.
    for _ in 0..10 {
        std::thread::yield_now();
    }
    assert_eq!(h.screen.capture_count(), 0);
}

#[test]
fn capture_gate_counts_only_user_actions() {
    let h = harness(HarnessOpts::default());
    let engine = VoiceEngine::new(h.deps.clone());
    engine.on_hotkey_down().unwrap();
    engine.on_hotkey_up().unwrap();
    assert_eq!(h.deps.gate.opens(), 1);
    assert_eq!(h.screen.capture_count(), 1);
}

#[test]
fn credentials_are_redacted_from_logs() {
    // Build the fixture at runtime so no token-like literal sits in the repo.
    let secret = ["abcdef12", "34567890", "abcdef12", "34567890", "abcdefgh"].concat();
    let header = ["Author", "ization: Bearer "].concat();
    let key = ["api", "_key="].concat();
    let line = format!("{header}{secret} {key}{secret} done");
    let out = redact(&line);
    assert!(!out.contains(&secret), "secret survived: {out}");
    assert!(out.contains("done"), "non-secret text must survive: {out}");
    assert!(out.contains("***"), "mask must be visible: {out}");
}
#[test]
fn secret_detection_catches_inline_keys() {
    assert!(contains_secret("here is my key sk-abcdefghijklmnop"));
    assert!(!contains_secret("a normal sentence about nothing"));
}

#[test]
fn deleting_everything_reports_each_table() {
    let store = Store::open_memory().unwrap();
    let now = chrono::Utc::now();
    let session = store.insert_session("ask", now).unwrap();
    store
        .append_message(&openclicky_core::store::MessageRow {
            id: "m1".into(),
            session_id: session,
            role: "user".into(),
            content: "hi".into(),
            model: None,
            created_at: now,
            meta_json: None,
        })
        .unwrap();

    let report = store.delete_everything().unwrap();
    assert!(
        report.all_ok(),
        "deletion failures: {:?}",
        report.failures()
    );
    assert!(report.steps.len() >= 9);
}
