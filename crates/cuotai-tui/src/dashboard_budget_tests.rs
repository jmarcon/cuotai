//! PRD 0041 re-validation F-12: the card line marks the person's budget.

use super::*;
use chrono::TimeZone;

fn budget_metric(derived: bool) -> Metric {
    let mut value = serde_json::json!({
        "label": "Team cap",
        "used": 24.75,
        "limit": 50.0,
        "remaining": 25.25,
        "unit": "USD",
        "kind": "quota",
        "window": "monthly",
        "period": null,
        "resets_at": null,
    });
    if derived {
        value["derived_from"] =
            serde_json::json!({ "kind": "spend", "label": "Spend", "budget": 0 });
    }
    serde_json::from_value(value).expect("a metric")
}

fn line_text(metric: &Metric, width: u16) -> String {
    let checked = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let row = metric_row(metric, checked, None, &[]);
    Line::from(metric_values_line(&row, width)).to_string()
}

#[test]
fn a_budget_carries_the_mark_on_the_card_line() {
    let line = line_text(&budget_metric(true), 80);
    assert!(line.starts_with("◇ Team cap: "), "{line}");
}

#[test]
fn a_provider_limit_has_no_budget_mark() {
    let line = line_text(&budget_metric(false), 80);
    assert!(line.starts_with("Team cap: "), "{line}");
    assert!(!line.contains('◇'), "{line}");
}

#[test]
fn a_narrow_card_keeps_the_mark_while_the_label_gives_way() {
    let full = line_text(&budget_metric(true), 80);
    let value = full.trim_start_matches("◇ Team cap: ");
    let narrow = line_text(&budget_metric(true), (value.chars().count() + 3) as u16);
    assert!(narrow.starts_with("◇ "), "{narrow}");
    assert!(!narrow.contains("Team cap"), "{narrow}");
}
