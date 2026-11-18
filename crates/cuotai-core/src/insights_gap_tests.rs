//! PRD 0043 re-validation (V-02, R-02, R-06): use across a gap in the
//! readings, the first sample of a span and the typical profile's hours.

use super::*;
use crate::config_file::ConfigIdentity;
use crate::history::HistoryStore;
use crate::model::{Account, Billing, Provider};
use chrono::TimeZone;

fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, hour, minute, 0)
        .unwrap()
}

fn store(dir: &std::path::Path) -> HistoryStore {
    HistoryStore::new(
        dir.join("history.sqlite3"),
        &ConfigIdentity::Explicit("insights-gap".into()),
    )
}

fn work() -> Account {
    Account {
        name: "Work".into(),
        provider: Provider::Claude,
        ..Default::default()
    }
}

/// Records `used` of a 200 limit at each time.
fn record(history: &HistoryStore, account: &Account, points: &[(DateTime<Utc>, f64)]) {
    for (when, used) in points {
        let metric = Metric {
            label: "Semanal".into(),
            used: Some(*used),
            limit: Some(200.0),
            remaining: None,
            unit: "pontos".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Weekly),
            period: None,
            resets_at: Some(at(30, 0, 0)),
            billing: Billing::Included,
            scope: None,
            derived_from: None,
            unlimited: false,
        };
        history.record(account, *when, &[metric]).unwrap();
    }
}

/// Readings every 30 minutes from 08:30 to 10:30 (+2 each), nothing from
/// 11:00 to 13:59, then a jump of +40 at 14:00 and +2 at 14:30.
fn gapped(history: &HistoryStore, account: &Account) {
    record(
        history,
        account,
        &[
            (at(20, 8, 30), 10.0),
            (at(20, 9, 0), 12.0),
            (at(20, 9, 30), 14.0),
            (at(20, 10, 0), 16.0),
            (at(20, 10, 30), 18.0),
            (at(20, 14, 0), 58.0),
            (at(20, 14, 30), 60.0),
        ],
    );
}

#[test]
fn a_gap_in_the_readings_is_not_a_spike() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(dir.path());
    let work = work();
    gapped(&history, &work);
    let query = RhythmQuery {
        scope: None,
        metric: None,
        range: RhythmRange::Last24h,
        days_off: DaysOff::default(),
    };
    // The span starts at 09:00 on the 20th: the 08:30 reading is only the
    // starting point of the 09:00 one (R-06).
    let rhythm = rhythm(&history, &[work], &query, at(21, 8, 30), &Utc).unwrap();
    use chrono::Datelike;
    let hour = |h: u8| {
        rhythm
            .buckets
            .iter()
            .find(|b| b.hour == Some(h) && b.start.day() == 20)
            .unwrap()
    };
    assert_eq!(hour(9).consumption, Some(2.0), "both 09:xx steps count");
    assert_eq!(hour(10).consumption, Some(2.0));
    for gap in 11..=13 {
        assert_eq!(hour(gap).consumption, None, "{gap}:00 is a gap, never zero");
    }
    // Only the 14:30 step: the +40 across the gap goes to no bucket.
    assert_eq!(hour(14).consumption, Some(1.0));
    assert_eq!(hour(14).samples, 2);
    assert!(!hour(14).reset);
}

#[test]
fn the_digest_hours_skip_a_gap_in_the_readings() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(dir.path());
    let work = work();
    gapped(&history, &work);
    let accounts = [work];
    let hours = hour_totals(&history, &accounts, None, at(20, 0, 0), at(21, 0, 0), &Utc).unwrap();
    assert_eq!(hours[9], 2.0);
    assert_eq!(hours[10], 2.0);
    assert_eq!(hours[14], 1.0, "the jump across the gap is not counted");
    assert_eq!(hours.iter().sum::<f64>(), 5.0);
    // A span starting at 09:15 still counts the 09:30 step from the 09:00
    // reading before it (R-06), and nothing earlier.
    let hours = hour_totals(&history, &accounts, None, at(20, 9, 15), at(21, 0, 0), &Utc).unwrap();
    assert_eq!(hours[9], 1.0);
    assert_eq!(hours[8], 0.0);
}

#[test]
fn typical_hours_follow_the_index_across_dst() {
    use chrono_tz::Europe::Berlin;
    let dir = tempfile::tempdir().unwrap();
    let history = store(dir.path());
    let query = RhythmQuery {
        scope: None,
        metric: None,
        range: RhythmRange::Typical30d,
        days_off: DaysOff::default(),
    };
    // Berlin springs forward on 2026-03-29 at 02:00: 02:xx does not exist.
    let now = Utc.with_ymd_and_hms(2026, 3, 29, 12, 0, 0).unwrap();
    let profile = rhythm(&history, &[work()], &query, now, &Berlin).unwrap();
    let hours: Vec<_> = profile.buckets.iter().map(|b| b.hour).collect();
    assert_eq!(hours, (0..=23u8).map(Some).collect::<Vec<_>>());
    assert_eq!(profile.buckets[3].label.resolve("de"), "03");
    // 03:00 CEST is 01:00 UTC; 01:00 CET is 00:00 UTC.
    assert_eq!(
        profile.buckets[3].start,
        Utc.with_ymd_and_hms(2026, 3, 29, 1, 0, 0).unwrap()
    );
    assert_eq!(
        profile.buckets[1].start,
        Utc.with_ymd_and_hms(2026, 3, 29, 0, 0, 0).unwrap()
    );
}
