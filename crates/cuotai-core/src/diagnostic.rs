//! PRD 0028 RF1 — a redacted, value-free description of what drifted when a
//! provider response no longer decodes. It keeps JSON paths (array indices
//! normalized, unusual object keys collapsed), JSON *types* and a structural
//! fingerprint; never a value, URL, header or status text. Serde error
//! messages are only classified by keyword, never copied.

use crate::text::{params, Localized, ParamValue};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Most failing paths one diagnostic names.
pub const MAX_PATHS: usize = 8;
/// Deepest level the fingerprint walks.
pub const MAX_DEPTH: usize = 4;
/// Most `path -> type` pairs a fingerprint keeps.
pub const MAX_ENTRIES: usize = 64;
/// Objects with more keys than this are maps keyed by data (model names,
/// e-mails, ids) and collapse to `{*}`.
pub const MAX_KEYS: usize = 32;
/// Placeholder for a collapsed object key.
pub const COLLAPSED: &str = "{*}";

/// One failing field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriftPath {
    /// e.g. `five_hour.utilization`; indices read `[]`.
    pub path: String,
    /// JSON type the adapter expected (`other` when it cannot be told).
    pub expected: String,
    /// JSON type found, or `missing`.
    pub found: String,
}

/// What a response decode failure left behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaDrift {
    pub paths: Vec<DriftPath>,
    /// Sorted `(path, type)` pairs of the whole response.
    pub fingerprint: Vec<(String, String)>,
    /// SHA-256 (hex) of the fingerprint.
    pub digest: String,
}

/// The redacted structural fingerprint of a response that decoded, kept by
/// the local canary (PRD 0028 RF4.1): keys and JSON types, never a value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ResponseShape {
    /// Sorted `(path, type)` pairs, at most [`MAX_ENTRIES`].
    pub fingerprint: Vec<(String, String)>,
    /// SHA-256 (hex) of the fingerprint.
    pub digest: String,
}

impl ResponseShape {
    /// The shape of one parsed response.
    pub fn of(value: &Value) -> Self {
        let fingerprint = fingerprint(value);
        let digest = digest(&fingerprint);
        Self {
            fingerprint,
            digest,
        }
    }

    /// The union with another response of the same query (adapters that
    /// make several requests), re-sorted and capped.
    pub fn merge(&mut self, other: &Self) {
        let all: BTreeSet<(String, String)> = self
            .fingerprint
            .drain(..)
            .chain(other.fingerprint.iter().cloned())
            .collect();
        self.fingerprint = all.into_iter().take(MAX_ENTRIES).collect();
        self.digest = digest(&self.fingerprint);
    }
}

/// The "What changed" block of DS-CMP-DIAG-1, ready for any front end.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DriftView {
    pub heading: Localized,
    pub sentence: Localized,
    pub paths: Vec<DriftPath>,
    pub privacy: Localized,
}

/// The diagnostic a [`crate::Report`] may carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Diagnostic {
    SchemaDrift(SchemaDrift),
}

impl Diagnostic {
    pub fn drift(&self) -> &SchemaDrift {
        let Self::SchemaDrift(drift) = self;
        drift
    }

