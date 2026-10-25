//! Usage threshold alerts (PRD 0008 RF1, RF2).
//!
//! [`evaluate`] is a pure function over the previous and the new reading of
//! one account, the user's [`AlertPrefs`], what [`AlertState`] remembers and
//! an injected `now`: no clock, no network, no extra query. It says which
//! notices a new reading deserves (a threshold crossed, the quota exhausted,
//! slack again in the next window); the caller delivers them and then calls
//! [`AlertState::update`] with the same reading, so a threshold fires once
//! per window instance and oscillating around it stays silent. The state
//! persists in `notified.json` beside the reset notices
//! ([`crate::resets::Ledger::alerts`]) and [`notice`] writes the text of
//! one notification.

use crate::metrics::{classify_metric, window_param, MetricState};
use crate::model::{Account, Metric, MetricWindow, Report, Status};
use crate::resets::{key_digest, Notice, DRIFT_TOLERANCE, LEDGER_KEEP};
use crate::text::{params, provider_l10n, Localized};
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
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
/// At most one usage alert notification per this span (PRD 0008 §6): the
/// alerts that arrive sooner wait and join the next one.
pub const NOTICE_SPACING: TimeDelta = TimeDelta::minutes(1);
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
    /// Notify `Exhausted` (on by default). Off, a reading reaching 100%
    /// notifies the highest threshold it crossed instead.
    pub exhaustion: bool,
    /// Notify `Recovered` (on by default).
    pub recovery: bool,
    /// [`crate::Account::key`]s whose alerts are muted.
    pub muted_accounts: BTreeSet<String>,
    /// Muted metrics: account key → metric labels.
    pub muted_metrics: BTreeMap<String, BTreeSet<String>>,
}

