//! One config lock, or two halves of the app disagree.
//!
//! Before this, `Deps::new` took a `Config` by value and wrapped it in its own
//! `RwLock`, while the desktop shell kept a second lock of its own. `save_provider`
//! wrote the shell's lock; `voice.rs` read the engine's. A save therefore changed
//! the settings page and changed what the HTTP layer sent, and left the model name
//! in the request at whatever it had been at start-up.

mod support;

use std::sync::Arc;

use openclicky_core::ports::{ChatProvider, SecretString};
use openclicky_core::providers::openai::OpenAi;
use openclicky_core::voice::VoiceEngine;
use support::{harness, HarnessOpts};

#[test]
fn the_engine_reads_the_lock_the_caller_handed_over() {
    let h = harness(HarnessOpts::default());
    assert!(
        Arc::ptr_eq(&h.deps.config, &h.config),
        "a second lock is how a save and a question ended up using different models"
    );
}

#[test]
fn a_question_after_a_save_asks_with_the_model_that_was_saved() {
    let h = harness(HarnessOpts::default());
    assert_eq!(
        h.deps.config_snapshot().provider.chat_model,
        "fake-model",
        "the harness starts with its own model, so the change below is the only explanation"
    );

    {
        let mut cfg = h.config.write().unwrap();
        cfg.provider.chat_model = "the-model-the-user-picked".to_string();
    }

    let engine = VoiceEngine::new(h.deps.clone());
    let outcome = engine.ask_typed("这个按钮是干什么的").unwrap();
    assert_eq!(outcome.model, "the-model-the-user-picked");

    let sent = h.chat.last_request().expect("the question reached a model");
    assert_eq!(
        sent.model, "the-model-the-user-picked",
        "the request carries the saved model, not the one from start-up"
    );
}

#[test]
fn after_a_save_the_address_the_engine_reads_is_the_one_on_the_wire() {
    let h = harness(HarnessOpts::default());
    {
        let mut cfg = h.config.write().unwrap();
        cfg.provider.base_url = "http://127.0.0.1:8795/v1".to_string();
    }

    let provider = OpenAi::new(
        h.http.clone(),
        h.deps.config_snapshot(),
        SecretString::from("sk-mock"),
    );
    provider.list_models().expect("the fake answers");

    assert_eq!(
        h.http.requests.lock().unwrap().last().map(String::as_str),
        Some("http://127.0.0.1:8795/v1/models"),
        "the saved address is the address asked, #143"
    );
}
