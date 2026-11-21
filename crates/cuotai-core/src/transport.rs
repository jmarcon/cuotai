//! Transport guard: credentials and tokens only travel over HTTPS.
//!
//! Every endpoint Cuotai talks to is an `https://` literal or is derived from
//! one, so this is a second barrier rather than a feature: a mistake in a
//! derived URL, a bad parameter or a future refactor fails closed instead of
//! sending a secret in clear text. Plain HTTP is accepted only for loopback
//! hosts, which never leave the machine (local OAuth callbacks and the
//! `wiremock` servers the contract tests run against).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
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

/// True for the hosts that never leave the machine: `localhost`,
/// `127.0.0.0/8` and `::1`.
pub(crate) fn is_loopback_host(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(ip) => IpAddr::V4(*ip).is_loopback(),
        Host::Ipv6(ip) => IpAddr::V6(*ip).is_loopback(),
    }
}

fn private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_private() || ip.is_link_local() || (a == 100 && (64..128).contains(&b))
}

fn private_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    // Unique local fc00::/7 and link-local fe80::/10.
    (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
}

/// PRD 0036 RF2.3: RFC 1918, link-local, CGNAT/Tailscale `100.64.0.0/10`,
/// ULA and `.local` / `.ts.net` names: the only hosts 9router's
/// private-network opt-in (`allow_private_http`) covers, for a
/// password-less instance only. No secret ever travels to them over plain
/// HTTP (security review 2026-11-21, L-4: the LiteLLM exception is gone).
pub(crate) fn is_private_network(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => {
            let domain = domain.to_ascii_lowercase();
            domain.ends_with(".local") || domain.ends_with(".ts.net")
        }
        Host::Ipv4(ip) => private_v4(*ip),
        Host::Ipv6(ip) => private_v6(*ip),
    }
}

/// The yes of a boolean account parameter: `"true"` or `"1"`, any case,
/// surrounding spaces ignored (one rule for every aggregator option).
pub(crate) fn truthy(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1"))
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

    // security-invariant: SEC-TRANSPORT (PRD 0050 RF7.1)
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

    /// Security review 2026-11-21, L-4 (owner rule: plain HTTP only to
    /// loopback): no private-network host may receive a secret over plain
    /// HTTP, whatever an account chose.
    #[test]
    fn no_secret_travels_over_plain_http_to_a_private_network() {
        for private in [
            "http://192.168.1.10:4000/provider/budgets",
            "http://10.0.0.5/x",
            "http://100.100.1.2/x",
            "http://box.local/x",
            "http://box.tail1234.ts.net/x",
            "http://[fd00::1]/x",
        ] {
            assert!(!is_secure_str(private), "{private}");
            let url = Url::parse(private).unwrap();
            assert!(is_private_network(&url.host().unwrap()), "{private}");
        }
        for yes in ["true", "TRUE", " 1 ", "True"] {
            assert!(truthy(Some(yes)), "{yes}");
        }
        for no in ["false", "0", "yes", "", " "] {
            assert!(!truthy(Some(no)), "{no}");
        }
        assert!(!truthy(None));
    }

    #[test]
    fn other_schemes_and_garbage_are_refused() {
        for bad in ["ftp://example.com", "file:///etc/passwd", "not a url", ""] {
            assert!(!is_secure_str(bad), "{bad}");
        }
    }
}
