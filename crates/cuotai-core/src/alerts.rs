//! Usage threshold alerts (PRD 0008 RF1).
//!
//! [`evaluate`] is a pure function over the previous and the new reading of
//! one account, the user's [`AlertPrefs`], what [`AlertState`] remembers and
//! an injected `now`: no clock, no network, no extra query. It says which
//! notices a new reading deserves (a threshold crossed, the quota exhausted,
//! slack again in the next window); the caller delivers them and then calls
//! [`AlertState::update`] with the same reading, so a threshold fires once
//! per window instance and oscillating around it stays silent.

use crate::metrics::{classify_metric, MetricState};
use crate::model::{Metric, MetricKind, MetricWindow, Report, Status};
use crate::resets::{key_digest, DRIFT_TOLERANCE};
use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use std::collections::BTreeSet;
use std::ops::RangeInclusive;

/// Consumption thresholds the user may choose, in percent.
pub const THRESHOLD_RANGE: RangeInclusive<u8> = 10..=99;
/// Thresholds of a fresh preference set, in percent.
pub const DEFAULT_THRESHOLDS: [u8; 2] = [75, 90];
/// A notified threshold re-arms once consumption falls this many points
/// below it (or a new window instance starts).
pub const HYSTERESIS_POINTS: f64 = 5.0;
/// A reading older than this at `now` is not news and fires nothing. On
/// the first reading of an account in this process (opening the app), the
/// previous reading (e.g. the last one PRD 0006 recorded) must also be at
/// most this much older, so a crossing is only notified when it is known
/// to have happened within it.
pub const CROSSING_GRACE: TimeDelta = TimeDelta::minutes(10);
/// The exhaustion level: 100% consumed.
const EXHAUSTED: u8 = 100;
/// Absorbs `f64` rounding in `used × 100 / limit` (29% must reach 29).
const EPSILON: f64 = 1e-9;

/// A threshold outside [`THRESHOLD_RANGE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("o limite de alerta deve estar entre 10% e 99% de consumo (recebido: {0}%)")]
pub struct ThresholdError(pub u8);

/// What the user wants to be alerted about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertPrefs {
    /// Off: [`evaluate`] returns nothing.
    pub enabled: bool,
    thresholds: BTreeSet<u8>,
}

impl Default for AlertPrefs {
    /// Off, with the [`DEFAULT_THRESHOLDS`].
    fn default() -> Self {
        Self {
            enabled: false,
            thresholds: BTreeSet::from(DEFAULT_THRESHOLDS),
        }
    }
}

impl AlertPrefs {
    /// Every threshold must be in [`THRESHOLD_RANGE`]; repeats collapse. No
    /// threshold at all still notifies exhaustion, which is always
    /// evaluated while `enabled`.
    pub fn new(
        enabled: bool,
        thresholds: impl IntoIterator<Item = u8>,
    ) -> Result<Self, ThresholdError> {
        let thresholds = thresholds
            .into_iter()
            .map(|threshold| {
                THRESHOLD_RANGE
                    .contains(&threshold)
                    .then_some(threshold)
                    .ok_or(ThresholdError(threshold))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            enabled,
            thresholds,
        })
    }

    /// Ascending.
    pub fn thresholds(&self) -> impl DoubleEndedIterator<Item = u8> + '_ {
        self.thresholds.iter().copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// Consumption reached `threshold` percent.
    Threshold,
    /// The quota is consumed (`threshold` is 100).
    Exhausted,
    /// A metric whose exhaustion was notified has slack in a new window
    /// instance (`threshold` is 100, the level it recovered from).
    Recovered,
}

/// One notice about one metric of one account.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlertEvent {
    /// [`crate::Account::key`] of the account.
    pub account_key: String,
    pub label: String,
    pub window: Option<MetricWindow>,
    pub kind: AlertKind,
    /// Percent: the crossed threshold, or 100 for `Exhausted`/`Recovered`.
    pub threshold: u8,
    /// Consumed fraction of the cap in the new reading, `0.0..=1.0`.
    pub consumed: f64,
    /// The window instance of the reading, as the provider reported it.
    pub resets_at: Option<DateTime<Utc>>,
}