impl Default for AlertPrefs {
    /// Off, with the [`DEFAULT_THRESHOLDS`], exhaustion and recovery
    /// notices and nothing muted.
    fn default() -> Self {
        Self {
            enabled: false,
            thresholds: BTreeSet::from(DEFAULT_THRESHOLDS),
            exhaustion: true,
            recovery: true,
            muted_accounts: BTreeSet::new(),
            muted_metrics: BTreeMap::new(),
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
            ..Self::default()
        })
    }

    /// Ascending.
    pub fn thresholds(&self) -> impl DoubleEndedIterator<Item = u8> + '_ {
        self.thresholds.iter().copied()
    }

    /// Whether `event` reaches the user: alerts on, its account and metric
    /// not muted and its kind wanted.
    pub fn wants(&self, event: &AlertEvent) -> bool {
        self.enabled
            && !self.muted_accounts.contains(&event.account_key)
            && !self
                .muted_metrics
                .get(&event.account_key)
                .is_some_and(|labels| labels.contains(&event.label))
            && match event.kind {
                AlertKind::Threshold | AlertKind::SessionExpiring => true,
                AlertKind::Exhausted => self.exhaustion,
                AlertKind::Recovered => self.recovery,
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// Consumption reached `threshold` percent.
    Threshold,
    /// The quota is consumed (`threshold` is 100).
    Exhausted,
    /// A metric whose exhaustion was notified has slack in a new window
    /// instance (`threshold` is 100, the level it recovered from).
    Recovered,
    /// PRD 0016 RF3.2: the web session the account's query used ends
    /// within [`crate::user_session::EXPIRY_WARNING`]. No metric: `label`
    /// is empty, `threshold` the whole days left and `resets_at` the
    /// session's expiry.
    SessionExpiring,
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
/// thresholds and dates, never names or usage values. Serialized (into
/// `notified.json`), it carries only the marks: the first-reading set
/// starts empty in every process.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertState {
    #[serde(default)]
    marks: Vec<Mark>,
    /// Account digests with a recent `Ready` reading since this state was
    /// created: their next reading is not a first reading. Per process,
    /// never persisted.
    #[serde(skip)]
    seen: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
/// instance (with `prefs.exhaustion` off, the highest threshold instead);
/// otherwise `Recovered` when the metric's exhaustion was notified in an
/// earlier instance, and `Threshold` with the highest crossed threshold
/// above every level already reached in this instance. Instances are told
/// apart by `resets_at` beyond [`DRIFT_TOLERANCE`]; an unknown `resets_at`
/// is the same instance. Only what [`AlertPrefs::wants`] is returned (a
/// muted account or metric, or an unwanted kind, stays silent).
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
    // PRD 0016 RF3.2: a near-expiry session needs no earlier reading.
    let mut events: Vec<AlertEvent> = session_expiring(next, state, now).into_iter().collect();
    events.retain(|event| prefs.wants(event));
    let Some(prev) = prev.filter(|prev| {
        prev.provider == next.provider
            && prev.account == next.account
            && prev.status == Status::Ready
            && prev.checked_at < next.checked_at
    }) else {
        return events;
    };
    let account_key = next.account_key();
    let first = !state.seen.contains(&account_digest(&account_key));
    if first && next.checked_at - prev.checked_at > CROSSING_GRACE {
        return events;
    }
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
                if prefs.exhaustion {
                    events.push(event(AlertKind::Exhausted, EXHAUSTED));
                } else if let Some(threshold) = prefs
                    .thresholds()
                    .next_back()
                    .filter(|t| highest.is_none_or(|level| *t > level))
                {
                    events.push(event(AlertKind::Threshold, threshold));
                }
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
    events.retain(|event| prefs.wants(event));
    events
}

/// Mark of the session expiry in [`AlertState`]: never a metric label.
const SESSION_MARK: &str = "\u{0}user_session";

/// PRD 0016 RF3.2: `next` used a web session that ends within
/// [`crate::user_session::EXPIRY_WARNING`] at `now`, and no earlier reading
/// already noticed this expiry. One notice per expiry date: a session
/// pasted again with a new date is noticed again when it nears its end.
fn session_expiring(next: &Report, state: &AlertState, now: DateTime<Utc>) -> Option<AlertEvent> {
    let at = next.session_expires_at?;
    let expiry = crate::user_session::expiry(at, now);
    if !expiry.soon {
        return None;
    }
    let account_key = next.account_key();
    let digest = key_digest(&account_key, SESSION_MARK);
    let noticed = state.marks.iter().any(|mark| {
        mark.metric == digest
            && mark.kind == AlertKind::SessionExpiring
            && mark.resets_at == Some(at)
    });
    (!noticed).then(|| AlertEvent {
        account_key,
        label: String::new(),
        window: None,
        kind: AlertKind::SessionExpiring,
        threshold: u8::try_from(expiry.days.max(0)).unwrap_or(u8::MAX),
        consumed: 0.0,
        resets_at: Some(at),
    })
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
    /// account's next reading a non-first one. `true` when what persists
    /// (the marks, not the first-reading set) changed.
    pub fn update(
        &mut self,
        next: &Report,
        prefs: &AlertPrefs,
        events: &[AlertEvent],
        now: DateTime<Utc>,
    ) -> bool {
        if next.status != Status::Ready {
            return false;
        }
        let mut changed = false;
        let account_key = next.account_key();
        let account = account_digest(&account_key);
        if now - next.checked_at <= CROSSING_GRACE {
            self.seen.insert(account.clone());
        }
        // PRD 0016 RF3.2: a near expiry seen once is not noticed again.
        if let Some(event) = session_expiring(next, self, now) {
            let notified = events.iter().any(|delivered| {
                delivered.account_key == account_key
                    && delivered.kind == AlertKind::SessionExpiring
                    && delivered.resets_at == event.resets_at
            });
            self.marks.push(Mark {
                account: account.clone(),
                metric: key_digest(&account_key, SESSION_MARK),
                kind: AlertKind::SessionExpiring,
                threshold: event.threshold,
                resets_at: event.resets_at,
                notified,
                at: now,
            });
            changed = true;
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
            let before = self.marks.len();
            self.marks.retain(|mark| {
                !(mark.metric == digest
                    && mark.kind != AlertKind::Recovered
                    && same_instance(mark.resets_at, at)
                    && consumption.percent
                        <= f64::from(mark.threshold) - HYSTERESIS_POINTS + EPSILON)
            });
            changed |= self.marks.len() != before;
            if let Some(at) = at {
                for mark in &mut self.marks {
                    if mark.metric == digest
                        && mark.resets_at.is_some_and(|resets_at| {
                            resets_at != at && (resets_at - at).abs() <= DRIFT_TOLERANCE
                        })
                    {
                        mark.resets_at = Some(at);
                        changed = true;
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
            let before = self.marks.len();
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
            changed |= self.marks.len() != before;
        }
        changed
    }

    /// Forget every account whose key is not in `keys` (removed from the
    /// configuration): its levels, recoveries and first-reading status.
    /// `true` when a mark was dropped.
    pub fn retain_accounts<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) -> bool {
        let kept: BTreeSet<String> = keys.into_iter().map(account_digest).collect();
        let before = self.marks.len();
        self.marks.retain(|mark| kept.contains(&mark.account));
        self.seen.retain(|account| kept.contains(account));
        self.marks.len() != before
    }

    /// Forget marks whose window instance and date are both more than
    /// [`LEDGER_KEEP`] (8 days) before `now`; `true` when one was dropped.
    pub fn prune(&mut self, now: DateTime<Utc>) -> bool {
        let before = self.marks.len();
        self.marks.retain(|mark| {
            mark.resets_at
                .map_or(mark.at, |resets_at| resets_at.max(mark.at))
                >= now - LEDGER_KEEP
        });
        self.marks.len() != before
    }

    /// Nothing to persist.
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The metrics of `report` with an active alert (PRD 0008 RF3.4): a
    /// level notified in the metric's current window instance and not
    /// re-armed since. A new window instance or a fall of
    /// [`HYSTERESIS_POINTS`] below the level ends it.
    pub fn active<'r>(&'r self, report: &'r Report) -> impl Iterator<Item = &'r Metric> + 'r {
        let account_key = report.account_key();
        report.metrics.iter().filter(move |metric| {
            let digest = key_digest(&account_key, &metric.label);
            self.marks.iter().any(|mark| {
                mark.notified
                    && mark.metric == digest
                    && mark.kind != AlertKind::Recovered
                    && same_instance(mark.resets_at, metric.resets_at)
            })
        })
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
    if !crate::metrics::is_plan_limit(metric) {
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

/// Whether `metric` can raise usage alerts now: a `Quota` with a positive
/// finite cap and a reading (the rule of [`evaluate`]).
pub fn eligible(metric: &Metric) -> bool {
    consumption(metric).is_some()
}

/// The text of one notification for the alerts delivered together: the
/// events of each account in one group, `groups` in account order (groups
/// without events are skipped; `None` when nothing is left). One event:
/// "Claude: 90% of the 5-hour window used" with "Work: Session (5h) ·
/// resets 14:30" (the reset in `tz`, with the date when it is not today
/// at `now`). Several events of one account: "Claude: 3 usage alerts" and
/// one item per metric. Several accounts (alerts held by
/// [`NOTICE_SPACING`]): "Cuotai: 4 usage alerts" and the items per
/// account. Only the provider, the account name and metric labels: never
/// an e-mail, credential, URL, provider note or usage value beyond the
/// threshold.
pub fn notice<Tz>(
    groups: &[(&Account, Vec<AlertEvent>)],
    now: DateTime<Utc>,
    tz: &Tz,
) -> Option<Notice>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let groups: Vec<&(&Account, Vec<AlertEvent>)> = groups
        .iter()
        .filter(|(_, events)| !events.is_empty())
        .collect();
    let count: usize = groups.iter().map(|(_, events)| events.len()).sum();
    match groups.as_slice() {
        [] => None,
        [(account, events)] if events.len() == 1 => Some(single(account, &events[0], now, tz)),
        [(account, events)] => {
            let provider_pt = account.provider.label();
            let items = join(events.iter().map(item))?;
            let body = format!("{}: {}", account.name, items.fallback);
            Some(Notice {
                title: Localized::with_params(
                    "alerts.title.many",
                    params([
                        ("provider", provider_l10n(account.provider).into()),
                        ("count", count.into()),
                    ]),
                    format!("{provider_pt}: {count} alertas de uso"),
                ),
                body: Localized::with_params(
                    "alerts.body.many",
                    params([
                        ("account", account.name.as_str().into()),
                        ("items", items.into()),
                    ]),
                    body,
                ),
            })
        }
        _ => {
            let body = join(groups.iter().filter_map(|(account, events)| {
                let items = join(events.iter().map(item))?;
                let fallback = format!(
                    "{} {}: {}",
                    account.provider.label(),
                    account.name,
                    items.fallback
                );
                Some(Localized::with_params(
                    "alerts.item.account",
                    params([
                        ("provider", provider_l10n(account.provider).into()),
                        ("account", account.name.as_str().into()),
                        ("items", items.into()),
                    ]),
                    fallback,
                ))
            }))?;
            Some(Notice {
                title: Localized::with_params(
                    "alerts.title.accounts",
                    params([("count", count.into())]),
                    format!("Cuotai: {count} alertas de uso"),
                ),
                body,
            })
        }
    }
}

/// One event: the title names the window (or the metric when it has no
/// known window), the body the account, the metric and the reset.
fn single<Tz>(account: &Account, event: &AlertEvent, now: DateTime<Utc>, tz: &Tz) -> Notice
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let provider = provider_l10n(account.provider);
    if event.kind == AlertKind::SessionExpiring {
        let at = event
            .resets_at
            .map(|at| local_time(at, now, tz))
            .unwrap_or_default();
        return Notice {
            title: Localized::with_param(
                "alerts.title.session_expiring",
                "provider",
                provider,
                format!("{}: sessão expira em breve", account.provider.label()),
            ),
            body: Localized::with_params(
                "alerts.body.session_expiring",
                params([
                    ("account", account.name.as_str().into()),
                    ("at", at.as_str().into()),
                ]),
                format!(
                    "{}: a sessão expira {at}; cole uma nova no assistente da conta.",
                    account.name
                ),
            ),
        };
    }
    let provider_pt = account.provider.label();
    let percent = event.threshold.to_string();
    let label = event.label.as_str();
    let known_window = matches!(
        event.window,
        Some(
            MetricWindow::Window5h
                | MetricWindow::Daily
                | MetricWindow::Weekly
                | MetricWindow::Monthly
        )
    );
    let (window, window_pt) = window_param(event.window, label);
    let title = match (event.kind, known_window) {
        (AlertKind::Threshold, true) => Localized::with_params(
            "alerts.title.threshold",
            params([
                ("provider", provider.into()),
                ("percent", percent.as_str().into()),
                ("window", window),
            ]),
            format!("{provider_pt}: {percent}% da janela {window_pt} usada"),
        ),
        (AlertKind::Threshold, false) => Localized::with_params(
            "alerts.title.threshold_metric",
            params([
                ("provider", provider.into()),
                ("percent", percent.as_str().into()),
                ("metric", label.into()),
            ]),
            format!("{provider_pt}: {percent}% usado em {label}"),
        ),
        (AlertKind::Exhausted, true) => Localized::with_params(
            "alerts.title.exhausted",
            params([("provider", provider.into()), ("window", window)]),
            format!("{provider_pt}: cota {window_pt} esgotada"),
        ),
        (AlertKind::Exhausted, false) => Localized::with_params(
            "alerts.title.exhausted_metric",
            params([("provider", provider.into()), ("metric", label.into())]),
            format!("{provider_pt}: limite de {label} atingido"),
        ),
        (AlertKind::Recovered, true) => Localized::with_params(
            "alerts.title.recovered",
            params([("provider", provider.into()), ("window", window)]),
            format!("{provider_pt}: cota {window_pt} disponível de novo"),
        ),
        (AlertKind::Recovered, false) => Localized::with_params(
            "alerts.title.recovered_metric",
            params([("provider", provider.into()), ("metric", label.into())]),
            format!("{provider_pt}: {label} disponível de novo"),
        ),
        // Returned above.
        (AlertKind::SessionExpiring, _) => unreachable!("session notices return early"),
    };
    let body = match event.resets_at {
        Some(at) => {
            let at = local_time(at, now, tz);
            Localized::with_params(
                "alerts.body",
                params([
                    ("account", account.name.as_str().into()),
                    ("metric", label.into()),
                    ("at", at.as_str().into()),
                ]),
                format!("{}: {label} · renova {at}", account.name),
            )
        }
        None => Localized::with_params(
            "alerts.body.no_reset",
            params([
                ("account", account.name.as_str().into()),
                ("metric", label.into()),
            ]),
            format!("{}: {label}", account.name),
        ),
    };
    Notice { title, body }
}

/// "Session (5h) 90%", "… at the limit", "… available again".
fn item(event: &AlertEvent) -> Localized {
    let label = event.label.as_str();
    let percent = event.threshold.to_string();
    match event.kind {
        AlertKind::Threshold => Localized::with_params(
            "alerts.item.threshold",
            params([
                ("metric", label.into()),
                ("percent", percent.as_str().into()),
            ]),
            format!("{label} {percent}%"),
        ),
        AlertKind::Exhausted => Localized::with_param(
            "alerts.item.exhausted",
            "metric",
            label,
            format!("{label} no limite"),
        ),
        AlertKind::Recovered => Localized::with_param(
            "alerts.item.recovered",
            "metric",
            label,
            format!("{label} disponível de novo"),
        ),
        AlertKind::SessionExpiring => {
            Localized::fixed("alerts.item.session_expiring", "sessão expira em breve")
        }
    }
}

/// The items separated by "; "; `None` without items.
fn join(mut items: impl Iterator<Item = Localized>) -> Option<Localized> {
    let first = items.next()?;
    Some(items.fold(first, |first, rest| {
        let fallback = format!("{}; {}", first.fallback, rest.fallback);
        Localized::with_params(
            "alerts.join",
            params([("first", first.into()), ("rest", rest.into())]),
            fallback,
        )
    }))
}

/// `HH:MM` in `tz`, with the date when it is not the day of `now` there.
fn local_time<Tz>(at: DateTime<Utc>, now: DateTime<Utc>, tz: &Tz) -> String
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let local = at.with_timezone(tz);
    let today = local.date_naive() == now.with_timezone(tz).date_naive();
    local
        .format(if today { "%H:%M" } else { "%Y-%m-%d %H:%M" })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MetricKind, Provider};
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
            billing: Default::default(),
            scope: None,
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
            session_expires_at: None,
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
            (
                "overage",
                with(|m| m.billing = crate::model::Billing::Overage),
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

    #[test]
    fn exhaustion_and_recovery_switches_and_mutes() {
        // Exhaustion off: reaching 100% notifies the highest threshold
        // instead, once, and without the exhaustion notice no recovery.
        let mut no_exhaustion = on();
        no_exhaustion.exhaustion = false;
        let jump = series(&[
            (Some(50.0), window()),
            (Some(100.0), window()),
            (Some(5.0), next_window()),
        ]);
        assert_eq!(
            run(&no_exhaustion, &mut AlertState::default(), &jump),
            vec![vec![], vec![(T, 90)], vec![]]
        );
        let climb = same_window(&[Some(70.0), Some(92.0), Some(100.0)]);
        assert_eq!(
            run(&no_exhaustion, &mut AlertState::default(), &climb),
            vec![vec![], vec![(T, 90)], vec![]]
        );
        let mut no_recovery = on();
        no_recovery.recovery = false;
        let cycle = series(&[
            (Some(90.0), window()),
            (Some(100.0), window()),
            (Some(5.0), next_window()),
        ]);
        assert_eq!(
            run(&no_recovery, &mut AlertState::default(), &cycle),
            vec![vec![], vec![(X, 100)], vec![]]
        );

        // A muted metric stays silent, and what it crossed while muted
        // stays silent once unmuted; only the next level fires.
        let mut muted = on();
        muted
            .muted_metrics
            .insert("claude:Work".into(), BTreeSet::from(["5h".to_string()]));
        let readings = same_window(&[Some(50.0), Some(80.0), Some(92.0)]);
        let mut state = AlertState::default();
        assert_eq!(run(&muted, &mut state, &readings[..2]), vec![vec![]; 2]);
        let unmuted = evaluate(
            Some(&readings[1]),
            &readings[2],
            &on(),
            &state,
            readings[2].checked_at,
        );
        assert_eq!(fired(&unmuted), vec![(T, 90)]);
        // Another account's metric of the same label is not muted.
        let mut other = muted.clone();
        other.muted_metrics = BTreeMap::from([(
            "claude:Home".to_string(),
            BTreeSet::from(["5h".to_string()]),
        )]);
        assert_eq!(
            run(&other, &mut AlertState::default(), &readings)[1],
            vec![(T, 75)]
        );
        let mut account = on();
        account.muted_accounts.insert("claude:Work".into());
        assert_eq!(
            run(&account, &mut AlertState::default(), &readings),
            vec![vec![]; 3]
        );
    }

    #[test]
    fn persisted_state_keeps_notices_across_a_restart_without_names_and_expires() {
        // 90% notified, then 87%: not re-armed (that takes 85%).
        let readings = same_window(&[Some(70.0), Some(91.0), Some(87.0)]);
        let mut state = AlertState::default();
        assert_eq!(
            run(&on(), &mut state, &readings),
            vec![vec![], vec![(T, 90)], vec![]]
        );
        let json = serde_json::to_string(&state).unwrap();
        for needle in ["Work", "5h", "claude", "87.0", "0.87", "91.0"] {
            assert!(!json.contains(needle), "{needle} in {json}");
        }
        let restored: AlertState = serde_json::from_str(&json).unwrap();
        // Reopened 5 minutes later: the recorded 87% is `prev` of a first
        // reading at 92%. Without the persisted state 90 would repeat.
        let at = readings[2].checked_at + mins(5);
        let next = report("Work", at, vec![quota(Some(92.0), window())]);
        let after =
            |state: &AlertState| fired(&evaluate(Some(&readings[2]), &next, &on(), state, at));
        assert_eq!(after(&AlertState::default()), vec![(T, 90)]);
        assert_eq!(after(&restored), vec![]);

        let mut kept = restored;
        assert!(!kept.prune(window() + LEDGER_KEEP), "eight days are kept");
        assert!(kept.prune(window() + LEDGER_KEEP + TimeDelta::seconds(1)));
        assert!(kept.is_empty());
        assert_eq!(
            serde_json::from_str::<AlertState>("{}").unwrap(),
            AlertState::default()
        );
    }

    #[test]
    fn an_alert_stays_active_until_rearmed_or_a_new_window() {
        let count = |readings: &[Report]| {
            let mut state = AlertState::default();
            let mut prev = None;
            readings
                .iter()
                .map(|next| {
                    let events = evaluate(prev, next, &on(), &state, next.checked_at);
                    state.update(next, &on(), &events, next.checked_at);
                    prev = Some(next);
                    state.active(next).count()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            count(&same_window(&[
                Some(70.0),
                Some(91.0),
                Some(87.0),
                Some(85.0)
            ])),
            vec![0, 1, 1, 0]
        );
        assert_eq!(
            count(&series(&[
                (Some(70.0), window()),
                (Some(91.0), window()),
                (Some(10.0), next_window()),
            ])),
            vec![0, 1, 0]
        );
        // Seen without a notice (first reading): nothing to highlight.
        assert_eq!(count(&same_window(&[Some(92.0), Some(93.0)])), vec![0, 0]);
    }

    #[test]
    fn notice_names_provider_window_threshold_account_metric_and_reset() {
        let account = |name: &str| Account {
            name: name.into(),
            provider: Provider::Claude,
            ..Account::default()
        };
        let (work, home) = (account("Work"), account("Home"));
        let event = |key: &str, label: &str, window, kind, threshold, resets_at| AlertEvent {
            account_key: key.into(),
            label: label.into(),
            window,
            kind,
            threshold,
            consumed: 0.9,
            resets_at,
        };
        let ninety = event(
            "claude:Work",
            "Session (5h)",
            Some(MetricWindow::Window5h),
            T,
            90,
            Some(t0() + mins(150)),
        );
        let week = event(
            "claude:Work",
            "Week",
            Some(MetricWindow::Weekly),
            X,
            100,
            None,
        );
        let back = event(
            "claude:Home",
            "5h",
            Some(MetricWindow::Window5h),
            R,
            100,
            Some(t0() + mins(300)),
        );
        let premium = event(
            "claude:Work",
            "Premium",
            Some(MetricWindow::Other),
            T,
            75,
            Some(t0() + TimeDelta::days(2)),
        );
        let text = |groups: &[(&Account, Vec<AlertEvent>)], locale: &str| {
            let notice = notice(groups, t0(), &Utc).unwrap();
            (notice.title.resolve(locale), notice.body.resolve(locale))
        };
        let one = [(&work, vec![ninety.clone()])];
        assert_eq!(
            text(&one, "en-US"),
            (
                "Claude: 90% of the 5-hour window used".into(),
                "Work: Session (5h) · resets 14:30".into()
            )
        );
        assert_eq!(
            text(&one, "pt-BR"),
            (
                "Claude: 90% da janela de 5 horas usada".into(),
                "Work: Session (5h) · renova 14:30".into()
            )
        );
        assert_eq!(
            text(&one, "es").0,
            "Claude: 90% de la ventana de 5 horas usado"
        );
        assert_eq!(
            text(&[(&work, vec![week.clone()])], "en-US"),
            ("Claude: weekly quota exhausted".into(), "Work: Week".into())
        );
        assert_eq!(
            text(&[(&home, vec![back.clone()])], "en-US").0,
            "Claude: 5-hour quota available again"
        );
        // No known window: the metric names it; another day shows the date.
        assert_eq!(
            text(&[(&work, vec![premium])], "en-US"),
            (
                "Claude: 75% of Premium used".into(),
                "Work: Premium · resets 2026-02-17 12:00".into()
            )
        );
        // One account, several alerts: one notice.
        let many = [(&work, vec![ninety.clone(), week])];
        assert_eq!(
            text(&many, "en-US"),
            (
                "Claude: 2 usage alerts".into(),
                "Work: Session (5h) 90%; Week at the limit".into()
            )
        );
        assert_eq!(
            text(&many, "pt-BR").1,
            "Work: Session (5h) 90%; Week no limite"
        );
        // Several accounts held by the spacing: one notice for all.
        let accounts = [
            (&work, vec![ninety]),
            (&home, Vec::new()),
            (&home, vec![back]),
        ];
        assert_eq!(
            text(&accounts, "es"),
            (
                "Cuotai: 2 alertas de uso".into(),
                "Claude Work: Session (5h) 90%; Claude Home: 5h disponible de nuevo".into()
            )
        );
        assert!(notice(&[(&work, Vec::new())], t0(), &Utc).is_none());
    }

    // PRD 0016 RF3.2: a session ending within 3 days is a notice of its
    // own, once per expiry date, with no earlier reading needed.
    #[test]
    fn a_session_near_its_expiry_is_noticed_once_per_date() {
        let prefs = on();
        let mut state = AlertState::default();
        let expiring = |at| Report {
            session_expires_at: Some(at),
            ..report("Work", t0(), Vec::new())
        };
        let soon = t0() + TimeDelta::days(2) + mins(30);

        let first = expiring(soon);
        let events = evaluate(None, &first, &prefs, &state, t0());
        assert_eq!(fired(&events), [(AlertKind::SessionExpiring, 2)]);
        assert_eq!(events[0].resets_at, Some(soon));
        state.update(&first, &prefs, &events, t0());
        let again = expiring(soon);
        assert!(evaluate(Some(&first), &again, &prefs, &state, t0() + mins(1)).is_empty());

        // A fresh paste with a later date that also nears its end.
        let renewed = expiring(soon + TimeDelta::hours(12));
        assert_eq!(
            fired(&evaluate(None, &renewed, &prefs, &state, t0())),
            [(AlertKind::SessionExpiring, 2)]
        );
        // Far from its end, unknown, alerts off or account muted: silent.
        let far = expiring(t0() + TimeDelta::days(5));
        assert!(evaluate(None, &far, &prefs, &state, t0()).is_empty());
        let unknown = report("Work", t0(), Vec::new());
        assert!(evaluate(None, &unknown, &prefs, &state, t0()).is_empty());
        assert!(evaluate(None, &renewed, &AlertPrefs::default(), &state, t0()).is_empty());
        let mut muted = on();
        muted.muted_accounts.insert(renewed.account_key());
        assert!(evaluate(None, &renewed, &muted, &state, t0()).is_empty());

        let account = Account {
            name: "Work".into(),
            provider: Provider::Claude,
            ..Account::default()
        };
        let notice = notice(&[(&account, events)], t0(), &Utc).expect("one notice");
        assert_eq!(notice.title.id, "alerts.title.session_expiring");
        assert_eq!(notice.body.id, "alerts.body.session_expiring");
        assert!(
            notice.body.fallback.contains("Work"),
            "{}",
            notice.body.fallback
        );
    }

    fn next_window() -> DateTime<Utc> {
        window() + TimeDelta::hours(5)
    }
}
