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

use crate::alerts::{
    self, AlertEvent, AlertKind, AlertPrefs, AlertState, Lead, PaceInput, ThresholdError,
};
use crate::export::v1::AccountExport;
use crate::incidents::IncidentState;
use crate::metric_text::metric_text;
use crate::metrics::{self, Binding, BindingReason, MetricState};
use crate::overage::{self, OverageExport, OverageState};
use crate::pace::PaceSource;
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
    /// PRD 0024 RF5: at the recent pace a quota runs out before its reset,
    /// within the lead (60 minutes by default). Once per window instance.
    QuotaExhaustionProjected,
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
    /// PRD 0023 RF5: the account began paying for extra usage (once per
    /// billing cycle; the edge of `AlertKind::OverageStarted`).
    OverageStarted,
    /// PRD 0023 RF5: an active extra usage went back to armed or disabled,
    /// or a new billing cycle started with nothing spent.
    OverageEnded,
    /// PRD 0031 RF7.1: an account's pasted session nears its expiry date.
    SessionExpiring,
    /// PRD 0031 RF7.2: a quota window resets within the reset lead.
    QuotaResetSoon,
    /// PRD 0036 RF7.3: every active connection of one upstream of an
    /// aggregator is exhausted (`metric.label` is the pool's name).
    PoolExhausted,
    /// PRD 0036 RF7.3: an exhausted pool has a connection available again.
    PoolRecovered,
}

impl EventKind {
    /// Every event, in the documented order.
    pub const ALL: [Self; 18] = [
        Self::QuotaThresholdCrossed,
        Self::QuotaExhausted,
        Self::QuotaRecovered,
        Self::QuotaExhaustionProjected,
        Self::QuotaReset,
        Self::AccountBlocked,
        Self::AccountUnblocked,
        Self::RefreshFailed,
        Self::RefreshRecovered,
        Self::AuthRequired,
        Self::ProviderIncident,
        Self::UsageUpdated,
        Self::OverageStarted,
        Self::OverageEnded,
        Self::SessionExpiring,
        Self::QuotaResetSoon,
        Self::PoolExhausted,
        Self::PoolRecovered,
    ];

