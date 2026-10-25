//! PRD 0032 RF3.2, RF8 and RF9: deferred marks, the "crossed while closed"
//! marker and recovery without `resets_at`.

use super::*;
use crate::model::{MetricKind, Provider};
use chrono::TimeZone;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 2, 15, 12, 0, 0).unwrap()
}

fn mins(n: i64) -> TimeDelta {
    TimeDelta::minutes(n)
}

fn window() -> DateTime<Utc> {
    t0() + TimeDelta::hours(3)
}

fn on() -> AlertPrefs {
    AlertPrefs::new(true, DEFAULT_THRESHOLDS).unwrap()
}

fn quota(used: f64, resets_at: Option<DateTime<Utc>>) -> Metric {
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
        resets_at,
    }
}

fn report(at: DateTime<Utc>, metric: Metric) -> Report {
    Report {
        as_of: Default::default(),
        overage_hint: Default::default(),
        sub_reports: Vec::new(),
        account_id: None,
        diagnostic: None,
        account: "Work".into(),
        provider: Provider::Claude,
        checked_at: at,
        status: Status::Ready,
        metrics: vec![metric],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
    }
}

type Fired = Vec<(AlertKind, u8)>;

/// One reading every 5 minutes, judged against the previous one and recorded.
fn run(used: &[f64], resets_at: Option<DateTime<Utc>>) -> Vec<Fired> {
    let prefs = on();
    let mut state = AlertState::default();
    let mut prev: Option<Report> = None;
    used.iter()
        .enumerate()
        .map(|(i, used)| {
            let next = report(t0() + mins(5 * i as i64), quota(*used, resets_at));
            let events = evaluate(prev.as_ref(), &next, &prefs, &state, next.checked_at);
            state.update(&next, &prefs, &events, next.checked_at);
            prev = Some(next);
            events.iter().map(|e| (e.kind, e.threshold)).collect()
        })
        .collect()
}

const X: AlertKind = AlertKind::Exhausted;
const R: AlertKind = AlertKind::Recovered;

#[test]
fn without_a_reset_date_a_20_point_drop_recovers_once() {
    let fired = run(&[50.0, 100.0, 85.0, 79.0, 78.0, 60.0], None);
    assert_eq!(fired[1], vec![(X, 100)]);
    assert_eq!(fired[2], vec![], "85% is not a recovery");
    assert_eq!(fired[3], vec![(R, 100)], "recovers at 79%");
    assert_eq!(fired[4], vec![]);
    assert_eq!(fired[5], vec![]);
}

#[test]
fn a_direct_fall_to_79_recovers_and_a_new_exhaustion_notifies_again() {
    let fired = run(&[50.0, 100.0, 79.0, 100.0], None);
    assert_eq!(fired[2], vec![(R, 100)]);
    assert_eq!(fired[3], vec![(X, 100)]);
}

#[test]
fn rising_back_before_the_drop_never_recovers_in_between() {
    // 100 -> 85 -> 100 re-arms the exhaustion (the 5-point hysteresis) and
    // never recovers: the metric did not fall 20 points.
    let fired = run(&[50.0, 100.0, 85.0, 100.0], None);
    assert_eq!(fired[2], vec![]);
    assert_eq!(fired[3], vec![(X, 100)]);
}

#[test]
fn a_metric_with_a_reset_date_keeps_the_window_instance_rule() {
    let next = window() + TimeDelta::hours(5);
    let prefs = on();
    let mut state = AlertState::default();
    let steps = [(50.0, window()), (100.0, window()), (30.0, next)];
    let mut prev: Option<Report> = None;
    let mut fired = Vec::new();
    for (i, (used, resets)) in steps.into_iter().enumerate() {
        let reading = report(t0() + mins(5 * i as i64), quota(used, Some(resets)));
        let events = evaluate(prev.as_ref(), &reading, &prefs, &state, reading.checked_at);
        state.update(&reading, &prefs, &events, reading.checked_at);
        fired.push(events.iter().map(|e| e.kind).collect::<Vec<_>>());
        prev = Some(reading);
    }
    assert_eq!(fired[1], vec![X]);
    assert_eq!(fired[2], vec![R], "a new instance recovers");
}

