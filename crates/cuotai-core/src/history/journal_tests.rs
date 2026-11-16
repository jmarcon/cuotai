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
        provider: Some(account.provider),
        accounts: Vec::new(),
        metric: Some(MetricRef::new("5h", None)),
        current: Values {
            threshold: Some(90),
            used_ratio: Some(0.9),
            ..Values::default()
        },
        previous: None,
        sample: false,
        row: None,
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
    assert_eq!(entries[1].summary.text(), "5h: 90% used");
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
    assert_eq!(removed_account().text(), "Removed account");
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

#[test]
fn a_connection_event_keeps_its_aggregator_and_travels_in_the_csv() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir.path().join("history.sqlite3"), "one");
    let accounts = config(CONFIG).accounts;
    let (router, other) = (&accounts[0], &accounts[1]);
    let mut sub = event(router, EventKind::QuotaExhausted, 0);
    sub.account_id = format!("{}/c1", router.state_key());
    sub.parent = Some(router.key());
    sub.metric = Some(MetricRef::new("=big, \"x\"", None));
    assert_eq!(history.journal_record(&[sub]), Ok(1));

    let entries = history.journal(.., &accounts, 20).unwrap();
    assert_eq!(entries[0].account, None);
    assert_eq!(entries[0].parent.as_deref(), Some(router.name.as_str()));
    assert!(entries[0].is_connection);

    let mut csv = Vec::new();
    history
        .export(&accounts, None, .., ExportFormat::Csv, &mut csv)
        .unwrap();
    let csv = String::from_utf8(csv).unwrap();
    let (_, journal) = csv.split_once("\n\noccurred_at,").unwrap();
    let mut lines = journal.lines();
    assert_eq!(
        lines.next(),
        Some("event,account,parent,metric,threshold,used_ratio,resets_at,category")
    );
    let row = lines.next().unwrap();
    assert!(row.contains(",quota_exhausted,,"), "{row}");
    assert!(row.contains(&router.name), "{row}");
    assert!(row.contains("\"'=big, \"\"x\"\"\",90,0.9"), "{row}");

    // Erasing the aggregator erases its connections' events too.
    history.erase_account(router).unwrap();
    assert_eq!(history.journal(.., &accounts, 20), Ok(Vec::new()));
    let _ = other;
}

#[test]
fn overage_and_projection_events_fill_spent_currency_and_projected_at() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir.path().join("history.sqlite3"), "one");
    let accounts = config(CONFIG).accounts;
    let alpha = &accounts[0];
    let mut started = event(alpha, EventKind::OverageStarted, 0);
    started.metric = None;
    started.current = Values {
        spent: Some(12.5),
        currency: Some("USD".into()),
        ..Values::default()
    };
    let mut ended = event(alpha, EventKind::OverageEnded, 2);
    ended.metric = None;
    ended.current = Values {
        spent: Some(f64::NAN),
        ..Values::default()
    };
    let mut projected = event(alpha, EventKind::QuotaExhaustionProjected, 4);
    projected.current = Values {
        projected_at: Some(t0() + TimeDelta::minutes(30)),
        ..Values::default()
    };
    assert_eq!(history.journal_record(&[started, ended, projected]), Ok(3));
    let entries = history.journal(.., &accounts, 20).unwrap();
    let projected = &entries[0];
    assert_eq!(projected.kind, EventKind::QuotaExhaustionProjected);
    assert_eq!(projected.projected_at, Some(t0() + TimeDelta::minutes(30)));
    assert_eq!(
        (projected.spent, projected.currency.as_deref()),
        (None, None)
    );
    // A non-finite amount is unknown, never stored.
    assert_eq!(entries[1].kind, EventKind::OverageEnded);
    assert_eq!(entries[1].spent, None);
    let started = &entries[2];
    assert_eq!(started.kind, EventKind::OverageStarted);
    assert_eq!(started.spent, Some(12.5));
    assert_eq!(started.currency.as_deref(), Some("USD"));
}

/// Re-validation S-01: the size ceiling trims the oldest rows of both
/// tables, so a journal holding the oldest rows goes first and the newest
/// readings survive, and the file ends under the ceiling.
#[test]
fn the_size_ceiling_trims_the_oldest_journal_rows_with_the_samples() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let max = 1 << 20;
    let history =
        store(&path, "one").with_retention(Retention::days(90).unwrap().with_max_bytes(max));
    let accounts = config(CONFIG).accounts;
    let alpha = &accounts[0];
    // A flapping account: 9,000 journal rows, one a minute, the oldest rows
    // of the file.
    let flaps: Vec<Event> = (0..9_000)
        .map(|minute| {
            let kind = if minute % 2 == 0 {
                EventKind::RefreshFailed
            } else {
                EventKind::RefreshRecovered
            };
            let mut flap = event(alpha, kind, minute);
            flap.current = Values {
                category: Some(FailureCategory::Offline),
                ..Values::default()
            };
            flap
        })
        .collect();
    assert_eq!(history.journal_record(&flaps), Ok(9_000));
    // 50 newer readings, after the journal.
    let after = t0() + TimeDelta::minutes(9_000);
    for n in 0..50 {
        history
            .record(
                alpha,
                after + TimeDelta::minutes(n),
                &[super::super::tests::quota(
                    "5h",
                    Some(n as f64),
                    Some(100.0),
                )],
            )
            .unwrap();
    }
    let now = after + TimeDelta::hours(2);
    let report = history.prune(now).unwrap();
    let ceiling = report.ceiling.expect("the file was over the ceiling");
    assert!(ceiling.journal_removed > 0, "{ceiling:?}");
    assert_eq!(
        ceiling.removed, 0,
        "the newer readings survive: {ceiling:?}"
    );

    let raw = rusqlite::Connection::open(&path).unwrap();
    let count = |sql: &str| raw.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap();
    assert_eq!(count("SELECT COUNT(*) FROM samples"), 50);
    let left = count("SELECT COUNT(*) FROM journal");
    assert!(left > 0 && left < 9_000, "{left}");
    // The oldest journal rows went first: what is left is the newest.
    let oldest_left = count("SELECT MIN(occurred_at) FROM journal");
    assert_eq!(
        oldest_left,
        (t0() + TimeDelta::minutes(9_000 - left)).timestamp_millis()
    );
    let used = count(
        "SELECT (page_count - freelist_count) * page_size
         FROM pragma_page_count, pragma_freelist_count, pragma_page_size",
    );
    assert!(used as u64 <= max, "{used} bytes over the {max} ceiling");
}