/// What earlier readings left behind: per account, metric, window instance
/// (`resets_at`) and level, whether the level was reached and whether it
/// was notified; recoveries already seen; and the accounts read in this
/// process. It keeps digests of the account key and metric label plus
/// thresholds and dates, never names or usage values.
#[derive(Debug, Clone, Default)]
pub struct AlertState {
    marks: Vec<Mark>,
    /// Account digests with a recent `Ready` reading since this state was
    /// created: their next reading is not a first reading. Per process,
    /// never meant to be persisted.
    seen: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct Mark {
    account: String,
    metric: String,
    /// `Threshold` and `Exhausted` mark a level reached in the instance;
    /// `Recovered`, the instance where a past exhaustion recovered.
    kind: AlertKind,
    threshold: u8,
    resets_at: Option<DateTime<Utc>>,
    notified: bool,
    at: DateTime<Utc>,
}

/// One eligible metric's consumption.
#[derive(Debug, Clone, Copy)]
struct Consumption {
    /// `0.0..=100.0`; exactly 100 when exhausted.
    percent: f64,
    exhausted: bool,
}

/// Notices `next` deserves after `prev` (the account's previous reading).
///
/// Nothing when alerts are off, `next` is not `Ready` or is older than
/// [`CROSSING_GRACE`] at `now`, or there is no usable `prev` (none, not
/// `Ready`, of another account, or not older than `next`): a first reading
/// never notifies a threshold crossed in the past. On the first reading of
/// the account in this state (opening the app), `prev` must be at most
/// [`CROSSING_GRACE`] older than `next`, and only levels `prev` had not
/// reached yet in the same window instance count as crossed.
///
/// Per eligible metric (a `Quota` with a positive finite cap; anything
/// without a cap and missing or non-finite values are skipped, never read
/// as zero), in report order: `Exhausted` once 100% is reached in a window
/// instance; otherwise `Recovered` when the metric's exhaustion was notified
/// in an earlier instance, and `Threshold` with the highest crossed
/// threshold above every level already reached in this instance. Instances
/// are told apart by `resets_at` beyond [`DRIFT_TOLERANCE`]; an unknown
/// `resets_at` is the same instance.
pub fn evaluate(
    prev: Option<&Report>,
    next: &Report,
    prefs: &AlertPrefs,
    state: &AlertState,
    now: DateTime<Utc>,
) -> Vec<AlertEvent> {
    if !prefs.enabled || next.status != Status::Ready || now - next.checked_at > CROSSING_GRACE {
        return Vec::new();
    }
    let Some(prev) = prev.filter(|prev| {
        prev.provider == next.provider
            && prev.account == next.account
            && prev.status == Status::Ready
            && prev.checked_at < next.checked_at
    }) else {
        return Vec::new();
    };
    let account_key = next.account_key();
    let first = !state.seen.contains(&account_digest(&account_key));
    if first && next.checked_at - prev.checked_at > CROSSING_GRACE {
        return Vec::new();
    }
    let mut events = Vec::new();
    for metric in &next.metrics {
        let Some(consumption) = consumption(metric) else {
            continue;
        };
        let digest = key_digest(&account_key, &metric.label);
        // A first reading has no levels in `state`: what `prev` already
        // reached was crossed before it, not within the grace.
        let known = first
            .then(|| {
                prev.metrics
                    .iter()
                    .filter(|before| {
                        before.label == metric.label
                            && same_instance(before.resets_at, metric.resets_at)
                    })
                    .filter_map(|before| level(self::consumption(before)?, prefs))
                    .max()
            })
            .flatten();
        let highest = state.highest(&digest, metric.resets_at).max(known);
        let event = |kind, threshold| AlertEvent {
            account_key: account_key.clone(),
            label: metric.label.clone(),
            window: metric.window,
            kind,
            threshold,
            consumed: consumption.percent / 100.0,
            resets_at: metric.resets_at,
        };
        if consumption.exhausted {
            if highest.is_none_or(|level| level < EXHAUSTED) {
                events.push(event(AlertKind::Exhausted, EXHAUSTED));
            }
            continue;
        }
        if state
            .awaiting_recovery(&digest, metric.resets_at)
            .is_some_and(|exhausted| exhausted.notified)
        {
            events.push(event(AlertKind::Recovered, EXHAUSTED));
        }
        let crossed = level(consumption, prefs);
        if let Some(threshold) = crossed.filter(|t| highest.is_none_or(|level| *t > level)) {
            events.push(event(AlertKind::Threshold, threshold));
        }
    }
    events
}

impl AlertState {
    /// Remember `next` after delivering the `events` [`evaluate`] returned
    /// for it (pass only the ones delivered): every level `next` reached
    /// in its window instance (with the thresholds of `prefs`), notified or
    /// not, so a crossing seen without a notice (first reading, alerts
    /// off) is not notified later; recoveries seen; the levels re-armed by
    /// falling [`HYSTERESIS_POINTS`] below them; and a `resets_at` drifting
    /// within [`DRIFT_TOLERANCE`], which follows the reading so gradual
    /// drift stays one instance. A recent `Ready` reading also makes the
    /// account's next reading a non-first one.
    pub fn update(
        &mut self,
        next: &Report,
        prefs: &AlertPrefs,
        events: &[AlertEvent],
        now: DateTime<Utc>,
    ) {
        if next.status != Status::Ready {
            return;
        }
        let account_key = next.account_key();
        let account = account_digest(&account_key);
        if now - next.checked_at <= CROSSING_GRACE {
            self.seen.insert(account.clone());
        }
        for metric in &next.metrics {
            let Some(consumption) = consumption(metric) else {
                continue;
            };
            let digest = key_digest(&account_key, &metric.label);
            let at = metric.resets_at;
            let notified = |kind: AlertKind, threshold: u8| {
                events.iter().any(|event| {
                    event.account_key == account_key
                        && event.label == metric.label
                        && event.kind == kind
                        && event.threshold == threshold
                })
            };
            self.marks.retain(|mark| {
                !(mark.metric == digest
                    && mark.kind != AlertKind::Recovered
                    && same_instance(mark.resets_at, at)
                    && consumption.percent
                        <= f64::from(mark.threshold) - HYSTERESIS_POINTS + EPSILON)
            });
            if let Some(at) = at {
                for mark in &mut self.marks {
                    if mark.metric == digest
                        && mark
                            .resets_at
                            .is_some_and(|resets_at| (resets_at - at).abs() <= DRIFT_TOLERANCE)
                    {
                        mark.resets_at = Some(at);
                    }
                }
            }
            let mark = |kind, threshold| Mark {
                account: account.clone(),
                metric: digest.clone(),
                kind,
                threshold,
                resets_at: at,
                notified: notified(kind, threshold),
                at: now,
            };
            if consumption.exhausted {
                if !self.has_level(&digest, at, EXHAUSTED) {
                    self.marks.push(mark(AlertKind::Exhausted, EXHAUSTED));
                }
            } else if self.awaiting_recovery(&digest, at).is_some() {
                self.marks.push(mark(AlertKind::Recovered, EXHAUSTED));
            }
            for threshold in prefs.thresholds() {
                if reached(consumption.percent, f64::from(threshold))
                    && !self.has_level(&digest, at, threshold)
                {
                    self.marks.push(mark(AlertKind::Threshold, threshold));
                }
            }
        }
    }

