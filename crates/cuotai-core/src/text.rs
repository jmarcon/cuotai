//! RF8 — stable localization contract for every text the core delivers to
//! a frontend (field labels and help, cost advisories, unsupported-login
//! reasons, edit errors, report notes, login modes/options/registration).
//!
//! Each user-facing string travels as a [`Localized`]: a **stable
//! identifier** (e.g. `field.kiro.auth_method.label`, `edit_error.duplicate_name`,
//! `note.kimchi.budget_404`, `login.oauth.cursor.label`), structured
//! **parameters**, and the English text as the **default value**
//! (`fallback`, PRD 0034). Constructors take the English template and
//! interpolate the parameters into `fallback`. Frontends resolve the id in
//! their own catalog and render `fallback` when the id is unknown — so
//! adding a language means adding a catalog, and no frontend ever matches
//! on the wording.
//!
//! The core **owns the catalogs** for every id it emits
//! (`locales/{pt-BR,es,it}.json`): [`core_catalog`] returns the static
//! `BTreeMap` for a given locale and [`Localized::resolve`] substitutes
//! `{name}` placeholders, recursing into nested [`Localized`] values.
//! Resolution is exact locale → same language (`pt-PT` → pt-BR, `es-AR` →
//! es) → the English `fallback`; en-US and unknown locales render the
//! fallback. New texts are written in English first and added to every
//! catalog; the core tests fail on a missing id, a renamed placeholder or
//! a Portuguese marker in an English fallback ([`portuguese_markers`]).
//!
//! Security invariant (unchanged): parameters carry only non-secret data —
//! env-var NAMES, param keys, HTTP status codes, provider/status slugs,
//! account names the user already sees. Never token material, request URLs
//! with credentials, or response bodies.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

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

/// One localizable text: stable id + structured params + English default.
///
/// `id` naming is dotted and stable: `field.<provider>.<key>.label`,
/// `native.<provider>.<key>.label`, `login.<provider>.<mode_id>.{label,help,...}`,
/// `edit_error.<case>`, `query.<case>`, `note.<adapter>.<case>`,
/// `credential_help.<provider>`, `oauth.registration_help.<provider>`,
/// `oauth.unsupported_reason.<provider>`, `access.<kind>.<slug>`,
/// `status.<snake_case>`, `provider.<slug>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Localized {
    pub id: Cow<'static, str>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
    /// English default text, already interpolated. Frontends render this
    /// verbatim when the id is absent from their catalog (or always, for
    /// en-US).
    pub fallback: Cow<'static, str>,
}

impl Localized {
    /// A fixed English text without parameters. `fallback` accepts both
    /// `&'static str` and owned `String` (e.g. formatted error messages).
    #[track_caller]
    pub fn fixed(id: impl Into<Cow<'static, str>>, fallback: impl Into<String>) -> Self {
        let fallback: String = fallback.into();
        #[cfg(test)]
        assert_english(&fallback);
        Self {
            id: id.into(),
            params: BTreeMap::new(),
            fallback: Cow::Owned(fallback),
        }
    }

    /// A text with structured parameters. `fallback` is the English template;
    /// its `{name}` placeholders are interpolated with `params` (nested
    /// [`Localized`] values in English).
    #[track_caller]
    pub fn with_params(
        id: impl Into<Cow<'static, str>>,
        params: BTreeMap<String, ParamValue>,
        fallback: impl Into<String>,
    ) -> Self {
        let fallback: String = fallback.into();
        #[cfg(test)]
        assert_english(&fallback);
        let fallback = if params.is_empty() {
            fallback
        } else {
            substitute(&fallback, &params, Locale::EnUs)
        };
        Self {
            id: id.into(),
            params,
            fallback: Cow::Owned(fallback),
        }
    }

    /// Convenience for the common single-parameter case.
    #[track_caller]
    pub fn with_param(
        id: impl Into<Cow<'static, str>>,
        key: &str,
        value: impl Into<ParamValue>,
        fallback: impl Into<String>,
    ) -> Self {
        Self::with_params(
            id,
            params([(key, value.into())]),
            Cow::Owned(fallback.into()),
        )
    }