    /// The stable name, as serialized in the payload's `event`.
    pub fn name(self) -> &'static str {
        match self {
            Self::QuotaThresholdCrossed => "quota_threshold_crossed",
            Self::QuotaExhausted => "quota_exhausted",
            Self::QuotaRecovered => "quota_recovered",
            Self::QuotaExhaustionProjected => "quota_exhaustion_projected",
            Self::QuotaReset => "quota_reset",
            Self::AccountBlocked => "account_blocked",
            Self::AccountUnblocked => "account_unblocked",
            Self::RefreshFailed => "refresh_failed",
            Self::RefreshRecovered => "refresh_recovered",
            Self::AuthRequired => "auth_required",
            Self::ProviderIncident => "provider_incident",
            Self::UsageUpdated => "usage_updated",
            Self::OverageStarted => "overage_started",
            Self::OverageEnded => "overage_ended",
            Self::SessionExpiring => "session_expiring",
            Self::QuotaResetSoon => "quota_reset_soon",
            Self::PoolExhausted => "pool_exhausted",
            Self::PoolRecovered => "pool_recovered",
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
    /// An extension's output broke the output contract (PRD 0017 RF2).
    InvalidOutput,
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
            Status::Error
                if noted(TIMEOUT_NOTE) || noted(crate::extensions::runner::TIMED_OUT_NOTE) =>
            {
                Self::Timeout
            }
            Status::Error if noted(TRANSPORT_NOTE) || noted(OAUTH_NETWORK_NOTE) => Self::Offline,
            Status::Error if noted(SCHEMA_NOTE) => Self::SchemaChanged,
            Status::Error if noted(crate::extensions::output::INVALID_NOTE) => Self::InvalidOutput,
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
            Self::InvalidOutput => "invalid_output",
            Self::ProviderError => "provider_error",
        }
    }

    /// `events.category.<name>`: a short description for people.
    pub fn label(self) -> Localized {
        let pt = match self {
            Self::Timeout => "timed out",
            Self::Offline => "provider unreachable",
            Self::AuthRequired => "authentication required",
            Self::RateLimited => "query rate limited",
            Self::SchemaChanged => "answer in an unknown format",
            Self::InvalidOutput => "extension output outside the contract",
            Self::ProviderError => "provider error",
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
    /// [`crate::Account::key`]; changes on a rename, prefer `account_id`.
    pub account_key: String,
    /// PRD 0029 RF4.3: [`crate::Account::state_key`], stable across renames.
    pub account_id: String,
    pub account_name: String,
    pub provider: Provider,
    /// `provider_incident` only (PRD 0031 RF8.1): the keys of every
    /// account that reads the same provider status, in configuration
    /// order, the first being `account_key`. Absent for other events.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<String>,
    /// PRD 0036 RF7.3: the aggregator account's key, for an event of one of
    /// its connections; absent for every direct account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
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
        let with_metric = |template: &'static str| {
            Localized::with_param(id.clone(), "metric", metric.as_str(), template)
        };
        match self.event {
            EventKind::QuotaThresholdCrossed => {
                let threshold = self.current.threshold.unwrap_or_default();
                Localized::with_params(
                    id,
                    params([
                        ("metric", ParamValue::from(metric.as_str())),
                        ("threshold", ParamValue::from(u64::from(threshold))),
                    ]),
                    "{metric}: {threshold}% used",
                )
            }
            EventKind::QuotaExhausted => with_metric("{metric}: quota exhausted"),
            EventKind::QuotaRecovered => with_metric("{metric}: quota available again"),
            EventKind::QuotaReset => with_metric("{metric}: window reset"),
            EventKind::QuotaExhaustionProjected => {
                with_metric("{metric}: may run out before the reset")
            }
            EventKind::AccountBlocked => with_metric("Account blocked by {metric}"),
            EventKind::AccountUnblocked => with_metric("Account unblocked ({metric})"),
            EventKind::RefreshFailed => {
                let category = self
                    .current
                    .category
                    .unwrap_or(FailureCategory::ProviderError)
                    .label();
                Localized::with_param(id, "category", category, "Refresh failed: {category}")
            }
            EventKind::RefreshRecovered => Localized::fixed(id, "Refresh working again"),
            EventKind::AuthRequired => Localized::fixed(id, "Authentication required"),
            EventKind::ProviderIncident => {
                let state = self
                    .current
                    .incident
                    .unwrap_or(IncidentState::Unknown)
                    .label();
                Localized::with_param(id, "state", state, "Provider status: {state}")
            }
            EventKind::OverageStarted => match self.current.spent.map(|spent| {
                overage::format_amount(
                    spent,
                    self.current.unit.as_deref().unwrap_or(""),
                    self.current.currency.as_deref(),
                )
            }) {
                Some(amount) => {
                    Localized::with_param(id, "amount", amount, "Extra usage started: {amount}")
                }
                None => Localized::fixed(
                    "events.summary.overage_started_unknown",
                    "Extra usage started",
                ),
            },
            EventKind::OverageEnded => Localized::fixed(id, "Extra usage ended"),
            EventKind::SessionExpiring => {
                let days = u64::from(self.current.days_left.unwrap_or_default());
                Localized::with_param(id, "days", days, "Session expires in {days} day(s)")
            }
            EventKind::QuotaResetSoon => with_metric("{metric}: quota resets soon"),
            EventKind::UsageUpdated => {
                let usage = usage_text(&self.current.metrics);
                Localized::with_param(id, "usage", usage, "Usage updated: {usage}")
            }
            EventKind::PoolExhausted => with_metric("{metric}: all accounts exhausted"),
            EventKind::PoolRecovered => with_metric("{metric}: an account is available again"),
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
    /// PRD 0034 RF2: display text of `label` (English `fallback`).
    pub label_text: Localized,
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
    /// PRD 0034 RF2: display text of `unit` (English `fallback`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_text: Option<Localized>,
    /// Consumed fraction of the cap, `0.0..=1.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
    /// When a blocked account is usable again, if the provider said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_until: Option<DateTime<Utc>>,
    /// PRD 0024 RF5.2: when the quota runs out at the recent pace, for
    /// `quota_exhaustion_projected`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projected_at: Option<DateTime<Utc>>,
    /// The lead (15, 30 or 60 minutes) of `quota_exhaustion_projected`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_minutes: Option<u16>,
    /// What the projection extended (always `recent`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<PaceSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incident: Option<IncidentState>,
    /// `session_expiring`: when the pasted session ends (never its value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// `session_expiring`: whole days left at the observation, from 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_left: Option<u32>,
    /// Every metric of the reading, for `usage_updated`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<MetricReading>,
    /// Paid extra usage state, for the overage events (PRD 0023 RF5.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overage_state: Option<OverageState>,
    /// Extra usage spent so far; absent when unknown, never zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spent: Option<f64>,
    /// ISO 4217 code when `spent` and `cap` are money.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_ends_at: Option<DateTime<Utc>>,
}

/// One metric's values in `usage_updated`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricReading {
    pub label: String,
    /// PRD 0034 RF2: display text of `label` (English `fallback`).
    pub label_text: Localized,
    pub window: Option<MetricWindow>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    pub unit: String,
    /// PRD 0034 RF2: display text of `unit` (English `fallback`).
    pub unit_text: Localized,
    /// Consumed fraction of the cap; `null` without a usable cap.
    pub used_ratio: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
}