    /// Forget every account whose key is not in `keys` (removed from the
    /// configuration): its levels, recoveries and first-reading status.
    pub fn retain_accounts<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        let kept: BTreeSet<String> = keys.into_iter().map(account_digest).collect();
        self.marks.retain(|mark| kept.contains(&mark.account));
        self.seen.retain(|account| kept.contains(account));
    }

    /// Highest level reached in the instance of `resets_at`.
    fn highest(&self, metric: &str, resets_at: Option<DateTime<Utc>>) -> Option<u8> {
        self.levels(metric, resets_at)
            .map(|mark| mark.threshold)
            .max()
    }

    fn has_level(&self, metric: &str, resets_at: Option<DateTime<Utc>>, level: u8) -> bool {
        self.levels(metric, resets_at)
            .any(|mark| mark.threshold == level)
    }

    fn levels<'s>(
        &'s self,
        metric: &'s str,
        resets_at: Option<DateTime<Utc>>,
    ) -> impl Iterator<Item = &'s Mark> + 's {
        self.marks.iter().filter(move |mark| {
            mark.metric == metric
                && mark.kind != AlertKind::Recovered
                && same_instance(mark.resets_at, resets_at)
        })
    }

    /// The exhaustion a reading in the instance of `resets_at` recovers
    /// from: the metric's latest exhaustion or recovery is an exhaustion of
    /// an earlier, known instance.
    fn awaiting_recovery(&self, metric: &str, resets_at: Option<DateTime<Utc>>) -> Option<&Mark> {
        let resets_at = resets_at?;
        self.marks
            .iter()
            .filter(|mark| mark.metric == metric && mark.kind != AlertKind::Threshold)
            .max_by_key(|mark| mark.at)
            .filter(|mark| {
                mark.kind == AlertKind::Exhausted
                    && mark
                        .resets_at
                        .is_some_and(|exhausted| resets_at - exhausted > DRIFT_TOLERANCE)
            })
    }
}

