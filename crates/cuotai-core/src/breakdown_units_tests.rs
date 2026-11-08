//! PRD 0045 RF5: the top rows' history, the "Alert on breakdown rows"
//! gate, budgets on a row and the row reference events carry.

use super::*;
use crate::alerts::{self, AlertKind, AlertPrefs, AlertState};
use crate::budget::{self, Budget, BudgetKind, BudgetPeriod, BudgetRule, RowTarget};
use crate::config_file::ConfigIdentity;
use crate::history::HistoryStore;
use crate::{Account, MetricWindow, Provider, Report, Status};
use chrono::{DateTime, TimeZone, Utc};

fn at(minute: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap() + chrono::TimeDelta::minutes(minute)
}

fn resets() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap()
}

fn account() -> Account {
    Account {
        id: Some(crate::AccountId::parse("org-router").unwrap()),
        name: "Org".into(),
        provider: Provider::OpenRouter,
        breakdown: vec![BreakdownKind::Keys],
        ..Account::default()
    }
}

/// A key row with a monthly cap of 100 USD and `used` spent.
fn capped(id: &str, label: &str, used: f64) -> BreakdownRow {
    BreakdownRow {
        scope: ScopeKind::Key,
        id: id.into(),
        label: label.into(),
        metrics: vec![
            Metric {
                limit: Some(100.0),
                remaining: Some(100.0 - used),
                resets_at: Some(resets()),
                ..Metric::usage(
                    "Limite",
                    used,
                    "USD",
                    MetricKind::Quota,
                    Some(MetricWindow::Monthly),
                )
            },
            Metric {
                resets_at: Some(resets()),
                ..Metric::usage(
                    "Gasto no mês",
                    used,
                    "USD",
                    MetricKind::Spend,
                    Some(MetricWindow::Monthly),
                )
            },
        ],
        truncated: false,
        notes: Vec::new(),
    }
}

fn report(minute: i64, rows: Vec<BreakdownRow>) -> Report {
    let account = account();
    Report {
        account_id: account.id.clone(),
        diagnostic: None,
        account: account.name,
        provider: account.provider,
        checked_at: at(minute),
        status: Status::Ready,
        metrics: vec![Metric::usage(
            "Créditos",
            10.0,
            "USD",
            MetricKind::Balance,
            None,
        )],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        sub_reports: Vec::new(),
        as_of: None,
        overage_hint: None,
        breakdown: rows,
    }
}

fn many(count: usize) -> Vec<BreakdownRow> {
    finish(
        (0..count)
            .map(|i| capped(&format!("k{i:02}"), &format!("Key {i:02}"), 50.0 - i as f64))
            .collect(),
    )
}

#[test]
fn row_keys_hash_the_id_and_name_the_scope() {
    let row = capped("sk-or-v1-0123456789abcdef", "Team A", 1.0);
    let key = row_key(&row);
    assert_eq!(key, format!("breakdown:key:{}", digest_id(&row.id)));
    assert!(!key.contains("sk-or") && !key.contains("Team"));
    let unit = row_account(&account(), &row);
    assert_eq!(unit.state_key(), format!("org-router/{key}"));
    assert_eq!(row_parent(&unit.state_key()), Some("org-router"));
    // A direct account or an aggregator connection is no row.
    assert_eq!(row_parent("org-router"), None);
    assert_eq!(row_parent("router/abc123"), None);
    assert!(unit.credential_env.is_none() && unit.params.is_empty());
    assert!(unit.budgets.is_empty() && unit.breakdown.is_empty());
}

#[test]
fn the_history_takes_the_top_ten_rows_never_others() {
    let rows = many(30);
    assert_eq!(rows.len(), MAX_ROWS);
    let top: Vec<_> = top_rows(&rows).collect();
    assert_eq!(top.len(), HISTORY_ROWS);
    assert!(top.iter().all(|row| !row.is_others()));
    assert_eq!(top[0].id, "k00");
    let units = row_units(&account(), &report(0, rows.clone()));
    assert_eq!(units.len(), HISTORY_ROWS);
    // A row a budget follows is recorded too, so it has a baseline.
    let mut budgeted = account();
    budgeted.budgets.push(Budget {
        row: Some(RowTarget {
            scope: ScopeKind::Key,
            id: "k17".into(),
        }),
        ..row_budget(40.0)
    });
    let units = row_units(&budgeted, &report(0, rows.clone()));
    assert_eq!(units.len(), HISTORY_ROWS + 1);
    assert!(units
        .last()
        .is_some_and(|(_, unit)| unit.metrics[1].used == Some(33.0)));
    // A failed reading records nothing.
    let mut failed = report(0, rows);
    failed.status = Status::RateLimited;
    assert!(row_units(&account(), &failed).is_empty());
}