    /// The ids of [`Diagnostic::text`]: front ends that render the block
    /// ([`Diagnostic::view`]) skip the report note with one of these ids,
    /// which stays for `--json` and other plain consumers.
    pub const NOTE_IDS: [&'static str; 2] =
        ["diagnostic.schema_drift", "diagnostic.schema_drift_unknown"];

    /// DS-CMP-DIAG-1, for every front end: the sentence, the heading "What
    /// changed", the field paths (at most [`MAX_PATHS`]) and the note
    /// "Values are never recorded". Paths are plain text; the front end
    /// sets them in its mono style, never with literal backticks.
    pub fn view(&self) -> DriftView {
        DriftView {
            heading: Localized::fixed("diagnostic.what_changed", "What changed"),
            sentence: self.text(),
            paths: self.drift().paths.iter().take(MAX_PATHS).cloned().collect(),
            privacy: Localized::fixed(
                "diagnostic.values_never_recorded",
                "Values are never recorded.",
            ),
        }
    }

    /// DS-CMP-DIAG-1: "The provider changed five_hour.utilization
    /// (number → string)", `diagnostic.schema_drift`.
    pub fn text(&self) -> Localized {
        let drift = self.drift();
        let Some(first) = drift.paths.first() else {
            return Localized::fixed(
                "diagnostic.schema_drift_unknown",
                "The provider changed the response format.",
            );
        };
        Localized::with_params(
            "diagnostic.schema_drift",
            params([
                ("path", ParamValue::from(first.path.clone())),
                ("expected", ParamValue::from(first.expected.clone())),
                ("found", ParamValue::from(first.found.clone())),
            ]),
            "The provider changed {path} ({expected} → {found}).",
        )
    }
}

fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= 64
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// PRD 0028 RF1.2: an identifier-shaped key that still looks like data (an
/// account or user id, a hash): a run of 5 or more digits, a `_`-separated
/// part of 8 or more characters that mixes letters and digits, or of 6
/// or more that mixes digits with both cases (`user_2NfXk9QpZacme`,
/// `sk_live_51HxQpZ`), or a part of 16 or more hex
/// digits (a UUID or digest without hyphens). Such keys collapse like
/// non-identifiers.
fn looks_like_id(key: &str) -> bool {
    let mut run = 0;
    for byte in key.bytes() {
        run = if byte.is_ascii_digit() { run + 1 } else { 0 };
        if run >= 5 {
            return true;
        }
    }
    key.split('_').any(|part| {
        let digits = part.bytes().any(|b| b.is_ascii_digit());
        let letters = part.bytes().any(|b| b.is_ascii_alphabetic());
        let upper = part.bytes().any(|b| b.is_ascii_uppercase());
        let lower = part.bytes().any(|b| b.is_ascii_lowercase());
        (part.len() >= 8 && digits && letters)
            || (part.len() >= 6 && digits && upper && lower)
            || (part.len() >= 16 && part.bytes().all(|b| b.is_ascii_hexdigit()))
    })
}

/// A key that may appear in a path as itself.
fn is_shown_key(key: &str) -> bool {
    is_identifier(key) && !looks_like_id(key)
}

/// The JSON type name of `value`.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Key as it may appear in a path: itself, or `{*}` when collapsed.
fn safe_key<'a>(parent: &serde_json::Map<String, Value>, key: &'a str) -> &'a str {
    if parent.len() > MAX_KEYS || !is_shown_key(key) {
        COLLAPSED
    } else {
        key
    }
}

/// Coarse JSON type named by a serde "expected ..." tail; never copies text.
fn expected_type(message: &str) -> &'static str {
    let tail = message
        .rsplit_once(", expected ")
        .map_or("", |(_, tail)| tail);
    let has = |words: &[&str]| words.iter().any(|w| tail.contains(w));
    if has(&[
        "f64", "f32", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "usize", "isize",
        "integer", "float", "number",
    ]) {
        "number"
    } else if has(&["string", "str", "char"]) {
        "string"
    } else if has(&["bool"]) {
        "boolean"
    } else if has(&["sequence", "seq", "array", "tuple"]) {
        "array"
    } else if has(&["map", "struct", "object"]) {
        "object"
    } else if has(&["unit", "null", "option"]) {
        "null"
    } else {
        "other"
    }
}

/// Field named by serde's "missing field `name`", when identifier-like.
fn missing_field(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("missing field `")?;
    let name = rest.split('`').next()?;
    is_identifier(name).then_some(name)
}

/// Describe a failed decode of `value`. `segments` are the path segments
/// serde reported (`Ok(key)` for a map key, `Err(())` for an index) and
/// `message` the serde error text, which is only classified.
pub fn capture(value: &Value, segments: &[Result<String, ()>], message: &str) -> SchemaDrift {
    let mut path = String::new();
    let mut current = Some(value);
    for segment in segments {
        match segment {
            Err(()) => {
                path.push_str("[]");
                current = current.and_then(|v| v.as_array()).and_then(|a| a.first());
            }
            Ok(key) => {
                let shown = match current.and_then(Value::as_object) {
                    Some(object) => safe_key(object, key).to_string(),
                    None if is_shown_key(key) => key.clone(),
                    None => COLLAPSED.to_string(),
                };
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(&shown);
                current = current.and_then(|v| v.get(key.as_str()));
            }
        }
    }
    let (expected, found) = match missing_field(message) {
        Some(field) => {
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(field);
            ("present".to_string(), "missing".to_string())
        }
        None => (
            expected_type(message).to_string(),
            current.map_or("unknown", type_name).to_string(),
        ),
    };
    let fingerprint = fingerprint(value);
    let digest = digest(&fingerprint);
    SchemaDrift {
        paths: vec![DriftPath {
            path,
            expected,
            found,
        }],
        fingerprint,
        digest,
    }
}