impl MetricRef {
    /// A reference to the metric `label` (its display text is derived).
    pub fn new(label: impl Into<String>, window: Option<MetricWindow>) -> Self {
        let label = label.into();
        Self {
            label_text: metric_text(&label),
            label,
            window,
        }
    }
}

impl MetricReading {
    fn of(metric: &Metric) -> Self {
        Self {
            label: metric.label.clone(),
            label_text: metric_text(&metric.label),
            window: metric.window,
            used: finite(metric.used),
            limit: finite(metric.limit),
            remaining: finite(metric.remaining),
            unit: metric.unit.clone(),
            unit_text: metric_text(&metric.unit),
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
            unit_text: Some(metric_text(&metric.unit)),
            used_ratio: used_ratio(metric),
            resets_at: metric.resets_at,
            ..Self::default()
        }
    }

    fn overage(report: &Report) -> Option<Self> {
        let state = overage::assess(report)?;
        let export = OverageExport::from(&state);
        Some(Self {
            overage_state: Some(export.state),
            spent: export.spent,
            unit: export.unit,
            currency: export.currency,
            cap: export.cap,
            cycle_ends_at: export.cycle_ends_at,
            ..Self::default()
        })
    }

    fn incident(state: IncidentState) -> Self {
        Self {
            incident: Some(state),
            ..Self::default()
        }
    }
}

/// What earlier readings left behind, per account id, in memory only.
#[derive(Debug, Clone)]
pub struct EventState {
    prefs: AlertPrefs,
    alerts: AlertState,
    accounts: BTreeMap<String, Track>,
    /// PRD 0031 RF8.1: per provider status source, not per account.
    incidents: BTreeMap<String, IncidentTrack>,
    /// PRD 0031 RF7.2: how long before `resets_at` `quota_reset_soon` fires.
    reset_lead: TimeDelta,
}

/// Default for [`EventState::with_reset_lead`] (`--reset-lead`, minutes).
pub const DEFAULT_RESET_LEAD: TimeDelta = TimeDelta::minutes(15);

/// What `provider_incident` last reported for one provider status source.
#[derive(Debug, Clone)]
struct IncidentTrack {
    /// The state consumers believe: a known one, or `Unknown` only after
    /// an incident ended with the source lost (RF8.2).
    reported: IncidentState,
    /// The ids of the accounts that read this source (the keys of
    /// [`EventState::retain_accounts`]), to forget it with the last one.
    accounts: BTreeSet<String>,
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
    /// Window instances `quota_reset_soon` already reported (label and
    /// `resets_at`, compared within [`resets::DRIFT_TOLERANCE`]).
    soon: Vec<(String, DateTime<Utc>)>,
}

#[derive(Debug, Clone)]
struct Usage {
    metrics: Vec<MetricReading>,
    sent_at: Option<DateTime<Utc>>,
}

