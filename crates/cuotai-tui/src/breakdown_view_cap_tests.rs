//! PRD 0045 RF6.2: a row near or at its own cap takes the dashboard's quota
//! colour in the details.

use super::*;
use crate::palette;
use cuotai_core::{Metric, MetricKind, ScopeKind};

fn row(label: &str, used: f64, limit: Option<f64>) -> BreakdownRow {
    let mut metric = Metric::usage("Budget", used, "USD", MetricKind::Quota, None);
    metric.limit = limit;
    metric.remaining = limit.map(|cap| (cap - used).max(0.0));
    BreakdownRow {
        scope: ScopeKind::Key,
        id: label.to_lowercase(),
        label: label.into(),
        metrics: vec![metric],
        truncated: false,
        notes: Vec::new(),
    }
}

fn styles(rows: Vec<BreakdownRow>) -> Vec<(String, Style)> {
    let mut report: Report = serde_json::from_value(serde_json::json!({
        "account": "org", "provider": "litellm", "checked_at": "2026-10-20T12:00:00Z",
        "status": "ready", "metrics": [], "notes": [], "source": ""
    }))
    .unwrap();
    report.breakdown = rows;
    crate::l10n::with_locale("en-US", || detail_lines(&report, "en-US"))
        .into_iter()
        .skip(2)
        .map(|line| (line.to_string(), line.spans[0].style))
        .collect()
}

#[test]
fn a_capped_row_takes_the_quota_colour_only_near_or_at_the_cap() {
    let lines = styles(vec![
        row("Full", 10.0, Some(10.0)),
        row("Near", 8.0, Some(10.0)),
        row("Half", 5.0, Some(10.0)),
        row("Room", 1.0, Some(10.0)),
        row("Open", 0.5, None),
    ]);
    let style_of = |name: &str| {
        lines
            .iter()
            .find(|(text, _)| text.contains(name))
            .map(|(_, style)| *style)
            .expect("row listed")
    };
    assert_eq!(style_of("Full").fg, Some(palette::BAD));
    assert_eq!(style_of("Near").fg, Some(palette::BAD));
    assert_eq!(style_of("Half").fg, Some(palette::WARN));
    assert_eq!(style_of("Room").fg, None);
    assert_eq!(style_of("Open").fg, None);
}
