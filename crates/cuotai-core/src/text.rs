//! RF8 — stable localization contract for every text the core delivers to
//! a frontend (field labels and help, cost advisories, unsupported-login
//! reasons, edit errors, report notes).
//!
//! Each user-facing string travels as a [`Localized`]: a **stable
//! identifier** (e.g. `field.kiro.auth_method.label`, `edit_error.duplicate_name`,
//! `note.kimchi.budget_404`), structured **parameters**, and the Portuguese
//! text as the **default value** (`fallback`). Frontends resolve the id in
//! their own catalog and render `fallback` (with parameters interpolated)
//! when the id is unknown — so adding a language means adding a catalog,
//! and no frontend ever matches on the Portuguese wording.
//!
//! Security invariant (unchanged): parameters carry only non-secret data —
//! env-var NAMES, param keys, HTTP status codes, provider/status slugs,
//! account names the user already sees. Never token material, request URLs
//! with credentials, or response bodies.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

/// `fallback` accepts both `&'static str` and owned `String` so callers can
/// pass either a static literal or a `format!`-rendered value.
pub enum FallbackCow {
    Static(&'static str),
    Owned(String),
}

impl From<&'static str> for FallbackCow {
    fn from(value: &'static str) -> Self {
        FallbackCow::Static(value)
    }
}
impl From<String> for FallbackCow {
    fn from(value: String) -> Self {
        FallbackCow::Owned(value)
    }
}
impl From<&String> for FallbackCow {
    fn from(value: &String) -> Self {
        FallbackCow::Owned(value.clone())
    }
}
impl From<Cow<'static, str>> for FallbackCow {
    fn from(value: Cow<'static, str>) -> Self {
        match value {
            Cow::Borrowed(s) => FallbackCow::Static(s),
            Cow::Owned(s) => FallbackCow::Owned(s),
        }
    }
}

/// A parameter value for a [`Localized`] message. Either plain text
/// (already safe to display) or a nested [`Localized`] so the frontend can
/// translate embedded fragments too (e.g. a status label inside a note).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Text(String),
    L10n(Box<Localized>),
}

impl ParamValue {
    pub fn l10n(note: Localized) -> Self {
        ParamValue::L10n(Box::new(note))
    }
}

impl From<String> for ParamValue {
    fn from(value: String) -> Self {
        ParamValue::Text(value)
    }
}
impl From<&str> for ParamValue {
    fn from(value: &str) -> Self {
        ParamValue::Text(value.to_string())
    }
}
impl From<Localized> for ParamValue {
    fn from(value: Localized) -> Self {
        ParamValue::l10n(value)
    }
}
impl From<bool> for ParamValue {
    fn from(value: bool) -> Self {
        ParamValue::Text(value.to_string())
    }
}
impl From<usize> for ParamValue {
    fn from(value: usize) -> Self {
        ParamValue::Text(value.to_string())
    }
}
impl From<u64> for ParamValue {
    fn from(value: u64) -> Self {
        ParamValue::Text(value.to_string())
    }
}
impl From<u16> for ParamValue {
    fn from(value: u16) -> Self {
        ParamValue::Text(value.to_string())
    }
}

/// Build the parameter map from `(name, value)` pairs.
pub fn params<const N: usize>(items: [(&str, ParamValue); N]) -> BTreeMap<String, ParamValue> {
    items.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// One localizable text: stable id + structured params + Portuguese default.
///
/// `id` naming is dotted and stable: `field.<provider>.<key>.label`,
/// `edit_error.<case>`, `query.<case>`, `note.<adapter>.<case>`,
/// `credential_help.<provider>`, `cost_advisory.<provider>`,
/// `oauth.registration_help.<provider>`, `oauth.unsupported_reason.<provider>`,
/// `status.<snake_case>`, `provider.<snake_case>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Localized {
    pub id: Cow<'static, str>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
    /// Portuguese default text, already interpolated. Frontends render this
    /// verbatim when the id is absent from their catalog.
    pub fallback: Cow<'static, str>,
}

impl Localized {
    /// A fixed text without parameters. `fallback` accepts both `&'static str`
    /// and owned `String` (e.g. formatted error messages).
    pub fn fixed(id: impl Into<Cow<'static, str>>, fallback: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            params: BTreeMap::new(),
            fallback: Cow::Owned(fallback.into()),
        }
    }

    /// A text with structured parameters. `fallback` is the Portuguese
    /// rendering with the values already interpolated.
    pub fn with_params(
        id: impl Into<Cow<'static, str>>,
        params: BTreeMap<String, ParamValue>,
        fallback: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            params,
            fallback: Cow::Owned(fallback.into()),
        }
    }

    /// Convenience for the common single-parameter case.
    pub fn with_param(
        id: impl Into<Cow<'static, str>>,
        key: &str,
        value: impl Into<ParamValue>,
        fallback: impl Into<String>,
    ) -> Self {
        Self::with_params(id, params([(key, value.into())]), fallback)
    }

    /// The Portuguese default text (what pre-RF8 callers received).
    pub fn text(&self) -> &str {
        &self.fallback
    }
}

