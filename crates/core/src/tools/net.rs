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
    let h = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") || h == "metadata.google.internal" {
        return true;
    }
    if h == "::1" || h.starts_with("fc") || h.starts_with("fd") || h.starts_with("fe80") {
        return true;
    }
    if !h.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
        return false;
    }
    let Some(addr) = inet_aton(&h) else {
        return false;
    };
    let octets = addr.octets();
    let (a, b) = (octets[0], octets[1]);
    a == 0
        || a == 10
        || a == 127
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
}

/// Accept the legacy shorthands (`127.1`, `2130706433`, `0x7f.1`) the way
/// `inet_aton` does, so the guard cannot be bypassed by notation.
fn inet_aton(s: &str) -> Option<std::net::Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut nums: Vec<u32> = Vec::with_capacity(parts.len());
    for p in &parts {
        let n = if let Some(hex) = p.strip_prefix("0x") {
            u32::from_str_radix(hex, 16).ok()?
        } else if p.len() > 1 && p.starts_with('0') {
            u32::from_str_radix(&p[1..], 8).ok()?
        } else {
            p.parse::<u32>().ok()?
        };
        nums.push(n);
    }
    let value: u64 = match nums.len() {
        1 => nums[0] as u64,
        2 => ((nums[0] as u64) << 24) | nums[1] as u64,
        3 => ((nums[0] as u64) << 24) | ((nums[1] as u64) << 16) | nums[2] as u64,
        _ => {
            ((nums[0] as u64) << 24)
                | ((nums[1] as u64) << 16)
                | ((nums[2] as u64) << 8)
                | nums[3] as u64
        }
    };
    if value > u32::MAX as u64 {
        return None;
    }
    Some(std::net::Ipv4Addr::from(value as u32))
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
    fn blocks_shorthand_ipv4_forms() {
        for url in [
            "http://127.1/x",
            "http://2130706433/x",
            "http://0x7f.0.0.1/x",
            "http://127.0.0.2/x",
            "http://100.64.0.1/x",
        ] {
            assert!(guard_url(url, false).is_err(), "{url} must be blocked");
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
