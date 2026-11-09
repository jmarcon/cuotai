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
        derived_from: None,
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
        breakdown: Vec::new(),
    }
}

type Fired = Vec<(AlertKind, u8)>;

/// One reading every 5 minutes, judged against the previous one and recorded.
fn run(used: &[f64], resets_at: Option<DateTime<Utc>>) -> Vec<Fired> {
    run_every(5, used, resets_at)
}

/// One reading every `step` minutes.
fn run_every(step: i64, used: &[f64], resets_at: Option<DateTime<Utc>>) -> Vec<Fired> {
    let prefs = on();
    let mut state = AlertState::default();
    let mut prev: Option<Report> = None;
    used.iter()
        .enumerate()
        .map(|(i, used)| {
            let next = report(t0() + mins(step * i as i64), quota(*used, resets_at));
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
    // PRD 0008 re-validation (S-03): once the 5-hour window's half (2 h 30)
    // has passed since the first exhaustion notice.
    let fired = run_every(90, &[50.0, 100.0, 79.0, 100.0], None);
    assert_eq!(fired[2], vec![(R, 100)]);
    assert_eq!(fired[3], vec![(X, 100)]);
    // Within it, the flapping stays silent.
    let fired = run(&[50.0, 100.0, 79.0, 100.0], None);
    assert_eq!(fired[2], vec![(R, 100)]);
    assert_eq!(fired[3], vec![]);
}

#[test]
fn rising_back_before_the_drop_never_recovers_in_between() {
    // 100 -> 85 -> 100 re-arms the exhaustion (the 5-point hysteresis) and
    // never recovers: the metric did not fall 20 points. PRD 0008
    // re-validation (S-03): told again only after half the window.
    let fired = run_every(90, &[50.0, 100.0, 85.0, 100.0], None);
    assert_eq!(fired[2], vec![]);
    assert_eq!(fired[3], vec![(X, 100)]);
    assert_eq!(run(&[50.0, 100.0, 85.0, 100.0], None)[3], vec![]);
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
        limit_change: None,
        row: None,
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

/// PRD 0008 re-validation (S-01, S-02): the overage, pool and windowless
/// notices honour private texts too, in every language: no account name,
/// no amount, no pool or metric label, in the notice and in the quiet-hours
/// digest that reuses its title.
#[test]
fn private_overage_pool_and_windowless_texts_carry_no_names_or_amounts() {
    use crate::overage::{OverageExport, OverageState};
    let secret = account("Secret Corp");
    let reading = report(t0(), quota(92.0, Some(window())));
    let base = event_of(&reading, AlertKind::Threshold, 90);
    let spent = OverageExport {
        state: OverageState::Active,
        spent: Some(1234.56),
        unit: Some("USD".into()),
        currency: Some("USD".into()),
        cap: Some(5000.0),
        cycle_ends_at: Some(window()),
    };
    let overage = |kind| AlertEvent {
        kind,
        label: "Extra".into(),
        window: Some(MetricWindow::Monthly),
        overage: Some(spent.clone()),
        ..base.clone()
    };
    let pool = |kind| AlertEvent {
        kind,
        label: "Hidden Pool".into(),
        window: None,
        ..base.clone()
    };
    let windowless = |kind, threshold, window| AlertEvent {
        kind,
        threshold,
        label: "Hidden Metric".into(),
        window,
        ..base.clone()
    };
    let mut cases = vec![
        overage(AlertKind::OverageStarted),
        overage(AlertKind::OverageCapReached),
        AlertEvent {
            resets_at: None,
            ..overage(AlertKind::OverageStarted)
        },
        pool(AlertKind::PoolExhausted),
        pool(AlertKind::PoolRecovered),
        AlertEvent {
            resets_at: None,
            ..pool(AlertKind::PoolExhausted)
        },
    ];
    for window in [None, Some(MetricWindow::Other)] {
        cases.push(windowless(AlertKind::Threshold, 90, window));
        cases.push(windowless(AlertKind::Exhausted, 100, window));
        cases.push(windowless(AlertKind::Recovered, 100, window));
    }
    let forbidden = ["Secret", "1234", "1,234", "1.234", "Hidden", "Extra"];
    for event in cases {
        let public = notice_with(&[(&secret, vec![event.clone()])], t0(), &Utc, false).unwrap();
        let public = format!(
            "{} | {}",
            public.title.resolve("en-US"),
            public.body.resolve("en-US")
        );
        assert!(
            public.contains("Secret") || public.contains("Hidden"),
            "the public text names the account or the label: {public}"
        );
        let private = notice_with(&[(&secret, vec![event.clone()])], t0(), &Utc, true).unwrap();
        let digest =
            crate::quiet::digest(&[(&secret, vec![event.clone()])], t0(), &Utc, true).unwrap();
        for locale in crate::text::LOCALES.iter().map(|row| row.code) {
            let text = format!(
                "{} | {} | {} | {}",
                private.title.resolve(locale),
                private.body.resolve(locale),
                digest.title.resolve(locale),
                digest.body.resolve(locale)
            );
            for word in forbidden {
                assert!(!text.contains(word), "{locale} {:?}: {text}", event.kind);
            }
            assert!(!text.contains('{'), "{locale} {:?}: {text}", event.kind);
        }
    }
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
        derived_from: None,
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

/// DS-CMP-NOTIF-1 (PRD 0008 re-validation V-07): with a short provider name,
/// every locale's alert title fits 50 characters and the body 110, for one
/// event, for one account's four windows and for several accounts.
#[test]
fn alert_notices_fit_the_notification_length_budget() {
    let work = account("Work");
    let home = account("Home");
    let reading = report(t0(), quota(92.0, Some(window())));
    let base = event_of(&reading, AlertKind::Threshold, 75);
    let labels = [
        ("Sessão (5 horas) (%)", MetricWindow::Window5h),
        ("Semana (7 dias) (%)", MetricWindow::Weekly),
        ("Semana — Opus (7 dias) (%)", MetricWindow::Weekly),
        ("Semana — Sonnet (7 dias) (%)", MetricWindow::Weekly),
    ];
    let four: Vec<AlertEvent> = labels
        .iter()
        .map(|(label, window)| AlertEvent {
            label: (*label).into(),
            window: Some(*window),
            ..base.clone()
        })
        .collect();
    let later = t0() + TimeDelta::days(9);
    let mut singles = Vec::new();
    for window in [
        MetricWindow::Window5h,
        MetricWindow::Daily,
        MetricWindow::Weekly,
        MetricWindow::Monthly,
    ] {
        for (kind, threshold) in [
            (AlertKind::Threshold, 90),
            (AlertKind::Exhausted, 100),
            (AlertKind::Recovered, 100),
        ] {
            for resets_at in [Some(window_at(window)), Some(later), None] {
                singles.push(vec![AlertEvent {
                    kind,
                    threshold,
                    window: Some(window),
                    label: "Sessão (5 horas) (%)".into(),
                    resets_at,
                    ..base.clone()
                }]);
            }
        }
    }
    let mut over = Vec::new();
    for locale in crate::text::LOCALES.iter().map(|row| row.code) {
        for private in [false, true] {
            let mut cases: Vec<Notice> = singles
                .iter()
                .map(|events| notice_with(&[(&work, events.clone())], t0(), &Utc, private).unwrap())
                .collect();
            cases.push(notice_with(&[(&work, four.clone())], t0(), &Utc, private).unwrap());
            cases.push(
                notice_with(
                    &[(&work, four.clone()), (&home, four[..2].to_vec())],
                    t0(),
                    &Utc,
                    private,
                )
                .unwrap(),
            );
            for notice in cases {
                let title = notice.title.resolve(locale);
                let body = notice.body.resolve(locale);
                if title.chars().count() > 50 {
                    over.push(format!("{locale} title: {title}"));
                }
                if body.chars().count() > 110 {
                    over.push(format!("{locale} body: {body}"));
                }
            }
        }
    }
    over.sort();
    over.dedup();
    assert!(over.is_empty(), "{over:#?}");
}

fn window_at(window: MetricWindow) -> DateTime<Utc> {
    match window {
        MetricWindow::Window5h => t0() + TimeDelta::hours(3),
        _ => t0() + TimeDelta::hours(5),
    }
}

/// PRD 0008 re-validation V-08 and V-09: an unnamed account is named after
/// its provider, and a reset on another day reads as a weekday or a short
/// date in the reader's order, never an ISO date.
#[test]
fn unnamed_accounts_and_later_resets_read_naturally() {
    let unnamed = account("");
    let reading = report(t0(), quota(92.0, Some(window())));
    let one = event_of(&reading, AlertKind::Threshold, 90);
    let body = notice_with(&[(&unnamed, vec![one.clone()])], t0(), &Utc, false)
        .unwrap()
        .body
        .resolve("en-US");
    assert!(body.starts_with("Claude: "), "{body}");
    let tomorrow = AlertEvent {
        resets_at: Some(t0() + TimeDelta::days(1)),
        ..one.clone()
    };
    let far = AlertEvent {
        resets_at: Some(t0() + TimeDelta::days(9)),
        ..one.clone()
    };
    // 2026-02-15 is a Sunday.
    let at = |event: &AlertEvent, locale| {
        notice_with(
            &[(&account("Work"), vec![event.clone()])],
            t0(),
            &Utc,
            false,
        )
        .unwrap()
        .body
        .resolve(locale)
    };
    assert!(
        at(&tomorrow, "en-US").ends_with("resets Monday 12:00"),
        "{}",
        at(&tomorrow, "en-US")
    );
    assert!(
        at(&far, "en-US").ends_with("resets 02/24 12:00"),
        "{}",
        at(&far, "en-US")
    );
    assert!(
        at(&far, "pt-BR").ends_with("24/02 12:00"),
        "{}",
        at(&far, "pt-BR")
    );
    for locale in crate::text::LOCALES.iter().map(|row| row.code) {
        for event in [&tomorrow, &far] {
            let text = at(event, locale);
            assert!(!text.contains("2026-"), "{locale}: {text}");
            assert!(!text.contains('{'), "{locale}: {text}");
        }
    }
}
