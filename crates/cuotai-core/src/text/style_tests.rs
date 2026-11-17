//! PRD 0035 RF11 (re-validation V35-02 to V35-09, V35-11): the register
//! table, the glossary and the punctuation rules of
//! `apps/desktop/ui/locales/README.md`, enforced on the built-in catalogs of
//! the three layers (desktop UI, core, TUI).
//!
//! The tests are grouped by delivery commit: `// G10` register, `// G11`
//! glossary ratchets, `// G12` punctuation. A G11 ratchet freezes the ids
//! whose text still uses the competing term on purpose: a new use fails
//! until it is reviewed and added, and a fixed one fails until it is
//! removed, so each list only shrinks.

use super::Catalog;
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;

/// The parsed catalog of `code` in `layer` (`ui`, `core` or `tui`), when
/// that layer has one.
fn layer_catalog(layer: &str, code: &str) -> Option<Catalog> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = match layer {
        "ui" => root.join("../../apps/desktop/ui/locales"),
        "core" => root.join("locales"),
        "tui" => root.join("../cuotai-tui/locales"),
        other => panic!("unknown layer {other}"),
    };
    let raw = std::fs::read_to_string(dir.join(format!("{code}.json"))).ok()?;
    Some(Catalog::parse(&raw).unwrap_or_else(|error| panic!("{layer}/{code}.json: {error}")))
}

/// Every built-in text of `code`, as (`layer:id`, template); a plural gives
/// one entry per form.
fn texts(code: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for layer in ["ui", "core", "tui"] {
        let Some(catalog) = layer_catalog(layer, code) else {
            continue;
        };
        for (id, message) in catalog.messages() {
            for template in message.templates() {
                out.push((format!("{layer}:{id}"), template.to_owned()));
            }
        }
    }
    assert!(!out.is_empty(), "no catalog found for {code}");
    out
}

/// The `layer:id`s of `code` whose text matches `hit`, each once.
fn offending_ids(code: &str, hit: impl Fn(&str, &str) -> bool) -> BTreeSet<String> {
    texts(code)
        .into_iter()
        .filter(|(id, text)| hit(id, text))
        .map(|(id, _)| id)
        .collect()
}

/// `text` with every `{placeholder}` replaced by `with`.
fn without_placeholders(text: &str, with: &str) -> String {
    Regex::new(r"\{[A-Za-z0-9_]*\}")
        .unwrap()
        .replace_all(text, with)
        .into_owned()
}

// G10 ---------------------------------------------------------------------

// G10
/// V35-02: German uses "Sie" (README register table), never du/dich/dir/dein….
/// The possessive is spelled out (`dein`, `deine`, `deinen`, …) so that
/// words such as "deinstallieren" are not taken for it.
#[test]
fn de_uses_the_sie_form() {
    let du = Regex::new(r"(?i)\b(du|dich|dir|dein(e[nmrs]?)?)\b").unwrap();
    let bad = offending_ids("de", |_, text| {
        du.is_match(&without_placeholders(text, " "))
    });
    assert!(bad.is_empty(), "de texts use du instead of Sie: {bad:#?}");
}

// G10
/// V35-03: Simplified Chinese drops the pronoun; 您 only in error, consent,
/// privacy and warning texts, never 你.
#[test]
fn zh_cn_never_uses_ni() {
    let bad = offending_ids("zh-CN", |_, text| text.contains('你'));
    assert!(bad.is_empty(), "zh-CN texts use 你: {bad:#?}");
}

// G10
/// V35-04: Japanese never writes あなた (README: use 現在のユーザー for
/// "your user").
#[test]
fn ja_never_uses_anata() {
    let bad = offending_ids("ja", |_, text| text.contains("あなた"));
    assert!(bad.is_empty(), "ja texts use あなた: {bad:#?}");
}

// G10
/// V35-05: Korean is written in 합쇼체 (confirmation titles end in
/// …하시겠습니까?), never a 해요체 sentence end. The polite imperative -세요
/// is allowed by the README.
#[test]
fn ko_has_no_haeyo_sentence_ends() {
    let haeyo = Regex::new(r#"(까요|해요|어요|아요|네요|죠)[.?!]*(\s|$|["'”’)\]])"#).unwrap();
    let bad = offending_ids("ko", |_, text| haeyo.is_match(text));
    assert!(
        bad.is_empty(),
        "ko texts end a sentence in 해요체: {bad:#?}"
    );
}
