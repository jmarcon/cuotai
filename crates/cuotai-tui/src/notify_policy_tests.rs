//! PRD 0032 RF1, RF4, RF6 and RF7.2 for `--notify`: the shared preferences
//! file, quiet hours with a digest, the shared ledger and private OSC 9
//! texts, with a fake clock in UTC.

use super::*;
use chrono::TimeZone;
use cuotai_core::{Metric, MetricKind, MetricWindow, Provider, Status};
use std::path::Path;

/// A scratch directory removed on drop.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("cuotai-notify-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn at(day: u32, h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, day, h, m, 0).unwrap()
}

fn account(name: &str) -> Account {
    Account {
        id: None,
        name: name.into(),
        provider: Provider::Claude,
        ..Account::default()
    }
}

fn config() -> Config {
    Config {
        accounts: vec![account("Secret Corp")],
        ..Config::default()
    }
}

fn reading(checked_at: DateTime<Utc>, used: f64, resets_at: DateTime<Utc>) -> Report {
    Report {
        as_of: Default::default(),
        overage_hint: Default::default(),
        sub_reports: Vec::new(),
        account_id: None,
        diagnostic: None,
        account: "Secret Corp".into(),
        provider: Provider::Claude,
        checked_at,
        status: Status::Ready,
        metrics: vec![Metric {
            billing: Default::default(),
            scope: None,
            label: "5h".into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: None,
            resets_at: Some(resets_at),
        }],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
    }
}

const NIGHTS: &str = r#"{"quiet":{"enabled":true,"digest":true,
    "days":{"mon":[{"from":"22:00","to":"08:00"}],"tue":[{"from":"22:00","to":"08:00"}]}}}"#;

/// A `--notify` session started at `started` over the files in `dir`.
fn session(dir: &Path, started: DateTime<Utc>, osc9: bool) -> Notices {
    let mut notices = Notices::new(started, osc9).shared(
        notify_prefs::Source::new(Some(dir.join("desktop.json"))),
        SharedLedger::open(Some(dir.join("notified.json"))),
    );
    notices.zone = chrono::FixedOffset::east_opt(0);
    notices
}

/// The cycle accepted `next` after `prev`: judge, record, then the pass.
fn observe(
    notices: &mut Notices,
    prev: Option<&Report>,
    next: &Report,
    now: DateTime<Utc>,
) -> Vec<u8> {
    let events = notices.assess(prev, next, now, None);
    notices.record(next, events, now, None);
    notices.tick(&config(), &[Some(next)], now, "en-US")
}

fn bells(out: &[u8]) -> usize {
    out.iter().filter(|b| **b == BEL).count()
}

#[test]
fn a_missing_file_applies_the_recommended_defaults() {
    let dir = Dir::new();
    let mut notices = session(dir.path(), at(2, 9, 0), false);
    let resets = at(2, 20, 0);
    let before = reading(at(2, 10, 0), 70.0, resets);
    let crossed = reading(at(2, 10, 5), 80.0, resets);
    // 75% is a default threshold: the TUI-only person keeps today's alerts.
    let out = observe(&mut notices, Some(&before), &crossed, at(2, 10, 5));
    assert_eq!(bells(&out), 1);
}

