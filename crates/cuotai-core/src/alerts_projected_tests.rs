//! PRD 0024 RF2/RF3: projected-exhaustion alerts over synthetic series
//! with a fixed clock.

use super::*;
use crate::model::{MetricKind, Provider};
use crate::pace::Point;
use chrono::TimeZone;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 2, 15, 12, 0, 0).unwrap()
}

fn mins(n: i64) -> TimeDelta {
    TimeDelta::minutes(n)
}

/// The 5-hour window instance of most series: started at `t0 − 2h`.
fn window() -> DateTime<Utc> {
    t0() + TimeDelta::hours(3)
}

const INTERVAL: Duration = Duration::from_secs(300);

fn quota(used: f64, resets_at: DateTime<Utc>) -> Metric {
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
        resets_at: Some(resets_at),
    }
}

fn report(checked_at: DateTime<Utc>, metrics: Vec<Metric>) -> Report {
    Report {
        sub_reports: Vec::new(),
        diagnostic: Default::default(),
        account_id: None,
        account: "Work".into(),
        provider: Provider::Claude,
        checked_at,
        status: Status::Ready,
        metrics,
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
    }
}

fn prefs(lead: Lead) -> AlertPrefs {
    let mut prefs = AlertPrefs::new(true, DEFAULT_THRESHOLDS).unwrap();
    prefs.projected = Some(lead);
    prefs
}

/// One step: reading time, used %, the window's `resets_at`.
type Step = (DateTime<Utc>, f64, DateTime<Utc>);

/// Readings every 5 minutes from `start`, one per `used`, in `window()`.
fn every_5(start: DateTime<Utc>, used: &[f64]) -> Vec<Step> {
    used.iter()
        .enumerate()
        .map(|(i, used)| (start + mins(5 * i as i64), *used, window()))
        .collect()
}

/// Each reading evaluated at its own time against the previous one, with
/// every earlier reading as its recent points (none with `recent` off),
/// then recorded. Whether each reading fired a projection.
fn run(prefs: &AlertPrefs, state: &mut AlertState, steps: &[Step], recent: bool) -> Vec<bool> {
    let mut points = Vec::new();
    let mut prev: Option<Report> = None;
    steps
        .iter()
        .map(|(at, used, resets_at)| {
            let next = report(*at, vec![quota(*used, *resets_at)]);
            points.push(Point {
                taken_at: *at,
                used: *used,
            });
            let lists = vec![if recent { points.clone() } else { Vec::new() }];
            let input = PaceInput {
                interval: INTERVAL,
                recent: &lists,
            };
            let events = evaluate_with(prev.as_ref(), &next, prefs, state, *at, Some(&input));
            state.update(&next, prefs, &events, *at);
            prev = Some(next);
            events
                .iter()
                .any(|event| event.kind == AlertKind::ProjectedExhaustion)
        })
        .collect()
}

fn fired_at(fired: &[bool]) -> Vec<usize> {
    fired
        .iter()
        .enumerate()
        .filter_map(|(i, fired)| fired.then_some(i))
        .collect()
}

/// 1 point a minute from 40% at `t0 − 30 min`: at this rate the quota
/// runs out at `t0 + 30 min`, 2.5 hours before the reset.
fn steep() -> Vec<Step> {
    let used: Vec<f64> = (0..12).map(|i| 40.0 + 5.0 * f64::from(i)).collect();
    every_5(t0() - mins(30), &used)
}

#[test]
fn off_by_default_and_recommended_at_60_minutes() {
    assert_eq!(AlertPrefs::default().projected, None);
    let recommended = AlertPrefs::recommended();
    assert!(recommended.enabled);
    assert_eq!(recommended.projected, Some(Lead::Min60));
    let mut state = AlertState::default();
    let mut on = AlertPrefs::new(true, DEFAULT_THRESHOLDS).unwrap();
    assert!(fired_at(&run(&on, &mut state, &steep(), true)).is_empty());
    on.projected = Some(Lead::Min60);
    let event = AlertEvent {
        account_key: "claude:Work".into(),
        label: "x".into(),
        window: None,
        kind: AlertKind::ProjectedExhaustion,
        threshold: 0,
        consumed: 0.5,
        resets_at: None,
        projection: None,
    };
    assert!(on.wants(&event));
    on.muted_metrics
        .entry("claude:Work".into())
        .or_default()
        .insert("x".into());
    assert!(!on.wants(&event), "a muted metric mutes the projection");
    assert_eq!(Lead::from_minutes(30), Some(Lead::Min30));
    assert_eq!(Lead::from_minutes(45), None);
}

