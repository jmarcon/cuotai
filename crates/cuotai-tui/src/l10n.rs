//! PRD 0034 RF2/RF3: the TUI's and the CLI's own texts (chrome, captions,
//! dialogs, `--once` labels, prompts and stderr messages).
//!
//! The English text is written in code, next to a stable id; the other
//! languages live in `crates/cuotai-tui/locales/<code>.json`, one file per
//! translated row of the core registry (`cuotai_core::text::LOCALES`,
//! PRD 0035 RF1), parsed by the core's one catalog parser
//! (`cuotai_core::text::Catalog`), and are resolved against the TUI locale
//! ([`crate::ui::tui_locale`]). `{name}` placeholders are substituted from
//! the call's parameters. Texts that come from the core are
//! [`cuotai_core::text::Localized`] and resolve through the core catalogs
//! instead.

use cuotai_core::text::{Catalog, Locale, Message};
#[cfg(test)]
use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::LazyLock;

/// One lazily parsed static per TUI catalog file (PRD 0035 RF1.1); a code
/// without a file (the source, a language in progress) is `None`.
macro_rules! tui_catalogs {
    ($($code:literal => $file:literal,)*) => {
        fn embedded(code: &str) -> Option<&'static Catalog> {
            match code {
                $($code => {
                    static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
                        Catalog::parse(include_str!($file))
                            .unwrap_or_else(|error| panic!("cuotai-tui {}: {error}", $file))
                    });
                    Some(&CATALOG)
                })*
                _ => None,
            }
        }

        #[cfg(test)]
        const EMBEDDED: &[&str] = &[$($code),*];
    };
}

tui_catalogs! {
    "pt-BR" => "../locales/pt-BR.json",
    "es" => "../locales/es.json",
    "it" => "../locales/it.json",
    "de" => "../locales/de.json",
    "ru" => "../locales/ru.json",
}

/// The TUI catalog of `locale`'s language; `None` for English and for a
/// language whose translation is still in progress.
pub(crate) fn catalog(locale: &str) -> Option<&'static Catalog> {
    embedded(Locale::resolve(locale).code())
}

