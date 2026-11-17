//! PRD 0041 RF1.2 acceptance (F-25): the dialog's draft check refuses each
//! rule through `check_budgets`, with rows as typed (F-05), the problem at
//! its field and named by the field's label rather than its TOML path
//! (F-10).

use super::*;
use cuotai_core::budget::editor::{BudgetRow, FloorRow, FloorTarget, Target};
use cuotai_core::budget::{BudgetPeriod, BudgetRule, RowTarget};

fn row(amount: &str) -> BudgetRow {
    BudgetRow {
        target: Target::Spend,
        amount: amount.into(),
        unit: "USD".into(),
        ..BudgetRow::default()
    }
}

/// The problems of `budgets`/`floors` for `provider`, as (field, rule).
fn problems(
    provider: Provider,
    budgets: &[BudgetRow],
    floors: &[FloorRow],
    locale: &str,
) -> Vec<(String, BudgetRule)> {
    let check = check_budgets(provider, budgets, floors, locale, None);
    for problem in &check.check.problems {
        let text = problem.note.resolve(locale);
        assert!(
            !text.contains("budgets[") && !text.contains("floors["),
            "the editor names the field, not the path: {text}"
        );
    }
    check
        .check
        .problems
        .into_iter()
        .map(|problem| (problem.field, problem.rule))
        .collect()
}

fn one(field: &str, rule: BudgetRule) -> Vec<(String, BudgetRule)> {
    vec![(field.to_owned(), rule)]
}

#[test]
fn a_valid_row_has_no_problem() {
    assert!(problems(Provider::OpenAiApi, &[row("50")], &[], "en-US").is_empty());
}

#[test]
fn each_rule_is_refused_at_its_field() {
    let p = Provider::OpenAiApi;
    // Amount: empty, zero, too large, not a number.
    for amount in ["", "0", "2000000000", "fifty"] {
        assert_eq!(
            problems(p, &[row(amount)], &[], "en-US"),
            one("budgets[0].amount", BudgetRule::Amount),
            "{amount:?}"
        );
    }
    // F-05: one separator before three digits reads two ways.
    for locale in ["en-US", "pt-BR", "de"] {
        assert_eq!(
            problems(p, &[row("1,000")], &[], locale),
            one("budgets[0].amount", BudgetRule::AmountAmbiguous),
            "{locale}"
        );
    }
    // Unit: longer than 24 characters.
    let long_unit = BudgetRow {
        unit: "x".repeat(30),
        ..row("5")
    };
    assert_eq!(
        problems(p, &[long_unit], &[], "en-US"),
        one("budgets[0].unit", BudgetRule::Unit)
    );
    // Selector: a label filter longer than 64 characters.
    let long_filter = BudgetRow {
        filter: "x".repeat(70),
        ..row("5")
    };
    assert_eq!(
        problems(p, &[long_filter], &[], "en-US"),
        one("budgets[0].label_contains", BudgetRule::Selector)
    );
    // Label: longer than 32 characters.
    let long_label = BudgetRow {
        label: "x".repeat(40),
        ..row("5")
    };
    assert_eq!(
        problems(p, &[long_label], &[], "en-US"),
        one("budgets[0].label", BudgetRule::Label)
    );
    // Duplicate: the same metric over the same period.
    assert_eq!(
        problems(p, &[row("5"), row("6")], &[], "en-US"),
        one("budgets[1].period", BudgetRule::Duplicate)
    );
    // TooMany: more than 8 budgets or 4 floors.
    let many: Vec<BudgetRow> = BudgetPeriod::ALL
        .iter()
        .flat_map(|period| {
            [Target::Spend, Target::Usage, Target::Credits].map(|target| BudgetRow {
                target,
                period: *period,
                ..row("5")
            })
        })
        .collect();
    assert!(many.len() > 8);
    assert_eq!(
        problems(p, &many, &[], "en-US")[0],
        ("budgets".to_owned(), BudgetRule::TooMany)
    );
    let floor = FloorRow {
        target: FloorTarget::Balance,
        amount: "5".into(),
        ..FloorRow::default()
    };
    assert_eq!(
        problems(p, &[], &vec![floor.clone(); 5], "en-US")[0],
        ("floors".to_owned(), BudgetRule::TooMany)
    );
    // A floor's amount is read like a budget's.
    let ambiguous_floor = FloorRow {
        amount: "1.500".into(),
        ..floor
    };
    assert_eq!(
        problems(p, &[], &[ambiguous_floor], "de"),
        one("floors[0].amount", BudgetRule::AmountAmbiguous)
    );
    // KindNotReported: JetBrains AI reports usage only.
    assert_eq!(
        problems(Provider::JetbrainsAi, &[row("5")], &[], "en-US"),
        one("budgets[0].kind", BudgetRule::KindNotReported)
    );
    // NoWindow: the provider's cycle on a metric that never resets.
    let window = BudgetRow {
        period: BudgetPeriod::Window,
        ..row("5")
    };
    assert!(problems(Provider::Antigravity, &[window], &[], "en-US")
        .contains(&("budgets[0].period".to_owned(), BudgetRule::NoWindow)));
    // Row: a breakdown row by model scope.
    let on_model = BudgetRow {
        row: Some(RowTarget {
            scope: cuotai_core::ScopeKind::Model,
            id: "gpt".into(),
        }),
        ..row("5")
    };
    assert_eq!(
        problems(p, &[on_model], &[], "en-US"),
        one("budgets[0].row", BudgetRule::Row)
    );
}

#[test]
fn an_ambiguous_amount_names_both_readings_in_the_dialogs_language() {
    let check = check_budgets(Provider::OpenAiApi, &[row("1.500")], &[], "de", None);
    let note = &check.check.problems[0].note;
    assert_eq!(note.id, "budget.error.amount_ambiguous");
    let english = note.resolve("en-US");
    assert!(
        english.starts_with("Amount: 1.500 could be 1500 or 1,5"),
        "{english}"
    );
    let german = note.resolve("de");
    assert!(
        german.contains("1500") && german.contains("1,5"),
        "{german}"
    );
}

#[test]
fn a_row_without_a_unit_is_described_in_its_metrics_unit() {
    let spend = cuotai_core::Metric::usage(
        "Gasto no mês",
        10.0,
        "EUR",
        cuotai_core::MetricKind::Spend,
        None,
    );
    let blank = BudgetRow {
        unit: String::new(),
        ..row("50")
    };
    let reading = [spend];
    let check = check_budgets(
        Provider::OpenAiApi,
        std::slice::from_ref(&blank),
        &[],
        "en-US",
        Some(&reading),
    );
    assert_eq!(check.check.units, [Some("EUR".to_owned())]);
    let sentence = check.check.sentences[0].resolve("en-US");
    assert!(sentence.contains("€50.00"), "{sentence}");
    // Without a reading the sentence has no unit to invent.
    let check = check_budgets(Provider::OpenAiApi, &[blank], &[], "en-US", None);
    assert_eq!(check.check.units, [None]);
    assert!(!check.check.sentences[0].resolve("en-US").contains('€'));
}