impl Default for EventState {
    /// Crossings of the [`alerts::DEFAULT_THRESHOLDS`] (75% and 90%) and
    /// projections 60 minutes ahead ([`EventState::with_projected_lead`]).
    fn default() -> Self {
        Self::with_prefs(
            AlertPrefs::new(true, alerts::DEFAULT_THRESHOLDS)
                .expect("the default thresholds are in range"),
        )
    }
}

/// PRD 0024 RF5.1: the lead of `quota_exhaustion_projected` without
/// `--projected-lead`.
pub const DEFAULT_PROJECTED_LEAD: Lead = Lead::Min60;

impl EventState {
    /// Crossings of `thresholds` (percent, each in
    /// [`alerts::THRESHOLD_RANGE`]); exhaustion is always reported.
    pub fn new(thresholds: impl IntoIterator<Item = u8>) -> Result<Self, ThresholdError> {
        AlertPrefs::new(true, thresholds).map(Self::with_prefs)
    }

    /// PRD 0024 RF5.1: `quota_exhaustion_projected` with `lead` instead of
    /// [`DEFAULT_PROJECTED_LEAD`]. The event needs the readings' pace input
    /// ([`diff_with`]); the script's selection (`--events` or a hook rule)
    /// is its opt-in.
    pub fn with_projected_lead(mut self, lead: Lead) -> Self {
        self.prefs.projected = Some(lead);
        self
    }

    fn with_prefs(mut prefs: AlertPrefs) -> Self {
        prefs.projected = Some(DEFAULT_PROJECTED_LEAD);
        Self {
            prefs,
            alerts: AlertState::default(),
            accounts: BTreeMap::new(),
            incidents: BTreeMap::new(),
            reset_lead: DEFAULT_RESET_LEAD,
        }
    }

    /// Fire `quota_reset_soon` this long before a window resets (default
    /// [`DEFAULT_RESET_LEAD`]). A zero lead turns the event off: a window
    /// is never reported once its reset is due.
    pub fn with_reset_lead(mut self, lead: TimeDelta) -> Self {
        self.reset_lead = lead.max(TimeDelta::zero());
        self
    }

    /// Forget every account whose id ([`AccountExport::id`]) is not in
    /// `keys` (removed from the configuration): its next reading is a new
    /// baseline. A rename keeps the id, so it keeps the baseline.
    pub fn retain_accounts<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        let kept: BTreeSet<&str> = keys.into_iter().collect();
        self.accounts.retain(|key, _| kept.contains(key.as_str()));
        self.incidents.retain(|_, track| {
            track.accounts.retain(|key| kept.contains(key.as_str()));
            !track.accounts.is_empty()
        });
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
/// - `provider_incident` (PRD 0031 RF8) once per change of a provider's
///   status source, whichever account sees it first (stale readings and
///   sources never read are ignored; the first status read is the
///   baseline). `unknown` is tracked: it resolves an incident once and is
///   silent after a quiet state.
pub fn diff(
    prev: &AccountExport,
    next: &AccountExport,
    state: &mut EventState,
    now: DateTime<Utc>,
) -> Vec<Event> {
    diff_with(prev, next, state, now, None)
}

/// [`diff`], plus PRD 0024 RF5's `quota_exhaustion_projected` when `pace`
/// (the account's refresh interval and the recent points of each metric
/// of `next.report`) is given: the edge of [`alerts::evaluate_with`].
pub fn diff_with(
    prev: &AccountExport,
    next: &AccountExport,
    state: &mut EventState,
    now: DateTime<Utc>,
    pace: Option<&PaceInput>,
) -> Vec<Event> {
    let mut out = Emit {
        next,
        now,
        events: Vec::new(),
    };
    let fresh = next.report.as_ref().filter(|report| {
        prev.id != next.id
            || prev
                .report
                .as_ref()
                .is_none_or(|before| before.checked_at != report.checked_at)
    });
    if let Some(report) = fresh {
        state.reading(report, &mut out, pace);
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
            account_id: self.next.id.clone(),
            account_name: self.next.name.clone(),
            provider: self.next.provider,
            accounts: Vec::new(),
            parent: None,
            metric,
            current,
            previous,
            sample: false,
        });
    }
}

