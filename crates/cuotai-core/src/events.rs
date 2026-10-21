//! Event engine (PRD 0013 RF1): the transitions between consecutive readings
//! of one account, as a stable vocabulary of edge events for
//! `cuotai --watch`, hooks and other integrations.
//!
//! [`diff`] is pure over two [`AccountExport`]s (`export::v1`), what
//! [`EventState`] kept from earlier readings and an injected `now`: no
//! clock, no network, no query. It reuses the rules the front ends already
//! follow instead of restating them: the threshold evaluator
//! ([`alerts::evaluate`], PRD 0008) for crossings, exhaustion and recovery;
//! the reset planner's eligibility and drift identity ([`resets::passed`],
//! PRD 0003) for window resets; the binding metric ([`metrics::binding`],
//! PRD 0009) for blocking; and the provider status (PRD 0011) for
//! incidents.
//!
//! Events are edges: a condition that persists does not repeat, and the
//! first reading of an account only sets its baseline. `usage_updated`
//! fires at most once per [`USAGE_SPACING`] per account. The state lives in
//! memory only: a new [`EventState`] (restarting `watch`) builds new
//! baselines and reports nothing that happened while no process watched.
//! Payloads carry no credentials, URLs, provider notes or raw error
//! messages; a failure is one [`FailureCategory`]. See `docs/events.md`.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::alerts::{self, AlertEvent, AlertKind, AlertPrefs, AlertState, ThresholdError};
use crate::export::v1::AccountExport;
use crate::incidents::IncidentState;
use crate::metrics::{self, Binding, BindingReason, MetricState};
use crate::providers::{SCHEMA_NOTE, TIMEOUT_NOTE, TRANSPORT_NOTE};
use crate::text::{params, Localized, ParamValue};
use crate::{resets, Metric, MetricWindow, Provider, Report, Status};

/// Version of the event payload; removing a field or changing its meaning
/// needs 2.
pub const SCHEMA: u32 = 1;

/// At most one `usage_updated` per account in this span.
pub const USAGE_SPACING: TimeDelta = TimeDelta::seconds(600);

/// Note id of an OAuth refresh that could not reach the provider.
const OAUTH_NETWORK_NOTE: &str = "query.oauth.network";

/// Stable event names (serialized in `snake_case`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A quota's consumption reached a threshold (75% or 90% by default)
    /// above every level already reached in its window instance.
    QuotaThresholdCrossed,
    /// A quota is fully consumed.
    QuotaExhausted,
    /// A quota whose exhaustion was reported (or held in the baseline) has
    /// slack in a new window instance.
    QuotaRecovered,
    /// A quota's window instance ended and a new one started.
    QuotaReset,
    /// The binding metric is exhausted: the account is blocked.
    AccountBlocked,
    /// The binding metric is no longer exhausted.
    AccountUnblocked,
    /// A reading failed after a successful one.
    RefreshFailed,
    /// A reading succeeded after a failed one.
    RefreshRecovered,
    /// The account needs the user to authenticate or configure it again.
    AuthRequired,
    /// The provider's public status changed.
    ProviderIncident,
    /// Usage values changed since the last `usage_updated` (or the baseline).
    UsageUpdated,
}

impl EventKind {
    /// Every event, in the documented order.
    pub const ALL: [Self; 11] = [
        Self::QuotaThresholdCrossed,
        Self::QuotaExhausted,
        Self::QuotaRecovered,
        Self::QuotaReset,
        Self::AccountBlocked,
        Self::AccountUnblocked,
        Self::RefreshFailed,
        Self::RefreshRecovered,
        Self::AuthRequired,
        Self::ProviderIncident,
        Self::UsageUpdated,
    ];