    /// The English default text (`fallback`).
    pub fn text(&self) -> &str {
        &self.fallback
    }

    /// Render this message against `locale`. Lookup order (PRD 0034 RF1.3):
    /// 1. the catalog of the locale's language (`pt-*` → pt-BR, `es-AR` → es);
    /// 2. the embedded English `fallback`, already interpolated (en-US,
    ///    unknown locales and ids missing from the catalog).
    ///
    /// `{name}` placeholders of a catalog template are substituted with the
    /// matching entry in `params`; missing keys are left verbatim. Nested
    /// [`Localized`] parameters are recursively resolved against the same
    /// locale.
    pub fn resolve(&self, locale: &str) -> String {
        let resolved = Locale::resolve(locale);
        match catalog_for(resolved).and_then(|catalog| catalog.get(&*self.id)) {
            Some(template) => substitute(template, &self.params, resolved),
            None => self.fallback.to_string(),
        }
    }

    /// PRD 0034 RF5.2: this text with its `fallback` (and the fallbacks of
    /// nested parameters) rendered in `locale`, for consumers that display
    /// `fallback` verbatim. Ids and params are unchanged.
    pub fn with_fallback_locale(&self, locale: &str) -> Localized {
        let params = self
            .params
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    ParamValue::L10n(note) => ParamValue::l10n(note.with_fallback_locale(locale)),
                    text => text.clone(),
                };
                (key.clone(), value)
            })
            .collect();
        Localized {
            id: self.id.clone(),
            params,
            fallback: Cow::Owned(self.resolve(locale)),
        }
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

/// `changelog.category.<slug>` — the label of a changelog category in the
/// What's new screen (PRD 0019 RF7.7). The entry text itself stays in
/// English, the changelog's language.
pub fn changelog_category_l10n(category: crate::changelog::Category) -> Localized {
    Localized::fixed(category.l10n_id(), category.fallback())
}

/// `changelog.entries_language` — the note stating that the entries stay in
/// English while the labels follow the interface language.
pub fn changelog_entries_language_l10n() -> Localized {
    Localized::fixed(
        crate::changelog::ENTRIES_LANGUAGE_ID,
        crate::changelog::ENTRIES_LANGUAGE_FALLBACK,
    )
}

/// `changelog.title` — the What's new screen title.
pub fn changelog_title_l10n() -> Localized {
    Localized::fixed(crate::changelog::TITLE_ID, crate::changelog::TITLE_FALLBACK)
}

/// PRD 0024 RF1.4: a span as the core words it, so the TUI and the desktop
/// agree: under an hour in whole minutes rounded up ("37 min", at least 1),
/// under a day in hours and minutes ("1h 35m", "2h"), otherwise in days
/// and hours ("3d 15h", "6d"). Abbreviations, so no plural forms are
/// needed. Zero or negative spans read "0 min".
pub fn duration(span: chrono::TimeDelta) -> Localized {
    let secs = span.num_seconds().max(0);
    if secs < 3_600 {
        // Rounded up; 59 min 30 s still reads "59 min", never "60 min".
        let minutes = ((secs + 59) / 60).min(59);
        let m = minutes.to_string();
        return Localized::with_param("text.duration.minutes", "m", m.as_str(), "{m} min");
    }
    let minutes = secs / 60;
    let (days, hours, mins) = (minutes / 1_440, minutes / 60 % 24, minutes % 60);
    let (d, h, m) = (days.to_string(), hours.to_string(), mins.to_string());
    match (days, hours, mins) {
        (0, _, 0) => Localized::with_param("text.duration.hours", "h", h.as_str(), "{h}h"),
        (0, _, _) => Localized::with_params(
            "text.duration.hours_minutes",
            params([("h", h.as_str().into()), ("m", m.as_str().into())]),
            "{h}h {m}m",
        ),
        (_, 0, _) => Localized::with_param("text.duration.days", "d", d.as_str(), "{d}d"),
        _ => Localized::with_params(
            "text.duration.days_hours",
            params([("d", d.as_str().into()), ("h", h.as_str().into())]),
            "{d}d {h}h",
        ),
    }
}