#[test]
fn lapsed_exhaustion_survives_a_round_trip() {
    let prefs = on();
    let mut state = AlertState::default();
    let mut prev: Option<Report> = None;
    for (i, used) in [50.0, 100.0, 85.0].into_iter().enumerate() {
        let next = report(t0() + mins(5 * i as i64), quota(used, None));
        let events = evaluate(prev.as_ref(), &next, &prefs, &state, next.checked_at);
        state.update(&next, &prefs, &events, next.checked_at);
        prev = Some(next);
    }
    let json = serde_json::to_string(&state).unwrap();
    let state: AlertState = serde_json::from_str(&json).unwrap();
    let next = report(t0() + mins(15), quota(70.0, None));
    let events = evaluate(prev.as_ref(), &next, &prefs, &state, next.checked_at);
    assert_eq!(events.iter().map(|e| e.kind).collect::<Vec<_>>(), vec![R]);
}

#[test]
fn closed_at_70_reopened_at_92_shows_the_marker_and_sends_nothing() {
    let prefs = on();
    let mut state = AlertState::default();
    let prev = report(t0(), quota(70.0, Some(window())));
    let next = report(t0() + TimeDelta::hours(2), quota(92.0, Some(window())));
    let now = next.checked_at;
    let events = evaluate(Some(&prev), &next, &prefs, &state, now);
    assert!(events.is_empty(), "the gap exceeds the crossing grace");
    state.update(&next, &prefs, &events, now);
    let missed = state.missed(Some(&prev), &next, &prefs);
    assert_eq!(missed.len(), 1);
    assert_eq!(missed[0].threshold, 90);
    assert_eq!(missed[0].label, "Session (5h)");
}

#[test]
fn a_new_window_or_a_notified_crossing_shows_no_marker() {
    let prefs = on();
    let mut state = AlertState::default();
    // A crossing notified while open.
    let before = report(t0(), quota(70.0, Some(window())));
    let crossed = report(t0() + mins(5), quota(92.0, Some(window())));
    let events = evaluate(Some(&before), &crossed, &prefs, &state, crossed.checked_at);
    assert_eq!(events.len(), 1);
    state.update(&crossed, &prefs, &events, crossed.checked_at);
    let later = report(t0() + mins(10), quota(93.0, Some(window())));
    assert!(state.missed(Some(&crossed), &later, &prefs).is_empty());
    // Reopened after the window reset at low usage.
    let fresh = report(
        t0() + TimeDelta::hours(9),
        quota(10.0, Some(window() + TimeDelta::hours(5))),
    );
    assert!(state.missed(Some(&later), &fresh, &prefs).is_empty());
    // Without any earlier reading nothing is known about a past.
    let empty = AlertState::default();
    assert!(empty.missed(None, &later, &prefs).is_empty());
}

#[test]
fn acknowledging_the_marker_records_it_as_seen() {
    let prefs = on();
    let mut state = AlertState::default();
    let prev = report(t0(), quota(70.0, Some(window())));
    let next = report(t0() + TimeDelta::hours(2), quota(92.0, Some(window())));
    state.update(&next, &prefs, &[], next.checked_at);
    let missed = state.missed(Some(&prev), &next, &prefs);
    assert_eq!(missed.len(), 1);
    assert!(state.acknowledge(&next.state_key(), &missed));
    assert!(state.missed(Some(&prev), &next, &prefs).is_empty());
    assert!(!state.acknowledge(&next.state_key(), &missed), "idempotent");
}

