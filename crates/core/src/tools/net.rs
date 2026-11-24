//! URL parsing and SSRF guard (SEC-6).
//!
//! Two layers, because string checks alone are bypassable:
//! 1. `guard_url` rejects a literal private host (including IPv6 forms and the
//!    legacy IPv4 shorthands).
//! 2. `guard_resolved` also resolves the name and rejects any address in a
//!    private range, so `localtest.me` style names cannot slip through.
//!
//! Redirects are disabled at the transport (`agents` -> `redirects(0)`), so a
//! public URL cannot bounce the request to `127.0.0.1` after the check.

use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

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
    let default_port = if scheme == "https" { 443 } else { 80 };
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

    let (host, port) = if let Some(after_bracket) = authority.strip_prefix('[') {
        // IPv6 literal: [::1] or [::1]:8080
        let (inner, rest) = after_bracket
            .split_once(']')
            .ok_or_else(|| Error::BadRequest("unterminated IPv6 host".into()))?;
        let port = match rest {
            "" => default_port,
            _ => {
                let p = rest
                    .strip_prefix(':')
                    .ok_or_else(|| Error::BadRequest("bad authority".into()))?;
                p.parse::<u16>()
                    .map_err(|_| Error::BadRequest("bad port".into()))?
            }
        };
        (inner.to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (
                h.to_string(),
                p.parse::<u16>()
                    .map_err(|_| Error::BadRequest("bad port".into()))?,
            ),
            None => (authority.to_string(), default_port),
        }
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

/// Rejects literal private hosts. Cheap, no DNS.
pub fn guard_url(url: &str, allow_private: bool) -> Result<String> {
    if url.len() > 4096 {
        return Err(Error::BadRequest("url is too long".into()));
    }
    let parsed = parse(url)?;
    if !allow_private && is_private_host(&parsed.host) {
        return Err(Error::PolicyDenied(format!(
            "拒绝访问内网地址：{}（可在设置里允许）",
            parsed.host
        )));
    }
    Ok(parsed.raw)
}

/// Rejects literal private hosts and any name that resolves to one.
/// Resolution failure is not treated as private: the request itself will fail
/// and report a network error, which is the honest outcome.
pub fn guard_resolved(url: &str, allow_private: bool) -> Result<String> {
    let parsed = parse(url)?;
    if allow_private {
        return Ok(parsed.raw);
    }
    if is_private_host(&parsed.host) {
        return Err(Error::PolicyDenied(format!(
            "拒绝访问内网地址：{}（可在设置里允许）",
            parsed.host
        )));
    }
    let addrs = (parsed.host.as_str(), parsed.port).to_socket_addrs();
    if let Ok(addrs) = addrs {
        for addr in addrs {
            if is_private_ip(addr.ip()) {
                return Err(Error::PolicyDenied(format!(
                    "拒绝访问内网地址：{} 解析到 {}（可在设置里允许）",
                    parsed.host,
                    addr.ip()
                )));
            }
        }
    }
    Ok(parsed.raw)
}

/// True when the URL carries an outbound payload rather than naming a page.
/// A GET with a long query string is the only way these tools can send data,
/// so it must go through approval (SEC-4).
pub fn url_carries_payload(url: &str) -> bool {
    let Some((_, query)) = url.split_once('?') else {
        return false;
    };
    let query = query.split('#').next().unwrap_or(query);
    if query.len() > 200 {
        return true;
    }
    query
        .split('&')
        .filter_map(|kv| kv.split_once('=').map(|(_, v)| v))
        .any(|v| v.len() > 120)
}

pub fn is_private_host(host: &str) -> bool {
    let h = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if h.is_empty() {
        return true;
    }
    if h == "localhost" || h.ends_with(".localhost") || h == "metadata.google.internal" {
        return true;
    }
    if let Ok(ip) = h.parse::<IpAddr>() {
        return is_private_ip(ip);
    }
    // Legacy IPv4 shorthands (`127.1`, `2130706433`, `0x7f.1`).
    match inet_aton(&h) {
        Some(v4) => is_private_ip(IpAddr::V4(v4)),
        None => false,
    }
}

pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() {
                return true;
            }
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_v4(v4);
            }
            let s = v6.segments();
            (s[0] & 0xfe00) == 0xfc00        // fc00::/7 unique local
                || (s[0] & 0xffc0) == 0xfe80 // fe80::/10 link local
                || (s[0] & 0xffc0) == 0xfec0 // fec0::/10 site local (deprecated)
        }
    }
}

fn is_private_v4(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    let (a, b) = (o[0], o[1]);
    a == 0
        || a == 10
        || a == 127
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
        || (a == 255 && b == 255)
}

/// Accept the legacy shorthands (`127.1`, `2130706433`, `0x7f.1`) the way
/// `inet_aton` does, so the guard cannot be bypassed by notation.
fn inet_aton(s: &str) -> Option<Ipv4Addr> {
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
    Some(Ipv4Addr::from(value as u32))
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
    fn parses_ipv6_with_port() {
        let u = parse("http://[::1]:11434/v1").unwrap();
        assert_eq!(u.host, "::1");
        assert_eq!(u.port, 11434);
        let v = parse("https://[fd00::1]/x").unwrap();
        assert_eq!(v.host, "fd00::1");
        assert_eq!(v.port, 443);
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
            "http://[::1]:11434/v1",
            "http://[::]:80/x",
            "http://[fd00::1]/x",
            "http://[fe80::1]/x",
            "http://[::ffff:127.0.0.1]/x",
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
            "http://0.0.0.0/x",
        ] {
            assert!(guard_url(url, false).is_err(), "{url} must be blocked");
        }
    }

    #[test]
    fn allows_public_and_opt_in_private() {
        assert!(guard_url("https://example.com/a", false).is_ok());
        assert!(guard_url("http://127.0.0.1:11434/v1", true).is_ok());
        // A hostname that merely starts with "fc" is not an IPv6 range.
        assert!(guard_url("https://fczone.example.com/x", false).is_ok());
    }

    #[test]
    fn resolution_catches_names_that_point_home() {
        // `localhost` needs no network to resolve.
        assert!(guard_resolved("http://localhost:8080/x", false).is_err());
        // Opt-in keeps local model endpoints usable.
        assert!(guard_resolved("http://localhost:8080/x", true).is_ok());
    }

    #[test]
    fn long_query_counts_as_an_outbound_payload() {
        assert!(!url_carries_payload("https://example.com/docs?page=2"));
        assert!(url_carries_payload(&format!(
            "https://example.com/?d={}",
            "x".repeat(200)
        )));
        assert!(!url_carries_payload("https://example.com/?a=1&b=2"));
    }

    #[test]
    fn rejects_credentials_in_url() {
        assert!(parse("https://user:pass@example.com/x").is_err());
    }
}
