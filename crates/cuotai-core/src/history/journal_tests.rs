use super::super::tests::{config, store, t0, CONFIG};
use super::super::{ExportFormat, Retention};
use super::*;
use chrono::TimeDelta;

fn event(account: &Account, kind: EventKind, minutes: i64) -> Event {
    Event {
        parent: Default::default(),
        schema: crate::events::SCHEMA,
        event: kind,
        occurred_at: t0() + TimeDelta::minutes(minutes),
        account_key: account.key(),
        account_id: account.state_key(),
        account_name: account.name.clone(),
        provider: account.provider,
        accounts: Vec::new(),
        metric: Some(MetricRef {
            label: "5h".into(),
            window: None,
        }),
        current: Values {
            threshold: Some(90),
            used_ratio: Some(0.9),
            ..Values::default()
        },
        previous: None,
        sample: false,
    }
}

#[test]
fn records_edge_events_newest_first_without_usage_updates() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir.path().join("history.sqlite3"), "one");
    let accounts = config(CONFIG).accounts;
    let alpha = &accounts[0];
    assert_eq!(history.journal(.., &accounts, 20), Ok(Vec::new()));
    assert!(!dir.path().join("history.sqlite3").exists());

    let events = [
        event(alpha, EventKind::QuotaThresholdCrossed, 0),
        event(alpha, EventKind::UsageUpdated, 1),
        event(alpha, EventKind::QuotaExhausted, 5),
    ];
    assert_eq!(history.journal_record(&events), Ok(2));
    let entries = history.journal(.., &accounts, 20).unwrap();
    let kinds: Vec<_> = entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [EventKind::QuotaExhausted, EventKind::QuotaThresholdCrossed]
    );
    assert_eq!(entries[1].account.as_deref(), Some(alpha.name.as_str()));
    assert_eq!(entries[1].threshold, Some(90));
    assert_eq!(entries[1].summary.text(), "5h: 90% usado");
    assert_eq!(history.journal(.., &accounts, 1).unwrap().len(), 1);
}

#[test]
fn two_writers_do_not_duplicate_and_a_rename_keeps_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let (first, second) = (store(&path, "one"), store(&path, "one"));
    let mut accounts = config(CONFIG).accounts;
    for account in &mut accounts {
        account.id = Some(account.id()); // a rename keeps an explicit id
    }
    let events = [event(&accounts[0], EventKind::QuotaExhausted, 0)];
    assert_eq!(first.journal_record(&events), Ok(1));
    // Same minute, different second, from another process.
    let mut again = events.clone();
    again[0].occurred_at += TimeDelta::seconds(30);
    assert_eq!(second.journal_record(&again), Ok(0));
    // The next minute is a new entry.
    let mut later = events.clone();
    later[0].occurred_at += TimeDelta::minutes(1);
    assert_eq!(second.journal_record(&later), Ok(1));

    accounts[0].name = "renamed".into();
    let entries = first.journal(.., &accounts, 20).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries
        .iter()
        .all(|e| e.account.as_deref() == Some("renamed")));
    // An account no longer configured is shown as removed.
    let others = &accounts[1..];
    let entries = first.journal(.., others, 20).unwrap();
    assert!(entries.iter().all(|e| e.account.is_none()));
    assert_eq!(removed_account().text(), "Conta removida");
}

#[test]
fn stores_only_names_and_categories_never_provider_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let history = store(&path, "one");
    let accounts = config(CONFIG).accounts;
    let mut failed = event(&accounts[0], EventKind::RefreshFailed, 0);
    failed.current = Values {
        category: Some(FailureCategory::Offline),
        ..Values::default()
    };
    // Secrets in free-text fields of the event never reach the table.
    failed.account_name = "me@example.com sk-secret-token".into();
    failed.metric = None;
    history.journal_record(&[failed]).unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    let dump: String = raw
        .query_row(
            "SELECT group_concat(quote(account_digest) || quote(kind) || quote(metric_label)
                || quote(category) || quote(incident) || quote(currency), '|') FROM journal",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for leak in ["example.com", "sk-secret", "https://"] {
        assert!(!dump.contains(leak), "{leak} leaked: {dump}");
    }
    assert!(dump.contains("refresh_failed") && dump.contains("offline"));
    let entry = &history.journal(.., &accounts, 5).unwrap()[0];
    assert_eq!(entry.category, Some(FailureCategory::Offline));
}

#[test]
fn retention_erase_and_export_follow_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let history = store(&path, "one").with_retention(Retention::days(7).unwrap());
    let accounts = config(CONFIG).accounts;
    let (alpha, beta) = (&accounts[0], &accounts[1]);
    let now = t0() + TimeDelta::days(10);
    let mut old = event(alpha, EventKind::QuotaExhausted, 0);
    old.occurred_at = now - TimeDelta::days(9);
    let mut fresh = event(alpha, EventKind::QuotaReset, 0);
    fresh.occurred_at = now - TimeDelta::days(1);
    let mut other = event(beta, EventKind::AuthRequired, 0);
    other.occurred_at = now - TimeDelta::days(2);
    assert_eq!(history.journal_record(&[old, fresh, other]), Ok(3));

    let mut json = Vec::new();
    history
        .export(&accounts, None, .., ExportFormat::Json, &mut json)
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(parsed["journal"].as_array().unwrap().len(), 3);

    history.prune(now).unwrap();
    assert_eq!(
        history.journal(.., &accounts, 20).unwrap().len(),
        2,
        "9 days is over the 7 day retention"
    );
    history.erase_account(alpha).unwrap();
    let left = history.journal(.., &accounts, 20).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].kind, EventKind::AuthRequired);
    history.erase_all().unwrap();
    assert_eq!(history.journal(.., &accounts, 20), Ok(Vec::new()));
}
