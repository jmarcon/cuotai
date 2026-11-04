//! PRD 0050 RF4.2: the exact web origins a browser-mode local server answers.
//!
//! An origin is `https://host[:port]`, or `http://` for `localhost` and the
//! loopback addresses only. It carries no credentials, no path beyond `/`,
//! no query or fragment, no wildcard and is never the opaque `null`.
//! Comparison is on the normalized scheme, lowercase host and effective
//! port, so `https://Example.com:443/` equals `https://example.com`.

use std::fmt;
use std::net::{IpAddr, Ipv6Addr};

use crate::text::Localized;

/// The most origins one server accepts.
pub const MAX_ORIGINS: usize = 16;

/// One normalized web origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Origin {
    https: bool,
    /// Lowercase name, dotted IPv4 or bracketed IPv6.
    host: String,
    /// The effective port (443 or 80 when the text had none).
    port: u16,
}

/// Why a text is not an [`Origin`]. Messages never echo the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OriginError {
    #[error("Use https://host[:port], or http:// only for localhost and loopback addresses.")]
    Scheme,
    #[error("An origin has no user name, password, path, query or fragment.")]
    Extra,
    #[error("An origin names one host: no wildcard, null or empty host.")]
    Host,
    #[error("The origin's port must be a number from 1 to 65535.")]
    Port,
}

impl OriginError {
    pub fn l10n(&self) -> Localized {
        let id = match self {
            Self::Scheme => "serve.origin.error.scheme",
            Self::Extra => "serve.origin.error.extra",
            Self::Host => "serve.origin.error.host",
            Self::Port => "serve.origin.error.port",
        };
        Localized::fixed(id, self.to_string())
    }
}

impl Origin {
    /// Parses and normalizes one origin (see the module docs).
    pub fn parse(text: &str) -> Result<Self, OriginError> {
        let text = text.trim();
        let lower = text.to_ascii_lowercase();
        let (https, rest) = if let Some(rest) = lower.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = lower.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(OriginError::Scheme);
        };
        // A single trailing `/` is how some tools print an origin.
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if rest.contains(['/', '?', '#', '@', '\\']) {
            return Err(OriginError::Extra);
        }
        if rest.is_empty()
            || rest.contains('*')
            || rest.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(OriginError::Host);
        }
        let (host, port) = split_host_port(rest)?;
        let port = match port {
            None => {
                if https {
                    443
                } else {
                    80
                }
            }
            Some(port) => {
                if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(OriginError::Port);
                }
                match port.parse::<u16>() {
                    Ok(0) | Err(_) => return Err(OriginError::Port),
                    Ok(port) => port,
                }
            }
        };
        let host = normalize_host(host)?;
        if !https && !is_loopback_host(&host) {
            return Err(OriginError::Scheme);
        }
        Ok(Self { https, host, port })
    }

    /// Whether the raw `Origin` header value names this origin.
    pub fn matches_header(&self, value: &[u8]) -> bool {
        std::str::from_utf8(value)
            .ok()
            // A browser never sends a trailing slash or a path; refuse them.
            .filter(|value| !value.trim().ends_with('/'))
            .and_then(|value| Self::parse(value).ok())
            .is_some_and(|origin| &origin == self)
    }

    fn default_port(&self) -> u16 {
        if self.https {
            443
        } else {
            80
        }
    }
}

impl fmt::Display for Origin {
    /// The canonical form, as a browser sends it (`https://example.com`,
    /// `http://localhost:8080`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.https { "https" } else { "http" };
        if self.port == self.default_port() {
            write!(f, "{scheme}://{}", self.host)
        } else {
            write!(f, "{scheme}://{}:{}", self.host, self.port)
        }
    }
}

impl serde::Serialize for Origin {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Origin {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Parses a list from a user: one origin per entry, every one valid, no
/// duplicates after normalization, 1 to [`MAX_ORIGINS`] in all. The error
/// names the 1-based position of the first bad entry (0 for the list).
pub fn parse_origins<S: AsRef<str>>(
    entries: &[S],
) -> Result<Vec<Origin>, (usize, OriginListError)> {
    let mut out: Vec<Origin> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let origin =
            Origin::parse(entry.as_ref()).map_err(|e| (index + 1, OriginListError::Invalid(e)))?;
        if out.contains(&origin) {
            return Err((index + 1, OriginListError::Duplicate));
        }
        out.push(origin);
    }
    check_origin_count(out.len()).map_err(|e| (0, e))?;
    Ok(out)
}

/// The browser-mode list must hold 1 to [`MAX_ORIGINS`] origins.
pub fn check_origin_count(count: usize) -> Result<(), OriginListError> {
    match count {
        0 => Err(OriginListError::Empty),
        n if n > MAX_ORIGINS => Err(OriginListError::TooMany),
        _ => Ok(()),
    }
}

/// Why a list of origins is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OriginListError {
    #[error("{0}")]
    Invalid(OriginError),
    #[error("This origin is already in the list.")]
    Duplicate,
    #[error("Add at least one trusted origin before allowing browsers.")]
    Empty,
    #[error("At most 16 trusted origins are allowed.")]
    TooMany,
}

impl OriginListError {
    pub fn l10n(&self) -> Localized {
        match self {
            Self::Invalid(e) => e.l10n(),
            Self::Duplicate => Localized::fixed("serve.origin.error.duplicate", self.to_string()),
            Self::Empty => Localized::fixed("serve.origin.error.empty", self.to_string()),
            Self::TooMany => Localized::fixed("serve.origin.error.too_many", self.to_string()),
        }
    }
}