#[test]
fn a_steep_slope_fires_once_at_the_lead() {
    let mut state = AlertState::default();
    // Third reading: the first recent slope, but the previous reading only
    // had an average. Fourth: two consecutive confirmations.
    assert_eq!(
        fired_at(&run(&prefs(Lead::Min60), &mut state, &steep(), true)),
        vec![3]
    );
    // The event carries what it projects.
    let steps = steep();
    let mut state = AlertState::default();
    let mut prev = None;
    let mut found = None;
    let mut points = Vec::new();
    for (at, used, resets_at) in &steps[..4] {
        let next = report(*at, vec![quota(*used, *resets_at)]);
        points.push(Point {
            taken_at: *at,
            used: *used,
        });
        let lists = vec![points.clone()];
        let input = PaceInput {
            interval: INTERVAL,
            recent: &lists,
        };
        let events = evaluate_with(
            prev.as_ref(),
            &next,
            &prefs(Lead::Min60),
            &state,
            *at,
            Some(&input),
        );
        state.update(&next, &prefs(Lead::Min60), &events, *at);
        found = events
            .into_iter()
            .find(|e| e.kind == AlertKind::ProjectedExhaustion)
            .or(found);
        prev = Some(next);
    }
    let projection = found.unwrap().projection.unwrap();
    assert!((projection.projected_at - (t0() + mins(30))).abs() < TimeDelta::seconds(1));
    assert_eq!(projection.lead_minutes, 60);
    assert_eq!(projection.margin_seconds, 150 * 60);
    assert_eq!(projection.source, PaceSource::Recent);
}

#[test]
fn leads_of_15_30_and_60_minutes() {
    for (lead, index) in [(Lead::Min60, 3), (Lead::Min30, 6), (Lead::Min15, 9)] {
        let mut state = AlertState::default();
        assert_eq!(
            fired_at(&run(&prefs(lead), &mut state, &steep(), true)),
            vec![index],
            "{lead:?}"
        );
    }
}

#[test]
fn an_average_only_projection_never_fires() {
    let mut state = AlertState::default();
    assert!(fired_at(&run(&prefs(Lead::Min60), &mut state, &steep(), false)).is_empty());
    // Without any pace input, `evaluate` never projects either.
    let steps = steep();
    let prev = report(steps[3].0, vec![quota(steps[3].1, window())]);
    let next = report(steps[4].0, vec![quota(steps[4].1, window())]);
    let events = evaluate(
        Some(&prev),
        &next,
        &prefs(Lead::Min60),
        &AlertState::default(),
        steps[4].0,
    );
    assert!(events
        .iter()
        .all(|e| e.kind != AlertKind::ProjectedExhaustion));
}

#[test]
fn a_short_early_span_does_not_fire_until_a_fifth_of_the_window() {
    // Window started 40 minutes before `t0`: 3 samples within 10 minutes,
    // and a fourth at 15 minutes, all under 20% of the window.
    let young = |start_ago: i64| -> Vec<Step> {
        let resets_at = t0() - mins(start_ago) + TimeDelta::hours(5);
        [30.0, 40.0, 50.0, 60.0]
            .iter()
            .enumerate()
            .map(|(i, used)| (t0() - mins(10) + mins(5 * i as i64), *used, resets_at))
            .collect()
    };
    let mut state = AlertState::default();
    assert!(fired_at(&run(&prefs(Lead::Min60), &mut state, &young(40), true)).is_empty());
    // The same samples once 20% of the window elapsed (window 80 minutes
    // old at `t0`): the second recent reading confirms the first.
    let mut state = AlertState::default();
    assert_eq!(
        fired_at(&run(&prefs(Lead::Min60), &mut state, &young(80), true)),
        vec![3]
    );
}