/// PRD 0024 RF1.4: time until something happens, "in 1h 35m" (see
/// [`duration`]), or "now" for a span under one second.
pub fn countdown(span: chrono::TimeDelta) -> Localized {
    if span.num_seconds() < 1 {
        return Localized::fixed("text.countdown.now", "now");
    }
    Localized::with_params(
        "text.countdown",
        params([("duration", duration(span).into())]),
        "in {duration}",
    )
}

// ---------------------------------------------------------------------------
// Core catalogs: parsed lazily from `include_str!` so the binary only pays
// for the JSON once and never blocks startup. The pt-BR/es/it maps share
// the same key set; equality of key sets is asserted by a unit test.
// ---------------------------------------------------------------------------

/// Available core locale buckets. en-US has no catalog: the embedded
/// English `fallback` is the source text (PRD 0034 RF1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    Pt,
    EnUs,
    Es,
    It,
}

impl Locale {
    /// Mirror the desktop `i18n.js#pickLocale` precedence:
    /// exact match → language base (`es-AR` → `es`, `it-IT` → `it`, `pt-*` → pt-BR)
    /// → `en-US`, the English fallback, for any other locale.
    pub fn resolve(locale: &str) -> Locale {
        let lower = locale.to_ascii_lowercase();
        if lower == "en-us" || lower == "en" {
            Locale::EnUs
        } else if lower == "es" {
            Locale::Es
        } else if lower == "it" {
            Locale::It
        } else if lower.starts_with("pt") {
            Locale::Pt
        } else if lower.starts_with("es") {
            // es-AR, es-MX, es-ES, …
            Locale::Es
        } else if lower.starts_with("it") {
            // it-IT, it-CH, …
            Locale::It
        } else {
            Locale::EnUs
        }
    }
}

fn parse_catalog(raw: &'static str) -> BTreeMap<String, String> {
    // Locale JSONs carry a small `_meta` object alongside the flat
    // `id → template` map; serde_json can't decode both shapes with the
    // same target type, so we ignore `_meta` after a structural check.
    let value: serde_json::Value = serde_json::from_str(raw)
        .expect("cuotai-core locale JSON must be valid; check locales/{pt-BR,es,it}.json");
    let mut map = BTreeMap::new();
    if let serde_json::Value::Object(object) = value {
        for (key, val) in object {
            if key == "_meta" {
                continue;
            }
            match val {
                serde_json::Value::String(s) => {
                    map.insert(key, s);
                }
                _ => panic!("cuotai-core locale JSON: id {key:?} must map to a string template"),
            }
        }
    } else {
        panic!("cuotai-core locale JSON must be an object at the top level");
    }
    map
}

fn core_catalog_pt_br() -> &'static BTreeMap<String, String> {
    static PT: LazyLock<BTreeMap<String, String>> =
        LazyLock::new(|| parse_catalog(include_str!("../locales/pt-BR.json")));
    &PT
}

fn core_catalog_es() -> &'static BTreeMap<String, String> {
    static ES: LazyLock<BTreeMap<String, String>> =
        LazyLock::new(|| parse_catalog(include_str!("../locales/es.json")));
    &ES
}

fn core_catalog_it() -> &'static BTreeMap<String, String> {
    static IT: LazyLock<BTreeMap<String, String>> =
        LazyLock::new(|| parse_catalog(include_str!("../locales/it.json")));
    &IT
}

fn catalog_for(locale: Locale) -> Option<&'static BTreeMap<String, String>> {
    match locale {
        Locale::Pt => Some(core_catalog_pt_br()),
        Locale::EnUs => None,
        Locale::Es => Some(core_catalog_es()),
        Locale::It => Some(core_catalog_it()),
    }
}

/// Lookup the core catalog for `locale`: the static map of the resolved
/// language (`pt-*` → pt-BR, `es-*` → es, `it-*` → it). Returns `None` for
/// en-US and unknown locales, whose text is `Localized::fallback` itself.
pub fn core_catalog(locale: &str) -> Option<&'static BTreeMap<String, String>> {
    catalog_for(Locale::resolve(locale))
}