fn event_of(report: &Report, kind: AlertKind, threshold: u8) -> AlertEvent {
    AlertEvent {
        overage: Default::default(),
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

#[test]
fn a_deferred_crossing_survives_a_restart_and_leaves_in_the_digest() {
    let prefs = on();
    let mut state = AlertState::default();
    let until = t0() + TimeDelta::hours(9);
    let reading = report(t0(), quota(92.0, Some(window())));
    let held = event_of(&reading, AlertKind::Threshold, 90);
    state.update_with(&reading, &prefs, &[], &[(held, until)], t0());
    assert!(state.has_deferred());
    assert_eq!(state.next_deferred(), Some(until));
    // A restart in the middle of quiet hours.
    let json = serde_json::to_string(&state).unwrap();
    let mut state: AlertState = serde_json::from_str(&json).unwrap();
    let still = report(t0() + TimeDelta::hours(1), quota(93.0, Some(window())));
    assert!(state
        .take_deferred([&still], &prefs, true, t0() + mins(90))
        .is_empty());
    assert!(state.has_deferred(), "not due yet");
    let late = report(until, quota(94.0, Some(window())));
    let digest = state.take_deferred([&late], &prefs, true, until);
    assert_eq!(digest.len(), 1);
    assert_eq!(
        (digest[0].kind, digest[0].threshold),
        (AlertKind::Threshold, 90)
    );
    assert!(
        (digest[0].consumed - 0.94).abs() < 1e-9,
        "the current state"
    );
    assert!(!state.has_deferred());
    assert!(
        state.take_deferred([&late], &prefs, true, until).is_empty(),
        "once"
    );
}

#[test]
fn a_crossing_that_no_longer_holds_or_with_the_digest_off_is_dropped() {
    let prefs = on();
    let until = t0() + TimeDelta::hours(9);
    let reading = report(t0(), quota(92.0, Some(window())));
    let held = event_of(&reading, AlertKind::Threshold, 90);
    let build = || {
        let mut state = AlertState::default();
        state.update_with(&reading, &prefs, &[], &[(held.clone(), until)], t0());
        state
    };
    // The window reset at 05:00: a new instance below the level.
    let reset = report(until, quota(5.0, Some(window() + TimeDelta::hours(5))));
    assert!(build()
        .take_deferred([&reset], &prefs, true, until)
        .is_empty());
    // Fell below the level in the same instance.
    let fell = report(until, quota(60.0, Some(window())));
    assert!(build()
        .take_deferred([&fell], &prefs, true, until)
        .is_empty());
    // Digest off.
    let high = report(until, quota(95.0, Some(window())));
    let mut state = build();
    assert!(state
        .take_deferred([&high], &prefs, false, until)
        .is_empty());
    assert!(!state.has_deferred());
}

#[test]
fn the_digest_keeps_the_newest_state_of_each_metric() {
    let prefs = on();
    let until = t0() + TimeDelta::hours(9);
    let reading = report(t0(), quota(80.0, Some(window())));
    let mut state = AlertState::default();
    state.update_with(
        &reading,
        &prefs,
        &[],
        &[(event_of(&reading, AlertKind::Threshold, 75), until)],
        t0(),
    );
    let higher = report(t0() + mins(30), quota(92.0, Some(window())));
    state.update_with(
        &higher,
        &prefs,
        &[],
        &[(event_of(&higher, AlertKind::Threshold, 90), until)],
        t0() + mins(30),
    );
    let late = report(until, quota(93.0, Some(window())));
    let digest = state.take_deferred([&late], &prefs, true, until);
    assert_eq!(digest.len(), 1);
    assert_eq!(digest[0].threshold, 90);
}

fn account(name: &str) -> Account {
    Account {
        id: None,
        name: name.into(),
        provider: Provider::Claude,
        ..Account::default()
    }
}

#[test]
fn private_texts_carry_no_account_name_in_any_language() {
    let secret = account("Secret Corp");
    let reading = report(t0(), quota(92.0, Some(window())));
    let one = event_of(&reading, AlertKind::Threshold, 90);
    let exhausted = event_of(&reading, AlertKind::Exhausted, 100);
    let none_reset = AlertEvent {
        resets_at: None,
        ..one.clone()
    };
    let expiring = AlertEvent {
        kind: AlertKind::SessionExpiring,
        label: String::new(),
        ..one.clone()
    };
    let projected = AlertEvent {
        kind: AlertKind::ProjectedExhaustion,
        threshold: 0,
        projection: Some(Projection {
            projected_at: t0() + mins(40),
            lead_minutes: 60,
            margin_seconds: 3600,
            source: PaceSource::Recent,
        }),
        ..one.clone()
    };
    let cases: Vec<Vec<AlertEvent>> = vec![
        vec![one.clone()],
        vec![exhausted.clone()],
        vec![none_reset],
        vec![expiring],
        vec![projected],
        vec![one.clone(), exhausted.clone()],
    ];
    let mut with_names = 0;
    for events in cases {
        let public = notice_with(&[(&secret, events.clone())], t0(), &Utc, false).unwrap();
        let private = notice_with(&[(&secret, events.clone())], t0(), &Utc, true).unwrap();
        for locale in ["en-US", "es", "it", "pt-BR"] {
            let text = format!(
                "{} | {}",
                private.title.resolve(locale),
                private.body.resolve(locale)
            );
            assert!(!text.contains("Secret"), "{locale}: {text}");
            assert!(!text.contains("Session (5h)"), "{locale}: {text}");
            assert!(!text.contains('{'), "{locale}: {text}");
        }
        let text = format!(
            "{} | {}",
            public.title.resolve("en-US"),
            public.body.resolve("en-US")
        );
        with_names += usize::from(text.contains("Secret"));
    }
    assert!(with_names >= 5, "the public texts still name the account");
    // Two accounts.
    let other = account("Other Co");
    let two = notice_with(
        &[(&secret, vec![one.clone()]), (&other, vec![one.clone()])],
        t0(),
        &Utc,
        true,
    )
    .unwrap();
    let text = format!(
        "{} {}",
        two.title.resolve("en-US"),
        two.body.resolve("en-US")
    );
    assert!(
        !text.contains("Secret") && !text.contains("Other"),
        "{text}"
    );
}

#[test]
fn private_reset_notices_carry_no_account_or_metric_names() {
    use crate::resets::{ResetEvent, ResetMetric};
    let secret = account("Secret Corp");
    let event = ResetEvent {
        account_index: 0,
        account_key: "k".into(),
        metrics: vec![ResetMetric {
            label: "Session (5h)".into(),
            window: MetricWindow::Window5h,
            resets_at: t0(),
        }],
        resets_at: t0(),
        notify_at: t0(),
    };
    assert!(event
        .notice(&secret, t0())
        .body
        .resolve("en-US")
        .contains("Secret"));
    let private = event.notice_with(&secret, t0(), true);
    for locale in ["en-US", "es", "it", "pt-BR"] {
        let text = format!(
            "{} {}",
            private.title.resolve(locale),
            private.body.resolve(locale)
        );
        assert!(
            !text.contains("Secret") && !text.contains("Session"),
            "{locale}: {text}"
        );
    }
}

/// PRD 0032 RF3: a projection held by quiet hours is a deferred mark that
/// survives a restart and leaves in the digest while it still projects
/// before the reset.
#[test]
fn a_held_projection_survives_a_restart_and_is_rejudged_in_the_digest() {
    let prefs = AlertPrefs {
        projected: Some(Lead::Min60),
        ..on()
    };
    let until = t0() + TimeDelta::hours(1);
    let reading = report(t0(), quota(70.0, Some(window())));
    let projection = Projection {
        projected_at: t0() + TimeDelta::hours(2),
        lead_minutes: 60,
        margin_seconds: 3600,
        source: crate::pace::PaceSource::Recent,
    };
    let held = AlertEvent {
        projection: Some(projection),
        ..event_of(&reading, AlertKind::ProjectedExhaustion, 0)
    };
    let mut state = AlertState::default();
    state.update_with(&reading, &prefs, &[], &[(held.clone(), until)], t0());
    assert!(state.has_deferred());
    assert!(state.deferred(&held));
    let json = serde_json::to_string(&state).unwrap();
    let restored: AlertState = serde_json::from_str(&json).unwrap();
    // Still projecting before the reset: one digest line with it.
    let mut kept = restored.clone();
    let later = report(until, quota(80.0, Some(window())));
    let digest = kept.take_deferred([&later], &prefs, true, until);
    assert_eq!(digest.len(), 1);
    assert_eq!(digest[0].kind, AlertKind::ProjectedExhaustion);
    assert_eq!(
        digest[0].projection.map(|p| p.projected_at),
        Some(projection.projected_at)
    );
    // The projected time passed, or the metric is exhausted: dropped.
    let mut passed = restored.clone();
    let late = report(t0() + TimeDelta::hours(2), quota(90.0, Some(window())));
    assert!(passed
        .take_deferred([&late], &prefs, true, t0() + TimeDelta::hours(2))
        .is_empty());
    let mut exhausted = restored;
    let out = report(until, quota(100.0, Some(window())));
    assert!(exhausted
        .take_deferred([&out], &prefs, true, until)
        .is_empty());
    assert!(!exhausted.has_deferred());
}

fn overage_report(at: DateTime<Utc>, spent: f64) -> Report {
    let cycle = t0() + TimeDelta::days(10);
    let metric = Metric {
        billing: crate::Billing::Overage,
        scope: None,
        label: "Extra usage".into(),
        used: Some(spent),
        limit: Some(50.0),
        remaining: None,
        unit: "usd".into(),
        kind: MetricKind::Spend,
        window: None,
        period: None,
        resets_at: Some(cycle),
    };
    report(at, metric)
}

/// PRD 0032 RF3.1: a PRD 0023 overage notice held by quiet hours is a
/// deferred mark (not seen-without-notice) and leaves in the digest while
/// the account still bills in the same cycle.
#[test]
fn a_held_overage_notice_waits_for_the_digest() {
    let prefs = on();
    let until = t0() + TimeDelta::hours(8);
    let before = overage_report(t0() - mins(5), 0.0);
    let next = overage_report(t0(), 3.0);
    let state = AlertState::default();
    let events = evaluate(Some(&before), &next, &prefs, &state, t0());
    let started: Vec<_> = events
        .into_iter()
        .filter(|event| event.kind == AlertKind::OverageStarted)
        .collect();
    assert_eq!(started.len(), 1);
    let window = crate::quiet::QuietWindow { ends_at: until };
    assert_eq!(
        crate::quiet::decide(crate::quiet::Subject::Alert(&started[0]), Some(window)),
        crate::quiet::Decision::Defer(until)
    );
    let mut state = AlertState::default();
    let held = vec![(started[0].clone(), until)];
    state.update_with(&next, &prefs, &[], &held, t0());
    assert!(state.deferred(&started[0]));
    assert!(!state.notified(&started[0]));
    let json = serde_json::to_string(&state).unwrap();
    let mut state: AlertState = serde_json::from_str(&json).unwrap();
    // Not raised again while held.
    let again = overage_report(t0() + mins(5), 4.0);
    assert!(
        evaluate(Some(&next), &again, &prefs, &state, again.checked_at)
            .iter()
            .all(|event| event.kind != AlertKind::OverageStarted)
    );
    let late = overage_report(until, 6.0);
    let digest = state.take_deferred([&late], &prefs, true, until);
    assert_eq!(digest.len(), 1);
    assert_eq!(digest[0].kind, AlertKind::OverageStarted);
    assert_eq!(digest[0].overage.as_ref().and_then(|o| o.spent), Some(6.0));
    assert!(state.notified(&digest[0]));
}