#[test]
fn a_reading_that_is_not_fresh_never_fires() {
    let steps = steep();
    let lists = vec![steps[..5]
        .iter()
        .map(|(at, used, _)| Point {
            taken_at: *at,
            used: *used,
        })
        .collect::<Vec<_>>()];
    let input = PaceInput {
        interval: INTERVAL,
        recent: &lists,
    };
    let prev = report(steps[3].0, vec![quota(steps[3].1, window())]);
    let next = report(steps[4].0, vec![quota(steps[4].1, window())]);
    let fires = |now| {
        evaluate_with(
            Some(&prev),
            &next,
            &prefs(Lead::Min60),
            &AlertState::default(),
            now,
            Some(&input),
        )
        .iter()
        .any(|e| e.kind == AlertKind::ProjectedExhaustion)
    };
    assert!(fires(steps[4].0));
    // 7.5 minutes is the Fresh edge for a 5-minute interval (PRD 0010).
    assert!(fires(steps[4].0 + TimeDelta::seconds(450)));
    assert!(!fires(steps[4].0 + TimeDelta::seconds(451)), "aging");
}

#[test]
fn a_single_spike_does_not_fire() {
    let mut used = vec![30.0; 8];
    used.push(80.0);
    used.push(30.0);
    let mut state = AlertState::default();
    let steps = every_5(t0() - mins(30), &used);
    assert!(fired_at(&run(&prefs(Lead::Min60), &mut state, &steps, true)).is_empty());
}

#[test]
fn one_instance_despite_drift_and_a_new_window_rearms() {
    // `resets_at` drifting within 2 minutes stays one instance.
    let drifting: Vec<Step> = steep()
        .into_iter()
        .enumerate()
        .map(|(i, (at, used, resets_at))| {
            let drift = if i % 2 == 0 { 0 } else { 90 };
            (at, used, resets_at + TimeDelta::seconds(drift))
        })
        .collect();
    let mut state = AlertState::default();
    let prefs = prefs(Lead::Min60);
    assert_eq!(fired_at(&run(&prefs, &mut state, &drifting, true)), vec![3]);
    // The same steep use in the next window instance warns again.
    let later: Vec<Step> = steep()
        .into_iter()
        .map(|(at, used, resets_at)| {
            let shift = TimeDelta::hours(5);
            (at + shift, used, resets_at + shift)
        })
        .collect();
    assert_eq!(fired_at(&run(&prefs, &mut state, &later, true)), vec![3]);
    // A restart with the persisted marks does not send it again.
    let restored: AlertState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    let mut restored = restored;
    assert!(fired_at(&run(&prefs, &mut restored, &later, true)).is_empty());
}

#[test]
fn first_reading_rule() {
    let steps = steep();
    let points: Vec<Point> = steps[..5]
        .iter()
        .map(|(at, used, _)| Point {
            taken_at: *at,
            used: *used,
        })
        .collect();
    let lists = vec![points];
    let input = PaceInput {
        interval: INTERVAL,
        recent: &lists,
    };
    let next = report(steps[4].0, vec![quota(steps[4].1, window())]);
    let fires = |prev: &Report| {
        evaluate_with(
            Some(prev),
            &next,
            &prefs(Lead::Min60),
            &AlertState::default(),
            next.checked_at,
            Some(&input),
        )
        .iter()
        .any(|e| e.kind == AlertKind::ProjectedExhaustion)
    };
    // The recorded previous reading qualifies and is 5 minutes older.
    assert!(fires(&report(
        steps[3].0,
        vec![quota(steps[3].1, window())]
    )));
    // 15 minutes older: past the 10-minute grace of a first reading.
    assert!(!fires(&report(
        steps[1].0,
        vec![quota(steps[1].1, window())]
    )));
    // No previous reading at all.
    assert!(evaluate_with(
        None,
        &next,
        &prefs(Lead::Min60),
        &AlertState::default(),
        next.checked_at,
        Some(&input)
    )
    .is_empty());
}

fn pending(resets_at: DateTime<Utc>) -> AlertEvent {
    AlertEvent {
        account_key: crate::AccountId::derive("claude:Work", None).to_string(),
        label: "Session (5h)".into(),
        window: Some(MetricWindow::Window5h),
        kind: AlertKind::ProjectedExhaustion,
        threshold: 0,
        consumed: 0.7,
        resets_at: Some(resets_at),
        projection: None,
    }
}

