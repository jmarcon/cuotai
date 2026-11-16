//! PRD 0034 re-validation V-15/V-16: the languages `--fallback-locale`
//! accepts are listed once, by the core's locale registry. The `--help` and
//! every document that lists them must carry that same list, and the event
//! example in `docs/events.md` must carry the display text the code emits.

use std::path::PathBuf;
use std::process::Command;

/// Every language with a core catalog except the English source, in
/// registry order.
fn catalog_codes() -> Vec<&'static str> {
    let source = cuotai_core::text::source_locale().code;
    cuotai_core::text::catalog_locales()
        .into_iter()
        .filter(|code| *code != source)
        .collect()
}

/// `a, b or c`, each item through `item`.
fn or_list(codes: &[&str], item: impl Fn(&str) -> String) -> String {
    let items: Vec<String> = codes.iter().map(|code| item(code)).collect();
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

fn doc(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn the_registry_has_the_shipped_catalogs() {
    let codes = catalog_codes();
    assert!(codes.len() >= 13, "{codes:?}");
    assert!(codes.contains(&"pt-BR"), "{codes:?}");
}

#[test]
fn the_help_lists_every_fallback_language() {
    let output = Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .arg("--help")
        .env("CUOTAI_LANG", "en-US")
        .output()
        .expect("run cuotai");
    let help = String::from_utf8(output.stdout).expect("UTF-8");
    let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
    let expected = format!("({})", or_list(&catalog_codes(), str::to_owned));
    assert!(flat.contains(&expected), "missing {expected}\n{flat}");
}

/// A document lists the languages as `` `pt-BR`, `es`, … or `vi` ``; add a
/// document here when it starts listing them.
#[test]
fn every_doc_lists_the_registry_fallback_languages() {
    let expected = or_list(&catalog_codes(), |code| format!("`{code}`"));
    for name in ["tui.md", "advanced.md"] {
        let text = doc(name);
        assert!(
            text.contains(&expected),
            "docs/{name} must list the fallback languages as {expected}"
        );
    }
}

/// The `metric` of the example payload in `docs/events.md` is what the
/// code serializes for that label.
#[test]
fn the_events_example_carries_the_label_text() {
    let metric = cuotai_core::events::MetricRef::new(
        "Sessão (5 horas)",
        Some(cuotai_core::MetricWindow::Window5h),
    );
    let json = serde_json::to_string(&metric).unwrap();
    let text = doc("events.md");
    assert!(
        text.contains(&format!("\"metric\":{json}")),
        "docs/events.md example must carry {json}"
    );
    let percent = serde_json::to_string(&cuotai_core::metric_text::metric_text("%")).unwrap();
    assert!(
        text.contains(&format!("\"unit_text\":{percent}")),
        "docs/events.md example must carry {percent}"
    );
}