/// Re-validation S-02: an event more than five minutes ahead of the clock
/// is refused, and one already stored (an older release) is pruned.
#[test]
fn rows_dated_in_the_future_are_refused_and_pruned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let history = store(&path, "one");
    let accounts = config(CONFIG).accounts;
    let alpha = &accounts[0];
    let now = Utc::now();
    let mut ahead = event(alpha, EventKind::QuotaExhausted, 0);
    ahead.occurred_at = now + TimeDelta::days(3650);
    let mut soon = event(alpha, EventKind::QuotaReset, 0);
    soon.occurred_at = now + TimeDelta::minutes(2);
    assert_eq!(history.journal_record(&[ahead.clone(), soon]), Ok(1));

    // As an older release wrote it: straight into the table.
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "INSERT INTO journal (config_key, account_digest, occurred_at, minute, kind,
            metric_digest) SELECT config_key, account_digest, ?1, ?1 / 60000,
            'quota_exhausted', metric_digest FROM journal LIMIT 1",
        [ahead.occurred_at.timestamp_millis()],
    )
    .unwrap();
    let rows = |raw: &rusqlite::Connection| {
        raw.query_row("SELECT COUNT(*) FROM journal", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    };
    assert_eq!(rows(&raw), 2);
    history.prune(now).unwrap();
    assert_eq!(rows(&raw), 1, "the row ten years ahead is pruned");
}

/// Re-validation S-03: an unreadable row is skipped and counted, and a
/// provider status a newer release added reads as a neutral entry; neither
/// fails the journal or the export.
#[test]
fn an_unreadable_row_is_skipped_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let history = store(&path, "one");
    let accounts = config(CONFIG).accounts;
    let alpha = &accounts[0];
    let events: Vec<Event> = (0..5)
        .map(|minute| event(alpha, EventKind::QuotaThresholdCrossed, minute))
        .collect();
    let mut incident = event(alpha, EventKind::ProviderIncident, 10);
    incident.current = Values {
        incident: Some(IncidentState::Degraded),
        ..Values::default()
    };
    assert_eq!(history.journal_record(&events), Ok(5));
    assert_eq!(history.journal_record(&[incident]), Ok(1));
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE journal SET incident = 'from_a_newer_release' WHERE kind = 'provider_incident'",
        [],
    )
    .unwrap();
    // A damaged row: a text where a number belongs.
    raw.execute(
        "UPDATE journal SET used_ratio = 'not a number' WHERE rowid =
            (SELECT MIN(rowid) FROM journal)",
        [],
    )
    .unwrap();
    raw.execute(
        "UPDATE journal SET projected_at = 9223372036854775807 WHERE rowid =
            (SELECT MAX(rowid) FROM journal WHERE kind = 'quota_threshold_crossed')",
        [],
    )
    .unwrap();

    let read = history.journal_read(.., &accounts, 20).unwrap();
    assert_eq!(read.skipped, 1);
    assert_eq!(read.entries.len(), 5);
    let neutral = &read.entries[0];
    assert_eq!(neutral.kind, EventKind::ProviderIncident);
    assert_eq!(neutral.summary.text(), "Provider status: Status unknown");
    assert!(read.entries.iter().all(|e| e.projected_at.is_none()));
    assert_eq!(history.journal(.., &accounts, 20).unwrap().len(), 5);

    let mut json = Vec::new();
    history
        .export(&accounts, None, .., ExportFormat::Json, &mut json)
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(parsed["journal"].as_array().unwrap().len(), 5);
}

/// Re-validation V-02/V-09: an entry's time and day heading come from the
/// core clock, in the reader's language.
#[test]
fn an_entry_reads_its_time_and_day_heading_in_the_language() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir.path().join("history.sqlite3"), "one");
    let accounts = config(CONFIG).accounts;
    let alpha = &accounts[0];
    let events: Vec<Event> = [0, 1, 3, 9]
        .into_iter()
        .map(|days_back| {
            let mut e = event(alpha, EventKind::QuotaExhausted, 0);
            e.occurred_at = t0() - TimeDelta::days(days_back) - TimeDelta::hours(1);
            e
        })
        .collect();
    assert_eq!(history.journal_record(&events), Ok(4));
    let entries = history.journal(.., &accounts, 20).unwrap();
    let now = t0();
    let heads: Vec<String> = entries
        .iter()
        .map(|e| e.day(now, &Utc).fallback.to_string())
        .collect();
    // t0 is Thursday 2026-10-01 12:00 UTC.
    assert_eq!(heads, ["Today", "Yesterday", "Mon", "09/22"]);
    assert_eq!(entries[0].when(now, &Utc).fallback, "11:00\u{a0}AM");
    assert_eq!(entries[2].when(now, &Utc).resolve("de"), "Montag, 11:00");
    assert_eq!(entries[3].when(now, &Utc).resolve("pt-BR"), "22/09 11:00");
    assert_eq!(entries[1].day(now, &Utc).resolve("de"), "Gestern");
}
