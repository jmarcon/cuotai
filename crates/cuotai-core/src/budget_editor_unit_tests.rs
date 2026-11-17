//! F-07 (PRD 0041 RF1.2): a budget in another currency or unit than its
//! metric is refused by the editors and named at reading time, never read
//! as a missing metric.

use super::*;
use crate::budget::{apply, NoHistory};
use crate::model::{Account, Billing, Report, Status};
use chrono::{TimeZone, Utc};

fn spend_usd() -> Metric {
    Metric {
        label: "Gasto no mês".into(),
        used: Some(10.0),
        limit: None,
        remaining: None,
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: None,
        period: None,
        resets_at: None,
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    }
}

fn budget_in(unit: Option<&str>) -> Budget {
    Budget {
        kind: Some(BudgetKind::Spend),
        scope: None,
        label_contains: None,
        amount: 50.0,
        unit: unit.map(str::to_owned),
        period: BudgetPeriod::Daily,
        starts: None,
        label: None,
        row: None,
    }
}

fn reading(metrics: Vec<Metric>) -> Report {
    Report {
        account: "Work".into(),
        account_id: None,
        provider: Provider::OpenAiApi,
        checked_at: Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap(),
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

#[test]
fn a_budget_in_another_currency_names_both_at_reading_time() {
    let account = Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        budgets: vec![budget_in(Some("EUR"))],
        ..Account::default()
    };
    let mut report = reading(vec![spend_usd()]);
    let now = report.checked_at;
    apply(&account, &mut report, &NoHistory, now, &Utc);
    let derived = report
        .metrics
        .iter()
        .find(|metric| metric.derived_from.is_some())
        .unwrap();
    assert_eq!(derived.used, None, "never 0, never counted in USD");
    assert_eq!(derived.unit, "EUR");
    // The note names the metric it would follow and both units.
    assert_eq!(derived.derived_from.as_ref().unwrap().label, "Gasto no mês");
    let ids: Vec<&str> = report.notes_l10n.iter().map(|n| &*n.id).collect();
    assert_eq!(ids, ["note.budget.unit_mismatch_named"]);
    let note = &report.notes_l10n[0];
    let english = note.resolve("en-US");
    assert!(
        english.contains("USD") && english.contains("EUR"),
        "{english}"
    );
    for locale in crate::text::catalog_locales() {
        let text = note.resolve(locale);
        assert!(
            text.contains("USD") && text.contains("EUR"),
            "{locale}: {text}"
        );
    }
}

#[test]
fn a_budget_without_any_metric_of_its_kind_stays_missing() {
    let account = Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        budgets: vec![budget_in(Some("EUR"))],
        ..Account::default()
    };
    let mut report = reading(Vec::new());
    let now = report.checked_at;
    apply(&account, &mut report, &NoHistory, now, &Utc);
    let ids: Vec<&str> = report.notes_l10n.iter().map(|n| &*n.id).collect();
    assert_eq!(ids, ["note.budget.target_missing"]);
}

#[test]
fn the_editors_refuse_a_budget_in_another_currency_than_its_metric() {
    let metrics = [spend_usd()];
    let eur = [budget_in(Some("EUR"))];
    let problems = check(Provider::OpenAiApi, &eur, &[], Some(&metrics));
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].field, "budgets[0].unit");
    assert_eq!(problems[0].rule, BudgetRule::UnitMismatch);
    assert_eq!(problems[0].note.id, "budget.error.unit_mismatch");
    let english = problems[0].note.resolve("en-US");
    assert!(english.contains("reported in USD"), "{english}");
    assert!(english.contains("this budget is in EUR"), "{english}");
    for locale in crate::text::catalog_locales() {
        let text = problems[0].note.resolve(locale);
        assert!(
            text.contains("USD") && text.contains("EUR"),
            "{locale}: {text}"
        );
    }
    // The same unit, no unit, or no reading: nothing to refuse.
    let usd = [budget_in(Some("usd"))];
    assert!(check(Provider::OpenAiApi, &usd, &[], Some(&metrics)).is_empty());
    let none = [budget_in(None)];
    assert!(check(Provider::OpenAiApi, &none, &[], Some(&metrics)).is_empty());
    assert!(check(Provider::OpenAiApi, &eur, &[], None).is_empty());
    // A budget of a kind the reading lacks is not a unit problem.
    let mut usage = budget_in(Some("EUR"));
    usage.kind = Some(BudgetKind::Usage);
    assert!(check(Provider::OpenAiApi, &[usage], &[], Some(&metrics)).is_empty());
    // Without the reading, the rule still has a text (the unit rule).
    assert_eq!(
        BudgetRule::UnitMismatch
            .note("accounts[0].budgets[0].unit")
            .id,
        "budget.error.unit"
    );
}
