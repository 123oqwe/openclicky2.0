//! Model providers: OpenAI-compatible and Anthropic. Thin by design: no SDK,
//! no async runtime, and the transport is injected so tests never touch the net.

pub mod anthropic;
pub mod openai;

use std::sync::Arc;

use crate::config::{Config, ProviderKind};
use crate::error::{Error, Result};
use crate::ports::{ChatProvider, HttpTransport, Secrets};

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
    Ok(match config.provider.kind {
        ProviderKind::Anthropic => Arc::new(anthropic::Anthropic::new(http, config.clone(), key)),
        _ => Arc::new(openai::OpenAi::new(http, config.clone(), key)),
    })
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
