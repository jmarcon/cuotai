//! PRD 0041 re-validation (F-05, F-10, F-11): the wizard's Budgets step
//! reads amounts like the core, names each problem by its field label and
//! waits for a new row's amount before calling it missing.

use super::*;
use crossterm::event::KeyEventKind;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }
}

fn typed(step: &mut BudgetStep, text: &str, provider: Provider) {
    for c in text.chars() {
        step.handle_key(key(KeyCode::Char(c)), provider);
    }
}

fn colors() -> Colors {
    Colors {
        prompt: Color::Reset,
        accent: Color::Reset,
        dim: Color::Reset,
        bad: Color::Reset,
    }
}

fn text(step: &BudgetStep, provider: Provider) -> String {
    step.lines(provider, &colors())
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_amount_that_reads_two_ways_is_refused_by_its_field_label() {
    crate::l10n::with_locale("en-US", || {
        let provider = Provider::OpenAiApi;
        let mut step = BudgetStep::default();
        step.handle_key(key(KeyCode::Char('a')), provider);
        step.handle_key(key(KeyCode::Down), provider);
        typed(&mut step, "1,000", provider);
        let note = step.problem(provider).expect("refused");
        assert_eq!(note.id, "budget.error.amount_ambiguous");
        let english = note.resolve("en-US");
        assert!(
            english.starts_with("Amount: 1,000 could be 1000 or 1"),
            "{english}"
        );
        assert!(!text(&step, provider).contains("budgets["));
        // A clear amount passes and is saved as typed.
        for _ in 0..5 {
            step.handle_key(key(KeyCode::Backspace), provider);
        }
        typed(&mut step, "1.000,50", provider);
        assert!(step.problem(provider).is_none());
        assert_eq!(step.budgets()[0].amount, 1000.5);
    });
}

#[test]
fn a_new_rows_empty_amount_waits_until_it_is_typed() {
    crate::l10n::with_locale("en-US", || {
        let provider = Provider::OpenAiApi;
        let mut step = BudgetStep::default();
        step.handle_key(key(KeyCode::Char('f')), provider);
        step.handle_key(key(KeyCode::Esc), provider);
        // The list does not open on an error, but Enter still stops on it.
        assert!(!text(&step, provider).contains("Amount:"));
        assert!(step.problem(provider).is_some());
        step.handle_key(key(KeyCode::Char('e')), provider);
        step.handle_key(key(KeyCode::Down), provider);
        typed(&mut step, "0", provider);
        assert!(text(&step, provider).contains("Amount:"));
    });
}

#[test]
fn a_new_budget_takes_the_unit_of_its_metric_in_the_last_reading() {
    crate::l10n::with_locale("en-US", || {
        let provider = Provider::OpenAiApi;
        let spend = cuotai_core::Metric::usage(
            "Gasto no mês",
            10.0,
            "EUR",
            cuotai_core::MetricKind::Spend,
            None,
        );
        let mut step = BudgetStep::default().with_reading(Some(vec![spend]));
        step.handle_key(key(KeyCode::Char('a')), provider);
        assert_eq!(step.budgets[0].unit, "EUR");
        step.handle_key(key(KeyCode::Down), provider);
        typed(&mut step, "50", provider);
        assert!(text(&step, provider).contains("€50.00"));
    });
}