/// Sorted `(path, type)` pairs of `value`: depth at most [`MAX_DEPTH`],
/// at most [`MAX_ENTRIES`], keys redacted by the rules above.
pub fn fingerprint(value: &Value) -> Vec<(String, String)> {
    let mut entries = BTreeSet::new();
    walk(value, String::new(), 0, &mut entries);
    entries.into_iter().take(MAX_ENTRIES).collect()
}

fn walk(value: &Value, path: String, depth: usize, out: &mut BTreeSet<(String, String)>) {
    out.insert((
        if path.is_empty() {
            ".".into()
        } else {
            path.clone()
        },
        type_name(value).into(),
    ));
    if depth >= MAX_DEPTH {
        return;
    }
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let shown = safe_key(object, key);
                let next = if path.is_empty() {
                    shown.to_string()
                } else {
                    format!("{path}.{shown}")
                };
                walk(child, next, depth + 1, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk(child, format!("{path}[]"), depth + 1, out);
            }
        }
        _ => {}
    }
}

/// Hex SHA-256 of a fingerprint.
pub fn digest(fingerprint: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    for (path, kind) in fingerprint {
        hasher.update(path.as_bytes());
        hasher.update(b":");
        hasher.update(kind.as_bytes());
        hasher.update(b"\n");
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Drift of one field an adapter reads by hand (PRD 0028 RF1): `path` is the
/// field names from `root` (`"[]"` steps into the first array element),
/// `expected` the JSON type the adapter needed. The found type is read from
/// `root`, `missing` when the path does not exist; no value is kept.
pub fn field_drift(root: &Value, path: &[&str], expected: &str) -> SchemaDrift {
    let mut shown = String::new();
    let mut current = Some(root);
    for step in path {
        if *step == "[]" {
            shown.push_str("[]");
            current = current.and_then(Value::as_array).and_then(|a| a.first());
        } else {
            let key = match current.and_then(Value::as_object) {
                Some(object) => safe_key(object, step),
                None if is_shown_key(step) => step,
                None => COLLAPSED,
            };
            if !shown.is_empty() {
                shown.push('.');
            }
            shown.push_str(key);
            current = current.and_then(|v| v.get(*step));
        }
    }
    let found = current.map_or("missing", type_name).to_string();
    if shown.is_empty() {
        shown.push('.');
    }
    let fingerprint = fingerprint(root);
    let digest = digest(&fingerprint);
    SchemaDrift {
        paths: vec![DriftPath {
            path: shown,
            expected: expected.to_string(),
            found,
        }],
        fingerprint,
        digest,
    }
}

/// [`field_drift`] for a field of one element (`item`) of a list or map in
/// `root`: `prefix` is the fixed path of that element as the adapter names
/// it (`keys[]`, `providers.{*}`), never data. The fingerprint is the whole
/// response's.
pub fn element_drift(
    root: &Value,
    prefix: &str,
    item: &Value,
    path: &[&str],
    expected: &str,
) -> SchemaDrift {
    let mut drift = field_drift(item, path, expected);
    for entry in &mut drift.paths {
        entry.path = if entry.path == "." {
            prefix.to_string()
        } else if entry.path.starts_with('[') {
            format!("{prefix}{}", entry.path)
        } else {
            format!("{prefix}.{}", entry.path)
        };
    }
    drift.fingerprint = fingerprint(root);
    drift.digest = digest(&drift.fingerprint);
    drift
}

/// Drift of a field an adapter finds by name anywhere in the response (no
/// fixed path): `name` is reported as the path, `found` is the type of what
/// the search returned, `missing` when nothing matched.
pub fn named_drift(root: &Value, name: &str, found: Option<&Value>, expected: &str) -> SchemaDrift {
    let fingerprint = fingerprint(root);
    let digest = digest(&fingerprint);
    SchemaDrift {
        paths: vec![DriftPath {
            path: if is_shown_key(name) {
                name.to_string()
            } else {
                COLLAPSED.to_string()
            },
            expected: expected.to_string(),
            found: found.map_or("missing", type_name).to_string(),
        }],
        fingerprint,
        digest,
    }
}

/// Decode `value` as `T`; on failure, the redacted drift.
pub fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, SchemaDrift> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let segments: Vec<Result<String, ()>> = error
            .path()
            .iter()
            .filter_map(|segment| match segment {
                serde_path_to_error::Segment::Seq { .. } => Some(Err(())),
                serde_path_to_error::Segment::Map { key } => Some(Ok(key.clone())),
                serde_path_to_error::Segment::Enum { variant } => Some(Ok(variant.clone())),
                serde_path_to_error::Segment::Unknown => None,
            })
            .collect();
        capture(value, &segments, &error.inner().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Window {
        utilization: f64,
        resets_at: String,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Usage {
        five_hour: Window,
        items: Vec<Window>,
    }

    fn good() -> Value {
        json!({
            "five_hour": {"utilization": 12.5, "resets_at": "2026-01-01T00:00:00Z"},
            "items": [{"utilization": 1.0, "resets_at": "x"}]
        })
    }

    fn drift(value: Value) -> SchemaDrift {
        decode::<Usage>(&value).unwrap_err()
    }

    #[test]
    fn hand_read_fields_report_their_path_and_types() {
        let value = json!({"a": {"n": "SECRET", "list": [{"x": 1}]}});
        let d = field_drift(&value, &["a", "n"], "number");
        assert_eq!(
            d.paths[0],
            DriftPath {
                path: "a.n".into(),
                expected: "number".into(),
                found: "string".into()
            }
        );
        assert_eq!(
            field_drift(&value, &["a", "gone"], "string").paths[0].found,
            "missing"
        );
        assert_eq!(
            field_drift(&value, &["a", "list", "[]", "x"], "string").paths[0].path,
            "a.list[].x"
        );
        assert!(!format!("{d:?}").contains("SECRET"));
    }

    #[test]
    fn a_type_change_names_the_path_and_both_types() {
        let mut value = good();
        value["five_hour"]["utilization"] = json!("SECRET-VALUE");
        let drift = drift(value);
        assert_eq!(
            drift.paths,
            [DriftPath {
                path: "five_hour.utilization".into(),
                expected: "number".into(),
                found: "string".into()
            }]
        );
        let all = format!(
            "{drift:?}{:?}",
            Diagnostic::SchemaDrift(drift.clone()).text()
        );
        assert!(!all.contains("SECRET-VALUE"));
    }

    #[test]
    fn removed_and_renamed_fields_are_missing() {
        let mut value = good();
        value["five_hour"]
            .as_object_mut()
            .unwrap()
            .remove("utilization");
        assert_eq!(drift(value).paths[0].path, "five_hour.utilization");
        let mut value = good();
        value["items"][0]["utilisation"] = json!(1.0);
        value["items"][0]
            .as_object_mut()
            .unwrap()
            .remove("utilization");
        let found = drift(value);
        assert_eq!(found.paths[0].path, "items[].utilization");
        assert_eq!(found.paths[0].found, "missing");
    }

    #[test]
    fn maps_keyed_by_data_collapse_and_leak_nothing() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct ByUser {
            users: std::collections::BTreeMap<String, f64>,
        }
        let value = json!({"users": {"ana@example.com": "oops", "b@example.com": 1.0}});
        let drift = decode::<ByUser>(&value).unwrap_err();
        assert_eq!(drift.paths[0].path, "users.{*}");
        let shown = format!("{drift:?}");
        assert!(!shown.contains("example.com"));
        // Large maps collapse even when the keys look like identifiers.
        let many: serde_json::Map<String, Value> = (0..40)
            .map(|i| (format!("model_{i}"), json!("x")))
            .collect();
        let value = json!({"users": many});
        let drift = decode::<ByUser>(&value).unwrap_err();
        assert_eq!(drift.paths[0].path, "users.{*}");
        assert!(drift.fingerprint.iter().all(|(p, _)| !p.contains("model_")));
    }

    /// PRD 0028 RF1.2 (S-02): identifier-shaped keys that look like ids
    /// collapse too; ordinary field names stay.
    #[test]
    fn id_like_keys_collapse_and_field_names_stay() {
        let seeded = [
            "user_2NfXk9QpZacme",
            "acct_12345678",
            "a3f09c1d2b4e5f60718293a4b5c6d7e8",
            "deadbeefdeadbeefdeadbeef",
            "sk_live_51HxQpZ",
            "jane@example.com",
        ];
        let mut object = serde_json::Map::new();
        for key in seeded {
            object.insert(key.to_string(), json!({"used": "x"}));
        }
        for key in [
            "five_hour",
            "utilization",
            "seven_day_opus",
            "displayNameShort",
            "gpt4o",
        ] {
            object.insert(key.to_string(), json!(1));
        }
        let value = Value::Object(object);
        let shape = format!("{:?}", fingerprint(&value));
        let drift = format!(
            "{:?}",
            field_drift(&value, &["user_2NfXk9QpZacme", "used"], "number")
        );
        for key in seeded {
            assert!(!shape.contains(key), "{key} in {shape}");
            assert!(!drift.contains(key), "{key} in {drift}");
        }
        for key in [
            "five_hour",
            "utilization",
            "seven_day_opus",
            "displayNameShort",
            "gpt4o",
        ] {
            assert!(shape.contains(key), "{key} missing from {shape}");
        }
        assert!(drift.contains("{*}.used"), "{drift}");
    }

    #[test]
    fn the_fingerprint_is_bounded_sorted_and_stable() {
        let deep = json!({"a": {"b": {"c": {"d": {"e": {"f": 1}}}}}});
        let fp = fingerprint(&deep);
        assert!(fp.iter().all(|(p, _)| p.matches('.').count() <= MAX_DEPTH));
        assert!(!fp.iter().any(|(p, _)| p.ends_with("e.f")));
        let wide: serde_json::Map<String, Value> = (0..30)
            .map(|i| (format!("k{i}"), json!({"x": 1, "y": [1]})))
            .collect();
        assert!(fingerprint(&Value::Object(wide)).len() <= MAX_ENTRIES);
        let mut sorted = fp.clone();
        sorted.sort();
        assert_eq!(fp, sorted);
        assert_eq!(digest(&fp), digest(&fingerprint(&deep)));
        assert_eq!(digest(&fp).len(), 64);
        // Values never reach the fingerprint.
        let secret = json!({"token": "sk-live-123", "list": ["a@example.com"]});
        assert!(!format!("{:?}", fingerprint(&secret)).contains("sk-live"));
        assert!(!format!("{:?}", fingerprint(&secret)).contains("example.com"));
    }

    /// A mutated provider response through the shared HTTP decoder: the
    /// error carries the path and types, and no seeded value anywhere.
    #[tokio::test]
    async fn the_http_decoder_attaches_a_value_free_diagnostic() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "five_hour": {"utilization": "sk-live-SEEDED", "resets_at": "ana@example.com"},
                "items": []
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let error = crate::providers::json::<Usage>(http.get(server.uri()))
            .await
            .unwrap_err();
        assert_eq!(error.note.id, crate::providers::SCHEMA_NOTE);
        let diagnostic = error.diagnostic.expect("a diagnostic");
        let shown = format!("{diagnostic:?} {:?}", diagnostic.text());
        assert!(shown.contains("five_hour.utilization"));
        assert!(!shown.contains("SEEDED") && !shown.contains("example.com"));
        let json = serde_json::to_string(&diagnostic).unwrap();
        assert!(!json.contains("SEEDED") && !json.contains("example.com"));
    }

    #[test]
    fn the_text_reads_like_the_requirement() {
        let mut value = good();
        value["five_hour"]["utilization"] = json!("x");
        let text = Diagnostic::SchemaDrift(drift(value)).text();
        assert_eq!(text.id, "diagnostic.schema_drift");
        assert_eq!(
            text.resolve("en-US"),
            "The provider changed five_hour.utilization (number → string)."
        );
    }

    /// C-02: the sentence carries no literal backticks in any catalog, and
    /// the block has its heading, paths and privacy note.
    #[test]
    fn the_drift_block_has_no_backticks_and_all_its_parts() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let catalog: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for key in [
                "diagnostic.schema_drift",
                "diagnostic.what_changed",
                "diagnostic.values_never_recorded",
            ] {
                let text = catalog[key]
                    .as_str()
                    .unwrap_or_else(|| panic!("{key} in {path:?}"));
                assert!(!text.contains('`'), "{path:?}: {text}");
            }
        }
        let drift = field_drift(
            &json!({"five_hour": {"utilization": "x"}}),
            &["five_hour", "utilization"],
            "number",
        );
        let view = Diagnostic::SchemaDrift(drift).view();
        assert_eq!(view.heading.resolve("en-US"), "What changed");
        assert_eq!(view.privacy.resolve("en-US"), "Values are never recorded.");
        assert_eq!(view.paths[0].path, "five_hour.utilization");
        assert!(Diagnostic::NOTE_IDS.contains(&&*view.sentence.id));
    }
}