impl fmt::Display for Localized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.fallback)
    }
}

impl From<Localized> for String {
    fn from(note: Localized) -> String {
        note.fallback.into_owned()
    }
}

/// `status.<snake_case>` — the same ids the desktop catalog already uses
/// for the serialized [`crate::Status`] enum.
pub fn status_l10n(status: crate::Status) -> Localized {
    use crate::Status::*;
    let slug = match status {
        Ready => "ready",
        NeedsConfiguration => "needs_configuration",
        Unsupported => "unsupported",
        AuthenticationError => "authentication_error",
        RateLimited => "rate_limited",
        Error => "error",
    };
    Localized::fixed(format!("status.{slug}"), status.label())
}

/// `provider.<slug>` — provider display name as a localizable fragment
/// (used as a nested parameter inside notes and errors).
pub fn provider_l10n(provider: crate::Provider) -> Localized {
    Localized::fixed(format!("provider.{}", provider.slug()), provider.label())
}

/// Accumulator for report notes: providers push the Portuguese text once
/// together with its stable id and parameters; [`Notes::finish`] splits the
/// legacy `Vec<String>` (fallbacks, unchanged ordering) from the structured
/// `Vec<Localized>` sidecar. This keeps `Report.notes` and the Snapshot/CLI
/// JSON byte-compatible while frontends migrate to `notes_l10n`.
#[derive(Debug, Default)]
pub struct Notes {
    texts: Vec<String>,
    l10n: Vec<Localized>,
}

impl Notes {
    pub fn new() -> Self {
        Self::default()
    }

    /// A single-note collection (e.g. the error path of a query).
    pub fn one(note: Localized) -> Self {
        Self {
            texts: vec![note.fallback.clone().into_owned()],
            l10n: vec![note],
        }
    }

    /// Push a fixed note (no parameters).
    pub fn push(&mut self, id: &'static str, text: impl Into<String>) {
        let text = text.into();
        self.l10n.push(Localized::fixed(id, text.clone()));
        self.texts.push(text);
    }

    /// Push a note with structured parameters.
    pub fn push_params(
        &mut self,
        id: &'static str,
        params: BTreeMap<String, ParamValue>,
        text: impl Into<String>,
    ) {
        let text = text.into();
        self.l10n
            .push(Localized::with_params(id, params, text.clone()));
        self.texts.push(text);
    }

    /// Push an already-built [`Localized`] (e.g. a forwarded error note).
    pub fn push_note(&mut self, note: Localized) {
        self.texts.push(note.fallback.clone().into_owned());
        self.l10n.push(note);
    }

    pub fn extend(&mut self, other: Notes) {
        self.texts.extend(other.texts);
        self.l10n.extend(other.l10n);
    }

    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    /// Split into the legacy parallel vectors: `(plain PT texts, l10n)`.
    /// Both have the same length and ordering.
    pub(crate) fn finish(self) -> (Vec<String>, Vec<Localized>) {
        (self.texts, self.l10n)
    }

    /// Test/portability view over the plain texts.
    #[allow(dead_code)]
    pub(crate) fn texts(&self) -> &[String] {
        &self.texts
    }
}

impl FromIterator<Localized> for Notes {
    fn from_iter<I: IntoIterator<Item = Localized>>(iter: I) -> Self {
        let mut notes = Notes::new();
        for note in iter {
            notes.push_note(note);
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_serializes_without_params() {
        let note = Localized::fixed("note.test.plain", "Texto fixo.");
        let json = serde_json::to_value(&note).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"id": "note.test.plain", "fallback": "Texto fixo."})
        );
        let back: Localized = serde_json::from_value(json).unwrap();
        assert_eq!(back, note);
    }

    #[test]
    fn params_roundtrip_including_nested_l10n() {
        let note = Localized::with_params(
            "note.test.nested",
            params([
                ("code", ParamValue::from(503u16)),
                (
                    "status",
                    ParamValue::from(status_l10n(crate::Status::RateLimited)),
                ),
            ]),
            "Consulta retornou HTTP 503 (Rate limit).",
        );
        let json = serde_json::to_string(&note).unwrap();
        let back: Localized = serde_json::from_str(&json).unwrap();
        assert_eq!(back, note);
        assert_eq!(back.text(), "Consulta retornou HTTP 503 (Rate limit).");
    }

    #[test]
    fn notes_keeps_texts_and_l10n_aligned() {
        let mut notes = Notes::new();
        notes.push("note.test.a", "Primeira.");
        notes.push_params(
            "note.test.b",
            params([("name", ParamValue::from("X_API_KEY"))]),
            "Credencial ausente: X_API_KEY",
        );
        assert!(!notes.is_empty());
        let (texts, l10n) = notes.finish();
        assert_eq!(texts.len(), l10n.len());
        assert_eq!(texts[1], l10n[1].fallback);
    }
}