fn account_digest(account_key: &str) -> String {
    crate::models::digest(account_key.as_bytes())
}

/// Two `resets_at` within [`DRIFT_TOLERANCE`] are one window instance; an
/// unknown one cannot tell instances apart.
fn same_instance(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= DRIFT_TOLERANCE,
        _ => true,
    }
}

fn reached(percent: f64, level: f64) -> bool {
    percent + EPSILON >= level
}

/// The highest level `consumption` reaches: 100 when exhausted, otherwise
/// the highest threshold of `prefs` it reached.
fn level(consumption: Consumption, prefs: &AlertPrefs) -> Option<u8> {
    if consumption.exhausted {
        return Some(EXHAUSTED);
    }
    prefs
        .thresholds()
        .rev()
        .find(|threshold| reached(consumption.percent, f64::from(*threshold)))
}

/// A `Quota` with a positive finite cap, available or exhausted. Balance,
/// credits, spend, usage, rate limits, uncapped and unmeasured metrics are
/// not eligible. Follows [`classify_metric`]: a finite `used` wins over
/// `remaining`.
fn consumption(metric: &Metric) -> Option<Consumption> {
    if metric.kind != MetricKind::Quota {
        return None;
    }
    let finite = |value: Option<f64>| value.filter(|value| value.is_finite());
    match classify_metric(metric) {
        MetricState::Exhausted { no_cap: false } => Some(Consumption {
            percent: 100.0,
            exhausted: true,
        }),
        MetricState::Available {
            uncapped: false, ..
        } => {
            let cap = finite(metric.limit)?;
            let used = finite(metric.used).or_else(|| finite(metric.remaining).map(|r| cap - r))?;
            Some(Consumption {
                percent: (used * 100.0 / cap).clamp(0.0, 100.0),
                exhausted: false,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 15, 12, 0, 0).unwrap()
    }

    fn mins(n: i64) -> TimeDelta {
        TimeDelta::minutes(n)
    }

    /// The window instance most readings belong to.
    fn window() -> DateTime<Utc> {
        t0() + TimeDelta::hours(3)
    }

    fn on() -> AlertPrefs {
        AlertPrefs::new(true, DEFAULT_THRESHOLDS).unwrap()
    }

    fn quota(used: Option<f64>, resets_at: DateTime<Utc>) -> Metric {
        Metric {
            label: "5h".into(),
            used,
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: None,
            resets_at: Some(resets_at),
        }
    }

    fn report(account: &str, checked_at: DateTime<Utc>, metrics: Vec<Metric>) -> Report {
        Report {
            account: account.into(),
            provider: Provider::Claude,
            checked_at,
            status: Status::Ready,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
        }
    }

    type Fired = Vec<(AlertKind, u8)>;

    fn fired(events: &[AlertEvent]) -> Fired {
        events.iter().map(|e| (e.kind, e.threshold)).collect()
    }

    /// One account read every 5 minutes, each reading evaluated against
    /// the previous one at its own time and then recorded.
    fn run(prefs: &AlertPrefs, state: &mut AlertState, readings: &[Report]) -> Vec<Fired> {
        let mut prev: Option<&Report> = None;
        readings
            .iter()
            .map(|next| {
                let events = evaluate(prev, next, prefs, state, next.checked_at);
                state.update(next, prefs, &events, next.checked_at);
                prev = Some(next);
                fired(&events)
            })
            .collect()
    }

    /// `(used %, resets_at)` per reading of the "5h" quota.
    fn series(steps: &[(Option<f64>, DateTime<Utc>)]) -> Vec<Report> {
        steps
            .iter()
            .enumerate()
            .map(|(i, (used, resets_at))| {
                report(
                    "Work",
                    t0() + mins(5 * i as i64),
                    vec![quota(*used, *resets_at)],
                )
            })
            .collect()
    }

    fn same_window(used: &[Option<f64>]) -> Vec<Report> {
        series(
            &used
                .iter()
                .map(|used| (*used, window()))
                .collect::<Vec<_>>(),
        )
    }

    const T: AlertKind = AlertKind::Threshold;
    const X: AlertKind = AlertKind::Exhausted;
    const R: AlertKind = AlertKind::Recovered;

    #[test]
    fn thresholds_must_be_between_10_and_99() {
        let defaults = AlertPrefs::default();
        assert!(!defaults.enabled, "off by default");
        assert_eq!(defaults.thresholds().collect::<Vec<_>>(), vec![75, 90]);
        assert_eq!(
            AlertPrefs::new(true, [99, 10, 10])
                .unwrap()
                .thresholds()
                .collect::<Vec<_>>(),
            vec![10, 99]
        );
        assert_eq!(AlertPrefs::new(true, [50, 9]), Err(ThresholdError(9)));
        assert_eq!(AlertPrefs::new(true, [100]), Err(ThresholdError(100)));
        assert!(AlertPrefs::new(true, []).is_ok(), "exhaustion only");
    }

    #[test]
    fn eligibility_table() {
        let with = |edit: fn(&mut Metric)| {
            let mut metric = quota(Some(95.0), window());
            edit(&mut metric);
            metric
        };
        let cases: Vec<(&str, Metric, Fired)> = vec![
            ("capped quota", with(|_| {}), vec![(T, 90)]),
            (
                "remaining against the cap",
                with(|m| {
                    m.used = None;
                    m.remaining = Some(5.0);
                }),
                vec![(T, 90)],
            ),
            (
                "exhausted with a cap",
                with(|m| m.used = Some(100.0)),
                vec![(X, 100)],
            ),
            ("no cap", with(|m| m.limit = None), vec![]),
            ("zero cap", with(|m| m.limit = Some(0.0)), vec![]),
            (
                "infinite cap",
                with(|m| m.limit = Some(f64::INFINITY)),
                vec![],
            ),
            (
                "exhausted without a cap",
                with(|m| {
                    m.used = None;
                    m.limit = None;
                    m.remaining = Some(0.0);
                }),
                vec![],
            ),
            ("missing value", with(|m| m.used = None), vec![]),
            (
                "non-finite value",
                with(|m| m.used = Some(f64::NAN)),
                vec![],
            ),
            ("balance", with(|m| m.kind = MetricKind::Balance), vec![]),
            ("credits", with(|m| m.kind = MetricKind::Credits), vec![]),
            ("spend", with(|m| m.kind = MetricKind::Spend), vec![]),
            ("usage", with(|m| m.kind = MetricKind::Usage), vec![]),
            (
                "rate limit",
                with(|m| m.kind = MetricKind::RateLimit),
                vec![],
            ),
            (
                "no window or reset still counts",
                with(|m| {
                    m.window = None;
                    m.resets_at = None;
                }),
                vec![(T, 90)],
            ),
        ];
        for (case, metric, expected) in cases {
            let mut before = metric.clone();
            before.used = Some(10.0);
            before.remaining = None;
            let readings = vec![
                report("Work", t0(), vec![before]),
                report("Work", t0() + mins(5), vec![metric]),
            ];
            let got = run(&on(), &mut AlertState::default(), &readings);
            assert_eq!(got, vec![vec![], expected], "{case}");
        }
    }

    #[test]
    fn sequence_table() {
        let some = |values: &[f64]| values.iter().map(|v| Some(*v)).collect::<Vec<_>>();
        let next = window() + TimeDelta::hours(5);
        let later = next + TimeDelta::hours(5);
        let cases: Vec<(&str, Vec<Report>, Vec<Fired>)> = vec![
            (
                "crossing one threshold",
                same_window(&some(&[50.0, 80.0, 85.0])),
                vec![vec![], vec![(T, 75)], vec![]],
            ),
            (
                "several thresholds in one query: only the highest",
                same_window(&some(&[50.0, 95.0, 97.0])),
                vec![vec![], vec![(T, 90)], vec![]],
            ),
            (
                "exhaustion beats every threshold",
                same_window(&some(&[50.0, 100.0])),
                vec![vec![], vec![(X, 100)]],
            ),
            (
                "threshold then exhaustion",
                same_window(&some(&[70.0, 80.0, 100.0, 100.0])),
                vec![vec![], vec![(T, 75)], vec![(X, 100)], vec![]],
            ),
            (
                "oscillation around the threshold",
                same_window(&some(&[70.0, 91.0, 89.0, 91.0, 86.0, 92.0])),
                vec![vec![], vec![(T, 90)], vec![], vec![], vec![], vec![]],
            ),
            (
                "re-armed by falling 5 points",
                same_window(&some(&[70.0, 91.0, 85.0, 91.0])),
                vec![vec![], vec![(T, 90)], vec![], vec![(T, 90)]],
            ),
            (
                "falling never notifies a lower threshold",
                same_window(&some(&[70.0, 95.0, 84.0, 80.0, 76.0])),
                vec![vec![], vec![(T, 90)], vec![], vec![], vec![]],
            ),
            (
                "exhaustion re-arms at 95",
                same_window(&some(&[90.0, 100.0, 96.0, 100.0, 95.0, 100.0])),
                vec![
                    vec![],
                    vec![(X, 100)],
                    vec![],
                    vec![],
                    vec![],
                    vec![(X, 100)],
                ],
            ),
            (
                "a missing value is skipped, not zero",
                same_window(&[Some(70.0), None, Some(91.0), None, Some(92.0)]),
                vec![vec![], vec![], vec![(T, 90)], vec![], vec![]],
            ),
            (
                "re-armed by a new window",
                series(&[
                    (Some(70.0), window()),
                    (Some(91.0), window()),
                    (Some(10.0), next),
                    (Some(92.0), next),
                ]),
                vec![vec![], vec![(T, 90)], vec![], vec![(T, 90)]],
            ),
            (
                "resets_at drifting up to 2 minutes is the same window",
                series(&[
                    (Some(70.0), window()),
                    (Some(91.0), window()),
                    (Some(91.0), window() + DRIFT_TOLERANCE),
                    (Some(92.0), window() + DRIFT_TOLERANCE + mins(2)),
                    (Some(93.0), window() + DRIFT_TOLERANCE + mins(3)),
                ]),
                vec![vec![], vec![(T, 90)], vec![], vec![], vec![]],
            ),
            (
                "resets_at jumping beyond 2 minutes is a new window",
                series(&[
                    (Some(70.0), window()),
                    (Some(91.0), window()),
                    (
                        Some(91.0),
                        window() + DRIFT_TOLERANCE + TimeDelta::seconds(1),
                    ),
                ]),
                vec![vec![], vec![(T, 90)], vec![(T, 90)]],
            ),
            (
                "recovered once in the next window",
                series(&[
                    (Some(90.0), window()),
                    (Some(100.0), window()),
                    (Some(5.0), next),
                    (Some(10.0), next),
                    (Some(100.0), next),
                    (Some(0.0), later),
                ]),
                vec![
                    vec![],
                    vec![(X, 100)],
                    vec![(R, 100)],
                    vec![],
                    vec![(X, 100)],
                    vec![(R, 100)],
                ],
            ),
            (
                "recovered into a window already past a threshold",
                series(&[
                    (Some(90.0), window()),
                    (Some(100.0), window()),
                    (Some(80.0), next),
                ]),
                vec![vec![], vec![(X, 100)], vec![(R, 100), (T, 75)]],
            ),
            (
                "no recovery without the exhaustion notice",
                series(&[
                    (Some(100.0), window()),
                    (Some(100.0), window()),
                    (Some(5.0), next),
                ]),
                vec![vec![], vec![], vec![]],
            ),
            (
                "slack in the same window is not a recovery",
                same_window(&some(&[90.0, 100.0, 50.0, 40.0])),
                vec![vec![], vec![(X, 100)], vec![], vec![]],
            ),
        ];
        for (case, readings, expected) in cases {
            let got = run(&on(), &mut AlertState::default(), &readings);
            assert_eq!(got, expected, "{case}");
        }
    }

    #[test]
    fn fractional_units_reach_their_threshold_despite_rounding() {
        // 0.29 × 100 is 28.999999999999996 in f64.
        let reading = |minutes, used| {
            let mut metric = quota(Some(used), window());
            metric.limit = Some(1.0);
            report("Work", t0() + mins(minutes), vec![metric])
        };
        let prefs = AlertPrefs::new(true, [29]).unwrap();
        let readings = [reading(0, 0.2), reading(5, 0.29)];
        let got = run(&prefs, &mut AlertState::default(), &readings);
        assert_eq!(got, vec![vec![], vec![(T, 29)]]);
    }

    #[test]
    fn event_carries_account_metric_window_fraction_and_reset() {
        let readings = same_window(&[Some(50.0), Some(91.0)]);
        let events = evaluate(
            Some(&readings[0]),
            &readings[1],
            &on(),
            &AlertState::default(),
            readings[1].checked_at,
        );
        assert_eq!(
            events,
            vec![AlertEvent {
                account_key: "claude:Work".into(),
                label: "5h".into(),
                window: Some(MetricWindow::Window5h),
                kind: AlertKind::Threshold,
                threshold: 90,
                consumed: 0.91,
                resets_at: Some(window()),
            }]
        );
    }

    #[test]
    fn first_reading_only_notifies_a_crossing_known_within_10_minutes() {
        let at = t0() + TimeDelta::hours(1);
        let reading =
            |checked_at, used| report("Work", checked_at, vec![quota(Some(used), window())]);
        let next = reading(at, 92.0);
        let fresh = AlertState::default();
        let eval = |prev: Option<&Report>, state: &AlertState, now| {
            fired(&evaluate(prev, &next, &on(), state, now))
        };
        assert_eq!(eval(None, &fresh, at), vec![], "no previous reading");
        // The last recorded reading (PRD 0006) right before opening the app.
        let recent = reading(at - CROSSING_GRACE, 70.0);
        assert_eq!(eval(Some(&recent), &fresh, at), vec![(T, 90)]);
        let old = reading(at - CROSSING_GRACE - TimeDelta::seconds(1), 70.0);
        assert_eq!(eval(Some(&old), &fresh, at), vec![], "crossed while closed");
        // A recent recorded reading already past 90: crossed before it.
        let past = reading(at - mins(5), 91.0);
        assert_eq!(eval(Some(&past), &fresh, at), vec![], "crossed before");
        // Only the levels the recorded reading had not reached yet.
        let between = reading(at - mins(5), 80.0);
        assert_eq!(eval(Some(&between), &fresh, at), vec![(T, 90)]);
        let mut other = reading(at - mins(5), 95.0);
        other.metrics[0].resets_at = Some(window() - TimeDelta::hours(5));
        assert_eq!(
            eval(Some(&other), &fresh, at),
            vec![(T, 90)],
            "another window instance"
        );
        // Within the session a long refresh interval still notifies.
        let mut session = AlertState::default();
        session.update(&old, &on(), &[], old.checked_at);
        assert_eq!(eval(Some(&old), &session, at), vec![(T, 90)]);
        // The reading itself must be new.
        assert_eq!(
            eval(Some(&recent), &fresh, at + CROSSING_GRACE),
            vec![(T, 90)]
        );
        assert_eq!(
            eval(
                Some(&recent),
                &fresh,
                at + CROSSING_GRACE + TimeDelta::seconds(1)
            ),
            vec![],
            "stale reading"
        );
        // Opening with nothing recorded: the crossing seen on the first
        // reading is never notified on the next ones.
        let readings = same_window(&[Some(92.0), Some(93.0), Some(89.0), Some(91.0)]);
        let got = run(&on(), &mut AlertState::default(), &readings);
        assert_eq!(got, vec![vec![], vec![], vec![], vec![]]);
    }

    #[test]
    fn alerts_off_never_fire_and_crossings_seen_meanwhile_stay_silent() {
        let readings = same_window(&[Some(50.0), Some(80.0), Some(92.0), Some(93.0), Some(100.0)]);
        let mut state = AlertState::default();
        let off = AlertPrefs::new(false, DEFAULT_THRESHOLDS).unwrap();
        assert_eq!(run(&off, &mut state, &readings[..3]), vec![vec![]; 3]);
        // Turned on above 90%: only what is crossed from now on.
        let events = |state: &AlertState, i: usize| {
            fired(&evaluate(
                Some(&readings[i - 1]),
                &readings[i],
                &on(),
                state,
                readings[i].checked_at,
            ))
        };
        assert_eq!(events(&state, 3), vec![]);
        state.update(&readings[3], &on(), &[], readings[3].checked_at);
        assert_eq!(events(&state, 4), vec![(X, 100)]);
    }

    #[test]
    fn removed_account_is_forgotten() {
        let prefs = on();
        let mut state = AlertState::default();
        let work = same_window(&[Some(70.0), Some(91.0)]);
        let home: Vec<Report> = same_window(&[Some(70.0), Some(91.0)])
            .into_iter()
            .map(|mut r| {
                r.account = "Home".into();
                r
            })
            .collect();
        assert_eq!(run(&prefs, &mut state, &work)[1], vec![(T, 90)]);
        assert_eq!(run(&prefs, &mut state, &home)[1], vec![(T, 90)]);
        // Another account's reading in the removed one's place is no prev.
        let late = t0() + mins(20);
        let next = report("Home", late, vec![quota(Some(95.0), next_window())]);
        assert_eq!(
            fired(&evaluate(Some(&work[1]), &next, &prefs, &state, late)),
            vec![]
        );

        state.retain_accounts(["claude:Home"]);
        let again = |state: &AlertState, account: &str, prev_used| {
            let prev = report(
                account,
                late - mins(5),
                vec![quota(Some(prev_used), window())],
            );
            let next = report(account, late, vec![quota(Some(92.0), window())]);
            fired(&evaluate(Some(&prev), &next, &prefs, state, late))
        };
        assert_eq!(
            again(&state, "Home", 89.0),
            vec![],
            "kept account remembers"
        );
        // Re-added under the same name: a first reading with nothing known.
        assert_eq!(again(&state, "Work", 89.0), vec![(T, 90)]);
        let old = report(
            "Work",
            late - TimeDelta::hours(1),
            vec![quota(Some(80.0), window())],
        );
        let next = report("Work", late, vec![quota(Some(92.0), window())]);
        assert_eq!(
            fired(&evaluate(Some(&old), &next, &prefs, &state, late)),
            vec![],
            "first reading again"
        );
    }

    fn next_window() -> DateTime<Utc> {
        window() + TimeDelta::hours(5)
    }
}
