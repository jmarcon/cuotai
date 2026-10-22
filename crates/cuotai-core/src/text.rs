//! RF8 — stable localization contract for every text the core delivers to
//! a frontend (field labels and help, cost advisories, unsupported-login
//! reasons, edit errors, report notes, login modes/options/registration).
//!
//! Each user-facing string travels as a [`Localized`]: a **stable
//! identifier** (e.g. `field.kiro.auth_method.label`, `edit_error.duplicate_name`,
//! `note.kimchi.budget_404`, `login.oauth.cursor.label`), structured
//! **parameters**, and the Portuguese text as the **default value**
//! (`fallback`). Frontends resolve the id in their own catalog and render
//! `fallback` (with parameters interpolated) when the id is unknown — so
//! adding a language means adding a catalog, and no frontend ever matches
//! on the Portuguese wording.
//!
//! The core **owns the catalogs** for every id it emits:
//! [`core_catalog`] returns the static `BTreeMap` for a given locale and
//! [`Localized::resolve`] substitutes `{name}` placeholders, recursing
//! into nested [`Localized`] values. pt-* never consults a catalog (the
//! fallback text is authoritative); unknown locales fall back to
//! `en-US`; unknown keys in a current language fall back to `en-US`
//! then to `fallback`.
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

/// One localizable text: stable id + structured params + Portuguese default.
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
    /// Portuguese default text, already interpolated. Frontends render this
    /// verbatim when the id is absent from their catalog (or always, when
    /// the locale is a pt-* variant).
    pub fallback: Cow<'static, str>,
}

impl Localized {
    /// A fixed text without parameters. `fallback` accepts both `&'static str`
    /// and owned `String` (e.g. formatted error messages).
    pub fn fixed(id: impl Into<Cow<'static, str>>, fallback: impl Into<String>) -> Self {
        let fallback: String = fallback.into();
        Self {
            id: id.into(),
            params: BTreeMap::new(),
            fallback: Cow::Owned(fallback),
        }
    }

    /// A text with structured parameters. `fallback` is the Portuguese
    /// rendering with the values already interpolated.
    pub fn with_params(
        id: impl Into<Cow<'static, str>>,
        params: BTreeMap<String, ParamValue>,
        fallback: impl Into<String>,
    ) -> Self {
        let fallback: String = fallback.into();
        Self {
            id: id.into(),
            params,
            fallback: Cow::Owned(fallback),
        }
    }

    /// Convenience for the common single-parameter case.
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

    /// The Portuguese default text (what pre-RF8 callers received).
    pub fn text(&self) -> &str {
        &self.fallback
    }

    /// Render this message against `locale`. Lookup order:
    /// 1. pt-* → embedded `fallback` (no catalog consulted).
    /// 2. locale catalog → current language (es-AR falls back to es).
    /// 3. en-US catalog (for unknown locales and missing keys).
    /// 4. embedded `fallback`.
    ///
    /// `{name}` placeholders are substituted with the matching entry in
    /// `params`; missing keys are left verbatim. Nested [`Localized`]
    /// parameters are recursively resolved against the same locale.
    pub fn resolve(&self, locale: &str) -> String {
        let resolved = Locale::resolve(locale);
        let template = match resolved {
            Locale::Pt => None,
            Locale::EnUs => core_catalog_en_us().get(&*self.id).cloned(),
            Locale::Es => core_catalog_es()
                .get(&*self.id)
                .cloned()
                .or_else(|| core_catalog_en_us().get(&*self.id).cloned()),
        };
        let template = template.unwrap_or_else(|| self.fallback.to_string());
        substitute(&template, &self.params, resolved)
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
    Localized::fixed(category.l10n_id(), category.fallback_pt())
}

/// `changelog.entries_language` — the note stating that the entries stay in
/// English while the labels follow the interface language.
pub fn changelog_entries_language_l10n() -> Localized {
    Localized::fixed(
        crate::changelog::ENTRIES_LANGUAGE_ID,
        crate::changelog::ENTRIES_LANGUAGE_PT,
    )
}

/// `changelog.title` — the What's new screen title.
pub fn changelog_title_l10n() -> Localized {
    Localized::fixed(crate::changelog::TITLE_ID, crate::changelog::TITLE_PT)
}

// ---------------------------------------------------------------------------
// Core catalogs: parsed lazily from `include_str!` so the binary only pays
// for the JSON once and never blocks startup. The two en-US/es maps share
// the same key set; equality of key sets is asserted by a unit test.
// ---------------------------------------------------------------------------

/// Available core locale buckets. pt-* never loads a catalog because the
/// embedded Portuguese fallback is authoritative for that variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    Pt,
    EnUs,
    Es,
}

