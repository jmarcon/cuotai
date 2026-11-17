//! PRD 0041 re-validation (D2, F-02, F-26): every budget and floor has an
//! identity of its own, derived from what it follows, and a default label
//! that names its period.

use super::*;
use crate::model::Status;
use chrono::TimeZone;

fn at(d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, d, h, 0, 0).unwrap()
}

fn spend(used: f64) -> Metric {
    Metric {
        label: "Gasto no mês".into(),
        used: Some(used),
        limit: None,
        remaining: None,
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: Some(MetricWindow::Monthly),
        period: None,
        resets_at: Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap()),
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    }
}

fn report(metrics: Vec<Metric>, checked_at: DateTime<Utc>) -> Report {
    Report {
        account: "Work".into(),
        account_id: None,
        provider: Provider::OpenAiApi,
        checked_at,
        status: Status::Ready,
        metrics,
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        diagnostic: None,
        sub_reports: Vec::new(),
        as_of: None,
        overage_hint: None,
        breakdown: Vec::new(),
    }
}

fn budget(period: BudgetPeriod) -> Budget {
    Budget {
        kind: Some(BudgetKind::Spend),
        scope: None,
        label_contains: None,
        amount: 50.0,
        unit: Some("USD".into()),
        period,
        starts: None,
        label: None,
        row: None,
    }
}

fn floor(amount: f64) -> Floor {
    Floor {
        kind: Some(FloorKind::Credits),
        label_contains: None,
        amount,
        unit: None,
    }
}

fn account(budgets: Vec<Budget>) -> Account {
    Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        budgets,
        ..Default::default()
    }
}

fn rules(budgets: &[Budget], floors: &[Floor]) -> Vec<(String, BudgetRule)> {
    problems(Provider::OpenAiApi, budgets, floors)
        .into_iter()
        .map(|problem| (problem.field, problem.rule))
        .collect()
}

#[test]
fn the_key_follows_the_target_and_period_only() {
    let base = budget(BudgetPeriod::Monthly);
    let key = budget_key(&base);
    assert!(key.starts_with('b') && key.len() == 13, "{key}");
    let same = [
        Budget {
            amount: 80.0,
            ..base.clone()
        },
        Budget {
            label: Some("Team".into()),
            ..base.clone()
        },
        Budget {
            unit: None,
            ..base.clone()
        },
        Budget {
            starts: Some(BudgetStart::Day(15)),
            ..base.clone()
        },
        Budget {
            label_contains: Some("Bedrock".into()),
            ..base.clone()
        },
    ];
    for (i, edited) in same.iter().enumerate().take(4) {
        assert_eq!(budget_key(edited), key, "edit {i} keeps the identity");
    }
    let mut other_filter = same[4].clone();
    assert_ne!(budget_key(&other_filter), key);
    other_filter.label_contains = Some(" bedrock ".into());
    assert_eq!(
        budget_key(&other_filter),
        budget_key(&same[4]),
        "the filter ignores case and spaces"
    );
    let different = [
        budget(BudgetPeriod::Weekly),
        Budget {
            kind: Some(BudgetKind::Usage),
            ..base.clone()
        },
        Budget {
            kind: None,
            scope: Some(BudgetScope::Account),
            ..base.clone()
        },
        Budget {
            row: Some(RowTarget {
                scope: ScopeKind::Key,
                id: "k1".into(),
            }),
            ..base.clone()
        },
    ];
    for edited in &different {
        assert_ne!(budget_key(edited), key, "{edited:?}");
    }
}

#[test]
fn a_duplicate_is_the_same_key_whatever_the_unit() {
    let with_unit = budget(BudgetPeriod::Monthly);
    let without = Budget {
        unit: None,
        ..with_unit.clone()
    };
    assert_eq!(
        rules(&[with_unit.clone(), without], &[]),
        [("budgets[1].period".to_owned(), BudgetRule::Duplicate)]
    );
    // A weekly and a monthly budget on one spend are two budgets.
    assert!(rules(&[with_unit, budget(BudgetPeriod::Weekly)], &[]).is_empty());
}

#[test]
fn two_budgets_of_an_account_never_share_a_label() {
    let named = |period, label: &str| Budget {
        label: Some(label.into()),
        ..budget(period)
    };
    assert_eq!(
        rules(
            &[
                named(BudgetPeriod::Monthly, "Team"),
                named(BudgetPeriod::Weekly, " team "),
            ],
            &[]
        ),
        [("budgets[1].label".to_owned(), BudgetRule::DuplicateLabel)]
    );
    assert!(rules(
        &[
            named(BudgetPeriod::Monthly, "Team"),
            named(BudgetPeriod::Weekly, "Team weekly"),
        ],
        &[]
    )
    .is_empty());
    let note = BudgetRule::DuplicateLabel.note("budgets[1].label");
    assert_eq!(note.id, "budget.error.duplicate_label");
}

#[test]
fn two_floors_at_the_same_amount_on_one_balance_are_refused() {
    assert_eq!(
        rules(&[], &[floor(10.0), floor(10.0)]),
        [("floors[1].amount".to_owned(), BudgetRule::DuplicateFloor)]
    );
    assert!(rules(&[], &[floor(10.0), floor(5.0)]).is_empty());
    let key = floor_key(&floor(10.0));
    assert!(key.starts_with('f') && key.len() == 13, "{key}");
    assert_ne!(key, floor_key(&floor(5.0)), "the amount is part of it");
    assert_eq!(
        floor_key(&Floor {
            unit: Some("usd".into()),
            ..floor(10.0)
        }),
        floor_key(&Floor {
            unit: Some("USD".into()),
            ..floor(10.0)
        })
    );
    assert_eq!(
        BudgetRule::DuplicateFloor.note("floors[1].amount").id,
        "budget.error.duplicate_floor"
    );
}