/// The text of `id` in the TUI locale, `en` (the English source) otherwise.
pub(crate) fn tr(id: &str, en: &'static str) -> &'static str {
    tr_in(crate::ui::tui_locale(), id, en)
}

/// [`tr`] for an explicit locale.
pub(crate) fn tr_in(locale: &str, id: &str, en: &'static str) -> &'static str {
    catalog(locale)
        .and_then(|catalog| catalog.get(id))
        .and_then(Message::as_text)
        .unwrap_or(en)
}

/// [`tr`] with `{name}` placeholders substituted from `params`.
pub(crate) fn trf(id: &str, en: &'static str, params: &[(&str, &dyn Display)]) -> String {
    substitute(tr(id, en), params)
}

/// [`trf`] for an explicit locale.
pub(crate) fn trf_in(
    locale: &str,
    id: &str,
    en: &'static str,
    params: &[(&str, &dyn Display)],
) -> String {
    substitute(tr_in(locale, id, en), params)
}

/// Replace every `{name}` of `template` that `params` carries; other braces
/// stay verbatim.
pub(crate) fn substitute(template: &str, params: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let name = &after[..name_len];
        let closed = after[name_len..].starts_with('}');
        match params.iter().find(|(key, _)| *key == name) {
            Some((_, value)) if closed && !name.is_empty() => {
                out.push_str(&value.to_string());
                rest = &after[name_len + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Every `tr`/`trf` call site's `(id, English)` pair, read from the sources
/// so the catalog tests can prove each id is translated everywhere.
#[cfg(test)]
pub(crate) fn source_ids() -> BTreeMap<String, String> {
    const SOURCES: &[(&str, &str)] = &[
        ("about_view.rs", include_str!("about_view.rs")),
        ("auth_ui.rs", include_str!("auth_ui.rs")),
        ("canary.rs", include_str!("canary.rs")),
        ("changelog_view.rs", include_str!("changelog_view.rs")),
        ("cost.rs", include_str!("cost.rs")),
        ("dashboard.rs", include_str!("dashboard.rs")),
        ("extension_check.rs", include_str!("extension_check.rs")),
        ("extensions_view.rs", include_str!("extensions_view.rs")),
        ("headless.rs", include_str!("headless.rs")),
        ("main.rs", include_str!("main.rs")),
        ("models_view.rs", include_str!("models_view.rs")),
        ("motion.rs", include_str!("motion.rs")),
        ("notify.rs", include_str!("notify.rs")),
        ("report_problem.rs", include_str!("report_problem.rs")),
        ("report_view.rs", include_str!("report_view.rs")),
        ("serve.rs", include_str!("serve.rs")),
        ("stats_view.rs", include_str!("stats_view.rs")),
        ("status_line.rs", include_str!("status_line.rs")),
        ("ui.rs", include_str!("ui.rs")),
        ("update_notice.rs", include_str!("update_notice.rs")),
        ("vault.rs", include_str!("vault.rs")),
        ("watch.rs", include_str!("watch.rs")),
        ("wizard.rs", include_str!("wizard.rs")),
    ];
    let mut ids = BTreeMap::new();
    for (file, source) in SOURCES {
        for (index, _) in source.match_indices("tr") {
            let tail = &source[index..];
            // Skip identifiers that merely end in `tr` (`str(`, `attr(`).
            let before = source[..index].chars().last();
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let args = if let Some(args) = tail
                .strip_prefix("tr_in(")
                .or_else(|| tail.strip_prefix("trf_in("))
            {
                match args.find(',') {
                    Some(comma) => &args[comma + 1..],
                    None => continue,
                }
            } else if let Some(args) = tail.strip_prefix("trf(") {
                args
            } else if let Some(args) = tail.strip_prefix("tr(") {
                args
            } else {
                continue;
            };
            let Some(open) = args.trim_start().strip_prefix('"') else {
                continue;
            };
            let id_end = open.find('"').expect("closed id");
            let id = &open[..id_end];
            let rest = open[id_end + 1..].trim_start_matches([',', ' ', '\n', '\r']);
            let Some(rest) = rest.strip_prefix('"') else {
                panic!("{file}: the English text of {id:?} must be a literal");
            };
            let mut english = String::new();
            let mut chars = rest.chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n') => english.push('\n'),
                        Some('\n') => {
                            while chars.as_str().starts_with([' ', '\t']) {
                                chars.next();
                            }
                        }
                        Some(other) => english.push(other),
                        None => break,
                    },
                    other => english.push(other),
                }
            }
            if let Some(previous) = ids.insert(id.to_string(), english.clone()) {
                assert_eq!(previous, english, "{file}: id {id:?} has two English texts");
            }
        }
    }
    ids
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_LOCALE: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

/// Run `body` with the TUI locale set to `locale` on this test thread.
#[cfg(test)]
pub(crate) fn with_locale<T>(locale: &'static str, body: impl FnOnce() -> T) -> T {
    let previous = TEST_LOCALE.with(|cell| cell.replace(Some(locale)));
    let out = body();
    TEST_LOCALE.with(|cell| cell.set(previous));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(text: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let len = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            if len > 0 && after[len..].starts_with('}') {
                out.insert(after[..len].to_string());
            }
            rest = after;
        }
        out
    }

    #[test]
    fn every_source_id_is_in_every_catalog_with_the_same_placeholders() {
        let ids = source_ids();
        assert!(ids.len() > 100, "the scanner found only {} ids", ids.len());
        for locale in EMBEDDED {
            let catalog = catalog(locale).expect("catalog");
            for (id, english) in &ids {
                let Some(text) = catalog.get(id).and_then(Message::as_text) else {
                    panic!("{locale}: id {id:?} ({english:?}) is missing");
                };
                assert_eq!(
                    placeholders(text),
                    placeholders(english),
                    "{locale}: placeholders of {id:?}"
                );
            }
            for id in catalog.keys() {
                assert!(ids.contains_key(id), "{locale}: stale id {id:?}");
            }
        }
    }

    #[test]
    fn english_sources_and_es_it_texts_carry_no_portuguese() {
        for (id, english) in source_ids() {
            let markers = cuotai_core::text::portuguese_markers(&english);
            assert!(markers.is_empty(), "{id}: {markers:?} in {english:?}");
        }
        for locale in ["es", "it"] {
            for (id, text) in catalog(locale).unwrap().iter() {
                let text = text.as_text().expect("TUI texts are strings");
                // "está" is Spanish too.
                let markers: Vec<_> = cuotai_core::text::portuguese_markers(text)
                    .into_iter()
                    .filter(|marker| !(locale == "es" && *marker == "está"))
                    .collect();
                assert!(markers.is_empty(), "{locale} {id}: {markers:?} in {text:?}");
            }
        }
    }

    /// PRD 0035 RF1.1 and RF5.3: the registry, the `include_str!` table
    /// and the files on disk list the same languages, and the catalogs are
    /// in parity with each other (ids, placeholders, `_meta.name`).
    #[test]
    fn the_registry_matches_the_tui_catalogs() {
        use cuotai_core::text::{locale_row, parity, LocaleStatus, LOCALES};
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let on_disk: BTreeSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".json").map(str::to_owned)
            })
            .collect();
        let embedded: BTreeSet<String> = EMBEDDED.iter().map(|c| c.to_string()).collect();
        let shipped: BTreeSet<String> = LOCALES
            .iter()
            .filter(|row| row.status == LocaleStatus::Shipped)
            .map(|row| row.code.to_string())
            .collect();
        assert_eq!(on_disk, embedded, "a TUI catalog file without include_str!");
        assert_eq!(embedded, shipped, "TUI catalogs and the registry");
        let reference = catalog(EMBEDDED[0]).unwrap();
        let problems: Vec<String> = EMBEDDED
            .iter()
            .flat_map(|code| {
                parity::problems(
                    locale_row(code).unwrap(),
                    catalog(code).unwrap(),
                    reference,
                    Default::default(),
                )
            })
            .collect();
        assert!(problems.is_empty(), "{problems:#?}");
        // A language in progress renders the English source.
        assert_eq!(with_locale("ja", || tr("x.missing", "Hello")), "Hello");
        assert!(catalog("ja").is_none());
    }

    #[test]
    fn substitute_replaces_known_placeholders_only() {
        assert_eq!(
            substitute("{n} of {total} {x} {", &[("n", &1), ("total", &"2")]),
            "1 of 2 {x} {"
        );
    }

    #[test]
    fn the_locale_override_picks_the_catalog() {
        assert_eq!(with_locale("en-US", || tr("x.missing", "Hello")), "Hello");
    }
}
