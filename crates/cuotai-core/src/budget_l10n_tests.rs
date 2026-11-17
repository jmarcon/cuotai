//! PRD 0041 re-validation (F-06, F-09, F-20): budget notes, the budget
//! period and the binding line read in the reader's language.

use super::*;
use crate::model::Status;
use crate::text::PSEUDO_LOCALE;
use chrono::TimeZone;

fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
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
        resets_at: Some(at(2026, 11, 1, 0)),
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

fn budget(kind: BudgetKind, amount: f64, period: BudgetPeriod) -> Budget {
    Budget {
        kind: Some(kind),
        scope: None,
        label_contains: None,
        amount,
        unit: Some("USD".into()),
        period,
        starts: None,
        label: None,
        row: None,
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

fn derived(report: &Report) -> &Metric {
    report
        .metrics
        .iter()
        .find(|metric| metric.derived_from.is_some())
        .expect("a budget metric")
}

/// F-09: every budget note names the budget by a nested text, so the
/// pseudo-locale (which accents every English letter it renders) leaves no
/// plain English name behind, and a real language translates it.
#[test]
fn budget_notes_name_the_budget_in_the_reader_language() {
    let now = at(2026, 10, 21, 12);
    let account = account(vec![
        // No baseline yet (a weekly budget on a monthly metric).
        budget(BudgetKind::Spend, 50.0, BudgetPeriod::Weekly),
        // A target the reading lacks.
        budget(BudgetKind::Usage, 10.0, BudgetPeriod::Monthly),
    ]);
    let mut reading = report(vec![spend(12.0)], now);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    let ids: Vec<&str> = reading.notes_l10n.iter().map(|n| n.id.as_ref()).collect();
    assert!(ids.contains(&"note.budget.no_baseline"), "{ids:?}");
    assert!(ids.contains(&"note.budget.target_missing"), "{ids:?}");
    for note in &reading.notes_l10n {
        let Some(ParamValue::L10n(name)) = note.params.get("budget") else {
            panic!("{}: the budget name is a nested text", note.id);
        };
        let english = name.fallback.to_string();
        assert!(english.contains("budget ·"), "{english}");
        let pseudo = note.resolve(PSEUDO_LOCALE);
        if cfg!(debug_assertions) {
            assert!(!pseudo.to_lowercase().contains("budget"), "{pseudo}");
        }
        let german = note.resolve("de");
        assert!(german.contains("budget ·"), "{german}");
        // A missing target is named by the budget's kind slug, which the
        // label keeps as its identity; a found one by its translated label.
        if note.id != "note.budget.target_missing" {
            assert!(!german.contains(&english), "{german}");
            assert!(german.starts_with("Wochenbudget · Ausgaben"), "{german}");
        }
    }
    // The plain English note list keeps the English name.
    assert!(reading.notes.iter().all(|note| note.contains("budget ·")));
}

/// F-06: the period of a budget is a day in the reader's format, never the
/// RFC 3339 start; the start itself stays in `period` and
/// `derived_from.period_start`.
#[test]
fn a_budget_period_reads_since_a_day_in_the_reader_format() {
    let now = at(2026, 10, 21, 12); // a Wednesday
    let account = account(vec![
        budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly),
        budget(BudgetKind::Spend, 20.0, BudgetPeriod::Weekly),
    ]);
    let mut reading = report(vec![spend(12.0)], now);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    let budgets: Vec<&Metric> = reading
        .metrics
        .iter()
        .filter(|metric| metric.derived_from.is_some())
        .collect();
    let monthly = budgets[0];
    assert_eq!(monthly.period.as_deref(), Some("2026-10-01T00:00:00+00:00"));
    let from = monthly.derived_from.as_ref().unwrap();
    assert_eq!(from.period_start, Some(at(2026, 10, 1, 0)));
    let text = monthly.period_text().unwrap();
    assert_eq!(text.id, "budget.period_since");
    assert_eq!(text.resolve("en-US"), "since 10/01");
    assert_eq!(text.resolve("de"), "seit 01.10.");
    assert_eq!(text.resolve("ja"), "10/01から");
    // Within the last six days: the short weekday.
    let weekly = budgets[1].period_text().unwrap();
    assert_eq!(weekly.resolve("en-US"), "since Mon");
    assert_eq!(weekly.resolve("pt-BR"), "desde seg.");
    for locale in crate::text::catalog_locales() {
        let text = text.resolve(locale);
        assert!(
            !text.contains("2026") && !text.contains('T'),
            "{locale}: {text}"
        );
    }
    // The export carries the worded period and the typed start.
    let json = serde_json::to_value(monthly).unwrap();
    assert_eq!(json["period"], "2026-10-01T00:00:00+00:00");
    assert_eq!(json["period_text"]["id"], "budget.period_since");
    assert_eq!(json["derived_from"]["period_start"], "2026-10-01T00:00:00Z");
    // A provider metric keeps the display text of its own period.
    let mut provider = spend(1.0);
    provider.period = Some("mês corrente".into());
    assert_eq!(
        provider.period_text().unwrap().id,
        "metric.period.current_month"
    );
    assert!(spend(1.0).period_text().is_none());
}

/// F-06: a reading restored from the history words its budget period again.
#[test]
fn a_restored_budget_words_its_period_again() {
    let now = at(2026, 10, 21, 12);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly)]);
    let mut reading = report(vec![spend(45.0)], now);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    for metric in &mut reading.metrics {
        metric.derived_from = None;
    }
    mark_restored(&account, &mut reading, now, &Utc);
    let from = derived(&reading).derived_from.as_ref().unwrap();
    assert_eq!(from.period_start, Some(at(2026, 10, 1, 0)));
    assert_eq!(
        derived(&reading).period_text().unwrap().resolve("de"),
        "seit 01.10."
    );
}

