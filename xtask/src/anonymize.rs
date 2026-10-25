//! Fixture anonymizer (PRD 0028 RF5): turns a captured provider response into
//! a `wiremock` fixture that is safe to commit. Structure and JSON types are
//! kept; strings become type-preserving placeholders; a refusal replaces the
//! output when anything that looks like a secret survives.

use crate::{err, Res};
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;

/// Objects with more keys than this are maps keyed by data (RF1.2).
const MAX_KEYS: usize = 32;
/// The only JWT-looking string a fixture may contain: the placeholder.
const FAKE_JWT: &str = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJ1c2VyIn0.ZmFrZQ";
const FAKE_DATE_TIME: &str = "2020-01-01T00:00:00Z";
const FAKE_DATE: &str = "2020-01-01";
const FAKE_EMAIL: &str = "user@example.com";
const FAKE_URL: &str = "https://example.com/";

/// Keys whose short word values are vocabulary the adapter matches on
/// (plan, status, unit...), not personal data. Everything else is replaced.
const VOCABULARY_KEYS: [&str; 16] = [
    "type", "status", "state", "kind", "plan", "tier", "currency", "unit", "period", "interval",
    "window", "role", "object", "mode", "level", "category",
];

/// Options of one run.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Options {
    /// Scale factor applied to numbers (`--perturb`); `None` keeps them.
    pub perturb: Option<f64>,
}

#[derive(Default)]
struct State {
    options: Options,
    texts: BTreeMap<String, String>,
    keys: BTreeMap<String, String>,
}

fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `YYYY-MM-DD` optionally followed by a time part.
fn date_kind(s: &str) -> Option<&'static str> {
    let b = s.as_bytes();
    if b.len() < 10
        || !(all_digits(&s[0..4]) && b[4] == b'-' && all_digits(&s[5..7]) && b[7] == b'-')
        || !all_digits(&s[8..10])
    {
        return None;
    }
    if b.len() == 10 {
        return Some("date");
    }
    (matches!(b[10], b'T' | b' ') && b.len() >= 16 && all_digits(&s[11..13])).then_some("time")
}

fn looks_like_jwt(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && s.starts_with("eyJ")
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'=')
        })
}

fn looks_like_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 24
        && s.bytes()
            .all(|b| b.is_ascii_alphabetic() || matches!(b, b'_' | b'-' | b'.'))
}

impl State {
    fn fake(map: &mut BTreeMap<String, String>, original: &str, prefix: &str) -> String {
        let next = map.len() + 1;
        map.entry(original.to_string())
            .or_insert_with(|| format!("{prefix}{next:04}"))
            .clone()
    }

    fn string(&mut self, key: Option<&str>, s: &str) -> String {
        if s.is_empty() {
            return String::new();
        }
        if looks_like_jwt(s) {
            return FAKE_JWT.to_string();
        }
        match date_kind(s) {
            Some("date") => return FAKE_DATE.to_string(),
            Some(_) => return FAKE_DATE_TIME.to_string(),
            None => {}
        }
        if s.contains('@') {
            return FAKE_EMAIL.to_string();
        }
        if s.starts_with("http://") || s.starts_with("https://") {
            return FAKE_URL.to_string();
        }
        if looks_like_uuid(s) {
            let n = self.texts.len() + 1;
            let id = self
                .texts
                .entry(s.to_string())
                .or_insert_with(|| format!("00000000-0000-4000-8000-{n:012}"));
            return id.clone();
        }
        if key.is_some_and(|k| VOCABULARY_KEYS.contains(&k)) && is_word(s) {
            return s.to_string();
        }
        Self::fake(&mut self.texts, s, "text-")
    }

    fn number(&self, n: &Number) -> Value {
        let Some(factor) = self.options.perturb else {
            return Value::Number(n.clone());
        };
        if let Some(i) = n.as_i64() {
            return Value::from((i as f64 * factor).round() as i64);
        }
        Number::from_f64(n.as_f64().unwrap_or(0.0) * factor)
            .map_or_else(|| Value::Number(n.clone()), Value::Number)
    }

