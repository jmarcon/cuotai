//! PRD 0041 re-validation (D2): a budget's series is keyed by the budget's
//! identity, a legacy series moves to it once, and the limit changes skip
//! budget series by key rather than by label.

use super::tests::{store, t0};
use super::*;
use crate::budget::{budget_key, Budget, BudgetKind, BudgetPeriod};
use crate::model::{Billing, DerivedFrom, Provider};
use chrono::TimeDelta;

fn account(budgets: Vec<Budget>) -> Account {
    Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        budgets,
        ..Default::default()
    }
}

fn budget(period: BudgetPeriod, label: Option<&str>) -> Budget {
    Budget {
        kind: Some(BudgetKind::Spend),
        scope: None,
        label_contains: None,
        amount: 50.0,
        unit: Some("USD".into()),
        period,
        starts: None,
        label: label.map(Into::into),
        row: None,
    }
}

fn quota(label: &str, used: f64, limit: f64) -> Metric {
    Metric {
        label: label.into(),
        used: Some(used),
        limit: Some(limit),
        remaining: Some(limit - used),
        unit: "USD".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Monthly),
        period: None,
        resets_at: None,
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    }
}

/// The budget metric of `budget` (position `index`) as the core derives
/// it: `legacy` gives the label and empty key an older release recorded.
fn derived(index: usize, budget: &Budget, used: f64, legacy: bool) -> Metric {
    let label = if legacy {
        budget
            .label
            .clone()
            .unwrap_or_else(|| "Orçamento · Gasto no mês".into())
    } else {
        crate::budget::metric_label(budget, "Gasto no mês")
    };
    Metric {
        derived_from: Some(DerivedFrom {
            kind: MetricKind::Spend,
            label: "Gasto no mês".into(),
            budget: index,
            partial: false,
            key: if legacy {
                String::new()
            } else {
                budget_key(budget)
            },
        }),
        ..quota(&label, used, budget.amount)
    }
}

#[test]
fn a_budget_series_is_keyed_by_the_budget_and_survives_a_rename() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir.path().join("history.sqlite3"), "budgets");
    let monthly = budget(BudgetPeriod::Monthly, None);
    let work = account(vec![monthly.clone()]);
    let metric = derived(0, &monthly, 10.0, false);
    let renamed = Metric {
        label: "Team".into(),
        limit: Some(80.0),
        ..derived(0, &monthly, 12.0, false)
    };
    assert_eq!(metric_key(&metric), metric_key(&renamed));
    history
        .record(&work, t0(), std::slice::from_ref(&metric))
        .unwrap();
    history
        .record(&work, t0() + TimeDelta::minutes(20), &[renamed])
        .unwrap();
    assert_eq!(
        history
            .series(&work, &metric_key(&metric), ..)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn a_legacy_budget_series_moves_to_the_budget_key_once() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir.path().join("history.sqlite3"), "budgets");
    let monthly = budget(BudgetPeriod::Monthly, None);
    let work = account(vec![monthly.clone()]);
    let old = derived(0, &monthly, 10.0, true);
    history
        .record(&work, t0(), std::slice::from_ref(&old))
        .unwrap();
    let new = derived(0, &monthly, 12.0, false);
    assert_ne!(metric_key(&old), metric_key(&new));
    history
        .record(
            &work,
            t0() + TimeDelta::minutes(20),
            std::slice::from_ref(&new),
        )
        .unwrap();
    let moved = history.series(&work, &metric_key(&new), ..).unwrap();
    assert_eq!(
        moved.iter().map(|point| point.used).collect::<Vec<_>>(),
        [10.0, 12.0]
    );
    assert!(history
        .series(&work, &metric_key(&old), ..)
        .unwrap()
        .is_empty());
}

#[test]
fn a_legacy_series_two_budgets_shared_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir.path().join("history.sqlite3"), "budgets");
    let (monthly, weekly) = (
        budget(BudgetPeriod::Monthly, None),
        budget(BudgetPeriod::Weekly, None),
    );
    let work = account(vec![monthly.clone(), weekly.clone()]);
    let old = derived(0, &monthly, 10.0, true);
    history
        .record(&work, t0(), std::slice::from_ref(&old))
        .unwrap();
    let news = [
        derived(0, &monthly, 12.0, false),
        derived(1, &weekly, 3.0, false),
    ];
    history
        .record(&work, t0() + TimeDelta::minutes(20), &news)
        .unwrap();
    for new in &news {
        assert_eq!(
            history.series(&work, &metric_key(new), ..).unwrap().len(),
            1
        );
    }
    assert_eq!(
        history.series(&work, &metric_key(&old), ..).unwrap().len(),
        1
    );
}

#[test]
fn limit_changes_skip_budget_series_by_key_not_by_label() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir.path().join("history.sqlite3"), "budgets");
    // A budget whose own label is the provider metric's label.
    let mut named = budget(BudgetPeriod::Monthly, Some("Semanal"));
    let work = account(vec![named.clone()]);
    history
        .record(
            &work,
            t0(),
            &[quota("Semanal", 1.0, 100.0), derived(0, &named, 1.0, false)],
        )
        .unwrap();
    named.amount = 80.0;
    history
        .record(
            &work,
            t0() + TimeDelta::minutes(20),
            &[quota("Semanal", 1.0, 150.0), derived(0, &named, 1.0, false)],
        )
        .unwrap();
    let changes = history.limit_changes(&work, ..).unwrap();
    assert_eq!(
        changes
            .iter()
            .map(|change| (change.from, change.to))
            .collect::<Vec<_>>(),
        [(100.0, 150.0)],
        "the provider's change is told, the budget's edit is not"
    );
}
