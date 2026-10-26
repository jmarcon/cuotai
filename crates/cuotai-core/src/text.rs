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
//! (`locales/<code>.json`): [`core_catalog`] returns the static
//! [`Catalog`] for a given locale and [`Localized::resolve`] substitutes
//! `{name}` placeholders, recursing into nested [`Localized`] values.
//! Every language is a row of the registry ([`LOCALES`], PRD 0035 RF1);
//! [`normalize`] resolves a raw code to its row by language subtag
//! (`pt-PT` → pt-BR, `es-AR` → es, `zh_CN.UTF-8` → zh-CN, never `zh-TW` →
//! zh-CN), then the row's catalog → the English `fallback`; en-US, unknown
//! locales and languages still in progress render the fallback. A catalog
//! value may be a plural object, selected by the `count` parameter
//! ([`plural_category`]). New texts are written in English first and added to every
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
            substitute(&fallback, &params, Locale(source_locale()))
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

    /// PRD 0035 RF4: a text that embeds a count. `params` must carry
    /// `count`; the English fallback is `one` or `other` by the English
    /// rule, and catalogs give the id a plural object with the categories
    /// of their language.
    #[track_caller]
    pub fn plural(
        id: impl Into<Cow<'static, str>>,
        params: BTreeMap<String, ParamValue>,
        one: &'static str,
        other: &'static str,
    ) -> Self {
        let count = match params.get("count") {
            Some(ParamValue::Text(count)) => count.as_str(),
            _ => "",
        };
        let english = match plural_category(source_locale().plural_rule, count) {
            "one" => one,
            _ => other,
        };
        Self::with_params(id, params, english)
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
        let count = match self.params.get("count") {
            Some(ParamValue::Text(count)) => Some(count.as_str()),
            _ => None,
        };
        match resolved
            .core_catalog()
            .and_then(|catalog| catalog.get(&*self.id))
            .and_then(|message| message.template(resolved.row().plural_rule, count))
        {
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
// for the JSON of the language in use and never blocks startup. The
// registry ([`LOCALES`]) lists every language; [`embedded_core_catalog`]
// carries one `include_str!` per catalog file, and a test proves the two
// agree with the files on disk (PRD 0035 RF1.1).
// ---------------------------------------------------------------------------

mod fold;
pub mod parity;
mod registry;
pub use fold::fold;
pub use registry::{
    locale_row, normalize, plural_category, source_locale, Direction, LocaleRow, LocaleStatus,
    PluralRule, LOCALES,
};

/// One catalog value: a template, or a plural object of CLDR categories
/// (`{ "one": "…", "other": "…" }`, PRD 0035 RF4.1) selected by the
/// parameter named `count`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Message {
    Text(String),
    Plural(BTreeMap<String, String>),
}

impl Message {
    /// The template for `count` under `rule`: the category's form, else
    /// `other`. A plain text ignores `count`.
    pub fn template(&self, rule: PluralRule, count: Option<&str>) -> Option<&str> {
        match self {
            Message::Text(text) => Some(text),
            Message::Plural(forms) => {
                let category = count.map_or("other", |count| plural_category(rule, count));
                forms
                    .get(category)
                    .or_else(|| forms.get("other"))
                    .map(String::as_str)
            }
        }
    }

    /// The plain text, `None` for a plural object.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Message::Text(text) => Some(text),
            Message::Plural(_) => None,
        }
    }

    /// Every template of this message (one, or each plural form).
    pub fn templates(&self) -> Vec<&str> {
        match self {
            Message::Text(text) => vec![text.as_str()],
            Message::Plural(forms) => forms.values().map(String::as_str).collect(),
        }
    }
}

/// A parsed catalog (core, TUI or desktop UI): `id -> message`, plus the
/// `_meta` object. One parser for every layer (PRD 0035 RF5.2). Serializes
/// as the bare map of messages (the `core_catalog` Tauri command).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    messages: BTreeMap<String, Message>,
    meta: serde_json::Map<String, serde_json::Value>,
}

/// Why a catalog file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError(pub String);

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CatalogError {}