    fn value(&mut self, key: Option<&str>, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.string(key, s)),
            Value::Number(n) => self.number(n),
            Value::Array(items) => Value::Array(items.iter().map(|v| self.value(key, v)).collect()),
            Value::Object(object) => {
                let map_like = object.len() > MAX_KEYS;
                let mut out = Map::new();
                for (k, v) in object {
                    let new_key = if map_like || !is_identifier(k) {
                        Self::fake(&mut self.keys, k, "key_")
                    } else {
                        k.clone()
                    };
                    let child = self.value(Some(k.as_str()), v);
                    out.insert(new_key, child);
                }
                Value::Object(out)
            }
            other => other.clone(),
        }
    }
}

/// The anonymized copy of `input`.
pub fn anonymize(input: &Value, options: Options) -> Value {
    State {
        options,
        ..State::default()
    }
    .value(None, input)
}

fn secret_pattern(s: &str, is_key: bool) -> Option<&'static str> {
    if looks_like_uuid(s) || s == FAKE_JWT || s == FAKE_EMAIL || s == FAKE_URL {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if looks_like_jwt(s) {
        return Some("JWT");
    }
    if s.contains("-----BEGIN") {
        return Some("private key block");
    }
    if lower.starts_with("bearer ") || lower.starts_with("basic ") {
        return Some("authorization header");
    }
    for prefix in [
        "sk-",
        "sk_",
        "pk_",
        "ghp_",
        "gho_",
        "ghs_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "akia",
        "asia",
        "aiza",
        "ya29.",
        "glpat-",
        "rk_",
    ] {
        if lower.starts_with(prefix) {
            return Some("token prefix");
        }
    }
    if s.contains('@') {
        return Some("e-mail address");
    }
    if lower.contains("http://") || lower.contains("https://") {
        return Some("URL");
    }
    if !is_key
        && (lower.contains("cookie") || lower.contains("session=") || lower.contains("token="))
    {
        return Some("cookie or token pair");
    }
    let run = s
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '-' | '_' | '=')))
        .any(|part| part.len() >= 32);
    run.then_some("long opaque string")
}

fn scan(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if let Some(kind) = secret_pattern(s, false) {
                found.push(format!("{path}: {kind}"));
            }
        }
        Value::Array(items) => items
            .iter()
            .for_each(|v| scan(v, &format!("{path}[]"), found)),
        Value::Object(object) => {
            for (k, v) in object {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                if let Some(kind) = secret_pattern(k, true) {
                    found.push(format!("{child} (key): {kind}"));
                }
                scan(v, &child, found);
            }
        }
        _ => {}
    }
}

/// The review checklist printed beside the output.
pub fn checklist(provider: &str) -> String {
    format!(
        "Review checklist for the `{provider}` fixture:\n\
         - Read the whole output once; no real name, organization, e-mail, id or URL may remain.\n\
         - Numbers are kept as captured unless --perturb was used; scale them if they are yours.\n\
         - Keep the fixture next to the adapter (a wiremock test) and run it to confirm it still decodes.\n\
         - Never commit the original capture.\n"
    )
}

