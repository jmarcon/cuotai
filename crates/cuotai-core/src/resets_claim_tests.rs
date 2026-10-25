//! PRD 0032 RF4: two processes sharing one `notified.json` deliver each
//! event once.

use super::*;
use crate::alerts::{AlertEvent, AlertKind, AlertPrefs};
use crate::model::{MetricKind, Provider, Status};
use chrono::TimeZone;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 2, 15, 23, 0, 0).unwrap()
}

fn window() -> DateTime<Utc> {
    t0() + TimeDelta::hours(6)
}

fn quota(used: f64) -> Metric {
    Metric {
        billing: Default::default(),
        scope: None,
        label: "Session (5h)".into(),
        used: Some(used),
        limit: Some(100.0),
        remaining: None,
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Window5h),
        period: None,
        resets_at: Some(window()),
    }
}

fn report(at: DateTime<Utc>, used: f64) -> Report {
    Report {
        account_id: None,
        diagnostic: None,
        account: "Work".into(),
        provider: Provider::Claude,
        checked_at: at,
        status: Status::Ready,
        metrics: vec![quota(used)],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
    }
}

fn event_of(report: &Report, kind: AlertKind, threshold: u8) -> AlertEvent {
    AlertEvent {
        account_key: report.state_key(),
        label: "Session (5h)".into(),
        window: Some(MetricWindow::Window5h),
        kind,
        threshold,
        consumed: 0.9,
        resets_at: Some(window()),
        projection: None,
    }
}

fn reset_event(report: &Report) -> ResetEvent {
    ResetEvent {
        account_index: 0,
        account_key: report.state_key(),
        metrics: vec![ResetMetric {
            label: "Session (5h)".into(),
            window: MetricWindow::Window5h,
            resets_at: window(),
        }],
        resets_at: window(),
        notify_at: window(),
    }
}

#[test]
fn a_reset_notice_is_delivered_by_one_of_two_processes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notified.json");
    let reading = report(t0(), 40.0);
    let (mut desktop, mut tui) = (Ledger::default(), Ledger::default());
    let now = window();
    let first = desktop.claim_resets(Some(&path), vec![reset_event(&reading)], Vec::new(), now);
    let second = tui.claim_resets(Some(&path), vec![reset_event(&reading)], Vec::new(), now);
    assert_eq!(first.len(), 1);
    assert!(
        second.is_empty(),
        "the TUI's copy was stale, the claim is not"
    );
    // A restart of the TUI sees it as well.
    let mut again = Ledger::load(&path);
    assert!(again
        .claim_resets(Some(&path), vec![reset_event(&reading)], Vec::new(), now)
        .is_empty());
}

#[test]
fn a_dropped_reset_is_recorded_as_skipped_and_never_sent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notified.json");
    let reading = report(t0(), 40.0);
    let mut ledger = Ledger::default();
    let now = window();
    let sent = ledger.claim_resets(Some(&path), Vec::new(), vec![reset_event(&reading)], now);
    assert!(sent.is_empty());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"skipped\":true"), "{text}");
    // Planned again at the end of quiet hours, it is already settled.
    let config = Config {
        accounts: vec![Account {
            id: None,
            name: "Work".into(),
            provider: Provider::Claude,
            ..Account::default()
        }],
        ..Config::default()
    };
    let plan = plan(
        &config,
        [Some(&reading)],
        window() + TimeDelta::minutes(1),
        &Prefs::default(),
        &ledger,
    );
    assert!(plan.due.is_empty());
}

#[test]
fn a_usage_alert_is_delivered_by_one_of_two_processes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notified.json");
    let prefs = AlertPrefs::new(true, [75, 90]).unwrap();
    let next = report(t0(), 92.0);
    let event = event_of(&next, AlertKind::Threshold, 90);
    let (mut desktop, mut tui) = (Ledger::default(), Ledger::default());
    let first = desktop.claim_alerts(
        Some(&path),
        &next,
        &prefs,
        vec![event.clone()],
        Vec::new(),
        t0(),
    );
    let second = tui.claim_alerts(
        Some(&path),
        &next,
        &prefs,
        vec![event.clone()],
        Vec::new(),
        t0(),
    );
    assert_eq!(first, vec![event]);
    assert!(second.is_empty(), "already notified by the desktop");
}

#[test]
fn the_digest_leaves_through_one_process_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notified.json");
    let prefs = AlertPrefs::new(true, [75, 90]).unwrap();
    let until = t0() + TimeDelta::hours(9);
    let next = report(t0(), 92.0);
    let held = event_of(&next, AlertKind::Threshold, 90);
    let (mut desktop, mut tui) = (Ledger::default(), Ledger::default());
    // Both processes judge the same crossing during quiet hours.
    let a = desktop.claim_alerts(
        Some(&path),
        &next,
        &prefs,
        Vec::new(),
        vec![(held.clone(), until)],
        t0(),
    );
    let b = tui.claim_alerts(
        Some(&path),
        &next,
        &prefs,
        Vec::new(),
        vec![(held, until)],
        t0(),
    );
    assert!(
        a.is_empty() && b.is_empty(),
        "nothing is delivered at 23:00"
    );
    // The window ends: the TUI notices the file changed and reloads.
    let late = report(until, 93.0);
    tui.reload(&path);
    let digest = tui.claim_digest(Some(&path), [&late], &prefs, true, until);
    assert_eq!(digest.len(), 1);
    desktop.reload(&path);
    assert!(
        desktop
            .claim_digest(Some(&path), [&late], &prefs, true, until)
            .is_empty(),
        "the desktop finds nothing left to send"
    );
}

#[test]
fn a_corrupt_ledger_is_set_aside_and_recreated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notified.json");
    std::fs::write(&path, "{not json").unwrap();
    let ledger = Ledger::load(&path);
    assert_eq!(ledger, Ledger::default());
    assert!(!path.exists());
    assert!(dir.path().join("notified.json.corrupt").exists());
    ledger.save(&path).unwrap();
    assert!(path.exists());
}