#[test]
fn default_labels_name_the_period() {
    let target = "Gasto no mês";
    let cases = [
        (
            budget(BudgetPeriod::Daily),
            "Orçamento diário · Gasto no mês",
            "Daily budget · Spending in the month",
        ),
        (
            budget(BudgetPeriod::Weekly),
            "Orçamento semanal · Gasto no mês",
            "Weekly budget · Spending in the month",
        ),
        (
            budget(BudgetPeriod::Monthly),
            "Orçamento mensal · Gasto no mês",
            "Monthly budget · Spending in the month",
        ),
        (
            Budget {
                starts: Some(BudgetStart::Day(1)),
                ..budget(BudgetPeriod::Monthly)
            },
            "Orçamento mensal · Gasto no mês",
            "Monthly budget · Spending in the month",
        ),
        (
            Budget {
                starts: Some(BudgetStart::Day(15)),
                ..budget(BudgetPeriod::Monthly)
            },
            "Orçamento mensal desde o dia 15 · Gasto no mês",
            "Monthly budget from day 15 · Spending in the month",
        ),
        (
            budget(BudgetPeriod::Window),
            "Orçamento do ciclo · Gasto no mês",
            "Cycle budget · Spending in the month",
        ),
    ];
    for (budget, raw, english) in cases {
        let label = metric_label(&budget, target);
        assert_eq!(label, raw);
        assert_eq!(crate::metric_text::metric_text(&label).fallback, english);
        assert!(is_default_label(&label));
    }
    // The plain form stays readable for older readings.
    assert_eq!(
        crate::metric_text::metric_text("Orçamento · Gasto no mês").fallback,
        "Budget · Spending in the month"
    );
    assert!(is_default_label("Orçamento · Gasto no mês"));
    assert!(!is_default_label("Orçamento (outro)"));
    assert!(!is_default_label("Orçamento mensal desde o dia x · Gasto"));
    // An own label is kept as written.
    let own = Budget {
        label: Some("Team".into()),
        ..budget(BudgetPeriod::Weekly)
    };
    assert_eq!(metric_label(&own, target), "Team");
}

#[test]
fn a_weekly_and_a_monthly_budget_on_one_spend_read_apart() {
    let now = at(21, 12);
    let account = account(vec![
        budget(BudgetPeriod::Monthly),
        budget(BudgetPeriod::Weekly),
    ]);
    let mut reading = report(vec![spend(40.0)], now);
    let history = |_: &Metric, _: DateTime<Utc>| {
        vec![Point {
            taken_at: at(19, 1),
            used: 30.0,
        }]
    };
    apply(&account, &mut reading, &history, now, &Utc);
    let derived: Vec<&Metric> = reading
        .metrics
        .iter()
        .filter(|metric| metric.derived_from.is_some())
        .collect();
    assert_eq!(derived.len(), 2);
    assert_ne!(derived[0].label, derived[1].label);
    let keys: Vec<&str> = derived
        .iter()
        .map(|metric| metric.derived_from.as_ref().unwrap().key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            budget_key(&account.budgets[0]),
            budget_key(&account.budgets[1])
        ]
    );
    assert_ne!(
        crate::history::metric_key(derived[0]),
        crate::history::metric_key(derived[1])
    );
    // The export carries the key (additive).
    let json = serde_json::to_value(derived[1]).unwrap();
    assert_eq!(json["derived_from"]["key"], keys[1]);
    assert_eq!(json["derived_from"]["budget"], 1);
    // A legacy label maps back to the plain form.
    assert_eq!(legacy_label(derived[1]), "Orçamento · Gasto no mês");
}

#[test]
fn a_restored_reading_marks_current_and_legacy_labels_and_drops_removed_budgets() {
    let now = at(21, 12);
    let monthly = account(vec![budget(BudgetPeriod::Monthly)]);
    let quota = |label: &str| Metric {
        label: label.into(),
        used: Some(40.0),
        limit: Some(50.0),
        remaining: Some(10.0),
        unit: "USD".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Monthly),
        period: None,
        resets_at: None,
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    };
    // Today's label.
    let mut restored = report(
        vec![spend(40.0), quota("Orçamento mensal · Gasto no mês")],
        now,
    );
    mark_restored(&monthly, &mut restored);
    let from = restored.metrics[1].derived_from.as_ref().expect("marked");
    assert_eq!(from.key, budget_key(&monthly.budgets[0]));
    // A reading recorded before the re-validation.
    let mut legacy = report(vec![spend(40.0), quota("Orçamento · Gasto no mês")], now);
    mark_restored(&monthly, &mut legacy);
    assert!(legacy.metrics[1].derived_from.is_some());
    // F-26: every budget removed, the quota never reads as a provider limit.
    for label in [
        "Orçamento mensal · Gasto no mês",
        "Orçamento · Gasto no mês",
    ] {
        let mut orphan = report(vec![spend(40.0), quota(label)], now);
        mark_restored(&account(Vec::new()), &mut orphan);
        assert_eq!(orphan.metrics.len(), 1, "{label}");
        assert_eq!(orphan.metrics[0].kind, MetricKind::Spend);
    }
    // A provider quota is never touched.
    let mut provider = report(vec![spend(40.0), quota("Orçamento (outro)")], now);
    mark_restored(&account(Vec::new()), &mut provider);
    assert_eq!(provider.metrics.len(), 2);
    assert!(provider.metrics[1].derived_from.is_none());
}