/// Substitute every `{name}` placeholder in `template` against `params`.
/// Nested [`Localized`] values are recursively resolved against the same
/// `locale`. Missing keys and unknown placeholders are left verbatim so
/// the rendered text still surfaces the literal `{name}` for debugging.
fn substitute(template: &str, params: &BTreeMap<String, ParamValue>, locale: Locale) -> String {
    let mut out = String::with_capacity(template.len());
    let chars: Vec<(usize, char)> = template.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (start, ch) = chars[i];
        if ch != '{' {
            out.push(ch);
            i += 1;
            continue;
        }
        // Find the closing `}` after an identifier.
        let mut j = i + 1;
        let mut name_end = None;
        while j < chars.len() {
            let (_, ch2) = chars[j];
            if ch2 == '}' {
                name_end = Some(j);
                break;
            }
            if !(ch2.is_ascii_alphanumeric() || ch2 == '_') {
                break;
            }
            j += 1;
        }
        let Some(close_idx) = name_end else {
            // Not a valid placeholder — emit `{` verbatim and continue.
            out.push('{');
            i += 1;
            continue;
        };
        let name_start = start + '{'.len_utf8();
        let name = &template[name_start..chars[close_idx].0];
        if name.is_empty() {
            out.push('{');
            i += 1;
            continue;
        }
        match params.get(name) {
            Some(ParamValue::Text(value)) => out.push_str(value),
            Some(ParamValue::L10n(note)) => {
                let resolved = note.resolve_as_locale(locale);
                out.push_str(&resolved);
            }
            None => {
                // Unknown placeholder — leave the original `{name}` verbatim.
                out.push_str(&template[start..=chars[close_idx].0]);
            }
        }
        i = close_idx + 1;
    }
    out
}

impl Localized {
    fn resolve_as_locale(&self, locale: Locale) -> String {
        let locale_str = match locale {
            Locale::Pt => "pt-BR",
            Locale::EnUs => "en-US",
            Locale::Es => "es",
            Locale::It => "it",
        };
        self.resolve(locale_str)
    }
}