/// F-20: a budget that binds is named, never "the monthly window".
#[test]
fn the_binding_line_names_a_budget() {
    let now = at(2026, 10, 21, 12);
    let account = account(vec![budget(BudgetKind::Spend, 50.0, BudgetPeriod::Monthly)]);
    let at_time = |t: DateTime<Utc>| ParamValue::from(t.format("%H:%M").to_string());
    // 45 of 50 used: the budget is the tightest limit.
    let mut reading = report(vec![spend(45.0)], now);
    apply(&account, &mut reading, &NoHistory, now, &Utc);
    let binding = crate::metrics::binding(&reading).expect("the budget binds");
    assert!(binding.budget);
    let line = binding.summary(at_time);
    assert_eq!(line.id, "binding.tightest_budget");
    assert_eq!(
        line.resolve("en-US"),
        "most restrictive: Monthly budget · Spending in the month (your budget) at 90%"
    );
    // Used up: the budget's own end, never a "window".
    let mut over = report(vec![spend(60.0)], now);
    apply(&account, &mut over, &NoHistory, now, &Utc);
    let binding = crate::metrics::binding(&over).expect("the budget binds");
    let line = binding.summary(at_time);
    assert_eq!(line.id, "binding.blocked_until_budget");
    let mut unknown = binding.clone();
    unknown.blocked_until = None;
    assert_eq!(unknown.summary(at_time).id, "binding.blocked_budget");
    for locale in crate::text::catalog_locales() {
        let catalog = crate::text::core_catalog(locale).unwrap();
        for id in [
            "binding.tightest_budget",
            "binding.blocked_until_budget",
            "binding.blocked_budget",
            "budget.period_since",
        ] {
            assert!(catalog.contains_key(id), "{locale} lacks {id}");
        }
        let text = line.resolve(locale);
        assert!(!text.contains("{"), "{locale}: {text}");
        assert!(!text.contains("Spending in the month"), "{locale}: {text}");
    }
}