#[test]
fn thresholds_and_mutes_come_from_the_shared_file() {
    let dir = Dir::new();
    let id = account("Secret Corp").state_key();
    std::fs::write(
        dir.path().join("desktop.json"),
        format!(r#"{{"alerts":{{"enabled":false,"thresholds":[60]}},"notify":{{"enabled":false,"off_accounts":["{id}-other"]}}}}"#),
    )
    .unwrap();
    let mut notices = session(dir.path(), at(2, 9, 0), false);
    let resets = at(2, 20, 0);
    let before = reading(at(2, 10, 0), 50.0, resets);
    let crossed = reading(at(2, 10, 5), 65.0, resets);
    // The desktop's switches are off, but `--notify` is the consent; the
    // threshold list is the file's: 60% rings, 75% would not have.
    let out = observe(&mut notices, Some(&before), &crossed, at(2, 10, 5));
    assert_eq!(bells(&out), 1);

    // A muted bell silences the account for every reader.
    let dir = Dir::new();
    std::fs::write(
        dir.path().join("desktop.json"),
        format!(r#"{{"notify":{{"off_accounts":["{id}"]}}}}"#),
    )
    .unwrap();
    let mut notices = session(dir.path(), at(2, 9, 0), false);
    let out = observe(&mut notices, Some(&before), &crossed, at(2, 10, 5));
    assert!(out.is_empty());
}

#[test]
fn a_preferences_edit_by_the_desktop_is_picked_up_without_a_restart() {
    let dir = Dir::new();
    let mut notices = session(dir.path(), at(2, 9, 0), false);
    let resets = at(2, 20, 0);
    let a = reading(at(2, 10, 0), 50.0, resets);
    let b = reading(at(2, 10, 5), 65.0, resets);
    assert!(observe(&mut notices, Some(&a), &b, at(2, 10, 5)).is_empty());
    std::fs::write(
        dir.path().join("desktop.json"),
        r#"{"alerts":{"enabled":true,"thresholds":[60]}}"#,
    )
    .unwrap();
    let c = reading(at(2, 10, 10), 40.0, resets);
    let d = reading(at(2, 10, 15), 70.0, resets);
    observe(&mut notices, Some(&b), &c, at(2, 10, 10));
    assert_eq!(
        bells(&observe(&mut notices, Some(&c), &d, at(2, 10, 15))),
        1
    );
}

#[test]
fn quiet_hours_hold_the_banner_and_the_bell_until_one_digest() {
    let dir = Dir::new();
    std::fs::write(dir.path().join("desktop.json"), NIGHTS).unwrap();
    let mut notices = session(dir.path(), at(2, 22, 0), true);
    let resets = at(4, 12, 0);
    let before = reading(at(2, 22, 55), 70.0, resets);
    let crossed = reading(at(2, 23, 0), 92.0, resets);
    let out = observe(&mut notices, Some(&before), &crossed, at(2, 23, 0));
    assert!(out.is_empty(), "no BEL and no OSC 9 at night");
    assert_eq!(notices.banner(at(2, 23, 0)), None);
    let late = reading(at(3, 8, 0), 94.0, resets);
    let out = notices.tick(&config(), &[Some(&late)], at(3, 8, 0), "en-US");
    let text = String::from_utf8(out.clone()).unwrap();
    assert_eq!(bells(&out), 1);
    assert_eq!(text.matches("\x1b]9;").count(), 1);
    assert!(
        text.contains("1 usage alert held during quiet hours"),
        "{text}"
    );
    let banner = notices.banner(at(3, 8, 0)).unwrap();
    assert!(banner.contains("quiet hours"), "{banner}");
    // Not again.
    assert!(notices
        .tick(&config(), &[Some(&late)], at(3, 8, 30), "en-US")
        .is_empty());
}

#[test]
fn exhaustion_breaks_through_and_a_night_reset_is_never_announced() {
    let dir = Dir::new();
    std::fs::write(dir.path().join("desktop.json"), NIGHTS).unwrap();
    let mut notices = session(dir.path(), at(2, 22, 0), false);
    let resets = at(3, 3, 0);
    let before = reading(at(2, 22, 55), 70.0, resets);
    let exhausted = reading(at(2, 23, 0), 100.0, resets);
    assert_eq!(
        bells(&observe(
            &mut notices,
            Some(&before),
            &exhausted,
            at(2, 23, 0)
        )),
        1
    );
    // The window resets at 03:00 inside quiet hours: dropped for good.
    let used = reading(at(3, 2, 59), 40.0, at(3, 3, 0));
    assert!(notices
        .tick(&config(), &[Some(&used)], at(3, 3, 0), "en-US")
        .is_empty());
    assert!(notices
        .tick(&config(), &[Some(&used)], at(3, 8, 0), "en-US")
        .is_empty());
}

#[test]
fn two_terminals_and_the_shared_ledger_announce_each_event_once() {
    let dir = Dir::new();
    let resets = at(2, 20, 0);
    let before = reading(at(2, 10, 0), 70.0, resets);
    let crossed = reading(at(2, 10, 5), 92.0, resets);
    let mut first = session(dir.path(), at(2, 9, 0), false);
    let mut second = session(dir.path(), at(2, 9, 0), false);
    let a = observe(&mut first, Some(&before), &crossed, at(2, 10, 5));
    let b = observe(&mut second, Some(&before), &crossed, at(2, 10, 5));
    assert_eq!((bells(&a), bells(&b)), (1, 0));
    // And a reset notice at the reset.
    let due = reading(at(2, 19, 59), 40.0, at(2, 20, 0));
    let a = first.tick(&config(), &[Some(&due)], at(2, 20, 0), "en-US");
    let b = second.tick(&config(), &[Some(&due)], at(2, 20, 0), "en-US");
    assert_eq!((bells(&a), bells(&b)), (1, 0));
}

#[test]
fn osc9_texts_follow_the_private_switch() {
    let resets = at(2, 20, 0);
    let before = reading(at(2, 10, 0), 70.0, resets);
    let crossed = reading(at(2, 10, 5), 92.0, resets);
    for (json, names) in [(r#"{}"#, false), (r#"{"notify":{"private":false}}"#, true)] {
        let dir = Dir::new();
        std::fs::write(dir.path().join("desktop.json"), json).unwrap();
        let mut notices = session(dir.path(), at(2, 9, 0), true);
        let out = observe(&mut notices, Some(&before), &crossed, at(2, 10, 5));
        let text = String::from_utf8(out).unwrap();
        let osc = text.split("\x1b]9;").nth(1).expect("an OSC 9 notice");
        assert_eq!(osc.contains("Secret Corp"), names, "{osc}");
        assert!(osc.contains("90%"), "{osc}");
        // The header line is the terminal's own screen and keeps the name.
        assert!(notices
            .banner(at(2, 10, 5))
            .unwrap()
            .contains("Secret Corp"));
    }
}