impl EventState {
    fn reading(&mut self, report: &Report, out: &mut Emit, pace: Option<&PaceInput>) {
        let Self {
            prefs,
            alerts,
            accounts,
            reset_lead,
            ..
        } = self;
        let now = out.now;
        let failure = FailureCategory::of(report);
        let Some(track) = accounts.get_mut(&out.next.id) else {
            let mut track = Track {
                status: report.status,
                failure,
                ready: None,
                binding: None,
                usage: None,
                soon: Vec::new(),
            };
            if report.status == Status::Ready {
                track.baseline(report, out.next.binding.as_ref(), prefs, alerts, now);
                track.reset_soon(report, *reset_lead, now);
            }
            accounts.insert(out.next.id.clone(), track);
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
            // A restart inside the lead reports nothing for those windows.
            track.reset_soon(report, *reset_lead, now);
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
            let metric = MetricRef::new(reset.label, Some(reset.window));
            out.push(EventKind::QuotaReset, Some(metric), current, previous);
        }
        // PRD 0036 RF7.3: an aggregator adds its pool transitions; its
        // connections have events of their own (`Export::units`).
        let crossed = alerts::evaluate_units(
            Some(&last),
            report,
            prefs,
            alerts,
            now,
            alerts::PoolAlerts::WholePool,
            pace,
        );
        for alert in &crossed {
            let mut current = find(report, &alert.label).unwrap_or_default();
            let kind = match alert.kind {
                AlertKind::Threshold => {
                    current.threshold = Some(alert.threshold);
                    EventKind::QuotaThresholdCrossed
                }
                AlertKind::Exhausted => EventKind::QuotaExhausted,
                AlertKind::Recovered => EventKind::QuotaRecovered,
                AlertKind::ProjectedExhaustion => {
                    // RF5.2: only what the projection says; no notes.
                    let Some(projection) = alert.projection else {
                        continue;
                    };
                    let metric = MetricRef::new(alert.label.clone(), alert.window);
                    let current = Values {
                        projected_at: Some(projection.projected_at),
                        resets_at: alert.resets_at,
                        lead_minutes: Some(projection.lead_minutes),
                        used_ratio: Some(alert.consumed),
                        source: Some(projection.source),
                        ..Values::default()
                    };
                    out.push(
                        EventKind::QuotaExhaustionProjected,
                        Some(metric),
                        current,
                        None,
                    );
                    continue;
                }
                // PRD 0031 RF7.1: the session's expiry, never its value.
                AlertKind::SessionExpiring => {
                    let current = Values {
                        expires_at: alert.resets_at,
                        days_left: Some(u32::from(alert.threshold)),
                        ..Values::default()
                    };
                    out.push(EventKind::SessionExpiring, None, current, None);
                    continue;
                }
                AlertKind::OverageStarted => {
                    out.push(
                        EventKind::OverageStarted,
                        None,
                        Values::overage(report).unwrap_or_default(),
                        Values::overage(&last),
                    );
                    continue;
                }
                // The cap is an alert only; the event is `overage_started`.
                AlertKind::OverageCapReached => continue,
                AlertKind::PoolExhausted => {
                    current.resets_at = alert.resets_at;
                    EventKind::PoolExhausted
                }
                AlertKind::PoolRecovered => EventKind::PoolRecovered,
            };
            let metric = MetricRef::new(alert.label.clone(), alert.window);
            out.push(kind, Some(metric), current, find(&last, &alert.label));
        }
        alerts.update(report, prefs, &crossed, now);
        let state = |report: &Report| overage::assess(report).map(|o| o.state);
        // Ended: it was billing and is now known to be off or empty (an
        // unreadable amount is neither).
        if state(&last).is_some_and(OverageState::is_billing)
            && matches!(
                state(report),
                Some(OverageState::Armed | OverageState::Disabled)
            )
        {
            if let Some(current) = Values::overage(report) {
                out.push(
                    EventKind::OverageEnded,
                    None,
                    current,
                    Values::overage(&last),
                );
            }
        }
        for (label, window, resets_at) in track.reset_soon(report, *reset_lead, now) {
            let mut current = find(report, &label).unwrap_or_default();
            current.resets_at = Some(resets_at);
            let metric = MetricRef::new(label, Some(window));
            out.push(EventKind::QuotaResetSoon, Some(metric), current, None);
        }
        if let Some(binding) = out.next.binding.as_ref() {
            let blocked = |binding: &Binding| binding.reason == BindingReason::Exhausted;
            match track.binding.as_ref() {
                Some(before) if !blocked(before) && blocked(binding) => {
                    let mut current = find(report, &binding.label).unwrap_or_default();
                    current.blocked_until = binding.blocked_until;
                    let metric = MetricRef::new(binding.label.clone(), binding.window);
                    let previous = find(&last, &binding.label);
                    out.push(EventKind::AccountBlocked, Some(metric), current, previous);
                }
                Some(before) if blocked(before) && !blocked(binding) => {
                    let current = find(report, &before.label).unwrap_or_default();
                    let mut previous = find(&last, &before.label).unwrap_or_default();
                    previous.blocked_until = before.blocked_until;
                    let metric = MetricRef::new(before.label.clone(), before.window);
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

    /// PRD 0031 RF8: one `provider_incident` per change of a provider's
    /// status source, whichever account sees it first; `Unknown` is
    /// tracked so an incident that ends with the source lost still
    /// resolves, and a stale or never-read status is no observation.
    fn incident(&mut self, out: &mut Emit) {
        let Some(incident) = out.next.incident.as_ref().filter(|incident| {
            !incident.stale
                && (incident.state != IncidentState::Unknown || incident.read_at.is_some())
        }) else {
            return;
        };
        let state = incident.state;
        // One status per provider (a regional provider reports the worst
        // of its sub-pages, whose page URL may change with it): the track
        // is keyed by the provider alone, so a region flip is no new source.
        let source = out.next.provider.slug().to_owned();
        let Some(track) = self.incidents.get_mut(&source) else {
            self.incidents.insert(
                source,
                IncidentTrack {
                    reported: state,
                    accounts: BTreeSet::from([out.next.id.clone()]),
                },
            );
            return;
        };
        track.accounts.insert(out.next.id.clone());
        let before = track.reported;
        let unknown = IncidentState::Unknown;
        // `Unknown` after a quiet state changes nothing (and so the way
        // back changes nothing either); after an incident it resolves it.
        let emit = match (before, state) {
            (before, state) if before == state => false,
            (before, state) if state == unknown => before.is_incident(),
            (before, state) if before == unknown => state.is_incident(),
            _ => true,
        };
        if state != unknown || before.is_incident() {
            track.reported = state;
        }
        if emit {
            out.push(
                EventKind::ProviderIncident,
                None,
                Values::incident(state),
                Some(Values::incident(before)),
            );
            if let Some(event) = out.events.last_mut() {
                event.accounts = vec![out.next.key.clone()];
            }
        }
    }
}

impl Track {
    /// The first `Ready` reading: what later ones are compared with. An
    /// exhaustion it holds counts as reported, so its recovery is.
    /// The eligible windows of `report` whose reset is within `lead` and
    /// not yet reported, remembered as reported (PRD 0031 RF7.2).
    fn reset_soon(
        &mut self,
        report: &Report,
        lead: TimeDelta,
        now: DateTime<Utc>,
    ) -> Vec<(String, MetricWindow, DateTime<Utc>)> {
        self.soon
            .retain(|(_, at)| *at > now - resets::DRIFT_TOLERANCE);
        let prefs = resets::Prefs::default();
        let mut due = Vec::new();
        for metric in &report.metrics {
            let Some(window) = resets::eligible(metric, now, &prefs) else {
                continue;
            };
            if window.resets_at - lead > now {
                continue;
            }
            let seen = self.soon.iter().any(|(label, at)| {
                *label == window.label && (*at - window.resets_at).abs() <= resets::DRIFT_TOLERANCE
            });
            if !seen {
                self.soon.push((window.label.clone(), window.resets_at));
                due.push((window.label, window.window, window.resets_at));
            }
        }
        due
    }

    fn baseline(
        &mut self,
        report: &Report,
        binding: Option<&Binding>,
        prefs: &AlertPrefs,
        alerts: &mut AlertState,
        now: DateTime<Utc>,
    ) {
        let account_key = report.state_key();
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
                projection: None,
                overage: None,
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
