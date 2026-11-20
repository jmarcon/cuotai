//! PRD 0052 RF1.3 — the fail-closed redactor every log line passes through
//! before it reaches disk. It does not trust its callers: whatever a field
//! carries, bearer/API/session tokens, cookies, authorization headers,
//! bodies, URL paths/queries/fragments/userinfo, e-mail-shaped strings and
//! control characters never survive it. Provider slugs, status classes,
//! numbers and durations are left alone.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

/// Longest value a field keeps after redaction, in characters.
pub const MAX_FIELD_CHARS: usize = 512;

/// What replaces a secret.
pub const SECRET: &str = "[redacted]";
/// What replaces an e-mail address.
pub const EMAIL: &str = "[email]";
/// What replaces a request or response body.
pub const BODY: &str = "[body]";

struct Patterns {
    body: Regex,
    header: Regex,
    bearer: Regex,
    assignment: Regex,
    url: Regex,
    email: Regex,
    prefixed: Regex,
    long_token: Regex,
    secret_name: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let re = |pattern: &str| Regex::new(pattern).expect("static redaction pattern");
        Patterns {
            // A JSON object or array with a string member, or HTML.
            body: re(r#"(?i)(\{\s*\\*"|\[\s*\{|<!doctype|<html|<\?xml)"#),
            // A whole header line whose value is a credential.
            header: re(
                r"(?i)\b(authorization|proxy-authorization|cookie|set-cookie|x-api-key|api-key|x-auth-token)\s*[:=]\s*.*",
            ),
            bearer: re(r"(?i)\b(bearer|basic|token)\s+[A-Za-z0-9._~+/=-]{6,}"),
            // key=value, key: value, "key":"value" where the key names a secret.
            assignment: re(
                r#"(?i)("?[A-Za-z0-9_.-]*(token|secret|password|passwd|pwd|api[_-]?key|apikey|session|cookie|auth|signature|credential|private[_-]?key|client[_-]?secret)[A-Za-z0-9_.-]*"?\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s,;&]+)"#,
            ),
            url: re(r#"(?i)\b([a-z][a-z0-9+.-]*)://([^\s/?#"'<>]+)([^\s"'<>]*)"#),
            email: re(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
            prefixed: re(
                r"\b(sk-[A-Za-z0-9_-]{8,}|sk_[A-Za-z0-9_]{8,}|rk_[A-Za-z0-9_]{8,}|gh[pousr]_[A-Za-z0-9]{8,}|github_pat_[A-Za-z0-9_]{8,}|xox[abprs]-[A-Za-z0-9-]{8,}|AKIA[A-Z0-9]{12,}|ASIA[A-Z0-9]{12,}|AIza[A-Za-z0-9_-]{20,}|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9._-]*|ya29\.[A-Za-z0-9._-]+|1//[A-Za-z0-9._-]{8,})",
            ),
            // A long run that mixes letters and digits reads as a key.
            long_token: re(r"[A-Za-z0-9_+/=.-]{32,}"),
            secret_name: re(
                r"(?i)(token|secret|password|passwd|pwd|api[_-]?key|apikey|session|cookie|authorization|signature|credential|private[_-]?key|header|body)",
            ),
        }
    })
}

/// Redact one value. Idempotent: redacting a redacted value changes nothing.
pub fn text(value: &str) -> String {
    let p = patterns();
    if p.body.is_match(value) {
        return BODY.to_string();
    }
    // PRD 0052 re-validation S-09: controls, bidi overrides and isolates
    // and the direction marks become spaces, so a logged value cannot
    // reorder or disguise the line.
    let value: String = value
        .chars()
        .map(|c| {
            if crate::text::is_disguising(c) || matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let value = p.header.replace_all(&value, |caps: &regex::Captures| {
        format!("{}: {SECRET}", &caps[1])
    });
    let value = p
        .url
        .replace_all(&value, |caps: &regex::Captures| url(caps));
    let value = p.bearer.replace_all(&value, |caps: &regex::Captures| {
        format!("{} {SECRET}", &caps[1])
    });
    let value = p.assignment.replace_all(&value, |caps: &regex::Captures| {
        format!("{}{SECRET}", &caps[1])
    });
    let value = p.email.replace_all(&value, EMAIL);
    let value = p.prefixed.replace_all(&value, SECRET);
    let value = p.long_token.replace_all(&value, |caps: &regex::Captures| {
        let run = &caps[0];
        let mixed =
            run.chars().any(|c| c.is_ascii_alphabetic()) && run.chars().any(|c| c.is_ascii_digit());
        if mixed {
            SECRET.to_string()
        } else {
            run.to_string()
        }
    });
    truncate(&value, MAX_FIELD_CHARS)
}

/// Redact key-shaped text only (PRD 0044 G-0044-3): credential header
/// lines, bearer/basic values, `secret=value` assignments, known key
/// prefixes and long runs mixing letters and digits. No body rule, no
/// truncation and no URL rule (the caller replaces whole web addresses);
/// newlines are kept. Meant for human texts (notification titles, bodies,
/// labels), so a bearer value or a `secret: value` with a colon is
/// redacted only when it looks like a key ([`key_shaped`]): "Tokens: 40"
/// and "token budget" stay as they are. Idempotent.
pub fn tokens(value: &str) -> String {
    let p = patterns();
    let value = p.header.replace_all(value, |caps: &regex::Captures| {
        format!("{}: {SECRET}", &caps[1])
    });
    let value = p.bearer.replace_all(&value, |caps: &regex::Captures| {
        let whole = &caps[0];
        let secret = whole[caps[1].len()..].trim_start();
        if key_shaped(secret) {
            format!("{} {SECRET}", &caps[1])
        } else {
            whole.to_string()
        }
    });
    let value = p.assignment.replace_all(&value, |caps: &regex::Captures| {
        let (key, secret) = (&caps[1], &caps[3]);
        if key.trim_end().ends_with('=') || key_shaped(secret.trim_matches(['"', '\''])) {
            format!("{key}{SECRET}")
        } else {
            caps[0].to_string()
        }
    });
    let value = p.prefixed.replace_all(&value, SECRET);
    p.long_token
        .replace_all(&value, |caps: &regex::Captures| {
            let run = &caps[0];
            let mixed = run.chars().any(|c| c.is_ascii_alphabetic())
                && run.chars().any(|c| c.is_ascii_digit());
            if mixed {
                SECRET.to_string()
            } else {
                run.to_string()
            }
        })
        .into_owned()
}

/// Whether `text` holds a known key prefix (`sk-…`, `ghp_…`, `AKIA…`,
/// `ASIA…`, `eyJ….`, …) at a word boundary anywhere, so a key glued to
/// other text (`key=sk-…`, `litellm:sk-…`) counts too (PRD 0045 S-02).
/// Shares the patterns of [`tokens`] and [`text`].
pub(crate) fn has_key_prefix(text: &str) -> bool {
    patterns().prefixed.is_match(text)
}

/// Whether `value` reads as a key rather than a word or a figure: at least
/// 16 characters, or at least 8 that mix letters with digits or key
/// punctuation. `[redacted]` itself is not key-shaped.
fn key_shaped(value: &str) -> bool {
    if value == SECRET {
        return false;
    }
    let count = value.chars().count();
    let letters = value.chars().any(|c| c.is_ascii_alphabetic());
    let marks = value
        .chars()
        .any(|c| c.is_ascii_digit() || matches!(c, '_' | '-' | '+' | '/' | '=' | '.' | '~'));
    count >= 16 || (count >= 8 && letters && marks)
}

/// Redact the value of a field called `name`: a name that reads as a
/// secret (`token`, `cookie`, `password`, …) hides the whole value.
pub fn field(name: &str, value: &str) -> String {
    if patterns().secret_name.is_match(name) {
        SECRET.to_string()
    } else {
        text(value)
    }
}

/// A URL keeps its scheme, host and port; userinfo is dropped, and a path,
/// query or fragment becomes `/…`.
fn url(caps: &regex::Captures) -> String {
    let scheme = caps[1].to_ascii_lowercase();
    let authority = &caps[2];
    let host = authority.rsplit('@').next().unwrap_or_default();
    let rest = &caps[3];
    let tail = if rest.is_empty() || rest == "/" {
        ""
    } else {
        "/…"
    };
    format!("{scheme}://{host}{tail}")
}

fn truncate(value: &str, max: usize) -> String {
    match value.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &value[..cut]),
        None => value.to_string(),
    }
}

/// A path with the home directory replaced by `~` (`%USERPROFILE%` on
/// Windows), so diagnostics keep the structure without the user name.
pub fn home_abbreviated(path: &Path) -> String {
    let home_mark = if cfg!(windows) { "%USERPROFILE%" } else { "~" };
    match dirs::home_dir().and_then(|home| path.strip_prefix(&home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => home_mark.to_string(),
        Some(rest) => format!("{home_mark}{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
pub(crate) use tests::{assert_clean, SEEDS};

#[cfg(test)]
mod tests {
    use super::*;

    /// Seeded values no redacted text may contain.
    pub(crate) const SEEDS: &[&str] = &[
        "sk-ant-api03-SEEDSECRETVALUE1234567890",
        "ghp_SEEDGITHUBTOKEN12345678",
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzZWVkIn0.c2lnbmF0dXJl",
        "SESSIONCOOKIEVALUE42",
        "seed.person@example.com",
        "q=SEEDQUERY",
        "SEEDFRAGMENT",
        "seeduser:seedpass",
        "SEEDBODYFIELD",
        "SEEDPATHID",
    ];

    pub(crate) fn assert_clean(text: &str) {
        for seed in SEEDS {
            assert!(!text.contains(seed), "{seed:?} leaked into {text:?}");
        }
        assert!(!text.contains("seedpass"), "{text:?}");
        assert!(!text.chars().any(char::is_control) || text.ends_with('\n'));
    }

    /// PRD 0052 re-validation S-09: bidi overrides, isolates and direction
    /// marks become spaces like controls, and redaction stays idempotent.
    #[test]
    fn disguising_characters_become_spaces() {
        for c in [
            '\u{202E}', '\u{2066}', '\u{2069}', '\u{200F}', '\u{061C}', '\u{1b}',
        ] {
            let once = text(&format!("file{c}name"));
            assert_eq!(once, "file name", "{c:?}");
            assert_eq!(text(&once), once);
        }
    }

    #[test]
    fn every_seeded_secret_is_removed() {
        let inputs = [
            "Authorization: Bearer sk-ant-api03-SEEDSECRETVALUE1234567890",
            "header token ghp_SEEDGITHUBTOKEN12345678 here",
            "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzZWVkIn0.c2lnbmF0dXJl",
            "Cookie: session=SESSIONCOOKIEVALUE42; other=1",
            "set-cookie: a=SESSIONCOOKIEVALUE42",
            "session_id=SESSIONCOOKIEVALUE42 more",
            r#"{"access_token":"sk-ant-api03-SEEDSECRETVALUE1234567890","SEEDBODYFIELD":1}"#,
            "contact seed.person@example.com now",
            "https://seeduser:seedpass@api.example.com/v1/users/SEEDPATHID/balance?q=SEEDQUERY#SEEDFRAGMENT",
            "<html><body>SEEDBODYFIELD</body></html>",
            "api_key: \"SEEDSECRETVALUE1234567890abcdef\"",
            "line\nbreak\u{1b}[31m",
        ];
        for input in inputs {
            let out = text(input);
            assert_clean(&out);
            assert!(!out.chars().any(char::is_control), "{out:?}");
            assert_eq!(text(&out), out, "idempotent for {input:?}");
        }
    }

    #[test]
    fn safe_values_survive() {
        for value in [
            "claude_code",
            "rate_limited",
            "1234",
            "2.5s",
            "429",
            "history.degraded",
        ] {
            assert_eq!(text(value), value);
        }
        assert_eq!(
            text("listening on http://127.0.0.1:6738"),
            "listening on http://127.0.0.1:6738"
        );
        assert_eq!(
            text("GET https://api.example.com/v1/x?y=1"),
            "GET https://api.example.com/…"
        );
    }

    #[test]
    fn a_field_named_like_a_secret_hides_its_value() {
        assert_eq!(field("cookie", "SESSIONCOOKIEVALUE42"), SECRET);
        assert_eq!(field("refresh_token", "abc"), SECRET);
        assert_eq!(field("provider", "openai"), "openai");
    }

    #[test]
    fn tokens_removes_key_shaped_text_and_keeps_sentences() {
        let inputs = [
            "Authorization: Bearer sk-ant-api03-SEEDSECRETVALUE1234567890",
            "header token ghp_SEEDGITHUBTOKEN12345678 here",
            "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzZWVkIn0.c2lnbmF0dXJl",
            "Cookie: session=SESSIONCOOKIEVALUE42; other=1",
            "session_id=SESSIONCOOKIEVALUE42 more",
            "api_key: \"SEEDSECRETVALUE1234567890abcdef\"",
            "Team sk-proj-abcdefghijklmnop1234",
        ];
        for input in inputs {
            let out = tokens(input);
            assert_clean(&out);
            assert!(!out.contains("sk-proj-"), "{out:?}");
            assert_eq!(tokens(&out), out, "idempotent for {input:?}");
        }
        for sentence in [
            "Claude · Work\nSession (5h) at 90% · resets 16:00",
            "Your week in Cuotai: 2026-10-19 – 2026-10-25",
            "Tokens: 40 of 100",
            "Monthly token budget: 75% used",
            "Basic plan · Session: 90%",
            "Heaviest day: Thursday 2026-10-22 (100%) · lightest: Saturday 2026-10-24 (10%)",
            "- Budgets: 1 of 2 used up\n- 3 limit changes\n",
            "[email] and [redacted]",
        ] {
            assert_eq!(tokens(sentence), sentence);
        }
    }

    #[test]
    fn long_values_are_bounded() {
        let out = text(&"word ".repeat(400));
        assert!(out.chars().count() <= MAX_FIELD_CHARS + 1);
    }
}
