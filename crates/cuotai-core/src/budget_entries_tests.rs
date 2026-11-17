//! PRD 0041 re-validation F-14, F-15, F-19: one invalid budget or floor is
//! left out with a note while the others keep working, labels with
//! disguising characters are refused, and a floor with nothing to watch
//! says so.

use super::*;
use crate::model::Status;
use chrono::TimeZone;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap()
}

fn metric(label: &str, kind: MetricKind, used: Option<f64>, remaining: Option<f64>) -> Metric {
    Metric {
        label: label.into(),
        used,
        limit: None,
        remaining,
        unit: "USD".into(),
        kind,
        window: Some(MetricWindow::Monthly),
        period: None,
        resets_at: None,
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    }
}

fn reading(metrics: Vec<Metric>) -> Report {
    Report {
        account: "Work".into(),
        account_id: None,
        provider: Provider::OpenAiApi,
        checked_at: now(),
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

fn spend_budget(amount: f64, period: BudgetPeriod) -> Budget {
    Budget {
        kind: Some(BudgetKind::Spend),
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

fn floor(amount: f64) -> Floor {
    Floor {
        kind: Some(FloorKind::Balance),
        label_contains: None,
        amount,
        unit: None,
    }
}

fn account(budgets: Vec<Budget>, floors: Vec<Floor>) -> Account {
    Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        budgets,
        floors,
        ..Default::default()
    }
}

fn note_ids(report: &Report) -> Vec<&str> {
    report
        .notes_l10n
        .iter()
        .map(|note| note.id.as_ref())
        .collect()
}

#[test]
fn one_invalid_budget_leaves_the_account_ready_and_the_others_applied() {
    let account = account(
        vec![
            spend_budget(0.0, BudgetPeriod::Daily),
            spend_budget(50.0, BudgetPeriod::Monthly),
        ],
        Vec::new(),
    );
    let valid = valid_indices(&account);
    assert!(!valid.budget(0));
    assert!(valid.budget(1));
    let mut report = reading(vec![metric("Spend", MetricKind::Spend, Some(5.0), None)]);
    apply(&account, &mut report, &NoHistory, now(), &Utc);
    assert_eq!(report.status, Status::Ready);
    let derived: Vec<&Metric> = report
        .metrics
        .iter()
        .filter(|metric| metric.derived_from.is_some())
        .collect();
    assert_eq!(derived.len(), 1, "only the valid budget is applied");
    assert_eq!(derived[0].derived_from.as_ref().unwrap().budget, 1);
    assert_eq!(derived[0].limit, Some(50.0));
    let ignored = report
        .notes_l10n
        .iter()
        .find(|note| note.id == "note.budget.ignored")
        .expect("the ignored note");
    assert!(
        ignored
            .fallback
            .starts_with("budgets[0].amount: the amount must be"),
        "{}",
        ignored.fallback
    );
    assert!(ignored.fallback.ends_with("left out until it is fixed."));
    // A reused reading applied again carries the note once.
    apply(&account, &mut report, &NoHistory, now(), &Utc);
    let count = note_ids(&report)
        .iter()
        .filter(|id| **id == "note.budget.ignored")
        .count();
    assert_eq!(count, 1);
}

#[test]
fn the_strict_validation_still_names_the_first_problem() {
    let account = account(vec![spend_budget(0.0, BudgetPeriod::Daily)], Vec::new());
    assert_eq!(
        validate(0, &account),
        Err(BudgetError {
            field: "accounts[0].budgets[0].amount".into(),
            rule: BudgetRule::Amount,
        })
    );
}

#[test]
fn the_ignored_note_is_added_to_a_failed_reading_too() {
    let account = account(vec![spend_budget(-1.0, BudgetPeriod::Daily)], Vec::new());
    let mut report = reading(Vec::new());
    report.status = Status::AuthenticationError;
    apply(&account, &mut report, &NoHistory, now(), &Utc);
    assert_eq!(note_ids(&report), ["note.budget.ignored"]);
}

#[test]
fn a_repeated_budget_keeps_the_first_and_leaves_out_the_second() {
    let account = account(
        vec![
            spend_budget(10.0, BudgetPeriod::Monthly),
            spend_budget(20.0, BudgetPeriod::Monthly),
        ],
        Vec::new(),
    );
    let valid = valid_indices(&account);
    assert!(valid.budget(0));
    assert!(!valid.budget(1));
}

#[test]
fn entries_past_the_limit_are_left_out_and_the_first_ones_work() {
    let budgets: Vec<Budget> = [
        BudgetPeriod::Daily,
        BudgetPeriod::Weekly,
        BudgetPeriod::Monthly,
        BudgetPeriod::Window,
    ]
    .into_iter()
    .flat_map(|period| {
        [BudgetKind::Spend, BudgetKind::Usage, BudgetKind::Credits]
            .into_iter()
            .map(move |kind| Budget {
                kind: Some(kind),
                ..spend_budget(5.0, period)
            })
    })
    .collect();
    assert_eq!(budgets.len(), 12);
    let account = account(budgets, (0..6).map(|i| floor(1.0 + f64::from(i))).collect());
    let valid = valid_indices(&account);
    assert!((0..MAX_BUDGETS).all(|index| valid.budget(index)));
    assert!((MAX_BUDGETS..12).all(|index| !valid.budget(index)));
    assert!((0..MAX_FLOORS).all(|index| valid.floor(index)));
    assert!(!valid.floor(MAX_FLOORS));
    let fields: Vec<&str> = valid
        .problems()
        .map(|problem| problem.field.as_str())
        .collect();
    assert_eq!(
        fields,
        [
            "budgets[8]",
            "budgets[9]",
            "budgets[10]",
            "budgets[11]",
            "floors[4]",
            "floors[5]"
        ]
    );
}

#[test]
fn twenty_thousand_budgets_are_refused_at_once() {
    let budgets = vec![spend_budget(5.0, BudgetPeriod::Monthly); 20_000];
    let started = std::time::Instant::now();
    let found = problems(Provider::OpenAiApi, &budgets, &[]);
    let valid = valid_entries(Provider::OpenAiApi, &budgets, &[]);
    let elapsed = started.elapsed();
    assert_eq!(
        found,
        [BudgetError {
            field: "budgets".into(),
            rule: BudgetRule::TooMany,
        }]
    );
    assert!(valid.budget(0) && !valid.budget(MAX_BUDGETS));
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "{elapsed:?}"
    );
}

#[test]
fn disguising_characters_are_refused_in_labels_filters_and_units() {
    let hidden = |text: &str| Some(format!("Cap{text}x"));
    let budget = Budget {
        label: hidden("\u{202e}"),
        label_contains: hidden("\u{2066}"),
        unit: hidden("\u{1b}"),
        ..spend_budget(5.0, BudgetPeriod::Monthly)
    };
    let floor = Floor {
        label_contains: hidden("\u{202a}"),
        unit: hidden("\u{7}"),
        ..floor(1.0)
    };
    let found: Vec<(String, BudgetRule)> = problems(Provider::OpenAiApi, &[budget], &[floor])
        .into_iter()
        .map(|problem| (problem.field, problem.rule))
        .collect();
    let rule = BudgetRule::Characters;
    assert_eq!(
        found,
        [
            ("budgets[0].unit".to_owned(), rule),
            ("budgets[0].label_contains".to_owned(), rule),
            ("budgets[0].label".to_owned(), rule),
            ("floors[0].unit".to_owned(), rule),
            ("floors[0].label_contains".to_owned(), rule),
        ]
    );
    assert_eq!(
        rule.note("budgets[0].label").fallback,
        "budgets[0].label: remove the invisible or control characters; only visible text is allowed."
    );
}

#[test]
fn a_budget_with_a_hidden_label_never_reaches_the_reading() {
    let account = account(
        vec![Budget {
            label: Some("Cap\u{202e}gnp.exe".into()),
            ..spend_budget(5.0, BudgetPeriod::Monthly)
        }],
        Vec::new(),
    );
    let mut report = reading(vec![metric("Spend", MetricKind::Spend, Some(1.0), None)]);
    apply(&account, &mut report, &NoHistory, now(), &Utc);
    assert!(report.metrics.iter().all(|m| m.derived_from.is_none()));
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains('\u{202e}'), "{json}");
    assert!(!json.contains("\\u202e"), "{json}");
}

#[test]
fn a_floor_with_nothing_to_watch_says_so() {
    let account = account(Vec::new(), vec![floor(5.0)]);
    let mut report = reading(vec![metric("Spend", MetricKind::Spend, Some(1.0), None)]);
    apply(&account, &mut report, &NoHistory, now(), &Utc);
    assert_eq!(note_ids(&report), ["note.budget.floor_missing"]);
    assert_eq!(
        report.notes,
        ["floors[0]: no balance or credits in this reading matches this floor, so it warns about nothing."]
    );
    assert!(floor_statuses(&account, &report).is_empty());
    // With its balance present the note goes away.
    let mut report = reading(vec![metric(
        "Balance",
        MetricKind::Balance,
        None,
        Some(9.0),
    )]);
    apply(&account, &mut report, &NoHistory, now(), &Utc);
    assert!(note_ids(&report).is_empty());
    assert_eq!(floor_statuses(&account, &report).len(), 1);
}

#[test]
fn an_invalid_floor_is_left_out_of_the_statuses_and_the_alerts() {
    let account = account(Vec::new(), vec![floor(0.0), floor(5.0)]);
    let mut report = reading(vec![metric(
        "Balance",
        MetricKind::Balance,
        None,
        Some(3.0),
    )]);
    apply(&account, &mut report, &NoHistory, now(), &Utc);
    assert_eq!(note_ids(&report), ["note.budget.ignored"]);
    let statuses = floor_statuses(&account, &report);
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].amount, 5.0);
    let config = crate::Config {
        accounts: vec![account.clone()],
        ..crate::Config::default()
    };
    let prefs = crate::alerts::AlertPrefs::default().with_floors(&config);
    assert_eq!(prefs.floors[&account.state_key()], [floor(5.0)]);
}