impl Catalog {
    /// Parse a catalog JSON: an object whose values are strings or plural
    /// objects of strings (with at least `other`), and an optional `_meta`
    /// object.
    pub fn parse(raw: &str) -> Result<Catalog, CatalogError> {
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|error| CatalogError(error.to_string()))?;
        let serde_json::Value::Object(object) = value else {
            return Err(CatalogError("a catalog must be a JSON object".into()));
        };
        let mut catalog = Catalog::default();
        for (key, value) in object {
            match value {
                serde_json::Value::Object(meta) if key == "_meta" => catalog.meta = meta,
                serde_json::Value::String(text) => {
                    catalog.messages.insert(key, Message::Text(text));
                }
                serde_json::Value::Object(forms) => {
                    let mut plural = BTreeMap::new();
                    for (category, form) in forms {
                        if !["zero", "one", "two", "few", "many", "other"]
                            .contains(&category.as_str())
                        {
                            return Err(CatalogError(format!(
                                "{key:?}: {category:?} is not a plural category"
                            )));
                        }
                        let serde_json::Value::String(form) = form else {
                            return Err(CatalogError(format!(
                                "{key:?}.{category}: a plural form must be a string"
                            )));
                        };
                        plural.insert(category, form);
                    }
                    if !plural.contains_key("other") {
                        return Err(CatalogError(format!("{key:?}: a plural needs `other`")));
                    }
                    catalog.messages.insert(key, Message::Plural(plural));
                }
                _ => {
                    return Err(CatalogError(format!(
                        "{key:?} must map to a string or a plural object"
                    )))
                }
            }
        }
        Ok(catalog)
    }

    /// The `_meta` object (`name`, provenance).
    pub fn meta(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.meta
    }

    /// The messages, by id.
    pub fn messages(&self) -> &BTreeMap<String, Message> {
        &self.messages
    }
}

impl std::ops::Deref for Catalog {
    type Target = BTreeMap<String, Message>;

    fn deref(&self) -> &Self::Target {
        &self.messages
    }
}

impl Serialize for Catalog {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.messages.serialize(serializer)
    }
}

/// One lazily parsed static per catalog file; a code without a file is
/// `None` (the source language and languages in progress).
macro_rules! embedded_catalogs {
    ($($code:literal => $file:literal,)*) => {
        fn embedded_core_catalog(code: &str) -> Option<&'static Catalog> {
            match code {
                $($code => {
                    static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
                        Catalog::parse(include_str!($file))
                            .unwrap_or_else(|error| panic!("cuotai-core {}: {error}", $file))
                    });
                    Some(&CATALOG)
                })*
                _ => None,
            }
        }

        /// The codes with an embedded core catalog, for the registry test.
        #[cfg(test)]
        const EMBEDDED_CORE_CATALOGS: &[&str] = &[$($code),*];
    };
}

embedded_catalogs! {
    "pt-BR" => "../locales/pt-BR.json",
    "es" => "../locales/es.json",
    "it" => "../locales/it.json",
    "de" => "../locales/de.json",
    "ru" => "../locales/ru.json",
    "ko" => "../locales/ko.json",
    "hi-IN" => "../locales/hi-IN.json",
    "id-ID" => "../locales/id-ID.json",
    "tr" => "../locales/tr.json",
    "vi" => "../locales/vi.json",
    "zh-CN" => "../locales/zh-CN.json",
    "fr" => "../locales/fr.json",
    "ja" => "../locales/ja.json",
}

/// A resolved language: a handle on its registry row. Unknown codes
/// resolve to the source (en-US).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locale(&'static LocaleRow);

impl Locale {
    /// PRD 0035 RF1.3: [`normalize`], else en-US.
    pub fn resolve(locale: &str) -> Locale {
        Locale(normalize(locale).unwrap_or_else(source_locale))
    }

    /// The canonical registry code (`zh-CN`, `es`).
    pub fn code(self) -> &'static str {
        self.0.code
    }

    /// The registry row.
    pub fn row(self) -> &'static LocaleRow {
        self.0
    }

    /// Whether this is the source language, whose text is the fallback.
    pub fn is_source(self) -> bool {
        self.0.status == LocaleStatus::Source
    }

    /// The core catalog of this language, if it has one.
    pub fn core_catalog(self) -> Option<&'static Catalog> {
        embedded_core_catalog(self.0.code)
    }
}

/// Lookup the core catalog for `locale`: the static map of the resolved
/// language (`pt-*` → pt-BR, `es-*` → es, `zh-Hans` → zh-CN). Returns
/// `None` for en-US, unknown locales and languages still in progress,
/// whose text is `Localized::fallback` itself.
pub fn core_catalog(locale: &str) -> Option<&'static Catalog> {
    Locale::resolve(locale).core_catalog()
}