/// Accumulator for report notes: providers push the English text once
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
    #[track_caller]
    pub fn push(&mut self, id: &'static str, text: impl Into<String>) {
        self.push_note(Localized::fixed(id, text));
    }

    /// Push a note with structured parameters.
    #[track_caller]
    pub fn push_params(
        &mut self,
        id: &'static str,
        params: BTreeMap<String, ParamValue>,
        text: impl Into<String>,
    ) {
        self.push_note(Localized::with_params(id, params, text));
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

    /// Split into the legacy parallel vectors: `(plain English texts, l10n)`.
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

/// PRD 0034 RF5.2: the environment variable that names the language of the
/// `fallback` texts in `--json`, `--json-v1`, `--serve` and `--watch --jsonl`.
pub const FALLBACK_LOCALE_ENV: &str = "CUOTAI_FALLBACK_LOCALE";

/// PRD 0034 RF5.2: the language in which machine-readable output renders its
/// `fallback` texts for consumers that display them verbatim: `flag`, else
/// [`FALLBACK_LOCALE_ENV`]. `None` keeps the English fallbacks: nothing set,
/// an empty value, en-US or a language without a core catalog.
pub fn fallback_locale(flag: Option<&str>) -> Option<String> {
    let chosen = match flag {
        Some(flag) => flag.to_owned(),
        None => std::env::var(FALLBACK_LOCALE_ENV).ok()?,
    };
    let chosen = chosen.trim();
    (!chosen.is_empty() && core_catalog(chosen).is_some()).then(|| chosen.to_owned())
}

/// PRD 0034 RF5.2: render, in `locale`, the `fallback` of every
/// [`Localized`] (an object with string `id` and `fallback`) of a JSON
/// document, nested ones included, and copy each `notes_l10n` fallback into
/// the parallel plain `notes` entry. Ids, params and the shape are unchanged.
pub fn localize_json_fallbacks(value: &mut serde_json::Value, locale: &str) {
    use serde_json::Value;
    match value {
        Value::Array(items) => {
            for item in items {
                localize_json_fallbacks(item, locale);
            }
        }
        Value::Object(map) => {
            let localized = matches!(map.get("id"), Some(Value::String(_)))
                && matches!(map.get("fallback"), Some(Value::String(_)));
            if localized {
                if let Ok(note) = serde_json::from_value::<Localized>(Value::Object(map.clone())) {
                    map.insert("fallback".to_owned(), Value::String(note.resolve(locale)));
                }
            }
            for item in map.values_mut() {
                localize_json_fallbacks(item, locale);
            }
            let texts: Option<Vec<Value>> = match map.get("notes_l10n") {
                Some(Value::Array(notes)) => notes
                    .iter()
                    .map(|note| note.get("fallback").cloned())
                    .collect(),
                _ => None,
            };
            if let (Some(texts), Some(Value::Array(notes))) = (texts, map.get_mut("notes")) {
                if texts.len() == notes.len() {
                    *notes = texts;
                }
            }
        }
        _ => {}
    }
}

/// PRD 0034 RF4.2: the Portuguese markers found in `text` (characters
/// `ã`, `õ`, `ç` and a short list of stop words), so a test can prove an
/// English text carries none. Proper names that contain a marker belong in
/// [`PORTUGUESE_MARKER_ALLOWED`].
pub fn portuguese_markers(text: &str) -> Vec<&'static str> {
    const CHARS: [(&str, [char; 2]); 3] = [("ã", ['ã', 'Ã']), ("õ", ['õ', 'Õ']), ("ç", ['ç', 'Ç'])];
    const WORDS: [&str; 5] = ["não", "já", "está", "conta", "cota"];
    let mut cleaned = text.to_string();
    for allowed in PORTUGUESE_MARKER_ALLOWED {
        cleaned = cleaned.replace(allowed, "");
    }
    let mut found: Vec<(usize, &'static str)> = Vec::new();
    for (name, chars) in CHARS {
        if let Some(at) = cleaned.find(|c| chars.contains(&c)) {
            found.push((at, name));
        }
    }
    let lower = cleaned.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect();
    for word in WORDS {
        if let Some(at) = words.iter().position(|w| *w == word) {
            found.push((usize::MAX / 2 + at, word));
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, name)| name).collect()
}

/// Proper names a Portuguese-marker check must not flag.
pub const PORTUGUESE_MARKER_ALLOWED: [&str; 0] = [];

/// PRD 0034 RF4.2: in the core's own tests every `Localized` built in the
/// core must have an English fallback template.
#[cfg(test)]
#[track_caller]
fn assert_english(template: &str) {
    let markers = portuguese_markers(template);
    assert!(
        markers.is_empty(),
        "Portuguese marker(s) {markers:?} in an English fallback: {template:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Every id the descriptor APIs emit for any provider must be translated
    /// in every shipped catalog; otherwise that frontend locale silently
    /// shows the English fallback.
    #[test]
    fn every_descriptor_id_is_translated_in_every_catalog() {
        use crate::account_edit::{self as edit, FieldDescriptorKind};
        use crate::oauth::login::{self, LoginOption};
        use crate::Provider;

        fn option_ids(option: &LoginOption, ids: &mut Vec<String>) {
            ids.push(option.label.id.to_string());
            ids.push(option.help.id.to_string());
            for choice in option.choices.iter().flatten() {
                ids.push(choice.label.id.to_string());
                if let Some(reason) = &choice.unavailable_reason {
                    ids.push(reason.id.to_string());
                }
            }
        }

        let mut ids = Vec::new();
        ids.push("edit_error.option_not_valid_for_auth_mode".to_string());
        ids.push("edit_notice.option_replaced_for_auth_mode".to_string());
        for error in [
            crate::models::ModelError::InvalidIndex,
            crate::models::ModelError::NoCache,
            crate::models::ModelError::Storage,
        ] {
            ids.push(error.l10n().id.to_string());
        }
        for live in [true, false] {
            ids.push(crate::models::claim_note(live).id.to_string());
        }
        for auth_mode in [
            edit::AUTH_MODE_CREDENTIAL,
            "oauth",
            "aws_sso",
            "bailian_console",
            "unknown",
        ] {
            ids.push(edit::auth_mode_label(auth_mode).id.to_string());
        }
        for provider in Provider::ALL {
            // Every source/reason the product/region params can select.
            for (key, value) in [
                ("", ""),
                ("product", "cli"),
                ("product", "coding"),
                ("region", "cn"),
            ] {
                let account = crate::Account {
                    id: None,
                    provider,
                    params: [(key.to_string(), value.to_string())].into(),
                    ..Default::default()
                };
                let models = crate::models::support(&account).unwrap_or_else(|reason| reason);
                ids.push(models.id.to_string());
            }
            ids.push(edit::credential_help(provider).id.to_string());
            for session in crate::user_session::descriptors(provider) {
                ids.push(session.label().id.to_string());
                ids.push(session.instructions().id.to_string());
                let legacy = crate::Account {
                    id: None,
                    provider,
                    params: [(session.slot.to_string(), "VAR".to_string())].into(),
                    ..Default::default()
                };
                for candidate in crate::user_session::migration_candidates(&legacy) {
                    ids.push(candidate.consent.id.to_string());
                }
                let notice = crate::user_session::notice(&legacy).expect("env session");
                ids.push(notice.level.id.to_string());
                ids.push(notice.warning.id.to_string());
            }
            if let Some(notice) = crate::user_session::paste_notice(provider) {
                ids.push(notice.warning.id.to_string());
            }
            ids.push(crate::user_session::migrated_notice().id.to_string());
            ids.push(crate::user_session::forgotten_notice().id.to_string());
            ids.push("user_session.forget.nothing".to_string());
            for d in edit::field_descriptors(provider) {
                ids.push(edit::field_label_id(provider, d.key));
                ids.push(edit::field_help_id(provider, d.key));
                if let FieldDescriptorKind::Choice(choice) = d.kind {
                    for option in choice.options {
                        ids.push(edit::field_option_id(provider, d.key, option.value));
                        if option.auth_modes.is_some() {
                            ids.push(edit::field_option_unavailable_id(
                                provider,
                                d.key,
                                option.value,
                            ));
                        }
                    }
                }
            }
            let offered = login::login_modes(provider);
            if let Some(reason) = &offered.unsupported_reason {
                ids.push(reason.id.to_string());
            }
            for mode in &offered.modes {
                ids.push(mode.label.id.to_string());
                ids.push(mode.help.id.to_string());
                ids.push(mode.completion_notice.id.to_string());
                if let Some(reason) = &mode.unavailable_reason {
                    ids.push(reason.id.to_string());
                }
                for option in mode.registration.iter().chain(&mode.options) {
                    option_ids(option, &mut ids);
                }
            }
        }
        for locale in ["pt-BR", "es", "it"] {
            let catalog = core_catalog(locale).expect("shipped catalog");
            let missing: BTreeSet<_> = ids
                .iter()
                .filter(|id| !catalog.contains_key(id.as_str()))
                .collect();
            assert!(missing.is_empty(), "{locale} lacks {missing:?}");
        }
    }

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

    #[test]
    fn locale_resolve_precedence() {
        assert_eq!(Locale::resolve("en-US"), Locale::EnUs);
        assert_eq!(Locale::resolve("EN"), Locale::EnUs);
        assert_eq!(Locale::resolve("es"), Locale::Es);
        assert_eq!(Locale::resolve("es-AR"), Locale::Es);
        assert_eq!(Locale::resolve("it"), Locale::It);
        assert_eq!(Locale::resolve("it-IT"), Locale::It);
        assert_eq!(Locale::resolve("it-CH"), Locale::It);
        assert_eq!(Locale::resolve("pt-BR"), Locale::Pt);
        assert_eq!(Locale::resolve("pt-PT"), Locale::Pt);
        assert_eq!(Locale::resolve("fr-FR"), Locale::EnUs); // unknown -> en-US
        assert_eq!(Locale::resolve(""), Locale::EnUs);
    }

    #[test]
    fn resolve_en_us_substitutes_params() {
        let note = Localized::with_param(
            "note.inference.perplexity_member_filter",
            "email",
            "alice@example.com",
            "Filtered by organisation member: {email}.",
        );
        // The English template is interpolated into the fallback, which is
        // what en-US renders.
        assert_eq!(
            note.fallback,
            "Filtered by organisation member: alice@example.com."
        );
        let en = note.resolve("en-US");
        assert_eq!(en, "Filtered by organisation member: alice@example.com.");
        let es = note.resolve("es");
        assert_eq!(
            es,
            "Filtrado por miembro de la organización: alice@example.com."
        );
        // pt-BR resolves the id in its catalog.
        let pt = note.resolve("pt-BR");
        assert_eq!(pt, "Filtrado por membro da organização: alice@example.com.");
        // An unknown locale renders the English fallback.
        let fr = note.resolve("fr-FR");
        assert_eq!(fr, "Filtered by organisation member: alice@example.com.");
    }

    #[test]
    fn resolve_unknown_key_falls_back_to_default() {
        let note = Localized::fixed("note.no.such.id", "Texto em português.");
        assert_eq!(note.resolve("en-US"), "Texto em português.");
        assert_eq!(note.resolve("es"), "Texto em português.");
        assert_eq!(note.resolve("pt-BR"), "Texto em português.");
    }

    #[test]
    fn resolve_recurses_into_nested_localized() {
        // Embedded provider/status fragments should translate in the
        // current locale, not parrot the English label.
        let note = Localized::with_param(
            "note.test.with_provider",
            "provider",
            provider_l10n(crate::Provider::AnthropicApi),
            "Account of provider {provider} failed.",
        );
        let en = note.resolve("en-US");
        assert!(en.contains("Anthropic API"), "got: {en}");
        let es = note.resolve("es");
        assert!(es.contains("Anthropic API"), "got: {es}");
        let pt = note.resolve("pt-BR");
        assert!(pt.contains("Anthropic API"), "got: {pt}");
    }

    #[test]
    fn catalog_keys_match_between_pt_es_and_it() {
        // PRD 0034 RF4.1: every shipped catalog carries the same ids.
        let pt = core_catalog("pt-BR").expect("pt-BR catalog");
        let pt_keys: BTreeSet<&str> = pt.keys().map(String::as_str).collect();
        for locale in ["es", "it"] {
            let catalog = core_catalog(locale).expect("shipped catalog");
            let keys: BTreeSet<&str> = catalog.keys().map(String::as_str).collect();
            let missing: Vec<&&str> = pt_keys.difference(&keys).collect();
            let extra: Vec<&&str> = keys.difference(&pt_keys).collect();
            assert!(missing.is_empty(), "{locale} lacks {missing:?}");
            assert!(extra.is_empty(), "{locale} has ids pt-BR lacks: {extra:?}");
        }
        assert!(core_catalog("en-US").is_none(), "English is the fallback");
    }

    /// PRD 0034 RF4.1: a translation keeps every placeholder of the others,
    /// so renaming one in a single catalog fails here.
    #[test]
    fn catalog_placeholders_match_between_catalogs() {
        fn names(template: &str) -> BTreeSet<&str> {
            let mut out = BTreeSet::new();
            let mut rest = template;
            while let Some(start) = rest.find('{') {
                rest = &rest[start + 1..];
                let Some(end) = rest.find('}') else { break };
                let name = &rest[..end];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    out.insert(name);
                }
                rest = &rest[end + 1..];
            }
            out
        }
        // Literal braces that are not parameters (a path or an example).
        const LITERAL: [&str; 4] = [
            "edit_error.param.resource_id",
            "field.azure_open_ai.resource_id.help",
            "note.coding.personal_balance_only",
            "query.cloud.invalid_azure_resource_id",
        ];
        let pt = core_catalog("pt-BR").expect("pt-BR catalog");
        let mut mismatched = Vec::new();
        for locale in ["es", "it"] {
            let catalog = core_catalog(locale).expect("shipped catalog");
            for (id, template) in pt {
                if LITERAL.contains(&id.as_str()) {
                    continue;
                }
                if let Some(other) = catalog.get(id) {
                    if names(template) != names(other) {
                        mismatched.push(format!("{locale} {id}"));
                    }
                }
            }
        }
        assert!(mismatched.is_empty(), "placeholders differ: {mismatched:?}");
    }

    /// PRD 0034 RF5.2: the fallback locale renders every `fallback`, nested
    /// ones and the plain `notes` included, leaving ids and params alone.
    #[test]
    fn json_fallbacks_render_in_the_fallback_locale() {
        let nested = Localized::with_param(
            "binding.blocked",
            "window",
            Localized::fixed("metric_window.weekly", "weekly"),
            "blocked by the {window} window; return time unknown",
        );
        let mut json = serde_json::json!({
            "notes": [nested.fallback.clone(), "kept"],
            "notes_l10n": [nested.clone(), {"id": "unknown.id", "fallback": "kept"}],
            "label": Localized::fixed("status.ready", "Up to date"),
            "other": {"id": "not-a-text"},
        });
        let original = json.clone();
        localize_json_fallbacks(&mut json, "pt-BR");
        let pt = "bloqueada pela janela semanal; horário de retorno desconhecido";
        assert_eq!(json["notes_l10n"][0]["fallback"], pt);
        assert_eq!(
            json["notes_l10n"][0]["params"]["window"]["fallback"],
            "semanal"
        );
        assert_eq!(
            json["notes_l10n"][0]["params"]["window"]["id"],
            "metric_window.weekly"
        );
        assert_eq!(json["notes"], serde_json::json!([pt, "kept"]));
        assert_eq!(json["label"]["fallback"], "Atualizado");
        assert_eq!(json["other"], original["other"]);
        let mut english = original.clone();
        localize_json_fallbacks(&mut english, "en-US");
        assert_eq!(english, original, "English is already the fallback");

        assert_eq!(fallback_locale(Some("pt-BR")).as_deref(), Some("pt-BR"));
        assert_eq!(fallback_locale(Some(" es ")).as_deref(), Some("es"));
        assert_eq!(fallback_locale(Some("en-US")), None);
        assert_eq!(fallback_locale(Some("fr")), None);
        assert_eq!(fallback_locale(Some("")), None);
    }

    /// PRD 0034 RF4.2: the detector flags Portuguese markers and lets
    /// English and proper names through.
    #[test]
    fn the_portuguese_detector_catches_a_seeded_fallback() {
        assert_eq!(
            portuguese_markers("Não foi possível ler a cota da conta."),
            ["ã", "não", "cota", "conta"]
        );
        assert_eq!(
            portuguese_markers("A sessão já está ativa."),
            ["ã", "já", "está"]
        );
        assert_eq!(portuguese_markers("Configuração"), ["ç", "ã"]);
        assert!(portuguese_markers("Could not read the account quota.").is_empty());
        assert!(portuguese_markers("{provider}: {count} usage alerts").is_empty());
        let seeded = std::panic::catch_unwind(|| {
            Localized::fixed("test.seeded", "A cota da conta não foi lida.")
        });
        assert!(
            seeded.is_err(),
            "a Portuguese fallback must not be accepted"
        );
    }

    /// Regression for the smoke bug: `OAuthError::l10n()` emits a
    /// `Localized` whose `{detail}` placeholder must resolve in en-US
    /// (and not leave a raw `{detail}` fragment inside English text).
    #[test]
    fn oauth_configuration_error_resolves_in_en_us() {
        use crate::oauth::OAuthError;
        // Smoke scenario: a Configuration error built from a raw message
        // must render in en-US through the `{detail}` placeholder, never
        // leaving a raw `{detail}` fragment nor concatenating Portuguese
        // into the English sentence structure.
        let note = OAuthError::from("client_id ausente".to_string()).l10n();
        let en = note.resolve("en-US");
        assert!(
            !en.contains('{'),
            "en-US must substitute the placeholder: got {en:?}"
        );
        assert!(
            en.contains("client_id ausente"),
            "en-US must keep the detail value: got {en:?}"
        );
        let es = note.resolve("es");
        assert!(
            !es.contains('{'),
            "es must substitute the placeholder: got {es:?}"
        );
        assert!(
            es.contains("client_id ausente"),
            "es must keep the detail value: got {es:?}"
        );
        // pt-BR resolves the id in its catalog.
        let pt = note.resolve("pt-BR");
        assert_eq!(pt, "Configuração OAuth: client_id ausente");
    }
}