#[test]
fn row_series_are_their_own_and_hold_only_digests() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let store = HistoryStore::new(path.clone(), &ConfigIdentity::Explicit("rf5".into()));
    let parent = account();
    let reading = report(0, vec![capped("key-hash-0001", "Marketing bot", 42.0)]);
    store
        .record(&parent, reading.checked_at, &reading.metrics)
        .unwrap();
    for (unit, unit_report) in row_units(&parent, &reading) {
        store
            .record(&unit, unit_report.checked_at, &unit_report.metrics)
            .unwrap();
    }
    let own = store.latest(&parent).unwrap().unwrap();
    assert_eq!(own.metrics.len(), 1, "the account keeps its own metrics");
    let row = &reading.breakdown[0];
    let stored = store
        .latest(&row_account(&parent, row))
        .unwrap()
        .expect("the row has a series");
    assert_eq!(stored.metrics.len(), 2);
    assert!(stored.metrics.iter().any(|m| m.metric.used == Some(42.0)));
    let bytes = std::fs::read(&path).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("key-hash-0001") && !text.contains("Marketing"));
}

#[test]
fn rows_alert_only_through_the_gate_and_never_for_the_past() {
    let prefs = AlertPrefs::new(true, alerts::DEFAULT_THRESHOLDS).unwrap();
    let mut state = AlertState::default();
    let first = report(0, vec![capped("k1", "Bot", 50.0)]);
    state.update_units(&first, &prefs, &[], at(0));
    // 92 of 100: one crossing of 90%, about the row.
    let next = report(5, vec![capped("k1", "Bot", 92.0)]);
    let events = alerts::evaluate_rows(Some(&first), &next, &prefs, &state, at(5));
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(event.kind, AlertKind::Threshold);
    assert_eq!(event.threshold, 90);
    assert_eq!(event.label, "Limite");
    let row = event.row.as_ref().expect("the row travels with the event");
    assert_eq!(
        (row.scope, row.id.as_str(), row.label.as_str()),
        (ScopeKind::Key, "k1", "Bot")
    );
    assert_eq!(row_parent(&event.account_key), Some("org-router"));
    // The account's own evaluation never sees rows.
    assert!(alerts::evaluate(Some(&first), &next, &prefs, &state, at(5)).is_empty());

    // With the switch off the levels are still recorded: turning it on
    // later does not notify the crossing that already happened.
    state.update_units(&next, &prefs, &[], at(5));
    let later = report(10, vec![capped("k1", "Bot", 93.0)]);
    assert!(alerts::evaluate_rows(Some(&next), &later, &prefs, &state, at(10)).is_empty());

    // A muted account silences its rows.
    let mut muted = prefs.clone();
    muted.muted_accounts.insert("org-router".into());
    let event = alerts::evaluate_rows(Some(&first), &next, &prefs, &AlertState::default(), at(5));
    assert!(event.iter().all(|event| !muted.wants(event)));
}

/// PRD 0045 RF5.2: keeping an account keeps the levels of its rows, also
/// after the state was saved and loaded again; removing it drops them.
#[test]
fn retaining_an_account_keeps_the_marks_of_its_rows() {
    let prefs = AlertPrefs::new(true, alerts::DEFAULT_THRESHOLDS).unwrap();
    let mut state = AlertState::default();
    let first = report(0, vec![capped("k1", "Bot", 50.0)]);
    state.update_units(&first, &prefs, &[], at(0));
    let next = report(5, vec![capped("k1", "Bot", 92.0)]);
    let events = alerts::evaluate_rows(Some(&first), &next, &prefs, &state, at(5));
    assert_eq!(events.len(), 1);
    state.update_units(&next, &prefs, &events, at(5));
    assert!(!state.retain_accounts(["org-router"]), "the row marks stay");
    let later = report(10, vec![capped("k1", "Bot", 93.0)]);
    assert!(
        alerts::evaluate_rows(Some(&next), &later, &prefs, &state, at(10)).is_empty(),
        "the row is no first reading and its level is not notified again"
    );
    let mut loaded: AlertState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert!(!loaded.retain_accounts(["org-router"]));
    assert!(
        loaded.retain_accounts(["other"]),
        "a removed account's rows go"
    );
    assert!(loaded.is_empty());
}

#[test]
fn row_events_group_under_their_account_in_a_notice() {
    let parent = account();
    let prefs = AlertPrefs::new(true, alerts::DEFAULT_THRESHOLDS).unwrap();
    let mut state = AlertState::default();
    let first = report(0, vec![capped("k1", "Bot", 50.0)]);
    state.update_units(&first, &prefs, &[], at(0));
    let next = report(5, vec![capped("k1", "Bot", 100.0)]);
    let events = alerts::evaluate_rows(Some(&first), &next, &prefs, &state, at(5));
    assert!(events
        .iter()
        .any(|event| event.kind == AlertKind::Exhausted));
    let keys = vec![parent.state_key()];
    assert!(events
        .iter()
        .all(|event| alerts::configured_unit(&keys, &event.account_key)));
    assert!(!alerts::configured_unit(
        &["other".into()],
        &events[0].account_key
    ));
    let groups = alerts::notice_groups([(parent.state_key(), &parent)], events.clone());
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0.name, "Org · Bot");
    assert_eq!(groups[0].1.len(), events.len());
}

