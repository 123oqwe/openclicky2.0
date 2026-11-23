//! Log redaction, secret detection and digests.

use sha2::{Digest, Sha256};

const MASK: &str = "***";

/// Remove anything that looks like a credential. Used on every log line and on
/// every user-facing diagnostic bundle.
pub fn redact(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for token in input.split_inclusive(|c: char| c.is_whitespace()) {
        out.push_str(&redact_token(token));
    }
    out
}

fn redact_token(token: &str) -> String {
    let trimmed = token.trim_end_matches(|c: char| {
        c.is_whitespace() || c == ',' || c == ';' || c == '"' || c == '\'' || c == ')' || c == ']'
    });
    let suffix_len = token.len() - trimmed.len();

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("bearer") || lower.starts_with("authorization:") {
        return format!("{MASK}{}", &token[token.len() - suffix_len..]);
    }
    if let Some((k, _)) = trimmed.split_once('=') {
        let key = k
            .trim_matches(|c: char| c == '"' || c == '\'')
            .to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "api_key"
                | "apikey"
                | "key"
                | "token"
                | "access_token"
                | "authorization"
                | "x-api-key"
                | "password"
                | "secret"
        ) {
            return format!("{k}={MASK}{}", &token[token.len() - suffix_len..]);
        }
    }
    if looks_like_key(trimmed) {
        return format!("{MASK}{}", &token[token.len() - suffix_len..]);
    }
    token.to_string()
}

/// Provider keys, long hex/base64 blobs and `sk-`-style prefixes.
pub fn looks_like_key(s: &str) -> bool {
    let s = s.trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ';');
    if s.len() >= 12 {
        let lowered = s.to_ascii_lowercase();
        if lowered.starts_with("sk-") || lowered.starts_with("xoxb-") || lowered.starts_with("ghp_")
        {
            return true;
        }
    }
    let alnum = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if alnum && s.len() >= 32 {
        let hex = s.chars().all(|c| c.is_ascii_hexdigit());
        if hex || s.len() >= 40 {
            return true;
        }
    }
    false
}

/// True when the *content* should not be written to logs (user text that looks
/// like an inline credential).
pub fn contains_secret(text: &str) -> bool {
    text.split_whitespace().any(looks_like_key)
}

pub fn digest_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let out = hasher.finalize();
    out.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Strip control characters from a value destined for a log field.
pub fn log_safe(input: &str, max: usize) -> String {
    let cleaned: String = input.chars().filter(|c| !c.is_control()).collect();
    let redacted = redact(&cleaned);
    redacted.chars().take(max).collect()
}
