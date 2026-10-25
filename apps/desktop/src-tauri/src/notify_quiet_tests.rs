//! PRD 0032: quiet hours, the digest, the shared ledger, private texts and
//! the "crossed while closed" marker, with a fake clock in UTC.

use super::*;
use chrono::TimeZone;
use cuotai_core::quiet::Interval;
use cuotai_core::{Account, MetricKind, MetricWindow, Provider, Status};
use std::cell::RefCell;

#[derive(Default)]
struct Fake(RefCell<Vec<(String, String)>>);

impl Notifier for Fake {
    fn deliver(&self, title: &str, body: &str) -> Result<(), String> {
        self.0.borrow_mut().push((title.into(), body.into()));
        Ok(())
    }
}

impl Fake {
    fn count(&self) -> usize {
        self.0.borrow().len()
    }

    fn text(&self, index: usize) -> String {
        let (title, body) = self.0.borrow()[index].clone();
        format!("{title} | {body}")
    }
}

/// Monday 2026-03-02 at `h:m` UTC (add days with `day`).
fn at(day: u32, h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, day, h, m, 0).unwrap()
}

fn config() -> Config {
    Config {
        accounts: vec![Account {
            id: None,
            name: "Work".into(),
            provider: Provider::Claude,
            ..Account::default()
        }],
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
        account: "Work".into(),
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

fn nights(digest: bool) -> QuietSchedule {
    QuietSchedule {
        enabled: true,
        days: std::array::from_fn(|_| vec![Interval::parse("22:00", "08:00").unwrap()]),
        digest,
    }
}

fn store(dir: &Path) -> Store {
    Store {
        settings: dir.join("desktop.json"),
        ledger: dir.join("notified.json"),
    }
}

/// A process: alerts and reset notices on, UTC quiet hours of 22:00–08:00.
fn process(dir: &Path, digest: bool, private: bool) -> Notices {
    let mut notices = Notices::open(Some(store(dir)));
    notices.zone = chrono::FixedOffset::east_opt(0);
    notices
        .set_prefs(NotifyPrefs {
            enabled: true,
            private,
            ..NotifyPrefs::default()
        })
        .unwrap();
    notices.switch_alerts("granted").unwrap();
    notices.set_quiet(nights(digest)).unwrap();
    notices
}

/// What the backend does with a new reading, then the timer's pass.
fn observe(notices: &mut Notices, prev: Option<&Report>, next: &Report, fake: &Fake) -> Tick {
    let now = next.checked_at;
    let events = notices.assess(prev, next, now, None);
    notices.record(prev, next, events, now, None);
    notices.tick(&config(), &[Some(next)], now, fake)
}

#[test]
fn a_crossing_at_23_is_one_digest_at_08_across_both_processes() {
    let dir = tempfile::tempdir().unwrap();
    let resets = at(4, 12, 0);
    let (mut desktop, mut tui) = (process(dir.path(), true, false), {
        let mut other = Notices::open(Some(store(dir.path())));
        other.zone = chrono::FixedOffset::east_opt(0);
        other
    });
    let fake = Fake::default();
    let before = reading(at(2, 22, 55), 70.0, resets);
    let crossed = reading(at(2, 23, 0), 92.0, resets);
    // Both processes judge the same reading while quiet hours hold it.
    observe(&mut desktop, Some(&before), &crossed, &fake);
    observe(&mut tui, Some(&before), &crossed, &fake);
    assert_eq!(fake.count(), 0, "nothing at 23:00");
    // Still silent at 07:59; the pass is due again when the window ends.
    let early = reading(at(3, 7, 59), 93.0, resets);
    let tick = desktop.tick(&config(), &[Some(&early)], early.checked_at, &fake);
    assert_eq!((fake.count(), tick.next), (0, Some(at(3, 8, 0))));
    // 08:00: the digest leaves once, whichever process gets there first.
    let late = reading(at(3, 8, 0), 94.0, resets);
    assert_eq!(
        tui.tick(&config(), &[Some(&late)], late.checked_at, &fake)
            .delivered,
        1
    );
    assert_eq!(
        desktop
            .tick(&config(), &[Some(&late)], late.checked_at, &fake)
            .delivered,
        0
    );
    assert_eq!(fake.count(), 1);
    let text = fake.text(0);
    assert!(
        text.contains("1 usage alerts held during quiet hours"),
        "{text}"
    );
    assert!(text.contains("90%") && text.contains("Work"), "{text}");
    // And never again.
    let later = reading(at(3, 8, 30), 95.0, resets);
    desktop.tick(&config(), &[Some(&later)], later.checked_at, &fake);
    assert_eq!(fake.count(), 1);
}

#[test]
fn exhaustion_at_23_breaks_through_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let resets = at(3, 3, 0);
    let mut notices = process(dir.path(), true, false);
    let fake = Fake::default();
    let before = reading(at(2, 22, 55), 70.0, resets);
    let exhausted = reading(at(2, 23, 0), 100.0, resets);
    let tick = observe(&mut notices, Some(&before), &exhausted, &fake);
    assert_eq!((tick.delivered, fake.count()), (1, 1));
    assert!(fake.text(0).contains("exhausted") || fake.text(0).contains("esgotada"));
}

#[test]
fn a_reset_at_03_is_never_notified_and_one_after_the_window_still_is() {
    let dir = tempfile::tempdir().unwrap();
    let mut notices = process(dir.path(), true, false);
    let fake = Fake::default();
    let inside = reading(at(3, 2, 0), 40.0, at(3, 3, 0));
    let t = |h, m| at(3, h, m);
    notices.tick(&config(), &[Some(&inside)], t(3, 0), &fake);
    assert_eq!(fake.count(), 0, "dropped, the window is quiet");
    // At the end of the window the same reset is already settled.
    notices.tick(&config(), &[Some(&inside)], t(8, 0), &fake);
    assert_eq!(fake.count(), 0, "never sent late");
    let ledger = std::fs::read_to_string(dir.path().join("notified.json")).unwrap();
    assert!(ledger.contains("\"skipped\":true"), "{ledger}");
    // A reset at 08:05 is after the window: notified at the reset.
    let after = reading(at(3, 7, 0), 40.0, t(8, 5));
    notices.tick(&config(), &[Some(&after)], t(8, 5), &fake);
    assert_eq!(fake.count(), 1);
}

#[test]
fn a_crossing_whose_window_resets_before_the_end_is_not_in_the_digest() {
    let dir = tempfile::tempdir().unwrap();
    let mut notices = process(dir.path(), true, false);
    let fake = Fake::default();
    let resets = at(3, 5, 0);
    let before = reading(at(2, 22, 55), 70.0, resets);
    let crossed = reading(at(2, 23, 0), 92.0, resets);
    observe(&mut notices, Some(&before), &crossed, &fake);
    // At 08:00 the 5-hour window is a new instance, barely used.
    let fresh = reading(at(3, 8, 0), 3.0, at(3, 13, 0));
    notices.tick(&config(), &[Some(&fresh)], fresh.checked_at, &fake);
    assert_eq!(fake.count(), 0);
}

#[test]
fn a_restart_during_quiet_hours_keeps_the_deferred_crossing() {
    let dir = tempfile::tempdir().unwrap();
    let resets = at(4, 12, 0);
    let fake = Fake::default();
    {
        let mut notices = process(dir.path(), true, false);
        let before = reading(at(2, 22, 55), 70.0, resets);
        let crossed = reading(at(2, 23, 0), 92.0, resets);
        observe(&mut notices, Some(&before), &crossed, &fake);
    }
    // 02:00: the app starts again; its first reading must not resend or
    // forget the held crossing.
    let mut notices = Notices::open(Some(store(dir.path())));
    notices.zone = chrono::FixedOffset::east_opt(0);
    notices.switch_alerts("granted").unwrap();
    let night = reading(at(3, 2, 0), 93.0, resets);
    notices.tick(&config(), &[Some(&night)], night.checked_at, &fake);
    assert_eq!(fake.count(), 0);
    let late = reading(at(3, 8, 0), 93.0, resets);
    assert_eq!(
        notices
            .tick(&config(), &[Some(&late)], late.checked_at, &fake)
            .delivered,
        1
    );
}

#[test]
fn without_the_digest_the_held_crossing_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let resets = at(4, 12, 0);
    let mut notices = process(dir.path(), false, false);
    let fake = Fake::default();
    let before = reading(at(2, 22, 55), 70.0, resets);
    let crossed = reading(at(2, 23, 0), 92.0, resets);
    observe(&mut notices, Some(&before), &crossed, &fake);
    let late = reading(at(3, 8, 0), 94.0, resets);
    notices.tick(&config(), &[Some(&late)], late.checked_at, &fake);
    notices.tick(&config(), &[Some(&late)], at(3, 9, 0), &fake);
    assert_eq!(fake.count(), 0);
}