    /// The stable name, as serialized in the payload's `event`.
    pub fn name(self) -> &'static str {
        match self {
            Self::QuotaThresholdCrossed => "quota_threshold_crossed",
            Self::QuotaExhausted => "quota_exhausted",
            Self::QuotaRecovered => "quota_recovered",
            Self::QuotaReset => "quota_reset",
            Self::AccountBlocked => "account_blocked",
            Self::AccountUnblocked => "account_unblocked",
            Self::RefreshFailed => "refresh_failed",
            Self::RefreshRecovered => "refresh_recovered",
            Self::AuthRequired => "auth_required",
            Self::ProviderIncident => "provider_incident",
            Self::UsageUpdated => "usage_updated",
        }
    }

    /// The event named `name` ([`Self::name`]); `None` for any other text.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// Coarse cause of a failed reading: the only failure detail an event
/// carries, never the provider's message, URL or body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    /// The query ran out of time.
    Timeout,
    /// The provider could not be reached.
    Offline,
    /// Credentials were refused, expired or are missing (or the account
    /// needs configuring before it can be read).
    AuthRequired,
    /// The provider limited the query rate.
    RateLimited,
    /// The answer did not match the interface Cuotai knows.
    SchemaChanged,
    /// Any other failure, including a provider that does not support the
    /// query.
    ProviderError,
}

impl FailureCategory {
    /// The category of `report`; `None` when it is `Ready`. Decided by the
    /// status and the stable note ids, never by text.
    pub fn of(report: &Report) -> Option<Self> {
        let noted = |id: &str| report.notes_l10n.iter().any(|note| note.id == id);
        Some(match report.status {
            Status::Ready => return None,
            Status::AuthenticationError | Status::NeedsConfiguration => Self::AuthRequired,
            Status::RateLimited => Self::RateLimited,
            Status::Unsupported => Self::ProviderError,
            Status::Error if noted(TIMEOUT_NOTE) => Self::Timeout,
            Status::Error if noted(TRANSPORT_NOTE) || noted(OAUTH_NETWORK_NOTE) => Self::Offline,
            Status::Error if noted(SCHEMA_NOTE) => Self::SchemaChanged,
            Status::Error => Self::ProviderError,
        })
    }

    /// The stable name, as serialized in the payload's `category`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Offline => "offline",
            Self::AuthRequired => "auth_required",
            Self::RateLimited => "rate_limited",
            Self::SchemaChanged => "schema_changed",
            Self::ProviderError => "provider_error",
        }
    }

    /// `events.category.<name>`: a short description for people.
    pub fn label(self) -> Localized {
        let pt = match self {
            Self::Timeout => "tempo esgotado",
            Self::Offline => "provedor inacessível",
            Self::AuthRequired => "autenticação necessária",
            Self::RateLimited => "limite de consultas",
            Self::SchemaChanged => "resposta em formato desconhecido",
            Self::ProviderError => "erro do provedor",
        };
        Localized::fixed(format!("events.category.{}", self.name()), pt)
    }
}

/// One transition of one account.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    /// Always [`SCHEMA`].
    pub schema: u32,
    pub event: EventKind,
    /// When the engine observed the transition (the `now` of [`diff`]).
    pub occurred_at: DateTime<Utc>,
    /// [`crate::Account::key`].
    pub account_key: String,
    pub account_name: String,
    pub provider: Provider,
    /// The metric the event is about; `null` for account-wide events
    /// (refresh, authentication, incident, usage).
    pub metric: Option<MetricRef>,
    /// The values after the transition.
    pub current: Values,
    /// The same values before it; `null` when nothing earlier is known.
    pub previous: Option<Values>,
    /// A made-up event, not a transition of a real account (the desktop's
    /// hook test, [`crate::hooks::Rule::sample`]); present only when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sample: bool,
}