/// The codes of the languages that have a core catalog, in registry order
/// (every shipped language but the source). Tests loop over this instead
/// of a list of literals (PRD 0035 RF2.2).
pub fn catalog_locales() -> Vec<&'static str> {
    LOCALES
        .iter()
        .filter(|row| embedded_core_catalog(row.code).is_some())
        .map(|row| row.code)
        .collect()
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
        self.resolve(locale.code())
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
        for locale in crate::text::catalog_locales() {
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
        for (raw, code) in [
            ("en-US", "en-US"),
            ("EN", "en-US"),
            ("es", "es"),
            ("es-AR", "es"),
            ("it", "it"),
            ("it-IT", "it"),
            ("it-CH", "it"),
            ("pt-BR", "pt-BR"),
            ("pt-PT", "pt-BR"),
            ("fr-FR", "fr"),
            ("zh-TW", "en-US"),
            ("est", "en-US"),
            ("", "en-US"),
        ] {
            assert_eq!(Locale::resolve(raw).code(), code, "{raw:?}");
        }
        assert!(Locale::resolve("xx").is_source());
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
        // A language Cuotai does not ship renders the English fallback.
        let xx = note.resolve("xx-XX");
        assert_eq!(xx, "Filtered by organisation member: alice@example.com.");
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

    /// Ids whose braces are literal text (a path or an example), not
    /// parameters.
    const LITERAL_BRACES: [&str; 4] = [
        "edit_error.param.resource_id",
        "field.azure_open_ai.resource_id.help",
        "note.coding.personal_balance_only",
        "query.cloud.invalid_azure_resource_id",
    ];

    /// PRD 0035 RF2.2 (and PRD 0034 RF4.1): every core catalog carries the
    /// same ids, placeholders and plural shapes. The English source lives
    /// in the code, so the first translated catalog is the reference.
    #[test]
    fn core_catalogs_are_in_parity() {
        let codes = catalog_locales();
        let reference = core_catalog(codes[0]).expect("a reference catalog");
        let mut problems = Vec::new();
        for code in &codes {
            let row = locale_row(code).unwrap();
            let catalog = core_catalog(code).unwrap();
            problems.extend(parity::problems(
                row,
                catalog,
                reference,
                parity::Allowances {
                    literal_braces: &LITERAL_BRACES,
                    ..Default::default()
                },
            ));
        }
        assert!(problems.is_empty(), "{problems:#?}");
        assert!(core_catalog("en-US").is_none(), "English is the fallback");
    }

    /// PRD 0035 RF1.1: the registry, the `include_str!` table and the files
    /// on disk list the same languages. A language in progress has no
    /// file; the source has none either.
    #[test]
    fn the_registry_matches_the_core_catalog_files() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let on_disk: BTreeSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".json").map(str::to_owned)
            })
            .collect();
        let embedded: BTreeSet<String> = EMBEDDED_CORE_CATALOGS
            .iter()
            .map(|code| code.to_string())
            .collect();
        assert_eq!(on_disk, embedded, "a catalog file without include_str!");
        let shipped: BTreeSet<String> = LOCALES
            .iter()
            .filter(|row| row.status == LocaleStatus::Shipped)
            .map(|row| row.code.to_string())
            .collect();
        assert_eq!(
            embedded, shipped,
            "a shipped row needs a catalog and a catalog needs a shipped row"
        );
        for code in &embedded {
            assert!(core_catalog(code).is_some_and(|c| !c.is_empty()), "{code}");
        }
    }

    /// PRD 0035 RF3.2: the desktop UI catalogs, against `en-US.json` (the
    /// UI source). en-US also carries core ids the core cannot give it
    /// (there is no en-US core catalog); the others take those from their
    /// core catalog. pt-BR keeps two leftovers of its time as the source.
    /// Both lists only shrink.
    #[test]
    fn desktop_ui_catalogs_are_in_parity() {
        const EN_US_ONLY: &[&str] = &[
            "pace.chip.ahead",
            "pace.chip.ahead_pct",
            "pace.chip.exhausted",
            "pace.chip.lasts",
            "pace.chip.on_pace",
            "pace.chip.reserve",
            "pace.chip.reserve_pct",
            "pace.chip.runs_out",
            "pace.chip.too_early",
            "status.authentication_error",
            "status.error",
            "status.needs_configuration",
            "status.rate_limited",
            "status.unsupported",
        ];
        const PT_BR_EXTRA: &[&str] = &["status.ready", "edit_error.login_cleanup_failed"];
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/ui/locales");
        let read = |code: &str| {
            let raw = std::fs::read_to_string(dir.join(format!("{code}.json")))
                .unwrap_or_else(|error| panic!("{code}.json: {error}"));
            Catalog::parse(&raw).unwrap_or_else(|error| panic!("{code}.json: {error}"))
        };
        let on_disk: BTreeSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".json").map(str::to_owned)
            })
            .collect();
        let translated: BTreeSet<String> = LOCALES
            .iter()
            .filter(|row| row.is_translated())
            .map(|row| row.code.to_string())
            .collect();
        assert_eq!(on_disk, translated, "UI catalog files and the registry");
        let source = source_locale();
        let reference = read(source.code);
        let mut problems = parity::problems(source, &reference, &reference, Default::default());
        for row in LOCALES.iter().filter(|row| row.is_translated()) {
            if row.code == source.code {
                continue;
            }
            let catalog = read(row.code);
            let extra = if row.code == "pt-BR" {
                PT_BR_EXTRA
            } else {
                &[]
            };
            problems.extend(parity::problems(
                row,
                &catalog,
                &reference,
                parity::Allowances {
                    missing: EN_US_ONLY,
                    extra,
                    literal_braces: &[],
                },
            ));
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// PRD 0035 RF1.5: the desktop's static registry copy (browser demo)
    /// and the pre-paint copies of the HTML pages equal the registry.
    /// `CUOTAI_REGENERATE_LOCALES=1` rewrites them.
    #[test]
    fn locale_registry_copies() {
        fn replace_between(text: &str, begin: &str, end: &str, body: &str) -> String {
            let start = text.find(begin).expect("begin marker") + begin.len();
            let stop = start + text[start..].find(end).expect("end marker");
            let line_start = text[..stop].rfind('\n').map_or(stop, |i| i + 1);
            let indent = &text[line_start..stop];
            format!("{}\n{body}\n{indent}{}", &text[..start], &text[stop..])
        }
        let regenerate = std::env::var_os("CUOTAI_REGENERATE_LOCALES").is_some();
        let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/ui");
        let rows: Vec<String> = LOCALES
            .iter()
            .map(|row| format!("  {},", serde_json::to_string(row).unwrap()))
            .collect();
        let js_body = format!("export const REGISTRY = [\n{}\n];", rows.join("\n"));
        let mut targets = vec![(
            ui.join("js/locale-registry.js"),
            "// registry:begin",
            "// registry:end",
            js_body,
        )];
        let translated: Vec<serde_json::Value> = LOCALES
            .iter()
            .filter(|row| row.is_translated())
            .map(|row| {
                serde_json::json!({
                    "code": row.code,
                    "languages": row.languages,
                    "excluded_scripts": row.excluded_scripts,
                    "excluded_regions": row.excluded_regions,
                })
            })
            .collect();
        let html_body = format!(
            "      var localeRows = {};",
            serde_json::to_string(&translated).unwrap()
        );
        for page in ["index.html", "tray.html", "settings.html"] {
            targets.push((
                ui.join(page),
                "/* registry:begin */",
                "/* registry:end */",
                html_body.clone(),
            ));
        }
        let mut stale = Vec::new();
        for (path, begin, end, body) in targets {
            let text = std::fs::read_to_string(&path).unwrap();
            let want = replace_between(&text, begin, end, &body);
            if want != text {
                if regenerate {
                    std::fs::write(&path, want).unwrap();
                } else {
                    stale.push(path.display().to_string());
                }
            }
        }
        assert!(
            stale.is_empty(),
            "stale registry copies {stale:?}: run CUOTAI_REGENERATE_LOCALES=1 cargo test -p cuotai-core locale_registry_copies"
        );
    }

    /// PRD 0035 RF8.2 (DS-TOK-TYP-7): every registry script beyond Latin
    /// and Cyrillic has its system font stack in `themes.css`, keyed by
    /// the row's language, so a new language cannot ship without one.
    #[test]
    fn every_script_has_a_font_stack() {
        let css = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/desktop/ui/css/themes.css"),
        )
        .unwrap();
        for row in LOCALES {
            if matches!(row.script, "Latn" | "Cyrl") {
                continue;
            }
            let selector = format!(":lang({}) {{", row.languages[0]);
            let rule = css
                .find(&selector)
                .map(|at| &css[at..at + css[at..].find('}').unwrap()]);
            assert!(
                rule.is_some_and(|rule| rule.contains("--font-script:")),
                "{}: no `{selector} --font-script` rule in themes.css",
                row.code
            );
        }
    }

    /// PRD 0035 RF2.2: the parity rules catch each kind of drift.
    #[test]
    fn parity_catches_each_kind_of_drift() {
        let row = locale_row("ru").unwrap();
        let reference = Catalog::parse(
            r#"{"_meta":{"name":"English"},"a":"Hi {name}","n":{"one":"{count} item","other":"{count} items"}}"#,
        )
        .unwrap();
        let check = |raw: &str| {
            let catalog = Catalog::parse(raw).unwrap();
            parity::problems(row, &catalog, &reference, Default::default())
        };
        let good = r#"{"_meta":{"name":"Русский"},"a":"Привет, {name}","n":{"one":"{count} элемент","few":"{count} элемента","many":"{count} элементов","other":"{count} элемента"}}"#;
        assert!(check(good).is_empty(), "{:?}", check(good));
        for (bad, why) in [
            (
                r#"{"_meta":{"name":"Русский"},"n":{"one":"a","few":"b","many":"c","other":"{count}"}}"#,
                "lacks a",
            ),
            (
                r#"{"_meta":{"name":"Русский"},"a":"Привет, {nom}","n":{"one":"{count}","few":"{count}","many":"{count}","other":"{count}"}}"#,
                "placeholders",
            ),
            (
                r#"{"_meta":{"name":"Русский"},"a":"{name}","n":{"one":"{count}","other":"{count}"}}"#,
                "plural",
            ),
            (
                r#"{"_meta":{"name":"Russian"},"a":"{name}","n":{"one":"{count}","few":"{count}","many":"{count}","other":"{count}"}}"#,
                "endonym",
            ),
            (
                r#"{"_meta":{"name":"Русский"},"a":"<b>{name}</b>","n":{"one":"{count}","few":"{count}","many":"{count}","other":"{count}"}}"#,
                "markup",
            ),
            (
                r#"{"_meta":{"name":"Русский"},"a":"{name}","n":"{count}"}"#,
                "plural in one",
            ),
            (
                r#"{"_meta":{"name":"Русский"},"a":"{name}","b":"x","n":{"one":"{count}","few":"{count}","many":"{count}","other":"{count}"}}"#,
                "has b",
            ),
            (
                r#"{"_meta":{"name":"Русский"},"a":" ","n":{"one":"{count}","few":"{count}","many":"{count}","other":"{count}"}}"#,
                "empty",
            ),
        ] {
            let found = check(bad);
            assert!(found.iter().any(|p| p.contains(why)), "{why}: {found:?}");
        }
        let vi = locale_row("vi").unwrap();
        let reference = Catalog::parse(r#"{"a":"x"}"#).unwrap();
        let decomposed =
            Catalog::parse("{\"_meta\":{\"name\":\"Tiếng Việt\"},\"a\":\"Vie\u{0302}\u{0323}t\"}")
                .unwrap();
        let found = parity::problems(vi, &decomposed, &reference, Default::default());
        assert!(found.iter().any(|p| p.contains("NFC")), "{found:?}");
        assert!(
            Catalog::parse(r#"{"n":{"one":"x"}}"#).is_err(),
            "a plural needs other"
        );
        assert!(Catalog::parse(r#"{"n":{"lots":"x","other":"y"}}"#).is_err());
        assert!(Catalog::parse(r#"{"n":3}"#).is_err());
    }

    /// PRD 0035 RF4.1: a plural object picks its form from `count`, by the
    /// rule of the locale, and falls to `other`.
    #[test]
    fn plural_messages_pick_the_form_of_the_count() {
        let message = Message::Plural(
            [
                ("one", "{count} alert"),
                ("few", "{count} few"),
                ("many", "{count} many"),
                ("other", "{count} alerts"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        );
        let ru = PluralRule::EastSlavic;
        assert_eq!(message.template(ru, Some("21")), Some("{count} alert"));
        assert_eq!(message.template(ru, Some("22")), Some("{count} few"));
        assert_eq!(message.template(ru, Some("11")), Some("{count} many"));
        assert_eq!(message.template(ru, Some("1.5")), Some("{count} alerts"));
        assert_eq!(message.template(ru, None), Some("{count} alerts"));
        let only_other = Message::Plural([("other".to_string(), "{count} 件".to_string())].into());
        assert_eq!(
            only_other.template(PluralRule::One, Some("1")),
            Some("{count} 件")
        );
        let note = Localized::plural(
            "test.plural",
            params([("count", 1usize.into())]),
            "{count} problem",
            "{count} problems",
        );
        assert_eq!(note.fallback, "1 problem");
        let note = Localized::plural(
            "test.plural",
            params([("count", 3usize.into())]),
            "{count} problem",
            "{count} problems",
        );
        assert_eq!(note.fallback, "3 problems");
        assert_eq!(note.resolve("ko"), "3 problems", "in progress: English");
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
        assert_eq!(fallback_locale(Some("de")).as_deref(), Some("de"));
        assert_eq!(fallback_locale(Some("xx")), None);
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
