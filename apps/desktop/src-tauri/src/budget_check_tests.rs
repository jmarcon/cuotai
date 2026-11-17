//! PRD 0041 F-07 (RF1.2): the dialog's budget check refuses a budget in
//! another currency than the metric of the account's last good reading.

use super::*;
use cuotai_core::budget::editor::BudgetRow;
use cuotai_core::budget::{Budget, BudgetKind, BudgetPeriod, BudgetRule};

fn budget(unit: &str) -> BudgetRow {
    BudgetRow::from_budget(&budget_in(unit), "en-US")
}

fn budget_in(unit: &str) -> Budget {
    Budget {
        kind: Some(BudgetKind::Spend),
        scope: None,
        label_contains: None,
        amount: 50.0,
        unit: Some(unit.into()),
        period: BudgetPeriod::Monthly,
        starts: None,
        label: None,
        row: None,
    }
}

#[test]
fn a_budget_in_another_currency_than_the_reading_is_refused() {
    let spend = cuotai_core::Metric::usage(
        "Gasto no mês",
        10.0,
        "USD",
        cuotai_core::MetricKind::Spend,
        None,
    );
    let reading = [spend];
    let check = check_budgets(
        Provider::OpenAiApi,
        &[budget("EUR")],
        &[],
        "en-US",
        Some(&reading),
    );
    assert_eq!(check.check.problems.len(), 1);
    assert_eq!(check.check.problems[0].field, "budgets[0].unit");
    assert_eq!(check.check.problems[0].rule, BudgetRule::UnitMismatch);
    let text = check.check.problems[0].note.resolve("de");
    assert!(text.contains("USD") && text.contains("EUR"), "{text}");
    // Same currency, or no reading (a new account): nothing to refuse.
    assert!(check_budgets(
        Provider::OpenAiApi,
        &[budget("USD")],
        &[],
        "en-US",
        Some(&reading)
    )
    .check
    .problems
    .is_empty());
    assert!(
        check_budgets(Provider::OpenAiApi, &[budget("EUR")], &[], "en-US", None)
            .check
            .problems
            .is_empty()
    );
}