#[test]
fn cancellation_of_a_queued_projection() {
    // 4 hours into the window, one hour to its reset; the average decides
    // (no recent points): 81% projects 3.7 minutes before the reset, 79%
    // lasts past it.
    let resets_at = t0() + mins(60);
    let none = vec![Vec::new()];
    let input = PaceInput {
        interval: INTERVAL,
        recent: &none,
    };
    let at = |used| report(t0(), vec![quota(used, resets_at)]);
    let event = pending(resets_at);
    // RF2.2.6: a margin under 5 minutes keeps it; reaching the reset drops it.
    assert!(!cancels(&event, &at(81.0), &input, t0()));
    assert!(cancels(&event, &at(79.0), &input, t0()));
    // Exhausted: the `Exhausted` alert supersedes it.
    assert!(cancels(&event, &at(100.0), &input, t0()));
    // No longer Fresh, failed, or the window reset.
    assert!(cancels(
        &event,
        &at(81.0),
        &input,
        t0() + TimeDelta::seconds(451)
    ));
    let mut failed = at(81.0);
    failed.status = Status::Error;
    assert!(cancels(&event, &failed, &input, t0()));
    let renewed = report(t0(), vec![quota(81.0, resets_at + TimeDelta::hours(5))]);
    assert!(cancels(&event, &renewed, &input, t0()));
    // Drift within 2 minutes is the same instance.
    let drifted = report(t0(), vec![quota(81.0, resets_at + mins(1))]);
    assert!(!cancels(&event, &drifted, &input, t0()));
    // Never another kind.
    let threshold = AlertEvent {
        kind: AlertKind::Threshold,
        ..pending(resets_at)
    };
    assert!(!cancels(&threshold, &at(100.0), &input, t0()));
}

#[test]
fn the_projection_comes_first_and_has_its_own_notice() {
    // At 75% the same reading also crosses the threshold.
    let steps = every_5(t0() - mins(30), &[60.0, 65.0, 70.0, 75.0]);
    let mut state = AlertState::default();
    let prefs = prefs(Lead::Min60);
    let mut prev = None;
    let mut points = Vec::new();
    let mut last = Vec::new();
    for (at, used, resets_at) in &steps {
        let next = report(*at, vec![quota(*used, *resets_at)]);
        points.push(Point {
            taken_at: *at,
            used: *used,
        });
        let lists = vec![points.clone()];
        let input = PaceInput {
            interval: INTERVAL,
            recent: &lists,
        };
        last = evaluate_with(prev.as_ref(), &next, &prefs, &state, *at, Some(&input));
        state.update(&next, &prefs, &last, *at);
        prev = Some(next);
    }
    let kinds: Vec<AlertKind> = last.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![AlertKind::ProjectedExhaustion, AlertKind::Threshold]
    );
    let account = Account {
        name: "Work".into(),
        provider: Provider::Claude,
        ..Account::default()
    };
    let now = steps[3].0;
    let one = notice(&[(&account, vec![last[0].clone()])], now, &Utc).unwrap();
    assert_eq!(
        one.title.resolve("en-US"),
        "Claude · Work: may run out before the reset"
    );
    // 25 left at 1 point a minute: 12:30, 2.5 hours before the reset.
    assert_eq!(
        one.body.resolve("en-US"),
        "At this rate Session (5h) runs out ≈12:10, 2h 50m before the reset"
    );
    assert_eq!(
        one.body.resolve("pt-BR"),
        "Neste ritmo Session (5h) acaba ≈12:10, 2h 50m antes da renovação"
    );
    for locale in ["es", "it"] {
        assert!(!one.title.resolve(locale).contains('{'), "{locale}");
        assert!(!one.body.resolve(locale).contains('{'), "{locale}");
    }
    let both = notice(&[(&account, last)], now, &Utc).unwrap();
    let body = both.body.resolve("en-US");
    assert!(
        body.starts_with("Work: Session (5h) runs out ≈12:10; Session (5h) 75%"),
        "{body}"
    );
}
