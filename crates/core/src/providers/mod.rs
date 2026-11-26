//! Model providers: OpenAI-compatible and Anthropic. Thin by design: no SDK,
//! no async runtime, and the transport is injected so tests never touch the net.

pub mod anthropic;
pub mod openai;

use std::sync::Arc;

use crate::config::{Config, ProviderKind};
use crate::error::{Error, Result};
use crate::ports::{ChatProvider, HttpTransport, SecretString, Secrets};

pub const KEY_NAME: &str = "provider/default";

/// Build the provider described by the configuration.
pub fn build(
    config: &Config,
    http: Arc<dyn HttpTransport>,
    secrets: Arc<dyn Secrets>,
) -> Result<Arc<dyn ChatProvider>> {
    let key = secrets.get(KEY_NAME)?.or_else(|| {
        secrets
            .get(&format!("provider/{}", config.provider.kind.as_str()))
            .ok()
            .flatten()
    });
    let key = key.ok_or(Error::NoApiKey)?;
    Ok(with_key(config, http, key))
}

/// Build a provider around a key the caller has not stored. The settings screen
/// needs to try a key before deciding whether to keep it, and writing a wrong key
/// into the keychain first is how a broken setting becomes the saved one.
pub fn build_with_key(
    config: &Config,
    http: Arc<dyn HttpTransport>,
    key: &SecretString,
) -> Result<Arc<dyn ChatProvider>> {
    Ok(with_key(config, http, key.clone()))
}

fn with_key(
    config: &Config,
    http: Arc<dyn HttpTransport>,
    key: SecretString,
) -> Arc<dyn ChatProvider> {
    match config.provider.kind {
        ProviderKind::Anthropic => Arc::new(anthropic::Anthropic::new(http, config.clone(), key)),
        _ => Arc::new(openai::OpenAi::new(http, config.clone(), key)),
    }
}

/// What a save has to prove before anything is written. Listing the models is
/// the only cheap way to tell a working key from a typo, and comparing the
/// chosen model against that list is the only way to tell a real model from a
/// stale one. Nothing is persisted here, so the rule is testable without a
/// keychain or a config file.
pub fn check_save(
    config: &Config,
    http: Arc<dyn HttpTransport>,
    candidate: Option<&SecretString>,
    model: &str,
) -> Result<Vec<String>> {
    let key = candidate.ok_or(Error::NoApiKey)?;
    let models = build_with_key(config, http, key)?.list_models()?;
    let wanted = model.trim();
    // No model chosen yet is not a reason to skip the check. Listing the models
    // is what proves the key, and that has already happened above.
    if wanted.is_empty() {
        return Ok(models);
    }
    if !models.iter().any(|m| m == wanted) {
        return Err(Error::BadRequest(format!(
            "模型 {wanted} 不在这个 provider 的列表里"
        )));
    }
    Ok(models)
}

/// Names what a request was aimed at. A refusal from one endpoint says nothing
/// about another, and a message that omits both leaves the user guessing which
/// setting to change.
/// The one save sequence. Two callers had their own copy of "check, then store
/// the key, then write the file", and only the order separates a refused save
/// from a clobbered working configuration. Keeping it here means a test can call
/// the whole thing and observe both the error and the untouched file.
pub fn save_provider(
    config: &mut Config,
    path: &std::path::Path,
    http: Arc<dyn HttpTransport>,
    secrets: &dyn Secrets,
    candidate: Option<&SecretString>,
    model: &str,
) -> Result<()> {
    config.provider.chat_model = model.trim().to_string();

    let key = match candidate {
        Some(k) => k.clone(),
        None => secrets.get(KEY_NAME)?.ok_or(Error::NoApiKey)?,
    };
    check_save(config, http, Some(&key), model)?;

    if candidate.is_some() {
        secrets.set(KEY_NAME, &key)?;
        if !secrets.is_durable() {
            return Err(Error::BadRequest(
                "这个系统没有接受写入钥匙串,key 会在这个进程结束时消失。改为存到本目录的加密文件。(E_KEY_NOT_STORED)"
                    .into(),
            ));
        }
    }
    config.save(path)
}

pub fn describe_target(config: &Config) -> String {
    format!(
        "{} @ {}",
        config.provider.kind.as_str(),
        config.provider.base_url
    )
}

pub(crate) fn region_blocked(status: u16, body: &str) -> bool {
    let b = body.to_lowercase();
    status == 403
        && (b.contains("region") || b.contains("country") || b.contains("unsupported_location"))
}

pub(crate) fn classify_status(status: u16, body: &str, retry_after: Option<f64>) -> Error {
    match status {
        403 if region_blocked(status, body) => Error::RegionBlocked,
        401 | 403 => Error::AuthFailed,
        429 => Error::RateLimited { retry_after },
        500..=599 => Error::Outage,
        400..=499 => Error::BadRequest(format!("HTTP {status}: {}", truncate(body, 200))),
        _ => Error::Protocol(format!(
            "unexpected status {status}: {}",
            truncate(body, 200)
        )),
    }
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub(crate) fn retry_after(headers: &[(String, String)]) -> Option<f64> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, v)| v.trim().parse::<f64>().ok())
}