fn split_host_port(rest: &str) -> Result<(&str, Option<&str>), OriginError> {
    if rest.starts_with('[') {
        let close = rest.find(']').ok_or(OriginError::Host)?;
        let after = &rest[close + 1..];
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':').ok_or(OriginError::Host)?),
        };
        return Ok((&rest[..=close], port));
    }
    match rest.rsplit_once(':') {
        Some((host, port)) => Ok((host, Some(port))),
        None => Ok((rest, None)),
    }
}

/// Lowercase ASCII names (letters, digits, `-` and `.`), dotted IPv4 or a
/// bracketed IPv6; `null` and other opaque spellings are refused.
fn normalize_host(host: &str) -> Result<String, OriginError> {
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let ip: Ipv6Addr = inner.parse().map_err(|_| OriginError::Host)?;
        return Ok(format!("[{ip}]"));
    }
    if host.is_empty() || host == "null" || host.len() > 253 {
        return Err(OriginError::Host);
    }
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return Ok(ip.to_string());
    }
    let labels_ok = host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    if !labels_ok || host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(OriginError::Host);
    }
    Ok(host.to_string())
}

fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_normalize_scheme_host_and_default_port() {
        for (text, canonical) in [
            ("https://example.com", "https://example.com"),
            ("HTTPS://Example.COM:443/", "https://example.com"),
            (
                "https://dash.example.com:8443",
                "https://dash.example.com:8443",
            ),
            ("http://localhost:5173", "http://localhost:5173"),
            ("http://LOCALHOST", "http://localhost"),
            ("http://127.0.0.1:8080", "http://127.0.0.1:8080"),
            ("http://[::1]:3000", "http://[::1]:3000"),
            ("https://[2001:db8::1]", "https://[2001:db8::1]"),
            ("https://192.0.2.10:80", "https://192.0.2.10:80"),
        ] {
            assert_eq!(
                Origin::parse(text).unwrap().to_string(),
                canonical,
                "{text}"
            );
        }
        assert_eq!(
            Origin::parse("https://example.com:443").unwrap(),
            Origin::parse("https://example.com").unwrap()
        );
        assert_ne!(
            Origin::parse("http://localhost").unwrap(),
            Origin::parse("http://localhost:8080").unwrap()
        );
    }

    #[test]
    fn wildcards_null_credentials_paths_and_remote_http_are_refused() {
        for (text, error) in [
            ("*", OriginError::Scheme),
            ("null", OriginError::Scheme),
            ("https://*", OriginError::Host),
            ("https://*.example.com", OriginError::Host),
            ("https://null", OriginError::Host),
            ("https://", OriginError::Host),
            ("https://user:pw@example.com", OriginError::Extra),
            ("https://example.com/path", OriginError::Extra),
            ("https://example.com/?q=1", OriginError::Extra),
            ("https://example.com#x", OriginError::Extra),
            ("https://example.com:0", OriginError::Port),
            ("https://example.com:70000", OriginError::Port),
            ("https://example.com:", OriginError::Port),
            ("http://example.com", OriginError::Scheme),
            ("http://192.0.2.10", OriginError::Scheme),
            ("ftp://example.com", OriginError::Scheme),
            ("example.com", OriginError::Scheme),
            ("https://exa mple.com", OriginError::Host),
            ("https://exämple.com", OriginError::Host),
            ("https://-bad.example.com", OriginError::Host),
            ("https://[::1", OriginError::Host),
        ] {
            assert_eq!(Origin::parse(text), Err(error), "{text}");
        }
    }

    #[test]
    fn a_header_matches_only_its_exact_origin() {
        let origin = Origin::parse("https://dash.example.com").unwrap();
        assert!(origin.matches_header(b"https://dash.example.com"));
        assert!(origin.matches_header(b"https://dash.example.com:443"));
        assert!(origin.matches_header(b"HTTPS://DASH.example.com"));
        for other in [
            &b"https://dash.example.com/"[..],
            b"http://dash.example.com",
            b"https://dash.example.com:8443",
            b"https://evil.example.com",
            b"https://dash.example.com.evil.example",
            b"null",
            b"",
            b"\xff",
        ] {
            assert!(!origin.matches_header(other), "{other:?}");
        }
    }

    #[test]
    fn lists_need_one_to_sixteen_unique_origins() {
        let list = parse_origins(&["https://a.example", "http://localhost:3000"]).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(parse_origins::<&str>(&[]), Err((0, OriginListError::Empty)));
        assert_eq!(
            parse_origins(&["https://a.example", "https://A.example:443/"]),
            Err((2, OriginListError::Duplicate))
        );
        assert_eq!(
            parse_origins(&["https://a.example", "*"]),
            Err((2, OriginListError::Invalid(OriginError::Scheme)))
        );
        let many: Vec<String> = (1..=17).map(|n| format!("https://h{n}.example")).collect();
        assert_eq!(parse_origins(&many), Err((0, OriginListError::TooMany)));
        assert!(parse_origins(&many[..16]).is_ok());
        let json = serde_json::to_string(&list).unwrap();
        assert_eq!(json, r#"["https://a.example","http://localhost:3000"]"#);
        let back: Vec<Origin> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, list);
        assert!(serde_json::from_str::<Origin>(r#""*""#).is_err());
    }
}