#[test]
fn texts_are_private_by_default_and_the_switch_brings_names_back() {
    for (private, names) in [(true, false), (false, true)] {
        let dir = tempfile::tempdir().unwrap();
        let mut notices = process(dir.path(), true, private);
        notices.set_quiet(QuietSchedule::default()).unwrap();
        let fake = Fake::default();
        let resets = at(2, 20, 0);
        let before = reading(at(2, 14, 55), 70.0, resets);
        let crossed = reading(at(2, 15, 0), 92.0, resets);
        observe(&mut notices, Some(&before), &crossed, &fake);
        assert_eq!(fake.count(), 1);
        let text = fake.text(0);
        assert_eq!(text.contains("Work"), names, "{text}");
        assert!(text.contains("90"), "{text}");
    }
    assert!(NotifyPrefs::default().private, "on by default");
}

#[test]
fn a_crossing_while_closed_leaves_a_marker_and_no_notice_until_acknowledged() {
    let dir = tempfile::tempdir().unwrap();
    let resets = at(2, 20, 0);
    let mut notices = Notices::open(Some(store(dir.path())));
    notices.switch_alerts("granted").unwrap();
    let fake = Fake::default();
    // Closed at 70%, reopened two hours later at 92%.
    let before = reading(at(2, 12, 0), 70.0, resets);
    let reopened = reading(at(2, 14, 0), 92.0, resets);
    let tick = observe(&mut notices, Some(&before), &reopened, &fake);
    assert_eq!(tick.delivered, 0);
    assert_eq!(fake.count(), 0, "no notice for a stale crossing");
    let key = reopened.state_key();
    assert_eq!(notices.missed(&key).len(), 1);
    assert_eq!(notices.missed(&key)[0].threshold, 90);
    // A restart still shows it, until the person opens the details.
    let mut again = Notices::open(Some(store(dir.path())));
    again.switch_alerts("granted").unwrap();
    let next = reading(at(2, 14, 5), 93.0, resets);
    let events = again.assess(Some(&reopened), &next, next.checked_at, None);
    again.record(Some(&reopened), &next, events, next.checked_at, None);
    assert_eq!(again.missed(&key).len(), 1);
    again.acknowledge_missed(&key);
    assert!(again.missed(&key).is_empty());
    let mut third = Notices::open(Some(store(dir.path())));
    third.switch_alerts("granted").unwrap();
    let last = reading(at(2, 14, 10), 94.0, resets);
    let events = third.assess(Some(&next), &last, last.checked_at, None);
    third.record(Some(&next), &last, events, last.checked_at, None);
    assert!(third.missed(&key).is_empty(), "recorded as seen");
}

#[test]
fn the_quiet_hours_round_trip_through_desktop_json() {
    let dir = tempfile::tempdir().unwrap();
    let mut notices = Notices::open(Some(store(dir.path())));
    notices.set_quiet(nights(false)).unwrap();
    let again = Notices::open(Some(store(dir.path())));
    assert_eq!(again.quiet(), &nights(false));
    notices.set_quiet(QuietSchedule::default()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("desktop.json")).unwrap();
    assert!(
        !text.contains("quiet"),
        "the default removes the key: {text}"
    );
}
