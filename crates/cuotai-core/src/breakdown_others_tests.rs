//! PRD 0045 re-validation (V-01, R-01, S-01): the "Others" row sums one
//! total per unit, each kind of row is capped and folded on its own, and a
//! provider id that reads "others" is not the sum row.

use super::*;
use crate::MetricWindow;
use chrono::{TimeZone, Utc};

fn spend_row(scope: ScopeKind, id: &str, used: f64) -> BreakdownRow {
    BreakdownRow {
        scope,
        id: id.into(),
        label: format!("Row-{id}"),
        metrics: vec![Metric::usage(
            "Gasto no mês",
            used,
            "USD",
            MetricKind::Spend,
            Some(MetricWindow::Monthly),
        )],
        truncated: false,
        notes: Vec::new(),
    }
}

fn token_row(id: &str, used: f64) -> BreakdownRow {
    BreakdownRow {
        metrics: vec![Metric::usage(
            "Tokens no mês",
            used,
            "tokens",
            MetricKind::Usage,
            Some(MetricWindow::Monthly),
        )],
        ..spend_row(ScopeKind::Key, id, 0.0)
    }
}

fn others(rows: &[BreakdownRow]) -> Vec<&BreakdownRow> {
    rows.iter().filter(|row| row.is_others()).collect()
}

#[test]
fn others_sums_one_total_per_unit() {
    let reset = Utc.with_ymd_and_hms(2026, 12, 2, 0, 0, 0).unwrap();
    let mut rows: Vec<_> = (0..24)
        .map(|i| spend_row(ScopeKind::Key, &format!("k{i:02}"), 100.0 + f64::from(i)))
        .collect();
    rows.push(BreakdownRow {
        metrics: vec![Metric {
            limit: Some(10.0),
            remaining: Some(6.0),
            resets_at: Some(reset),
            ..Metric::usage(
                "Orçamento da chave",
                4.0,
                "USD",
                MetricKind::Quota,
                Some(MetricWindow::Monthly),
            )
        }],
        ..spend_row(ScopeKind::Key, "capped", 0.0)
    });
    // The plain spend rows report no window: only the capped key has one.
    rows.extend((0..7).map(|i| {
        let mut row = spend_row(ScopeKind::Key, &format!("s{i}"), 2.0 + f64::from(i));
        row.metrics[0].window = None;
        row
    }));
    let done = finish(rows);
    assert_eq!(done.len(), MAX_ROWS);
    let sum = others(&done);
    assert_eq!(sum.len(), 1);
    let sum = sum[0];
    assert_eq!(sum.label, "Others (8)");
    assert_eq!(sum.metrics.len(), 1);
    let total = &sum.metrics[0];
    assert_eq!(total.kind, MetricKind::Spend);
    assert_eq!(total.label, "Gasto total");
    // 4 + (2 + 3 + … + 8)
    assert_eq!(total.used, Some(39.0));
    assert!(total.resets_at.is_none() && total.window.is_none());
    assert!(total.limit.is_none() && total.remaining.is_none());
    assert!(sum.capped().is_none());
    assert_eq!(sum.primary().and_then(|m| m.used), Some(39.0));
}

#[test]
fn others_keeps_a_reset_every_folded_row_shares() {
    let reset = Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap();
    let rows: Vec<_> = (0..30)
        .map(|i| {
            let mut row = spend_row(ScopeKind::Key, &format!("k{i:02}"), f64::from(i));
            row.metrics[0].resets_at = Some(reset);
            row
        })
        .collect();
    let done = finish(rows);
    let total = &others(&done)[0].metrics[0];
    assert_eq!(total.resets_at, Some(reset));
    assert_eq!(total.window, Some(MetricWindow::Monthly));
    // 0 + 1 + … + 5
    assert_eq!(total.used, Some(15.0));
}

#[test]
fn each_kind_is_capped_and_folded_on_its_own() {
    let mut rows: Vec<_> = (0..30)
        .map(|i| token_row(&format!("k{i:02}"), 50_000.0 + f64::from(i)))
        .collect();
    rows.extend((0..30).map(|i| {
        spend_row(
            ScopeKind::Project,
            &format!("p{i:02}"),
            900.0 + f64::from(i),
        )
    }));
    let done = finish(rows);
    let keys: Vec<_> = done.iter().filter(|r| r.scope == ScopeKind::Key).collect();
    let projects: Vec<_> = done
        .iter()
        .filter(|r| r.scope == ScopeKind::Project)
        .collect();
    assert_eq!(keys.len(), 12);
    assert_eq!(projects.len(), 12);
    let sums = others(&done);
    assert_eq!(sums.len(), 2);
    assert_eq!(sums[0].scope, ScopeKind::Key);
    assert_eq!(sums[1].scope, ScopeKind::Project);
    // No cross-unit sum: the key "Others" holds tokens only, the project
    // one money only.
    assert!(sums[0].metrics.iter().all(|m| m.unit == "tokens"));
    assert!(sums[1].metrics.iter().all(|m| m.unit == "USD"));
    // `sorted` keeps the kinds apart, each "Others" last in its kind.
    let order: Vec<_> = sorted(&done, SortBy::Spend);
    assert!(order[..12].iter().all(|r| r.scope == ScopeKind::Key));
    assert!(order[11].is_others() && order[23].is_others());
    assert_eq!(order[12].id, "p29");
    // The card line names projects, in money.
    let top = top_line(&done, CARD_ROWS).unwrap();
    let top = top.text();
    assert!(top.contains("Row-p29") && top.contains("Row-p28"), "{top}");
    assert!(!top.contains("Row-k"), "{top}");
    assert_eq!(
        truncated_note(&done).unwrap().text(),
        "Showing the top 22 of 60; the rest are summed in Others."
    );
    // The history shares its rows between the kinds.
    let history: Vec<_> = top_rows(&done).collect();
    assert_eq!(history.len(), HISTORY_ROWS);
    assert_eq!(
        history
            .iter()
            .filter(|r| r.scope == ScopeKind::Project)
            .count(),
        HISTORY_ROWS / 2
    );
}

#[test]
fn a_provider_id_named_others_is_not_the_sum_row() {
    let mut forged = spend_row(ScopeKind::Team, "others", 500.0);
    forged.label = "Others (999)".into();
    let upper = spend_row(ScopeKind::Team, "OTHERS", 1.0);
    let done = finish(vec![
        spend_row(ScopeKind::Team, "t1", 10.0),
        forged,
        spend_row(ScopeKind::Team, "t2", 5.0),
        upper,
    ]);
    assert_eq!(done[0].primary().and_then(|m| m.used), Some(500.0));
    assert_eq!(done[0].id, digest_id("others"));
    assert_eq!(done[0].id.len(), 16);
    assert!(done.iter().all(|row| !row.is_others()));
    assert!(truncated_note(&done).is_none());
    assert_eq!(top_rows(&done).count(), 4);
    let top = top_line(&done, CARD_ROWS).unwrap();
    let top = top.text();
    assert!(top.contains("Others (999)"), "{top}");
}