impl Event {
    /// `events.summary.<event>`: one line for people describing the
    /// transition (the account is not part of it), as `cuotai --watch`
    /// prints it without `--jsonl`. Carries only the payload's data.
    pub fn summary(&self) -> Localized {
        let metric = self
            .metric
            .as_ref()
            .map_or_else(String::new, |metric| metric.label.clone());
        let id = format!("events.summary.{}", self.event.name());
        let with_metric =
            |pt: String| Localized::with_param(id.clone(), "metric", metric.as_str(), pt);
        match self.event {
            EventKind::QuotaThresholdCrossed => {
                let threshold = self.current.threshold.unwrap_or_default();
                Localized::with_params(
                    id,
                    params([
                        ("metric", ParamValue::from(metric.as_str())),
                        ("threshold", ParamValue::from(u64::from(threshold))),
                    ]),
                    format!("{metric}: {threshold}% usado"),
                )
            }
            EventKind::QuotaExhausted => with_metric(format!("{metric}: cota esgotada")),
            EventKind::QuotaRecovered => with_metric(format!("{metric}: cota disponível de novo")),
            EventKind::QuotaReset => with_metric(format!("{metric}: janela renovada")),
            EventKind::AccountBlocked => with_metric(format!("Conta bloqueada por {metric}")),
            EventKind::AccountUnblocked => with_metric(format!("Conta desbloqueada ({metric})")),
            EventKind::RefreshFailed => {
                let category = self
                    .current
                    .category
                    .unwrap_or(FailureCategory::ProviderError)
                    .label();
                let pt = format!("Falha na consulta: {}", category.text());
                Localized::with_param(id, "category", category, pt)
            }
            EventKind::RefreshRecovered => Localized::fixed(id, "Consulta voltou a funcionar"),
            EventKind::AuthRequired => Localized::fixed(id, "Autenticação necessária"),
            EventKind::ProviderIncident => {
                let state = self
                    .current
                    .incident
                    .unwrap_or(IncidentState::Unknown)
                    .label();
                let pt = format!("Status do provedor: {}", state.text());
                Localized::with_param(id, "state", state, pt)
            }
            EventKind::UsageUpdated => {
                let usage = usage_text(&self.current.metrics);
                let pt = format!("Uso atualizado: {usage}");
                Localized::with_param(id, "usage", usage, pt)
            }
        }
    }
}

/// `Label 42%` per metric with a cap (`Label 3.5 USD` without one), joined
/// by commas; `-` when no metric has a known value.
fn usage_text(metrics: &[MetricReading]) -> String {
    let parts: Vec<String> = metrics
        .iter()
        .filter_map(|metric| match (metric.used_ratio, metric.used) {
            (Some(ratio), _) => Some(format!("{} {:.0}%", metric.label, ratio * 100.0)),
            (None, Some(used)) => {
                let value = format!("{used:.2}");
                let value = value.trim_end_matches('0').trim_end_matches('.');
                Some(
                    format!("{} {value} {}", metric.label, metric.unit)
                        .trim_end()
                        .to_owned(),
                )
            }
            (None, None) => None,
        })
        .collect();
    if parts.is_empty() {
        "-".to_owned()
    } else {
        parts.join(", ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricRef {
    pub label: String,
    pub window: Option<MetricWindow>,
}

/// The values relevant to an event. A field that does not apply or is
/// unknown is absent: a missing value is never zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Values {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<FailureCategory>,
    /// Percent, for `quota_threshold_crossed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Consumed fraction of the cap, `0.0..=1.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
    /// When a blocked account is usable again, if the provider said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_until: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incident: Option<IncidentState>,
    /// Every metric of the reading, for `usage_updated`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<MetricReading>,
}

/// One metric's values in `usage_updated`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricReading {
    pub label: String,
    pub window: Option<MetricWindow>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    pub unit: String,
    /// Consumed fraction of the cap; `null` without a usable cap.
    pub used_ratio: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
}

impl MetricReading {
    fn of(metric: &Metric) -> Self {
        Self {
            label: metric.label.clone(),
            window: metric.window,
            used: finite(metric.used),
            limit: finite(metric.limit),
            remaining: finite(metric.remaining),
            unit: metric.unit.clone(),
            used_ratio: used_ratio(metric),
            resets_at: metric.resets_at,
        }
    }

    /// Same metric with the same values (a drifting `resets_at` is not a
    /// usage change).
    fn same_usage(&self, other: &Self) -> bool {
        self.label == other.label
            && self.used == other.used
            && self.limit == other.limit
            && self.remaining == other.remaining
    }
}

impl Values {
    fn status(status: Status, category: Option<FailureCategory>) -> Self {
        Self {
            status: Some(status),
            category,
            ..Self::default()
        }
    }

