use super::*;
use crate::config_file::ConfigIdentity;
use crate::history::HistoryStore;
use crate::model::{Account, Billing, Provider, Status};
use chrono::{TimeDelta, TimeZone};
use std::path::Path;

fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, hour, minute, 0)
        .unwrap()
}

fn quota(label: &str, used: f64, limit: Option<f64>, resets_at: Option<DateTime<Utc>>) -> Metric {
    Metric {
        label: label.into(),
        used: Some(used),
        limit,
        remaining: None,
        unit: "pontos".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Weekly),
        period: None,
        resets_at,
        billing: Billing::Included,
        scope: None,
        derived_from: None,
    }
}

fn report(metrics: Vec<Metric>, checked_at: DateTime<Utc>) -> Report {
    Report {
        account: "Work".into(),
        account_id: None,
        provider: Provider::Claude,
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
    }
}

// ---- RF2.1 limit changes ----------------------------------------------------

#[test]
fn limit_changes_detect_increases_and_decreases_beyond_the_threshold() {
    let resets = Some(at(25, 0, 0));
    let before = report(
        vec![quota("Semanal", 40.0, Some(100.0), resets)],
        at(20, 10, 0),
    );
    let up = report(
        vec![quota("Semanal", 41.0, Some(150.0), resets)],
        at(20, 10, 5),
    );
    let changes = limit_changes(&before, &up);
    assert_eq!(changes.len(), 1);
    assert_eq!((changes[0].from, changes[0].to), (100.0, 150.0));
    assert_eq!(changes[0].observed_at, at(20, 10, 5));
    let down = report(
        vec![quota("Semanal", 41.0, Some(80.0), resets)],
        at(20, 10, 5),
    );
    assert_eq!(limit_changes(&before, &down)[0].to, 80.0);
    // Rounding: 0.4% or less than one whole unit is not a change.
    let noise = report(
        vec![quota("Semanal", 41.0, Some(100.4), resets)],
        at(20, 10, 5),
    );
    assert!(limit_changes(&before, &noise).is_empty());
    assert!(is_limit_change(1000.0, 1006.0));
    assert!(!is_limit_change(100.0, 100.0));
    assert!(!is_limit_change(10.0, 10.04));
}

#[test]
fn a_reset_alone_or_a_missing_limit_is_not_a_change() {
    let before = report(
        vec![quota("Semanal", 90.0, Some(100.0), Some(at(20, 0, 0)))],
        at(19, 23, 0),
    );
    // The renewed window is collapsed (no limit) until it reports one.
    let collapsed = report(
        vec![quota("Semanal", 0.0, None, Some(at(27, 0, 0)))],
        at(20, 0, 5),
    );
    assert!(limit_changes(&before, &collapsed).is_empty());
    // The new window with the same limit: no change; with another: one.
    let same = report(
        vec![quota("Semanal", 1.0, Some(100.0), Some(at(27, 0, 0)))],
        at(20, 0, 10),
    );
    assert!(limit_changes(&before, &same).is_empty());
    let more = report(
        vec![quota("Semanal", 1.0, Some(150.0), Some(at(27, 0, 0)))],
        at(20, 0, 10),
    );
    assert_eq!(limit_changes(&before, &more).len(), 1);
    // A budget metric is the person's number, never a provider change.
    let mut budget = quota("Orçamento · Gasto", 1.0, Some(50.0), None);
    budget.derived_from = Some(crate::model::DerivedFrom {
        kind: MetricKind::Spend,
        label: "Gasto".into(),
        budget: 0,
    });
    let mut edited = budget.clone();
    edited.limit = Some(80.0);
    assert!(limit_changes(
        &report(vec![budget], at(20, 1, 0)),
        &report(vec![edited], at(20, 1, 5))
    )
    .is_empty());
}

#[test]
fn limit_changes_alert_only_when_opted_in_and_become_events() {
    use crate::alerts::{evaluate, notice_with, AlertKind, AlertPrefs, AlertState};
    let resets = Some(at(25, 0, 0));
    let before = report(
        vec![quota("Semanal", 40.0, Some(100.0), resets)],
        at(20, 10, 0),
    );
    let after = report(
        vec![quota("Semanal", 41.0, Some(150.0), resets)],
        at(20, 10, 5),
    );
    let mut prefs = AlertPrefs::new(true, crate::alerts::DEFAULT_THRESHOLDS).unwrap();
    let mut state = AlertState::default();
    state.update(&before, &prefs, &[], at(20, 10, 0));
    assert!(
        evaluate(Some(&before), &after, &prefs, &state, at(20, 10, 5)).is_empty(),
        "off by default"
    );
    prefs.limit_changes = true;
    let events = evaluate(Some(&before), &after, &prefs, &state, at(20, 10, 5));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, AlertKind::LimitChanged);
    let change = events[0].limit_change.as_ref().unwrap();
    assert_eq!((change.from, change.to), (100.0, 150.0));
    let account = Account {
        name: "Work".into(),
        provider: Provider::Claude,
        ..Default::default()
    };
    let private = notice_with(&[(&account, events.clone())], at(20, 10, 5), &Utc, true).unwrap();
    assert_eq!(private.title.resolve("en"), "Claude: a limit changed");
    assert!(!private.body.resolve("en").contains("Work"));
    let open = notice_with(&[(&account, events)], at(20, 10, 5), &Utc, false).unwrap();
    assert_eq!(open.body.resolve("en"), "Work: Semanal 100 → 150");
}

