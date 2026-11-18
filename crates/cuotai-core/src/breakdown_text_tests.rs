//! PRD 0045 re-validation (V-02, V-04): generated row names and the card's
//! "Top" line read in the reader's language and number format.

use super::*;

fn named(scope: ScopeKind, label: &str, used: f64) -> BreakdownRow {
    BreakdownRow {
        scope,
        id: label.to_ascii_lowercase(),
        label: label.into(),
        metrics: vec![Metric::usage(
            "Gasto no mês",
            used,
            "USD",
            MetricKind::Spend,
            None,
        )],
        truncated: false,
        notes: Vec::new(),
    }
}

#[test]
fn generated_labels_are_translated_and_provider_names_are_not() {
    let virtual_key = named(ScopeKind::VirtualKey, "Virtual key …5553", 1.0);
    let text = virtual_key.label_text();
    assert_eq!(text.id, "breakdown.fallback");
    assert_eq!(text.text(), "Virtual key …5553");
    assert_eq!(text.resolve("de"), "Virtueller Schlüssel …5553");
    let bare = named(ScopeKind::Key, "Key", 1.0).label_text();
    assert_eq!(bare.id, "breakdown.noun.key");
    assert_eq!(bare.resolve("de"), "Schlüssel");
    // A provider name stays verbatim, in every language.
    assert_eq!(
        named(ScopeKind::Project, "Proj-A", 1.0)
            .label_text()
            .resolve("de"),
        "Proj-A"
    );
    // Only the row's own scope noun, and only a short alphanumeric tail.
    for (scope, label) in [
        (ScopeKind::Project, "Key …ab12"),
        (ScopeKind::Key, "Key …abcde"),
        (ScopeKind::Key, "Key …a-b"),
        (ScopeKind::Key, "Keys"),
    ] {
        assert_eq!(named(scope, label, 1.0).label_text().resolve("de"), label);
    }
    // The stored label stays English.
    assert_eq!(
        fallback_label(ScopeKind::VirtualKey, "x5553"),
        "Virtual key …5553"
    );
}

#[test]
fn the_top_line_follows_the_number_format() {
    let rows = vec![
        named(ScopeKind::Project, "Proj-A", 33.333),
        named(ScopeKind::Project, "Project …ab12", 2.5),
    ];
    let top = top_line(&rows, CARD_ROWS).unwrap();
    assert_eq!(top.text(), "Top: Proj-A $33.33 · Project …ab12 $2.50");
    let de = top.resolve("de");
    assert!(de.contains("33,33\u{a0}$"), "{de}");
    assert!(de.contains("Projekt …ab12"), "{de}");
    assert!(de.starts_with("Größte: "), "{de}");
    // One row: no pair.
    let one = top_line(&rows[..1], CARD_ROWS).unwrap();
    assert!(
        matches!(&one.params["items"], ParamValue::L10n(item) if item.id == "breakdown.top_item")
    );
    assert_eq!(one.text(), "Top: Proj-A $33.33");
}
