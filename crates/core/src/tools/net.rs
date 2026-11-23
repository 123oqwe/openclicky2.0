//! URL parsing and SSRF guard (SEC-6).

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedUrl {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub raw: String,
}

pub fn parse(url: &str) -> Result<ParsedUrl> {
    let url = url.trim();
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| Error::BadRequest("url must start with http:// or https://".into()))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(Error::BadRequest(format!("scheme not allowed: {scheme}")));
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(Error::BadRequest("url has no host".into()));
    }
    if authority.contains('@') {
        return Err(Error::BadRequest(
            "credentials in url are not allowed".into(),
        ));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| Error::BadRequest("bad port".into()))?,
        ),
        _ => (
            authority.to_string(),
            if scheme == "https" { 443 } else { 80 },
        ),
    };
    if host.is_empty() {
        return Err(Error::BadRequest("url has no host".into()));
    }
    Ok(ParsedUrl {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
        path: path.to_string(),
        raw: url.to_string(),
    })
}

/// Blocks loopback, link-local, private ranges and metadata endpoints unless the
/// user explicitly opted into private networking.
pub fn guard_url(url: &str, allow_private: bool) -> Result<String> {
    let parsed = parse(url)?;
    if !allow_private && is_private_host(&parsed.host) {
        return Err(Error::PolicyDenied(format!(
            "拒绝访问内网地址：{}（可在设置里允许）",
            parsed.host
        )));
    }
    Ok(parsed.raw)
}

pub fn is_private_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    let lower = h.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower == "metadata.google.internal"
    {
        return true;
    }
    if lower == "::1"
        || lower.starts_with("fc")
        || lower.starts_with("fd")
        || lower.starts_with("fe80")
    {
        return true;
    }
    let parts: Vec<&str> = lower.split('.').collect();
    if parts.len() == 4 {
        if let (Ok(a), Ok(b)) = (parts[0].parse::<u8>(), parts[1].parse::<u8>()) {
            return a == 127
                || a == 10
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 169 && b == 254)
                || a == 0;
        }
    }
    false
}

/// Every host the process may talk to (used by the invariant check).
pub fn allowed_hosts(base_url: &str) -> Vec<String> {
    parse(base_url).map(|p| vec![p.host]).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_default_port() {
        let u = parse("https://api.example.com/v1/chat").unwrap();
        assert_eq!(u.port, 443);
        assert_eq!(u.host, "api.example.com");
        assert_eq!(u.path, "/v1/chat");
    }

    #[test]
    fn rejects_non_http_scheme() {
        assert!(parse("file:///etc/passwd").is_err());
        assert!(parse("ftp://x/y").is_err());
        assert!(parse("no-scheme").is_err());
    }

    #[test]
    fn blocks_loopback_and_private() {
        for url in [
            "http://127.0.0.1:8080/x",
            "http://localhost/x",
            "http://10.1.2.3/x",
            "http://192.168.0.1/x",
            "http://169.254.169.254/latest/meta-data",
            "http://172.20.0.1/x",
        ] {
            assert!(guard_url(url, false).is_err(), "{url} should be blocked");
        }
    }

    #[test]
    fn allows_public_and_opt_in_private() {
        assert!(guard_url("https://example.com/a", false).is_ok());
        assert!(guard_url("http://127.0.0.1:11434/v1", true).is_ok());
    }

    #[test]
    fn rejects_credentials_in_url() {
        assert!(parse("https://user:pass@example.com/x").is_err());
    }
}