fn row_budget(amount: f64) -> Budget {
    Budget {
        kind: Some(BudgetKind::Spend),
        scope: None,
        label_contains: None,
        amount,
        unit: None,
        period: BudgetPeriod::Window,
        starts: None,
        label: None,
        row: Some(RowTarget {
            scope: ScopeKind::Key,
            id: "k1".into(),
        }),
    }
}

#[test]
fn a_budget_on_a_row_becomes_a_quota_of_that_row() {
    let mut parent = account();
    parent.budgets = vec![row_budget(80.0)];
    let mut reading = report(0, vec![capped("k1", "Bot", 72.0), capped("k2", "CI", 5.0)]);
    let none = |_: &BreakdownRow, _: &Metric, _: DateTime<Utc>| Vec::new();
    budget::apply(&parent, &mut reading, &budget::NoHistory, at(0), &Utc);
    budget::apply_rows(&parent, &mut reading, &none, at(0), &Utc);
    assert_eq!(
        reading.metrics.len(),
        1,
        "the account's metrics are untouched"
    );
    let derived = reading.breakdown[0]
        .metrics
        .iter()
        .find(|m| m.derived_from.is_some())
        .expect("the row carries its budget");
    assert_eq!((derived.used, derived.limit), (Some(72.0), Some(80.0)));
    assert_eq!(derived.resets_at, Some(resets()));
    assert!(reading.breakdown[1]
        .metrics
        .iter()
        .all(|m| m.derived_from.is_none()));
    // Applying again changes nothing.
    budget::apply_rows(&parent, &mut reading, &none, at(0), &Utc);
    assert_eq!(reading.breakdown[0].metrics.len(), 3);

    // 72 → 81 of the 80 budget: the row's budget is exhausted (gated).
    let prefs = AlertPrefs::new(true, alerts::DEFAULT_THRESHOLDS).unwrap();
    let mut state = AlertState::default();
    state.update_units(&reading, &prefs, &[], at(0));
    let mut next = report(5, vec![capped("k1", "Bot", 81.0)]);
    budget::apply_rows(&parent, &mut next, &none, at(5), &Utc);
    let events = alerts::evaluate_rows(Some(&reading), &next, &prefs, &state, at(5));
    assert!(events.iter().any(|event| event.kind == AlertKind::Exhausted
        && event.label.starts_with(budget::DEFAULT_LABEL_PREFIX)));

    // A row gone from the reading says so on the account, never zero.
    let mut gone = report(10, vec![capped("k2", "CI", 5.0)]);
    budget::apply_rows(&parent, &mut gone, &none, at(10), &Utc);
    assert!(gone
        .notes_l10n
        .iter()
        .any(|note| note.id == "note.budget.target_missing"));
    assert!(gone.breakdown[0]
        .metrics
        .iter()
        .all(|m| m.derived_from.is_none()));
}

#[test]
fn row_budgets_are_validated_and_drafted() {
    let ok = row_budget(10.0);
    let rules = |provider, budgets: Vec<Budget>| {
        budget::problems(provider, &budgets, &[])
            .into_iter()
            .map(|p| (p.field, p.rule))
            .collect::<Vec<_>>()
    };
    assert!(rules(Provider::OpenRouter, vec![ok.clone()]).is_empty());
    assert!(rules(Provider::OpenAiApi, vec![ok.clone()]).is_empty());
    let row_rule = vec![("budgets[0].row".to_owned(), BudgetRule::Row)];
    assert_eq!(rules(Provider::Groq, vec![ok.clone()]), row_rule);
    let mut model = ok.clone();
    model.row.as_mut().unwrap().scope = ScopeKind::Model;
    assert_eq!(rules(Provider::OpenRouter, vec![model]), row_rule);
    let mut empty = ok.clone();
    empty.row.as_mut().unwrap().id = " ".into();
    assert_eq!(rules(Provider::OpenRouter, vec![empty]), row_rule);
    // The same budget on another row is no duplicate.
    let mut other = ok.clone();
    other.row.as_mut().unwrap().id = "k2".into();
    assert!(rules(Provider::OpenRouter, vec![ok.clone(), other]).is_empty());
    assert_eq!(
        budget::BudgetRule::Row.note("budgets[0].row").id,
        "budget.error.row"
    );

    // TOML round trip: `row = { scope = "key", id = "k1" }`.
    let toml = toml::to_string(&ok).unwrap();
    assert!(
        toml.contains("scope = \"key\"") && toml.contains("id = \"k1\""),
        "{toml}"
    );
    assert_eq!(toml::from_str::<Budget>(&toml).unwrap(), ok);

    let row = capped("k1", "Bot", 3.0);
    let draft = budget::draft_for_row(&row).expect("a spend row has a draft");
    assert_eq!(draft.row, ok.row);
    assert_eq!(draft.kind, Some(BudgetKind::Spend));
    let mut parent = account();
    assert!(budget::offers_row_budget(&parent, &row));
    parent.budgets = vec![ok];
    assert!(!budget::offers_row_budget(&parent, &row));
    let others = finish(
        (0..30)
            .map(|i| capped(&format!("k{i}"), "x", 1.0))
            .collect(),
    );
    assert!(budget::draft_for_row(others.last().unwrap()).is_none());
}