    fn metric(metric: &Metric) -> Self {
        Self {
            used: finite(metric.used),
            limit: finite(metric.limit),
            remaining: finite(metric.remaining),
            unit: Some(metric.unit.clone()),
            used_ratio: used_ratio(metric),
            resets_at: metric.resets_at,
            ..Self::default()
        }
    }

    fn incident(state: IncidentState) -> Self {
        Self {
            incident: Some(state),
            ..Self::default()
        }
    }
}

/// What earlier readings left behind, per account key, in memory only.
#[derive(Debug, Clone)]
pub struct EventState {
    prefs: AlertPrefs,
    alerts: AlertState,
    accounts: BTreeMap<String, Track>,
    incidents: BTreeMap<String, IncidentState>,
}

/// One account's baseline: the last reading's status, the last `Ready`
/// reading (kept across failures, so the next success compares with it),
/// the last known binding metric and the usage last reported.
#[derive(Debug, Clone)]
struct Track {
    status: Status,
    failure: Option<FailureCategory>,
    ready: Option<Report>,
    binding: Option<Binding>,
    usage: Option<Usage>,
}

#[derive(Debug, Clone)]
struct Usage {
    metrics: Vec<MetricReading>,
    sent_at: Option<DateTime<Utc>>,
}

impl Default for EventState {
    /// Crossings of the [`alerts::DEFAULT_THRESHOLDS`] (75% and 90%).
    fn default() -> Self {
        Self::with_prefs(
            AlertPrefs::new(true, alerts::DEFAULT_THRESHOLDS)
                .expect("the default thresholds are in range"),
        )
    }
}

impl EventState {
    /// Crossings of `thresholds` (percent, each in
    /// [`alerts::THRESHOLD_RANGE`]); exhaustion is always reported.
    pub fn new(thresholds: impl IntoIterator<Item = u8>) -> Result<Self, ThresholdError> {
        AlertPrefs::new(true, thresholds).map(Self::with_prefs)
    }

    fn with_prefs(prefs: AlertPrefs) -> Self {
        Self {
            prefs,
            alerts: AlertState::default(),
            accounts: BTreeMap::new(),
            incidents: BTreeMap::new(),
        }
    }

    /// Forget every account whose key is not in `keys` (removed or edited
    /// away): its next reading is a new baseline.
    pub fn retain_accounts<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        let kept: BTreeSet<&str> = keys.into_iter().collect();
        self.accounts.retain(|key, _| kept.contains(key.as_str()));
        self.incidents.retain(|key, _| kept.contains(key.as_str()));
        self.alerts.retain_accounts(kept);
    }
}

/// The events `next` deserves after `prev`, the same account's previous
/// export (before its first reading, an export whose `report` is `null`).
///
/// A report already seen in `prev` (same key and `checked_at`) is no new
/// reading and yields no reading events. The first reading of an account in
/// `state` only sets the baseline. After it, in this order:
///
/// - `refresh_failed` when a reading fails after a success, with its
///   [`FailureCategory`] (a failure that persists, whatever its category,
///   does not repeat); `refresh_recovered` on the first success after a
///   failure; `auth_required` when the category becomes `auth_required`.
/// - On a `Ready` reading compared with the last `Ready` one (kept across
///   failures): `quota_reset` per window instance that ended
///   ([`resets::passed`]); `quota_threshold_crossed`, `quota_exhausted` and
///   `quota_recovered` from [`alerts::evaluate`] (once per window instance,
///   5-point hysteresis); `account_blocked`/`account_unblocked` when the
///   binding metric becomes or stops being exhausted; `usage_updated` when
///   the values changed since the last one, at most once per
///   [`USAGE_SPACING`].
/// - `provider_incident` when the provider's known public status (neither
///   `unknown` nor stale) changes; the first known status is its baseline.
pub fn diff(
    prev: &AccountExport,
    next: &AccountExport,
    state: &mut EventState,
    now: DateTime<Utc>,
) -> Vec<Event> {
    let mut out = Emit {
        next,
        now,
        events: Vec::new(),
    };
    let fresh = next.report.as_ref().filter(|report| {
        prev.key != next.key
            || prev
                .report
                .as_ref()
                .is_none_or(|before| before.checked_at != report.checked_at)
    });
    if let Some(report) = fresh {
        state.reading(report, &mut out);
    }
    state.incident(&mut out);
    out.events
}