#[test]
fn the_card_note_names_both_values_and_lasts_48_hours() {
    let change = LimitChange {
        label: "Semanal".into(),
        label_text: crate::metric_text::metric_text("Semanal"),
        window: Some(MetricWindow::Weekly),
        from: 100.0,
        to: 150.0,
        unit: "pontos".into(),
        observed_at: at(25, 9, 0),
    };
    let note = change.note(|at| at.format("%b %d").to_string());
    assert_eq!(
        note.resolve("en"),
        "Weekly limit changed 100 → 150 pontos on Oct 25"
    );
    assert!(change.shows_at(at(26, 9, 0)));
    assert!(!change.shows_at(at(27, 9, 1)));
}

// ---- history-backed reads ----------------------------------------------------

fn store(dir: &Path) -> HistoryStore {
    HistoryStore::new(
        dir.join("history.sqlite3"),
        &ConfigIdentity::Explicit("insights".into()),
    )
}

fn account(name: &str) -> Account {
    Account {
        name: name.into(),
        provider: Provider::Claude,
        ..Default::default()
    }
}

#[test]
fn recorded_limit_changes_come_back_from_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(dir.path());
    let work = account("Work");
    let resets = Some(at(25, 0, 0));
    for (minute, limit) in [(0, 100.0), (20, 100.0), (40, 150.0)] {
        history
            .record(
                &work,
                at(20, 10, minute),
                &[quota(
                    "Semanal",
                    40.0 + f64::from(minute),
                    Some(limit),
                    resets,
                )],
            )
            .unwrap();
    }
    let changes = history
        .limit_changes(&work, at(20, 0, 0)..=at(21, 0, 0))
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!((changes[0].from, changes[0].to), (100.0, 150.0));
    assert_eq!(changes[0].observed_at, at(20, 10, 40));
    // A span after the change sees nothing new; the sample before the span
    // still counts as the starting point.
    assert!(history
        .limit_changes(&work, at(20, 11, 0)..=at(21, 0, 0))
        .unwrap()
        .is_empty());
    assert_eq!(
        history
            .limit_changes(&work, at(20, 10, 30)..=at(21, 0, 0))
            .unwrap()
            .len(),
        1
    );
}

fn record_series(history: &HistoryStore, account: &Account, points: &[(DateTime<Utc>, f64)]) {
    for (when, used) in points {
        history
            .record(
                account,
                *when,
                &[quota("Semanal", *used, Some(200.0), Some(at(30, 0, 0)))],
            )
            .unwrap();
    }
}

#[test]
fn rhythm_24h_buckets_consumption_marks_resets_and_leaves_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(dir.path());
    let work = account("Work");
    // 10:00 → 10:40: +20 (10 points of a 200 limit); 11:00 reset to 2;
    // 12:00 +8; nothing between 13:00 and the current hour.
    record_series(
        &history,
        &work,
        &[
            (at(20, 10, 0), 50.0),
            (at(20, 10, 20), 60.0),
            (at(20, 10, 40), 70.0),
            (at(20, 11, 5), 2.0),
            (at(20, 12, 10), 10.0),
        ],
    );
    let query = RhythmQuery {
        scope: None,
        metric: None,
        range: RhythmRange::Last24h,
        days_off: DaysOff::default(),
    };
    let rhythm = rhythm(&history, &[work], &query, at(20, 15, 30), &Utc).unwrap();
    assert_eq!(rhythm.buckets.len(), 24);
    let hour = |h: u8| rhythm.buckets.iter().find(|b| b.hour == Some(h)).unwrap();
    assert_eq!(hour(10).consumption, Some(10.0));
    assert_eq!(hour(10).samples, 3);
    assert_eq!(hour(11).consumption, Some(0.0));
    assert!(hour(11).reset);
    assert_eq!(hour(12).consumption, Some(4.0));
    assert_eq!(hour(13).consumption, None, "a gap, never zero");
    assert_eq!(rhythm.buckets.last().unwrap().hour, Some(15));
    assert_eq!(rhythm.metrics, ["Semanal"]);
    let text = rhythm.text_chart("en", 10);
    assert!(text.contains("13               ·"), "{text}");
    assert!(text.contains("↺"));
}

