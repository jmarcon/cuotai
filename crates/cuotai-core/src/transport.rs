//! Transport guard: credentials and tokens only travel over HTTPS.
//!
//! Every endpoint Cuotai talks to is an `https://` literal or is derived from
//! one, so this is a second barrier rather than a feature: a mistake in a
//! derived URL, a bad parameter or a future refactor fails closed instead of
//! sending a secret in clear text. Plain HTTP is accepted only for loopback
//! hosts, which never leave the machine (local OAuth callbacks and the
//! `wiremock` servers the contract tests run against).

use std::net::IpAddr;
use url::{Host, Url};

/// True when `url` may carry a credential: HTTPS, or HTTP to a loopback host.
pub(crate) fn is_secure(url: &Url) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => match url.host() {
            Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
            Some(Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
            Some(Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
            None => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same check for a raw string; anything that does not parse is refused.
    fn is_secure_str(raw: &str) -> bool {
        Url::parse(raw).is_ok_and(|url| is_secure(&url))
    }

    #[test]
    fn https_is_always_secure() {
        assert!(is_secure_str("https://api.example.com/v1/usage"));
        assert!(is_secure_str("HTTPS://api.example.com"));
    }

    #[test]
    fn plain_http_is_secure_only_for_loopback() {
        for ok in [
            "http://127.0.0.1:8080/token",
            "http://localhost/token",
            "http://LOCALHOST:3000",
            "http://[::1]:9000/x",
            "http://127.1.2.3/x",
        ] {
            assert!(is_secure_str(ok), "{ok}");
        }
        for bad in [
            "http://api.example.com/token",
            "http://10.0.0.5/token",
            "http://192.168.1.10",
            "http://localhost.evil.com/token",
            "http://127.0.0.1.evil.com/token",
            "http://unused",
        ] {
            assert!(!is_secure_str(bad), "{bad}");
        }
    }

    #[test]
    fn other_schemes_and_garbage_are_refused() {
        for bad in ["ftp://example.com", "file:///etc/passwd", "not a url", ""] {
            assert!(!is_secure_str(bad), "{bad}");
        }
    }
}