struct Emit<'a> {
    next: &'a AccountExport,
    now: DateTime<Utc>,
    events: Vec<Event>,
}

impl Emit<'_> {
    fn push(
        &mut self,
        event: EventKind,
        metric: Option<MetricRef>,
        current: Values,
        previous: Option<Values>,
    ) {
        self.events.push(Event {
            schema: SCHEMA,
            event,
            occurred_at: self.now,
            account_key: self.next.key.clone(),
            account_name: self.next.name.clone(),
            provider: self.next.provider,
            metric,
            current,
            previous,
            sample: false,
        });
    }
}

impl EventState {
    fn reading(&mut self, report: &Report, out: &mut Emit) {
        let Self {
            prefs,
            alerts,
            accounts,
            ..
        } = self;
        let now = out.now;
        let failure = FailureCategory::of(report);
        let Some(track) = accounts.get_mut(&out.next.key) else {
            let mut track = Track {
                status: report.status,
                failure,
                ready: None,
                binding: None,
                usage: None,
            };
            if report.status == Status::Ready {
                track.baseline(report, out.next.binding.as_ref(), prefs, alerts, now);
            }
            accounts.insert(out.next.key.clone(), track);
            return;
        };
        match (track.failure, failure) {
            (None, Some(category)) => out.push(
                EventKind::RefreshFailed,
                None,
                Values::status(report.status, Some(category)),
                Some(Values::status(track.status, None)),
            ),
            (Some(before), None) => out.push(
                EventKind::RefreshRecovered,
                None,
                Values::status(report.status, None),
                Some(Values::status(track.status, Some(before))),
            ),
            _ => {}
        }
        if failure == Some(FailureCategory::AuthRequired)
            && track.failure != Some(FailureCategory::AuthRequired)
        {
            out.push(
                EventKind::AuthRequired,
                None,
                Values::status(report.status, failure),
                Some(Values::status(track.status, track.failure)),
            );
        }
        track.status = report.status;
        track.failure = failure;
        if report.status != Status::Ready {
            return;
        }
        let Some(last) = track.ready.take() else {
            track.baseline(report, out.next.binding.as_ref(), prefs, alerts, now);
            return;
        };
        let find = |report: &'_ Report, label: &str| {
            report
                .metrics
                .iter()
                .find(|metric| metric.label == label)
                .map(Values::metric)
        };
        for reset in resets::passed(&last, report) {
            let current = find(report, &reset.label).unwrap_or_default();
            let previous = find(&last, &reset.label);
            let metric = MetricRef {
                label: reset.label,
                window: Some(reset.window),
            };
            out.push(EventKind::QuotaReset, Some(metric), current, previous);
        }
        let crossed = alerts::evaluate(Some(&last), report, prefs, alerts, now);
        for alert in &crossed {
            let mut current = find(report, &alert.label).unwrap_or_default();
            let kind = match alert.kind {
                AlertKind::Threshold => {
                    current.threshold = Some(alert.threshold);
                    EventKind::QuotaThresholdCrossed
                }
                AlertKind::Exhausted => EventKind::QuotaExhausted,
                AlertKind::Recovered => EventKind::QuotaRecovered,
                // PRD 0016 RF3.2: a notice of its own, not a quota event.
                AlertKind::SessionExpiring => continue,
            };
            let metric = MetricRef {
                label: alert.label.clone(),
                window: alert.window,
            };
            out.push(kind, Some(metric), current, find(&last, &alert.label));
        }
        alerts.update(report, prefs, &crossed, now);
        if let Some(binding) = out.next.binding.as_ref() {
            let blocked = |binding: &Binding| binding.reason == BindingReason::Exhausted;
            match track.binding.as_ref() {
                Some(before) if !blocked(before) && blocked(binding) => {
                    let mut current = find(report, &binding.label).unwrap_or_default();
                    current.blocked_until = binding.blocked_until;
                    let metric = MetricRef {
                        label: binding.label.clone(),
                        window: binding.window,
                    };
                    let previous = find(&last, &binding.label);
                    out.push(EventKind::AccountBlocked, Some(metric), current, previous);
                }
                Some(before) if blocked(before) && !blocked(binding) => {
                    let current = find(report, &before.label).unwrap_or_default();
                    let mut previous = find(&last, &before.label).unwrap_or_default();
                    previous.blocked_until = before.blocked_until;
                    let metric = MetricRef {
                        label: before.label.clone(),
                        window: before.window,
                    };
                    out.push(
                        EventKind::AccountUnblocked,
                        Some(metric),
                        current,
                        Some(previous),
                    );
                }
                _ => {}
            }
            track.binding = Some(binding.clone());
        }
        let readings: Vec<MetricReading> = report.metrics.iter().map(MetricReading::of).collect();
        match track.usage.as_mut() {
            Some(usage)
                if !same_usage(&usage.metrics, &readings)
                    && usage.sent_at.is_none_or(|sent| now - sent >= USAGE_SPACING) =>
            {
                let previous = Values {
                    metrics: std::mem::replace(&mut usage.metrics, readings.clone()),
                    ..Values::default()
                };
                usage.sent_at = Some(now);
                let current = Values {
                    metrics: readings,
                    ..Values::default()
                };
                out.push(EventKind::UsageUpdated, None, current, Some(previous));
            }
            Some(_) => {}
            None => {
                track.usage = Some(Usage {
                    metrics: readings,
                    sent_at: None,
                })
            }
        }
        track.ready = Some(report.clone());
    }

    fn incident(&mut self, out: &mut Emit) {
        let Some(state) = out
            .next
            .incident
            .as_ref()
            .filter(|incident| !incident.stale && incident.state != IncidentState::Unknown)
            .map(|incident| incident.state)
        else {
            return;
        };
        match self.incidents.get_mut(&out.next.key) {
            Some(before) if *before != state => {
                let previous = std::mem::replace(before, state);
                out.push(
                    EventKind::ProviderIncident,
                    None,
                    Values::incident(state),
                    Some(Values::incident(previous)),
                );
            }
            Some(_) => {}
            None => {
                self.incidents.insert(out.next.key.clone(), state);
            }
        }
    }
}