#[test]
fn rhythm_7d_marks_days_off_and_typical_needs_three_days() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(dir.path());
    let work = account("Work");
    let mut points = Vec::new();
    // Five days with +10 at 09:xx; 2026-10-17 and -18 are a weekend.
    for day in 15..=19 {
        points.push((at(day, 9, 0), f64::from(day - 15) * 20.0));
        points.push((at(day, 9, 30), f64::from(day - 15) * 20.0 + 10.0));
    }
    record_series(&history, &work, &points);
    let week = RhythmQuery {
        scope: Some(0),
        metric: None,
        range: RhythmRange::Last7d,
        days_off: DaysOff::default(),
    };
    let rhythm = rhythm(
        &history,
        std::slice::from_ref(&work),
        &week,
        at(20, 12, 0),
        &Utc,
    )
    .unwrap();
    assert_eq!(rhythm.buckets.len(), 7);
    let day = |d: u32| {
        rhythm
            .buckets
            .iter()
            .find(|b| b.day == chrono::NaiveDate::from_ymd_opt(2026, 10, d))
            .unwrap()
    };
    assert!(day(17).day_off && day(18).day_off && !day(19).day_off);
    assert_eq!(day(19).consumption, Some(10.0));
    assert_eq!(day(20).consumption, None);

    let typical = RhythmQuery {
        range: RhythmRange::Typical30d,
        ..week
    };
    let profile = super::rhythm(&history, &[work], &typical, at(20, 12, 0), &Utc).unwrap();
    assert_eq!(profile.buckets.len(), 24);
    let nine = &profile.buckets[9];
    assert_eq!(nine.samples, 5);
    // Points of a 200 limit: the first day +5, the next four +10 each
    // (the 09:00 jump from the day before plus the 09:30 one): 9 on average.
    assert!((nine.consumption.unwrap() - 9.0).abs() < 1e-9);
    assert_eq!(profile.buckets[3].consumption, None, "no data at 03:00");
}

#[test]
fn rhythm_hours_follow_the_local_zone_across_dst() {
    use chrono_tz::America::New_York;
    let dir = tempfile::tempdir().unwrap();
    let history = store(dir.path());
    let work = account("Work");
    // New York falls back on 2026-11-01 at 02:00: the hour 01 happens twice.
    let first = Utc.with_ymd_and_hms(2026, 11, 1, 5, 10, 0).unwrap(); // 01:10 EDT
    let second = Utc.with_ymd_and_hms(2026, 11, 1, 6, 10, 0).unwrap(); // 01:10 EST
    record_series(
        &history,
        &work,
        &[
            (first - TimeDelta::minutes(5), 0.0),
            (first, 20.0),
            (second, 40.0),
        ],
    );
    let query = RhythmQuery {
        scope: None,
        metric: None,
        range: RhythmRange::Last24h,
        days_off: DaysOff::default(),
    };
    let rhythm = rhythm(
        &history,
        &[work],
        &query,
        second + TimeDelta::minutes(30),
        &New_York,
    )
    .unwrap();
    let ones: Vec<_> = rhythm
        .buckets
        .iter()
        .filter(|b| b.hour == Some(1))
        .collect();
    assert_eq!(ones.len(), 2, "both 01:00 hours are their own bar");
    assert_eq!(ones[0].consumption, Some(10.0));
    assert_eq!(ones[1].consumption, Some(10.0));
}

#[test]
fn days_off_and_digest_prefs_read_leniently() {
    use serde_json::json;
    assert_eq!(DaysOff::default().slugs(), ["sat", "sun"]);
    let custom = DaysOff::from_value(&json!(["fri", "sun", "x", 3]));
    assert_eq!(custom.slugs(), ["fri", "sun"]);
    assert!(custom.contains(chrono::Weekday::Fri));
    assert_eq!(DaysOff::from_value(&json!("sat")), DaysOff::default());
    assert_eq!(serde_json::to_value(custom).unwrap(), json!(["fri", "sun"]));
    let digest = DigestPrefs::from_value(&json!({"enabled": true, "weekday": 9, "hour": 7}));
    assert_eq!((digest.enabled, digest.weekday, digest.hour), (true, 0, 7));
    let prefs = crate::notify_prefs::NotifyPrefs::from_value(
        &json!({"days_off": ["mon"], "limit_changes": true, "digest": {"enabled": true}}),
    );
    assert!(prefs.limit_changes && prefs.digest.enabled);
    assert_eq!(prefs.days_off.slugs(), ["mon"]);
    let loaded = crate::notify_prefs::Loaded {
        notify: prefs,
        ..Default::default()
    };
    assert!(loaded.evaluator().limit_changes);
}
