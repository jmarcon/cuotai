//! PRD 0045 RF6.2: an account's breakdown rows (keys, projects, workspaces,
//! teams) in the TUI details and under the account in `--once`. The rows,
//! their order, caps and labels come from the core
//! (`cuotai_core::breakdown`); this module only lays them out.

use crate::l10n::{tr, trf_in};
use crate::palette;
use crate::ui::{metric_label_text, metric_text, resolve_l10n, safe};
use cuotai_core::breakdown::{self, BreakdownRow, SortBy};
use cuotai_core::Report;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// The text of one row: its label, then each metric as "label: value".
fn row_text(row: &BreakdownRow, locale: &str) -> String {
    let metrics: Vec<String> = row
        .metrics
        .iter()
        .map(|metric| {
            format!(
                "{}: {}",
                safe(&metric_label_text(&metric.label)),
                metric_text(metric)
            )
        })
        .collect();
    let label = row.label_text().resolve(locale);
    if metrics.is_empty() {
        safe(&label).into_owned()
    } else {
        format!("{} — {}", safe(&label), metrics.join(" · "))
    }
}

/// `--once`: up to [`breakdown::ONCE_ROWS`] rows in spend order under the
/// account, then how many more the details list. Empty without rows.
pub(crate) fn once_text(report: &Report, locale: &str) -> String {
    if report.breakdown.is_empty() {
        return String::new();
    }
    let rows = breakdown::sorted(&report.breakdown, SortBy::Spend);
    let mut out = crate::l10n::tr_in(locale, "once.breakdown", "  Breakdown:").to_string();
    out.push('\n');
    for row in rows.iter().take(breakdown::ONCE_ROWS) {
        out += &format!("    • {}\n", row_text(row, locale));
    }
    let more = rows.len().saturating_sub(breakdown::ONCE_ROWS);
    if more > 0 {
        out += &trf_in(
            locale,
            "once.breakdown.more",
            "    … {count} more in the details",
            &[("count", &more)],
        );
        out.push('\n');
    }
    out
}

/// The details' BREAKDOWN section: every row in spend order (row notes
/// beside it), then the truncation note when rows were summed. Empty
/// without rows.
pub(crate) fn detail_lines(report: &Report, locale: &str) -> Vec<Line<'static>> {
    if report.breakdown.is_empty() {
        return Vec::new();
    }
    let heading = Style::default()
        .fg(palette::INFO)
        .add_modifier(Modifier::BOLD);
    let mut lines = vec![
        Line::default(),
        Line::styled(tr("dashboard.detail.breakdown", "  BREAKDOWN"), heading),
    ];
    for row in breakdown::sorted(&report.breakdown, SortBy::Spend) {
        let mut spans = vec![Span::raw(format!("  {}", row_text(row, locale)))];
        for note in &row.notes {
            spans.push(Span::styled(
                format!("  ({})", safe(&resolve_l10n(note))),
                Style::default().fg(palette::WARN),
            ));
        }
        lines.push(Line::from(spans));
    }
    if let Some(note) = breakdown::truncated_note(&report.breakdown) {
        lines.push(Line::styled(
            format!("  {}", safe(&resolve_l10n(&note))),
            Style::default().fg(palette::MUTED),
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::{Metric, MetricKind, Provider, ScopeKind, Status};

    fn report(rows: usize) -> Report {
        let breakdown = (0..rows)
            .map(|n| BreakdownRow {
                scope: ScopeKind::Project,
                id: format!("p{n}"),
                label: format!("Proj-{n}"),
                metrics: vec![Metric::usage(
                    "Custo no mês corrente (USD)",
                    n as f64,
                    "USD",
                    MetricKind::Spend,
                    None,
                )],
                truncated: false,
                notes: Vec::new(),
            })
            .collect();
        let mut report: Report = serde_json::from_value(serde_json::json!({
            "account": "org", "provider": "open_ai_api", "checked_at": "2026-10-20T12:00:00Z",
            "status": "ready", "metrics": [], "notes": [], "source": ""
        }))
        .unwrap();
        report.breakdown = breakdown;
        assert_eq!(
            (report.provider, report.status),
            (Provider::OpenAiApi, Status::Ready)
        );
        report
    }

    #[test]
    fn once_prints_five_rows_and_counts_the_rest() {
        assert_eq!(once_text(&report(0), "en-US"), "");
        let text = once_text(&report(8), "en-US");
        assert!(text.starts_with("  Breakdown:\n    • Proj-7 — "), "{text}");
        assert_eq!(text.matches("    • ").count(), breakdown::ONCE_ROWS);
        assert!(text.contains("… 3 more in the details"), "{text}");
        assert!(!text.contains("Proj-0"));
        let few = once_text(&report(3), "en-US");
        assert!(!few.contains("more in the details"));
    }

    #[test]
    fn details_list_every_row_and_the_others_note() {
        assert!(detail_lines(&report(0), "en-US").is_empty());
        let mut capped = report(26);
        capped.breakdown = breakdown_finish(capped.breakdown);
        let lines = crate::l10n::with_locale("en-US", || detail_lines(&capped, "en-US"));
        let text: Vec<String> = lines.iter().map(ToString::to_string).collect();
        assert!(text.iter().any(|line| line.contains("BREAKDOWN")));
        assert!(text.iter().any(|line| line.contains("Others (2)")));
        assert!(text
            .iter()
            .any(|line| line.contains("Showing the top 24 of 26")));
    }

    /// The core's cap, through its public serialization.
    fn breakdown_finish(rows: Vec<BreakdownRow>) -> Vec<BreakdownRow> {
        let mut sorted = rows;
        sorted.sort_by(|a, b| {
            b.metrics[0]
                .used
                .unwrap_or_default()
                .total_cmp(&a.metrics[0].used.unwrap_or_default())
        });
        let folded = sorted.split_off(breakdown::MAX_ROWS - 1);
        sorted.push(BreakdownRow {
            scope: ScopeKind::Project,
            id: breakdown::OTHERS_ID.into(),
            label: format!("Others ({})", folded.len()),
            metrics: Vec::new(),
            truncated: false,
            notes: Vec::new(),
        });
        sorted
    }
}
