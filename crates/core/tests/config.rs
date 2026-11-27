//! A config file outlives the build that wrote it.
//!
//! `#133` removed two keys that nothing read: `provider.tts_model` and
//! `provider.region_hint`. A file on disk still carries them, so the thing worth
//! pinning is that removing them did not make an existing file unreadable.

use openclicky_core::config::Config;

const AN_OLD_FILE: &str = r#"
config_version = 1

[provider]
kind = "openai"
base_url = "https://api.openai.com/v1"
chat_model = "gpt-4o"
tts_model = "gpt-4o-mini-tts"
region_hint = "us-east"

[voice]
captions = true
max_answer_tokens = 900
"#;

#[test]
fn a_file_that_still_names_the_retired_keys_parses() {
    let cfg = Config::from_toml(AN_OLD_FILE).expect("a retired key must not be a parse error");
    assert_eq!(cfg.provider.chat_model, "gpt-4o");
    assert_eq!(cfg.provider.base_url, "https://api.openai.com/v1");
    assert_eq!(
        cfg.voice.max_answer_tokens, 900,
        "everything around the retired keys still arrives"
    );
}

#[test]
fn the_retired_keys_do_not_survive_a_save() {
    let cfg = Config::from_toml(AN_OLD_FILE).unwrap();
    let written = cfg.to_toml();
    assert!(
        !written.contains("tts_model"),
        "nothing reads it, so it must not be written back: {written}"
    );
    assert!(
        !written.contains("region_hint"),
        "nothing reads it, so it must not be written back: {written}"
    );
}