/// Anonymize `input` (JSON text). Refuses when a secret pattern survives.
pub fn run(provider: &str, input: &str, options: Options) -> Res<String> {
    if provider.is_empty()
        || !provider
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return err("--provider needs a provider slug such as claude or openai_admin");
    }
    let Ok(value) = serde_json::from_str::<Value>(input) else {
        return err("the input is not valid JSON");
    };
    let output = anonymize(&value, options);
    let mut found = Vec::new();
    scan(&output, "", &mut found);
    if !found.is_empty() {
        found.truncate(10);
        return err(format!(
            "refusing: the anonymized output still looks like it holds secrets ({}). Remove or replace these by hand and run again",
            found.join("; ")
        ));
    }
    let mut text =
        serde_json::to_string_pretty(&output).map_err(|e| crate::Error::Msg(e.to_string()))?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SECRETS: [&str; 7] = [
        "alice.smith@corp.io",
        "Acme Holdings",
        "sk-live-0123456789abcdef0123456789abcdef",
        "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJhbGljZSJ9.c2lnbmF0dXJl",
        "9f1c2d3e-aaaa-bbbb-cccc-1234567890ab",
        "sessionKey=abc123",
        "https://api.corp.io/v1/org/acme?token=zzz",
    ];

    fn sample() -> Value {
        json!({
            "email": SECRETS[0], "organization": SECRETS[1], "api_key": SECRETS[2],
            "id_token": SECRETS[3], "org_id": SECRETS[4], "cookie": SECRETS[5],
            "next": SECRETS[6], "plan": "pro", "used": 42, "limit": 100.5,
            "reset_at": "2025-03-04T05:06:07.123Z", "day": "2025-03-04", "active": true,
            "none": null, "windows": [{"unit": "tokens", "label": "Acme Holdings"}],
            "bob@corp.io": 1
        })
    }

    #[test]
    fn no_seeded_secret_survives() {
        let out = run("claude", &sample().to_string(), Options::default()).unwrap();
        for secret in SECRETS.iter().chain(&["bob@corp.io"]) {
            assert!(!out.contains(secret), "{secret} leaked:\n{out}");
        }
        assert!(!out.contains("Acme"));
    }

    #[test]
    fn structure_types_and_vocabulary_are_kept() {
        let out: Value = serde_json::from_str(
            &run("claude", &sample().to_string(), Options::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(out["plan"], "pro");
        assert_eq!(out["used"], 42);
        assert_eq!(out["limit"], 100.5);
        assert_eq!(out["active"], true);
        assert!(out["none"].is_null());
        assert_eq!(out["reset_at"], FAKE_DATE_TIME);
        assert_eq!(out["day"], FAKE_DATE);
        assert_eq!(out["email"], FAKE_EMAIL);
        assert_eq!(out["id_token"], FAKE_JWT);
        assert_eq!(out["windows"][0]["unit"], "tokens");
        assert!(out["windows"][0]["label"]
            .as_str()
            .unwrap()
            .starts_with("text-"));
        assert!(out.get("key_0001").is_some());
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        let input = json!({"a": "Acme", "b": "Acme", "c": "Other"});
        let out = anonymize(&input, Options::default());
        assert_eq!(out["a"], out["b"]);
        assert_ne!(out["a"], out["c"]);
    }

    #[test]
    fn maps_keyed_by_data_are_collapsed() {
        let mut map = Map::new();
        for i in 0..40 {
            map.insert(format!("model_{i}"), json!(i));
        }
        let out = anonymize(&Value::Object(map), Options::default());
        assert!(out
            .as_object()
            .unwrap()
            .keys()
            .all(|k| k.starts_with("key_")));
    }

    #[test]
    fn perturb_scales_numbers() {
        let out = anonymize(
            &json!({"used": 10, "ratio": 0.5}),
            Options { perturb: Some(2.0) },
        );
        assert_eq!(out["used"], 20);
        assert_eq!(out["ratio"], 1.0);
    }

    #[test]
    fn a_surviving_secret_pattern_is_refused() {
        // A vocabulary value that is shaped like a token is kept by the
        // replacement rules and then refused by the scan.
        let out = run(
            "claude",
            r#"{"plan": "sk-live-abcdefghijk"}"#,
            Options::default(),
        );
        assert!(out.is_err());
        // A key shaped like a secret is refused too.
        let key = "ghp_abcdefghijklmnop";
        let value = json!({ key: 1 });
        let mut found = Vec::new();
        scan(&value, "", &mut found);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn bad_provider_and_bad_json_are_rejected() {
        assert!(run("Bad Slug", "{}", Options::default()).is_err());
        assert!(run("claude", "not json", Options::default()).is_err());
    }

    #[test]
    fn the_output_still_decodes_into_the_original_shape() {
        #[derive(serde::Deserialize)]
        struct Usage {
            used: u64,
            plan: String,
            reset_at: String,
        }
        let text = run("claude", &sample().to_string(), Options::default()).unwrap();
        let usage: Usage = serde_json::from_str(&text).unwrap();
        assert_eq!((usage.used, usage.plan.as_str()), (42, "pro"));
        assert_eq!(usage.reset_at, FAKE_DATE_TIME);
    }
}