impl Locale {
    /// Mirror the desktop `i18n.js#pickLocale` precedence:
    /// exact match → language base (`es-AR` → `es`) → `pt-*` → `en-US`
    /// for any other locale (so unknown languages still get a translation).
    pub fn resolve(locale: &str) -> Locale {
        let lower = locale.to_ascii_lowercase();
        if lower == "en-us" || lower == "en" {
            Locale::EnUs
        } else if lower == "es" {
            Locale::Es
        } else if lower.starts_with("pt") {
            Locale::Pt
        } else if lower.starts_with("es") {
            // es-AR, es-MX, es-ES, …
            Locale::Es
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
        .expect("cuotai-core locale JSON must be valid; check locales/{en-US,es}.json");
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

fn core_catalog_en_us() -> &'static BTreeMap<String, String> {
    static EN: LazyLock<BTreeMap<String, String>> =
        LazyLock::new(|| parse_catalog(include_str!("../locales/en-US.json")));
    &EN
}

fn core_catalog_es() -> &'static BTreeMap<String, String> {
    static ES: LazyLock<BTreeMap<String, String>> =
        LazyLock::new(|| parse_catalog(include_str!("../locales/es.json")));
    &ES
}

/// Lookup the core catalog for `locale`. Returns `None` for pt-* (the
/// caller should use `Localized::fallback`); otherwise returns the static
/// map for the resolved language (en-US fallback when a key is absent).
pub fn core_catalog(locale: &str) -> Option<&'static BTreeMap<String, String>> {
    match Locale::resolve(locale) {
        Locale::Pt => None,
        Locale::EnUs => Some(core_catalog_en_us()),
        Locale::Es => Some(core_catalog_es()),
    }
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
        };
        self.resolve(locale_str)
    }
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
    use std::collections::BTreeSet;

    /// Every id the descriptor APIs emit for any provider must be translated
    /// in every shipped catalog; otherwise that frontend locale silently
    /// shows the Portuguese fallback.
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
        for locale in ["en-US", "es"] {
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
            "Filtrado por membro da organização: alice@example.com.",
        );
        // en-US catalog carries the same id with the {email} placeholder.
        let en = note.resolve("en-US");
        assert_eq!(en, "Filtered by organisation member: alice@example.com.");
        let es = note.resolve("es");
        assert_eq!(
            es,
            "Filtrado por miembro de la organización: alice@example.com."
        );
        // pt-BR never consults a catalog.
        let pt = note.resolve("pt-BR");
        assert_eq!(pt, "Filtrado por membro da organização: alice@example.com.");
        // Unknown locale falls back to en-US.
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
        // current locale, not parrot the Portuguese label.
        let note = Localized::with_param(
            "note.test.with_provider",
            "provider",
            provider_l10n(crate::Provider::AnthropicApi),
            "Conta do provedor {provider} falhou.",
        );
        let en = note.resolve("en-US");
        assert!(en.contains("Anthropic API"), "got: {en}");
        let es = note.resolve("es");
        assert!(es.contains("Anthropic API"), "got: {es}");
        let pt = note.resolve("pt-BR");
        assert!(pt.contains("Anthropic API"), "got: {pt}");
    }

    #[test]
    fn catalog_keys_match_between_en_and_es() {
        let en = parse_catalog(include_str!("../locales/en-US.json"));
        let es = parse_catalog(include_str!("../locales/es.json"));
        let en_keys: BTreeSet<&str> = en.keys().map(String::as_str).collect();
        let es_keys: BTreeSet<&str> = es.keys().map(String::as_str).collect();
        let only_en: Vec<&&str> = en_keys.difference(&es_keys).collect();
        let only_es: Vec<&&str> = es_keys.difference(&es_keys).collect();
        assert!(only_en.is_empty(), "en-only keys: {only_en:?}");
        assert!(only_es.is_empty(), "es-only keys: {only_es:?}");
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
        // pt-BR keeps the Portuguese fallback (no catalog consulted).
        let pt = note.resolve("pt-BR");
        assert_eq!(pt, "Configuração OAuth: client_id ausente");
    }
}