impl Track {
    /// The first `Ready` reading: what later ones are compared with. An
    /// exhaustion it holds counts as reported, so its recovery is.
    fn baseline(
        &mut self,
        report: &Report,
        binding: Option<&Binding>,
        prefs: &AlertPrefs,
        alerts: &mut AlertState,
        now: DateTime<Utc>,
    ) {
        let account_key = report.account_key();
        let exhausted: Vec<AlertEvent> = report
            .metrics
            .iter()
            .filter(|metric| {
                alerts::eligible(metric)
                    && metrics::classify_metric(metric) == MetricState::Exhausted { no_cap: false }
            })
            .map(|metric| AlertEvent {
                account_key: account_key.clone(),
                label: metric.label.clone(),
                window: metric.window,
                kind: AlertKind::Exhausted,
                threshold: 100,
                consumed: 1.0,
                resets_at: metric.resets_at,
            })
            .collect();
        alerts.update(report, prefs, &exhausted, now);
        self.binding = binding.cloned();
        self.usage = Some(Usage {
            metrics: report.metrics.iter().map(MetricReading::of).collect(),
            sent_at: None,
        });
        self.ready = Some(report.clone());
    }
}

fn same_usage(a: &[MetricReading], b: &[MetricReading]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.same_usage(b))
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

/// [`metrics::classify_metric`]'s consumption: `1.0` when exhausted
/// against a usable cap, `None` without one.
fn used_ratio(metric: &Metric) -> Option<f64> {
    match metrics::classify_metric(metric) {
        MetricState::Exhausted { no_cap: false } => Some(1.0),
        MetricState::Available {
            ratio,
            uncapped: false,
        } => Some(1.0 - ratio),
        _ => None,
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
