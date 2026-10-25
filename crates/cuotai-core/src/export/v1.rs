//! `export::v1` (PRD 0012 RF1): the last known state of every account, built
//! from a [`RefreshCycle`] without querying anything, with the derived fields
//! the front ends already show (class, age, freshness, the preserved reading,
//! binding metric, pace, usage access and, PRD 0011 RF5, the provider's
//! public status), plus, PRD 0015 RF4.3, `local_cost`: the estimated cost
//! from this computer's local logs, apart from every account.
//!
//! The contract is additive within `schema: 1` and documented in
//! `docs/export-v1.md` with `docs/export-v1.schema.json`; the reference
//! `docs/export-v1.example.json` is checked by a test. It never carries
//! credentials, credential variable names, authenticated URLs or (unless
//! [`Options::include_identity`]) e-mails: notes and `source` go through
//! [`REDACTED`] substitution. A missing value stays `null`, never zero.

use std::borrow::Cow;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::access::{self, UsageAccess};
use crate::advice;
use crate::config_file::ConfigIdentity;
use crate::cost::CostSummary;
use crate::freshness::{self, Freshness};
use crate::incidents::{Incident, IncidentState, ProviderStatus};
use crate::metrics::{self, AccountClass, Binding};
use crate::overage::{self, OverageExport};
use crate::pace::{self, Pace};
use crate::refresh::RefreshCycle;
use crate::text::{Localized, ParamValue};
use crate::{
    account_edit, aggregators, identity, Account, Config, Metric, Provider, Report, Status,
    SubReport,
};

/// Version of this contract; removing a field or changing its meaning needs 2.
pub const SCHEMA: u32 = 1;

/// What replaces a credential variable name, an e-mail or an unparsable URL
/// in notes and `source`.
pub const REDACTED: &str = "[redacted]";

/// Choices of whoever produces the export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Publish each account's e-mail (`email`). Off by default: the field is
    /// then absent and the e-mail is redacted from notes.
    pub include_identity: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Export {
    /// Always [`SCHEMA`].
    pub schema: u32,
    pub generated_at: DateTime<Utc>,
    /// [`ConfigIdentity::digest`]: tells configurations apart without
    /// publishing the file path.
    pub config_identity: String,
    /// In configuration order.
    pub accounts: Vec<AccountExport>,
    /// PRD 0024 RF7.4: the best account now of each group with two or more
    /// comparable accounts ([`crate::advice::best`]), in configuration
    /// order; empty when no group qualifies. Advisory. Additive.
    pub best: Vec<BestExport>,
    /// PRD 0015 RF4.3: the estimated cost at API prices from this
    /// computer's local logs, separate from the providers' reports (never
    /// summed with them); `null` while every local cost source is off or
    /// when the producer does not read the cost index.
    pub local_cost: Option<CostSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountExport {
    /// Position in the configuration.
    pub index: usize,
    /// PRD 0029 RF4.1: [`Account::state_key`], stable across renames,
    /// reordering and parameter edits. Prefer it to `key`.
    pub id: String,
    /// [`Account::key`]: `<provider slug>:<name>`, stable across reordering
    /// but changed by a rename; prefer `id`.
    pub key: String,
    /// PRD 0029 RF3.4: [`Account::display_name`], never empty.
    pub name: String,
    /// PRD 0029 RF4.1: whether `name` was configured (false: it is the
    /// provider's name).
    pub named: bool,
    pub provider: Provider,
    /// PRD 0024 RF7.4: the account's `group` (accounts that can replace one
    /// another); `null` when it has none. Additive.
    pub group: Option<String>,
    /// Present only with [`Options::include_identity`]; `null` when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<Option<String>>,
    /// [`metrics::classify_report`] of `report`.
    pub class: AccountClass,
    /// The current report, `null` while the account has none yet (first
    /// query or an edit pending). Notes and `source` are redacted.
    pub report: Option<Report>,
    /// Age of `report.checked_at`; `null` without a report.
    pub age_seconds: Option<u64>,
    /// PRD 0033 RF3.3: age of `report.as_of`, present only when the
    /// provider states the time of its data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_age_seconds: Option<u64>,
    /// [`freshness::assess`] of `report.checked_at` against the account's
    /// effective interval; `null` without a report.
    pub freshness: Option<Freshness>,
    /// `report.checked_at` lies in the future (the clock went backwards);
    /// the age then reads zero.
    pub clock_adjusted: bool,
    /// `true` exactly when `last_good` is present: what is known of the
    /// account comes from an earlier reading, not from `report`.
    pub stale: bool,
    /// PRD 0031 RF5.4: the same value as `stale`, under a name that says
    /// what it means (a preserved reading is shown instead of a current
    /// one). Additive; `stale` keeps its name and meaning.
    pub showing_last_good: bool,
    /// The last `Ready` reading, offered only while `report` is missing or
    /// failed ([`RefreshCycle::last_good_report`]). Never current data.
    pub last_good: Option<LastGood>,
    /// [`metrics::binding`] of `report`.
    pub binding: Option<Binding>,
    /// [`pace::assess_reading`] per metric of `report`, same length and
    /// order: evaluated at `report.checked_at`, `null` for a metric without
    /// a pace and for every metric of a reading that is not `Fresh` (PRD
    /// 0024 FIX-11). Empty without a report.
    pub pace: Vec<Option<Pace>>,
    /// [`access::describe`] of the account.
    pub access: UsageAccess,
    /// PRD 0028 RF1.3: why the current reading failed; absent for a ready
    /// account or one without a report. Additive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// PRD 0023 RF6: the paid extra usage state of `report`, `null` when
    /// the provider exposes no overage data or there is no report.
    pub overage: Option<OverageExport>,
    /// PRD 0011 RF5: the public status of the account's provider, `null`
    /// while status queries are off or when the provider has no verified
    /// source (never "operational" without a reading).
    pub incident: Option<IncidentExport>,
    /// PRD 0036 RF7.1: the sub-accounts of an aggregator account (one per
    /// upstream connection), omitted when empty. They are never appended
    /// to [`Export::accounts`], so `index` and counts keep their meaning.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sub_accounts: Vec<SubAccountExport>,
    /// PRD 0036 RF7.1: the instance of an aggregator account; omitted for
    /// every other account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregator: Option<AggregatorExport>,
}

/// One upstream connection of an aggregator account (PRD 0036 RF7.1); each
/// field means what it means in [`AccountExport`], evaluated on the
/// connection's own reading.
#[derive(Debug, Clone, Serialize)]
pub struct SubAccountExport {
    /// `<aggregator id>/<sub id>` (PRD 0029: survives a rename of the
    /// aggregator account).
    pub id: String,
    /// `<aggregator key>/<sub id>`.
    pub key: String,
    /// The connection's label (masked by the aggregator, redacted here).
    pub name: String,
    /// The mapped Cuotai provider; `null` for an upstream Cuotai does not
    /// know.
    pub provider: Option<Provider>,
    /// The aggregator's own provider slug, verbatim.
    pub upstream: String,
    pub class: AccountClass,
    /// The connection's current reading as a report (its `account` is
    /// `<aggregator name>/<sub id>`); `null` while the instance failed.
    pub report: Option<Report>,
    pub age_seconds: Option<u64>,
    pub freshness: Option<Freshness>,
    pub clock_adjusted: bool,
    pub stale: bool,
    pub last_good: Option<LastGood>,
    pub binding: Option<Binding>,
    pub pace: Vec<Option<Pace>>,
    /// The aggregator's own on/off flag for the connection.
    pub enabled_upstream: Option<bool>,
    /// Key of the direct account this connection duplicates (PRD 0036
    /// RF5; `null` until duplicate detection exists).
    pub duplicate_of: Option<String>,
    pub plan: Option<String>,
    /// Public file that defines the upstream reading in the aggregator.
    pub via: Option<String>,
    /// PRD 0036 RF6.6: the public status of the mapped provider.
    pub incident: Option<IncidentExport>,
}

/// The instance of an aggregator account (PRD 0036 RF7.1).
#[derive(Debug, Clone, Serialize)]
pub struct AggregatorExport {
    /// The aggregator's display name ("9router").
    pub kind: String,
    /// The instance's version, when it answered it.
    pub version: Option<String>,
    /// Per upstream, over active connections only.
    pub pools: Vec<PoolExport>,
}

/// One pool of [`AggregatorExport::pools`].
#[derive(Debug, Clone, Serialize)]
pub struct PoolExport {
    pub upstream: String,
    pub provider: Option<Provider>,
    pub available: usize,
    pub exhausted: usize,
    pub total: usize,
    /// When every connection is exhausted: the earliest return.
    pub returns_at: Option<DateTime<Utc>>,
    /// "Claude · 2 of 3 available".
    pub summary: Localized,
}

/// PRD 0033 RF2: the metrics of a preserved reading at `now`, each expired
/// one collapsed and marked.
fn last_good_metrics(metrics: &[crate::Metric], now: DateTime<Utc>) -> Vec<LastGoodMetric> {
    metrics
        .iter()
        .map(|metric| {
            let expired = metric.is_expired(now);
            LastGoodMetric {
                metric: if expired {
                    metric.collapsed()
                } else {
                    metric.clone()
                },
                expired,
                renewed_waiting: expired.then(|| metric.renewed_waiting()).flatten(),
            }
        })
        .collect()
}

impl SubAccountExport {
    /// This connection as an [`AccountExport`] of the aggregator `parent`,
    /// for the engines that take one ([`crate::events::diff`]): the mapped
    /// provider (the aggregator's when unknown) and no sub-accounts.
    pub fn as_account(&self, parent: &AccountExport) -> AccountExport {
        AccountExport {
            group: Default::default(),
            index: parent.index,
            id: self.id.clone(),
            key: self.key.clone(),
            name: self.name.clone(),
            named: true,
            provider: self.provider.unwrap_or(parent.provider),
            email: None,
            class: self.class,
            report: self.report.clone(),
            age_seconds: self.age_seconds,
            data_age_seconds: None,
            freshness: self.freshness,
            clock_adjusted: self.clock_adjusted,
            stale: self.stale,
            showing_last_good: self.last_good.is_some(),
            last_good: self.last_good.clone(),
            binding: self.binding.clone(),
            pace: self.pace.clone(),
            access: parent.access.clone(),
            failure: None,
            overage: self
                .report
                .as_ref()
                .and_then(crate::overage::assess)
                .map(|state| OverageExport::from(&state)),
            incident: self.incident.clone(),
            sub_accounts: Vec::new(),
            aggregator: None,
        }
    }
}

/// The failure of [`AccountExport::failure`]: a category and, when a
/// response no longer matched the interface, the JSON paths that drifted
/// (never a value).
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub category: crate::events::FailureCategory,
    pub paths: Vec<String>,
}

/// The provider status of [`AccountExport::incident`].
#[derive(Debug, Clone, Serialize)]
pub struct IncidentExport {
    pub state: IncidentState,
    /// [`IncidentState::label`].
    pub label: Localized,
    /// The most severe active event in the provider's scope, if any.
    pub event: Option<Incident>,
    /// Public status page of the source.
    pub page_url: String,
    /// When the reading shown was obtained; `null` before the first one.
    pub read_at: Option<DateTime<Utc>>,
    /// The latest status query failed: `state` is the previous reading (or
    /// `unknown` without one).
    pub stale: bool,
    /// The retained reading exceeded the status age bound, so `state` is
    /// `unknown` and `event` is `null`.
    pub expired: bool,
}

impl From<&ProviderStatus> for IncidentExport {
    fn from(status: &ProviderStatus) -> Self {
        Self {
            state: status.state,
            label: status.state.label(),
            event: status.incident.clone(),
            page_url: status.page_url.clone(),
            read_at: status.read_at,
            stale: status.stale,
            expired: status.expired,
        }
    }
}

/// The preserved reading of [`AccountExport::last_good`].
#[derive(Debug, Clone, Serialize)]
pub struct LastGood {
    /// Original measurement time, never refreshed.
    pub checked_at: DateTime<Utc>,
    pub age_seconds: u64,
    /// Expired metrics are collapsed (PRD 0033 RF2.1).
    pub metrics: Vec<LastGoodMetric>,
}

/// A metric of [`LastGood`].
#[derive(Debug, Clone, Serialize)]
pub struct LastGoodMetric {
    #[serde(flatten)]
    pub metric: Metric,
    /// PRD 0033 RF2.3: the window renewed after the reading; the value is
    /// absent and `renewed_waiting` says so.
    pub expired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renewed_waiting: Option<Localized>,
}

impl Export {
    /// The export of `config` with the reports `cycle` holds at `now` and
    /// the provider `incidents` already read ([`crate::Monitor::incident_statuses`];
    /// empty while status queries are off). Pure: reads its inputs and never
    /// starts a query.
    pub fn from_cycle(
        config: &Config,
        identity: &ConfigIdentity,
        cycle: &RefreshCycle,
        incidents: &[ProviderStatus],
        now: DateTime<Utc>,
        options: Options,
    ) -> Self {
        Self::from_cycle_with_history(
            config,
            identity,
            cycle,
            incidents,
            now,
            options,
            |_, _, _| Vec::new(),
        )
    }

    /// [`Export::from_cycle`] with the recorded points the app reads for its
    /// pace (PRD 0031 RF13): `recent(index, metrics, now)` returns, per
    /// metric of the account's report, the points [`pace::assess_reading`]
    /// takes as `recent` ([`crate::Monitor::recent_points`] is the usual
    /// source; an empty answer, as without history, gives the average
    /// since the window start).
    pub fn from_cycle_with_history(
        config: &Config,
        identity: &ConfigIdentity,
        cycle: &RefreshCycle,
        incidents: &[ProviderStatus],
        now: DateTime<Utc>,
        options: Options,
        recent: impl Fn(usize, &[Metric], DateTime<Utc>) -> Vec<Vec<pace::Point>>,
    ) -> Self {
        let accounts = config
            .accounts
            .iter()
            .enumerate()
            .map(|(index, account)| {
                let interval =
                    Duration::from_secs(account.effective_refresh_seconds(config.refresh_seconds));
                let report = cycle.account_report(index);
                let points = report
                    .map(|report| recent(index, &report.metrics, now))
                    .unwrap_or_default();
                let mut export = account_export(
                    index,
                    account,
                    report,
                    cycle.last_good_report(index),
                    interval,
                    now,
                    options,
                    &points,
                );
                export.incident = incidents
                    .iter()
                    .find(|status| status.provider == account.provider)
                    .map(IncidentExport::from);
                if account.provider.is_aggregator() {
                    let report = cycle.account_report(index);
                    let last_good = cycle.last_good_report(index);
                    let scope = SubScope {
                        account,
                        parent_key: &export.key,
                        interval,
                        now,
                        options,
                    };
                    export.sub_accounts =
                        scope.exports(report, last_good, |id| cycle.last_good_sub(index, id));
                    for sub in &mut export.sub_accounts {
                        sub.incident = sub.provider.and_then(|provider| {
                            incidents
                                .iter()
                                .find(|status| status.provider == provider)
                                .map(IncidentExport::from)
                        });
                    }
                    export.aggregator = Some(aggregator_export(report, last_good));
                }
                export
            })
            .collect();
        let best = best_of(config, cycle, now);
        Self {
            schema: SCHEMA,
            generated_at: now,
            config_identity: identity.digest(),
            accounts,
            best,
            local_cost: None,
        }
    }

    /// Every account and, after each aggregator account, its connections
    /// as accounts ([`SubAccountExport::as_account`]) with the aggregator's
    /// key: the units the event engine diffs (PRD 0036 RF1.4, RF7.3).
    pub fn units(&self) -> Vec<(AccountExport, Option<String>)> {
        let mut units = Vec::with_capacity(self.accounts.len());
        for account in &self.accounts {
            units.push((account.clone(), None));
            for sub in &account.sub_accounts {
                units.push((sub.as_account(account), Some(account.key.clone())));
            }
        }
        units
    }

    /// RF4.3 of PRD 0015: the export with `local_cost` (see
    /// [`Export::local_cost`]).
    pub fn with_local_cost(self, local_cost: Option<CostSummary>) -> Self {
        Self { local_cost, ..self }
    }
}

/// What the sub-account exports of one aggregator account share.
struct SubScope<'a> {
    account: &'a Account,
    parent_key: &'a str,
    interval: Duration,
    now: DateTime<Utc>,
    options: Options,
}

impl SubScope<'_> {
    /// The connections of the current report when it is `Ready` (each with
    /// its own `last_good` while its reading failed, PRD 0036 RF4.2);
    /// while the instance failed, the connections of its last good reading,
    /// all stale with no current report.
    fn exports<'r>(
        &self,
        report: Option<&'r Report>,
        last_good: Option<&'r Report>,
        last_good_sub: impl Fn(&str) -> Option<&'r SubReport>,
    ) -> Vec<SubAccountExport> {
        let scrub = Scrub::new(self.account, self.options.include_identity);
        if let Some(report) = report.filter(|report| report.status == Status::Ready) {
            return report
                .sub_reports
                .iter()
                .map(|sub| {
                    let reading = aggregators::sub_as_report(report, sub);
                    self.export(sub, Some(&reading), last_good_sub(&sub.id), &scrub)
                })
                .collect();
        }
        last_good.map_or_else(Vec::new, |good| {
            good.sub_reports
                .iter()
                .map(|sub| {
                    let kept = (sub.status == Status::Ready).then_some(sub);
                    self.export(sub, None, kept, &scrub)
                })
                .collect()
        })
    }

    fn export(
        &self,
        sub: &SubReport,
        reading: Option<&Report>,
        last_good: Option<&SubReport>,
        scrub: &Scrub<'_>,
    ) -> SubAccountExport {
        let (interval, now) = (self.interval, self.now);
        let age = |at| freshness::age(at, now).as_secs();
        SubAccountExport {
            id: format!("{}/{}", self.account.state_key(), sub.id),
            key: format!("{}/{}", self.parent_key, sub.id),
            name: scrub.text(&sub.label),
            provider: sub.provider,
            upstream: sub.upstream.clone(),
            class: if reading.is_some() {
                aggregators::sub_class(sub)
            } else {
                AccountClass::Unmeasured
            },
            report: reading.map(|report| scrub.report(report)),
            age_seconds: reading.map(|report| age(report.checked_at)),
            freshness: reading.map(|report| freshness::assess(report.checked_at, interval, now)),
            clock_adjusted: reading
                .is_some_and(|report| freshness::clock_adjusted(report.checked_at, now)),
            stale: last_good.is_some(),
            last_good: last_good.map(|good| LastGood {
                checked_at: good.checked_at,
                age_seconds: age(good.checked_at),
                metrics: last_good_metrics(&good.metrics, now),
            }),
            binding: reading.and_then(metrics::binding),
            pace: reading.map_or_else(Vec::new, |report| {
                report
                    .metrics
                    .iter()
                    .map(|metric| {
                        pace::assess_reading(metric, report.checked_at, interval, now, None)
                    })
                    .collect()
            }),
            enabled_upstream: sub.enabled_upstream,
            duplicate_of: sub.duplicate_of.clone(),
            plan: sub.plan.clone(),
            via: sub.via.clone(),
            incident: None,
        }
    }
}

fn aggregator_export(report: Option<&Report>, last_good: Option<&Report>) -> AggregatorExport {
    let source = report
        .filter(|report| report.status == Status::Ready)
        .or(last_good);
    AggregatorExport {
        kind: Provider::NineRouter.label().to_string(),
        version: source.and_then(aggregators::instance_version),
        pools: source.map_or_else(Vec::new, |report| {
            aggregators::pools(report)
                .into_iter()
                .map(|pool| PoolExport {
                    summary: pool.summary(),
                    upstream: pool.upstream,
                    provider: pool.provider,
                    available: pool.available,
                    exhausted: pool.exhausted,
                    total: pool.total,
                    returns_at: pool.returns_at,
                })
                .collect()
        }),
    }
}

/// One group's best account (PRD 0024 RF7.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BestExport {
    /// The group's `group` value, or `provider:<slug>` for the ungrouped
    /// accounts of one provider.
    pub group: String,
    /// The best account's `id`.
    pub account: String,
    /// PRD 0036 RF6.5: the aggregator account's `id` when the best account
    /// is one of its connections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// PRD 0024 RF7.4 with PRD 0036 RF6.5: the best account now of each group
/// ([`advice::best`]) among the direct accounts and the active connections
/// of aggregators. A connection competes under its mapped provider (an
/// unknown upstream never does) and an aggregator account itself never
/// competes; connections off in the aggregator or duplicating a direct
/// account are not candidates ([`aggregators::expand`]).
fn best_of(config: &Config, cycle: &RefreshCycle, now: DateTime<Utc>) -> Vec<BestExport> {
    let readings: Vec<Option<&Report>> = (0..config.accounts.len())
        .map(|index| cycle.account_report(index))
        .collect();
    let expanded = aggregators::expand(config, &readings);
    let mut candidates = expanded.config.clone();
    for (index, origin) in expanded.origin.iter().enumerate() {
        if let Some(unit) = origin {
            candidates.accounts[index].provider = unit.display.provider;
        }
    }
    let views = advice::views(
        &candidates,
        |index| expanded.readings.get(index).and_then(Option::as_ref),
        |index| !candidates.accounts[index].provider.is_aggregator(),
        now,
    );
    advice::best(&views, now)
        .into_iter()
        .filter_map(|group| {
            let parent = expanded
                .origin
                .get(group.best)
                .and_then(Option::as_ref)
                .and_then(|unit| config.accounts.get(unit.parent_index))
                .map(Account::state_key);
            Some(BestExport {
                group: group.group,
                account: expanded.config.accounts.get(group.best)?.state_key(),
                parent,
            })
        })
        .collect()
}

/// The summary of `/v1/limits` (PRD 0012 RF2.2): per account, only what a
/// status bar shows (class, binding metric and age of the reading), taken
/// from an [`Export`] as it is.
#[derive(Debug, Clone, Serialize)]
pub struct Limits {
    /// Always [`SCHEMA`].
    pub schema: u32,
    pub generated_at: DateTime<Utc>,
    /// In configuration order; PRD 0036 RF7.2: each aggregator account is
    /// followed by its connections, marked with `parent`.
    pub accounts: Vec<AccountLimit>,
    /// PRD 0031 RF4.2: what a status bar needs without walking `accounts`.
    pub summary: LimitsSummary,
    /// PRD 0024 RF7.4 and PRD 0036 RF6.5: [`Export::best`]. Additive.
    pub best: Vec<BestExport>,
}

/// The cross-account digest of [`Limits`] (PRD 0031 RF4.2).
#[derive(Debug, Clone, Serialize)]
pub struct LimitsSummary {
    /// The most severe [`AccountClass`] (`exhausted`, then `unmeasured`,
    /// then `available`); `null` without accounts.
    pub worst_class: Option<AccountClass>,
    /// The account whose binding metric is the most consumed; `null` when
    /// no account has a binding.
    pub tightest: Option<TightestAccount>,
    /// Earliest `resets_at` after `generated_at` among the metrics of the
    /// current reports; `null` when none is known.
    pub next_reset_at: Option<DateTime<Utc>>,
    /// The most severe state among the non-stale provider statuses; `null`
    /// without a status reading.
    pub worst_incident: Option<IncidentState>,
}

/// [`LimitsSummary::tightest`].
#[derive(Debug, Clone, Serialize)]
pub struct TightestAccount {
    /// The stable account id (PRD 0029), as in [`AccountLimit::id`].
    pub id: String,
    pub key: String,
    pub name: String,
    pub used_ratio: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

/// One account of [`Limits`]; every field means what it means in
/// [`AccountExport`].
#[derive(Debug, Clone, Serialize)]
pub struct AccountLimit {
    pub index: usize,
    pub id: String,
    pub key: String,
    pub name: String,
    pub named: bool,
    pub provider: Provider,
    pub class: AccountClass,
    pub binding: Option<Binding>,
    pub age_seconds: Option<u64>,
    pub freshness: Option<Freshness>,
    pub stale: bool,
    /// PRD 0031 RF4.1: the provider status as in [`AccountExport::incident`],
    /// reduced to `state` and `stale`.
    pub incident: Option<AccountIncident>,
    /// PRD 0036 RF7.2: the aggregator account's key, for a connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// The subset of [`IncidentExport`] kept by [`AccountLimit`].
#[derive(Debug, Clone, Serialize)]
pub struct AccountIncident {
    pub state: IncidentState,
    pub stale: bool,
}

fn class_rank(class: AccountClass) -> u8 {
    match class {
        AccountClass::Available => 0,
        AccountClass::Unmeasured => 1,
        AccountClass::Exhausted => 2,
    }
}

fn incident_rank(state: IncidentState) -> u8 {
    match state {
        IncidentState::Operational => 0,
        IncidentState::Unknown => 1,
        IncidentState::Maintenance => 2,
        IncidentState::Degraded => 3,
        IncidentState::PartialOutage => 4,
        IncidentState::MajorOutage => 5,
    }
}

impl Limits {
    pub fn from_export(export: &Export) -> Self {
        Self {
            schema: export.schema,
            generated_at: export.generated_at,
            summary: LimitsSummary::from_export(export),
            best: export.best.clone(),
            accounts: export
                .units()
                .into_iter()
                .map(|(account, parent)| AccountLimit {
                    index: account.index,
                    id: account.id.clone(),
                    key: account.key.clone(),
                    name: account.name.clone(),
                    named: account.named,
                    provider: account.provider,
                    class: account.class,
                    binding: account.binding,
                    age_seconds: account.age_seconds,
                    freshness: account.freshness,
                    stale: account.stale,
                    incident: account.incident.as_ref().map(|incident| AccountIncident {
                        state: incident.state,
                        stale: incident.stale,
                    }),
                    parent,
                })
                .collect(),
        }
    }
}

impl LimitsSummary {
    fn from_export(export: &Export) -> Self {
        let tightest = export
            .accounts
            .iter()
            .filter_map(|account| Some((account, account.binding.as_ref()?)))
            .fold(
                None::<(&AccountExport, &Binding)>,
                |best, candidate| match best {
                    Some(best) if best.1.used_ratio >= candidate.1.used_ratio => Some(best),
                    _ => Some(candidate),
                },
            )
            .map(|(account, binding)| TightestAccount {
                id: account.id.clone(),
                key: account.key.clone(),
                name: account.name.clone(),
                used_ratio: binding.used_ratio,
                resets_at: binding.resets_at,
            });
        Self {
            worst_class: export
                .accounts
                .iter()
                .map(|account| account.class)
                .max_by_key(|class| class_rank(*class)),
            tightest,
            next_reset_at: export
                .accounts
                .iter()
                .filter_map(|account| account.report.as_ref())
                .flat_map(|report| report.metrics.iter())
                .filter_map(|metric| metric.resets_at)
                .filter(|at| *at > export.generated_at)
                .min(),
            worst_incident: export
                .accounts
                .iter()
                .filter_map(|account| account.incident.as_ref())
                .filter(|incident| !incident.stale)
                .map(|incident| incident.state)
                .max_by_key(|state| incident_rank(*state)),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn account_export(
    index: usize,
    account: &Account,
    report: Option<&Report>,
    last_good: Option<&Report>,
    interval: Duration,
    now: DateTime<Utc>,
    options: Options,
    recent: &[Vec<pace::Point>],
) -> AccountExport {
    let scrub = Scrub::new(account, options.include_identity);
    let age = |at| freshness::age(at, now).as_secs();
    AccountExport {
        index,
        id: account.state_key(),
        key: account.key(),
        name: account.display_name().into_owned(),
        named: account.named(),
        provider: account.provider,
        group: account.group.clone(),
        email: options
            .include_identity
            .then(|| identity::email(account).map(str::to_owned)),
        class: metrics::classify_report(report),
        report: report.map(|report| scrub.report(report)),
        age_seconds: report.map(|report| age(report.checked_at)),
        data_age_seconds: report
            .and_then(|report| freshness::data_age(report, now))
            .map(|age| age.as_secs()),
        freshness: report.map(|report| freshness::assess(report.checked_at, interval, now)),
        clock_adjusted: report
            .is_some_and(|report| freshness::clock_adjusted(report.checked_at, now)),
        stale: last_good.is_some(),
        showing_last_good: last_good.is_some(),
        last_good: last_good.map(|reading| LastGood {
            checked_at: reading.checked_at,
            age_seconds: age(reading.checked_at),
            metrics: last_good_metrics(&reading.metrics, now),
        }),
        binding: report.and_then(metrics::binding),
        pace: report.map_or_else(Vec::new, |report| {
            report
                .metrics
                .iter()
                .enumerate()
                .map(|(i, metric)| {
                    let points = recent.get(i).map(Vec::as_slice);
                    pace::assess_reading(metric, report.checked_at, interval, now, points)
                })
                .collect()
        }),
        access: access::describe(account),
        failure: report.and_then(|report| {
            Some(Failure {
                category: crate::events::FailureCategory::of(report)?,
                paths: report
                    .diagnostic
                    .iter()
                    .flat_map(|d| d.drift().paths.iter().map(|p| p.path.clone()))
                    .collect(),
            })
        }),
        overage: report
            .and_then(overage::assess)
            .map(|state| OverageExport::from(&state)),
        incident: None,
        sub_accounts: Vec::new(),
        aggregator: None,
    }
}

/// Removes from free text what RF1.3 keeps out of an export: the account's
/// credential variable names (and any `UPPER_SNAKE` name, the shape of an
/// environment variable), e-mails (the account's and any address a provider
/// put in a note) unless identity is included, and the credentials, query
/// and fragment of URLs.
struct Scrub<'a> {
    names: Vec<&'a str>,
    email: Option<&'a str>,
    hide_emails: bool,
}

impl<'a> Scrub<'a> {
    fn new(account: &'a Account, include_identity: bool) -> Self {
        let params = account
            .params
            .iter()
            .filter(|(key, _)| key.ends_with("_env"))
            .map(|(_, name)| name.as_str());
        let names = account
            .credential_env
            .as_deref()
            .into_iter()
            .chain(params)
            .chain([account_edit::default_env(account.provider)])
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect();
        Self {
            names,
            email: identity::email(account).filter(|_| !include_identity),
            hide_emails: !include_identity,
        }
    }

    fn report(&self, report: &Report) -> Report {
        let mut report = report.clone();
        for note in &mut report.notes {
            *note = self.text(note);
        }
        for note in &mut report.notes_l10n {
            self.localized(note);
        }
        report.source = self.text(&report.source);
        // PRD 0036: a connection label may be derived from an e-mail; the
        // same redaction as the account's own texts applies to it.
        for sub in &mut report.sub_reports {
            sub.label = self.text(&sub.label);
            for note in &mut sub.notes_l10n {
                self.localized(note);
            }
        }
        report
    }

    fn localized(&self, note: &mut Localized) {
        note.fallback = Cow::Owned(self.text(&note.fallback));
        for value in note.params.values_mut() {
            match value {
                ParamValue::Text(text) => *text = self.text(text),
                ParamValue::L10n(inner) => self.localized(inner),
            }
        }
    }

    fn text(&self, text: &str) -> String {
        let text = match self.email {
            Some(email) if text.contains(email) => Cow::Owned(text.replace(email, REDACTED)),
            _ => Cow::Borrowed(text),
        };
        let mut out = String::with_capacity(text.len());
        let mut rest = &*text;
        while let Some(start) = url_start(rest) {
            self.plain(&rest[..start], &mut out);
            let url = &rest[start..];
            let end = url
                .find(|c: char| c.is_whitespace() || "\"'<>;()[]{}".contains(c))
                .unwrap_or(url.len());
            let url = url[..end].trim_end_matches(['.', ',', ':']);
            out.push_str(&clean_url(url));
            rest = &rest[start + url.len()..];
        }
        self.plain(rest, &mut out);
        out
    }

    /// [`Self::words`], with every e-mail address replaced unless identity
    /// is included.
    fn plain(&self, text: &str, out: &mut String) {
        let mut rest = text;
        while let Some((start, end)) = self.hide_emails.then(|| email_span(rest)).flatten() {
            self.words(&rest[..start], out);
            out.push_str(REDACTED);
            rest = &rest[end..];
        }
        self.words(rest, out);
    }

    /// Copy `text`, replacing each identifier-like word that names a
    /// credential variable.
    fn words(&self, text: &str, out: &mut String) {
        let mut word_start = None;
        for (at, c) in text.char_indices() {
            match (c.is_ascii_alphanumeric() || c == '_', word_start) {
                (true, None) => word_start = Some(at),
                (true, Some(_)) => {}
                (false, start) => {
                    if let Some(start) = start {
                        self.word(&text[start..at], out);
                        word_start = None;
                    }
                    out.push(c);
                }
            }
        }
        if let Some(start) = word_start {
            self.word(&text[start..], out);
        }
    }

    fn word(&self, word: &str, out: &mut String) {
        if self.names.contains(&word) || env_like(word) {
            out.push_str(REDACTED);
        } else {
            out.push_str(word);
        }
    }
}

/// `UPPER_SNAKE` with at least one `_` between characters, e.g. `GROQ_API_KEY`.
fn env_like(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_uppercase)
        && bytes.last().is_some_and(|b| *b != b'_')
        && bytes.contains(&b'_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

/// Byte range of the first e-mail address (`local@domain.tld`) in `text`.
fn email_span(text: &str) -> Option<(usize, usize)> {
    let local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let host = |c: char| c.is_ascii_alphanumeric() || ".-".contains(c);
    text.match_indices('@').find_map(|(at, _)| {
        let start = text[..at]
            .char_indices()
            .rev()
            .find(|&(_, c)| !local(c))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let after = &text[at + 1..];
        let domain = after[..after.find(|c: char| !host(c)).unwrap_or(after.len())]
            .trim_end_matches(['.', '-']);
        let labels = domain.split('.').count();
        (start < at && labels >= 2 && domain.split('.').all(|label| !label.is_empty()))
            .then_some((start, at + 1 + domain.len()))
    })
}

fn url_start(text: &str) -> Option<usize> {
    text.match_indices("http")
        .map(|(at, _)| at)
        .find(|&at| text[at..].starts_with("https://") || text[at..].starts_with("http://"))
}

/// `url` without user, password, query or fragment; [`REDACTED`] when it
/// does not parse. Kept verbatim when there is nothing to remove.
fn clean_url(url: &str) -> Cow<'_, str> {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return Cow::Borrowed(REDACTED);
    };
    if parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
    {
        return Cow::Borrowed(url);
    }
    // A URL with a host always accepts an empty user and password.
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    Cow::Owned(parsed.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::params;
    use crate::{MetricKind, MetricWindow, Status};
    use chrono::TimeZone;
    use serde_json::Value;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    const REFERENCE: &str = "docs/export-v1.example.json";
    const SCHEMA_DOC: &str = "docs/export-v1.schema.json";
    const REGENERATE: &str = "CUOTAI_REGENERATE_DOCS";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
    }

    fn minutes_ago(minutes: i64) -> DateTime<Utc> {
        now() - chrono::TimeDelta::minutes(minutes)
    }

    fn account(name: &str, provider: Provider) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider,
            ..Account::default()
        }
    }

    fn quota(label: &str, used: f64, window: MetricWindow, resets_in_hours: i64) -> Metric {
        Metric {
            limit: Some(100.0),
            resets_at: Some(now() + chrono::TimeDelta::hours(resets_in_hours)),
            ..Metric::usage(label, used, "%", MetricKind::Quota, Some(window))
        }
    }

    fn report(account: &Account, status: Status, checked_at: DateTime<Utc>) -> Report {
        Report {
            account_id: None,
            diagnostic: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at,
            status,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }

    fn note(report: &mut Report, note: Localized) {
        report.notes.push(note.fallback.to_string());
        report.notes_l10n.push(note);
    }

    fn finish(cycle: &mut RefreshCycle, index: usize, report: Report) {
        let token = cycle.begin(index).unwrap();
        cycle.finish_account(index, Some(report), token, Instant::now());
    }

    fn build(config: &Config, cycle: &RefreshCycle, options: Options) -> Export {
        build_with(config, cycle, &statuses(), options)
    }

    fn build_with(
        config: &Config,
        cycle: &RefreshCycle,
        incidents: &[ProviderStatus],
        options: Options,
    ) -> Export {
        let identity = ConfigIdentity::Explicit("reference".into());
        Export::from_cycle(config, &identity, cycle, incidents, now(), options)
            .with_local_cost(Some(local_cost()))
    }

    /// PRD 0015 RF4.3: Claude Code on and read, with a priced model and one
    /// without a price, over the last three days; Codex CLI off.
    fn local_cost() -> CostSummary {
        use crate::cost::{
            summarize, DailyUsage, PriceTable, Prices, Rate, Rates, ScanReport, SourceId,
            SourceScan, SourceState, Span, SummaryInput, Tokens,
        };
        let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let usage = |d, model: &str, input, output, cache_read, cache_write, requests| DailyUsage {
            source: SourceId::ClaudeCode,
            day: day(d),
            model: model.into(),
            tokens: Tokens {
                input,
                output,
                cache_read,
                cache_write,
                cache_write_1h: 0,
            },
            requests,
            sessions: 1,
        };
        let rate = |usd| Rate::usd_per_million(usd).unwrap();
        let prices = Prices::with_tables(
            PriceTable {
                currency: crate::cost::CURRENCY,
                effective: day(15),
                source: Some("https://claude.com/pricing#api".into()),
                models: [(
                    "claude-sonnet-4-5".to_owned(),
                    Rates {
                        input: rate(3.0),
                        output: rate(15.0),
                        cache_read: rate(0.3),
                        cache_write: rate(3.75),
                        cache_write_1h: None,
                    },
                )]
                .into(),
            },
            None,
        );
        let rows = [
            usage(
                19,
                "claude-sonnet-4-5-20250929",
                120_000,
                8_000,
                900_000,
                40_000,
                42,
            ),
            usage(
                20,
                "claude-sonnet-4-5-20250929",
                30_000,
                2_000,
                200_000,
                10_000,
                9,
            ),
            usage(20, "claude-mystery-9", 1_000, 100, 0, 0, 1),
        ];
        let scan = SourceScan {
            source: SourceId::ClaudeCode,
            state: SourceState::Ready,
            roots: Vec::new(),
            files: 4,
            read: 1,
            restarted: 0,
            unreadable: 0,
            foreign: 0,
            lines: 52,
            requests: 10,
            duplicates: 0,
            invalid: 0,
            expired: 0,
            complete: true,
        };
        let last = ScanReport {
            scanned_at: minutes_ago(3),
            sources: vec![scan],
            complete: true,
        };
        summarize(&SummaryInput {
            today: day(20),
            span: Span::days(3).unwrap(),
            enabled: &[SourceId::ClaudeCode],
            usage: &rows,
            prices: &prices,
            pricing_error: None,
            last: Some(&last),
        })
    }

    /// PRD 0011 RF5: what the status client read for the reference: Claude
    /// in a partial outage with its incident, Groq's page never readable.
    /// OpenAI and Kimi have no reading (as while status queries are off).
    fn statuses() -> Vec<ProviderStatus> {
        vec![
            ProviderStatus {
                provider: Provider::Claude,
                state: IncidentState::PartialOutage,
                incident: Some(Incident {
                    title: "Elevated errors on Claude Code".into(),
                    started_at: Some(minutes_ago(30)),
                    url: "https://status.claude.com/incidents/abc123".into(),
                }),
                page_url: "https://status.claude.com/".into(),
                read_at: Some(minutes_ago(1)),
                stale: false,
                expired: false,
            },
            ProviderStatus {
                provider: Provider::Groq,
                state: IncidentState::Unknown,
                incident: None,
                page_url: "https://groqstatus.com/".into(),
                read_at: None,
                stale: true,
                expired: false,
            },
        ]
    }

    fn export(config: &Config, cycle: &RefreshCycle, options: Options) -> Value {
        serde_json::to_value(build(config, cycle, options)).unwrap()
    }

    /// One account per case the contract distinguishes: a healthy one with a
    /// binding metric, a pace and a missing value; a failure with the
    /// preserved reading; a failure without one whose note names the
    /// credential variable; and one still waiting for its first report.
    fn reference() -> (Config, RefreshCycle) {
        let mut personal = account("Personal", Provider::Claude);
        personal
            .params
            .insert("email".into(), "person@example.com".into());
        let mut work = account("Work", Provider::OpenAiApi);
        work.credential_env = Some("OPENAI_ADMIN_KEY".into());
        let groq = account("Groq", Provider::Groq);
        let pending = account("Kimi", Provider::Kimi);
        let config = Config {
            accounts: vec![personal, work, groq, pending],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let accounts = &config.accounts;

        let mut ready = report(&accounts[0], Status::Ready, minutes_ago(2));
        ready.metrics = vec![
            quota("Sessão (5h)", 30.0, MetricWindow::Window5h, 2),
            quota("Semana", 80.0, MetricWindow::Weekly, 48),
            Metric {
                used: None,
                ..Metric::usage("Créditos extras", 0.0, "USD", MetricKind::Credits, None)
            },
            Metric {
                limit: Some(50.0),
                remaining: Some(37.5),
                ..Metric::usage(
                    "Uso extra — utilizado",
                    12.5,
                    "unid. provedor",
                    MetricKind::Quota,
                    Some(MetricWindow::Monthly),
                )
                .overage()
            },
        ];
        ready.source = "https://api.anthropic.com/api/oauth/usage".into();
        finish(&mut cycle, 0, ready);

        let mut good = report(&accounts[1], Status::Ready, minutes_ago(42));
        good.metrics = vec![Metric {
            period: Some("2026-10".into()),
            ..Metric::usage(
                "Gasto do mês",
                12.5,
                "USD",
                MetricKind::Spend,
                Some(MetricWindow::Monthly),
            )
        }];
        finish(&mut cycle, 1, good);
        let mut failed = report(&accounts[1], Status::Error, minutes_ago(1));
        note(
            &mut failed,
            Localized::with_params(
                "query.http",
                params([("code", ParamValue::from(503u16))]),
                "Consulta retornou HTTP 503. Verifique acesso e permissões.",
            ),
        );
        finish(&mut cycle, 1, failed);

        let mut missing = report(&accounts[2], Status::NeedsConfiguration, minutes_ago(1));
        note(
            &mut missing,
            Localized::with_param(
                "query.missing_credential",
                "name",
                "GROQ_API_KEY",
                "Credencial ausente: GROQ_API_KEY",
            ),
        );
        finish(&mut cycle, 2, missing);
        (config, cycle)
    }

    /// PRD 0024 RF7.4: `group` per account and `best` per group with two
    /// candidates, also in the `/v1/limits` summary; an ungrouped pair of
    /// different providers is never compared.
    #[test]
    fn best_names_the_account_with_more_headroom_per_group() {
        let mut work = account("Work", Provider::Claude);
        work.group = Some("coding".into());
        let mut personal = account("Personal", Provider::Claude);
        personal.group = Some("coding".into());
        let chat = account("Chat", Provider::ChatGpt);
        let config = Config {
            accounts: vec![work, personal, chat],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        for (index, used) in [(0usize, 80.0), (1, 30.0), (2, 5.0)] {
            let mut ready = report(&config.accounts[index], Status::Ready, minutes_ago(1));
            ready.metrics = vec![quota("Sessão (5h)", used, MetricWindow::Window5h, 3)];
            finish(&mut cycle, index, ready);
        }
        let export = build(&config, &cycle, Options::default());
        assert_eq!(export.accounts[0].group.as_deref(), Some("coding"));
        assert_eq!(export.accounts[2].group, None);
        assert_eq!(
            export.best,
            [BestExport {
                parent: Default::default(),
                group: "coding".into(),
                account: export.accounts[1].id.clone(),
            }]
        );
        let limits = serde_json::to_value(Limits::from_export(&export)).unwrap();
        assert_eq!(limits["best"][0]["group"], "coding");
        assert_eq!(limits["best"][0]["account"], export.accounts[1].id.as_str());
        // One of the two fails: a group of one has no best.
        let mut failed = report(&config.accounts[1], Status::Error, minutes_ago(0));
        failed.metrics.clear();
        finish(&mut cycle, 1, failed);
        assert!(build(&config, &cycle, Options::default()).best.is_empty());
    }

    fn doc_path(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    /// RF1.2: the output matches the reference file; any change to the
    /// contract shows up as a diff to review (regenerate it with
    /// `CUOTAI_REGENERATE_DOCS=1` only for an additive change).
    #[test]
    fn export_v1_reference_matches_the_document() {
        let (config, cycle) = reference();
        // The struct itself, so the document keeps the declared field order.
        let rendered = format!(
            "{}\n",
            serde_json::to_string_pretty(&build(&config, &cycle, Options::default())).unwrap()
        );
        let path = doc_path(REFERENCE);
        if std::env::var_os(REGENERATE).is_some() {
            std::fs::write(&path, &rendered).unwrap();
            return;
        }
        // Windows checkouts may convert the document to CRLF.
        let current = std::fs::read_to_string(&path)
            .expect("docs/export-v1.example.json")
            .replace("\r\n", "\n");
        assert!(
            current == rendered,
            "{REFERENCE} differs from export::v1; review the change (removing or redefining a \
             field needs schema 2) and regenerate with \
             `{REGENERATE}=1 cargo test -p cuotai-core export_v1_reference`"
        );
    }

    #[test]
    fn reference_values_follow_the_cycle_without_inventing_data() {
        let (config, cycle) = reference();
        let json = export(&config, &cycle, Options::default());
        assert_eq!(json["schema"], 1);
        assert_eq!(
            json["config_identity"],
            ConfigIdentity::Explicit("reference".into()).digest()
        );
        let [personal, work, groq, pending] = json["accounts"].as_array().unwrap().as_slice()
        else {
            panic!("four accounts in configuration order");
        };
        assert_eq!(personal["key"], "claude:Personal");
        assert_eq!(personal["class"], "available");
        assert_eq!(personal["age_seconds"], 120);
        assert_eq!(personal["freshness"], "fresh");
        assert_eq!(personal["stale"], false);
        assert_eq!(personal["binding"]["label"], "Semana");
        assert_eq!(personal["pace"].as_array().unwrap().len(), 4);
        assert_eq!(personal["pace"][0]["stage"], "reserve");
        assert!(personal["pace"][2].is_null());
        assert!(personal["pace"][3].is_null(), "overage has no pace");
        assert_eq!(personal["report"]["metrics"][3]["billing"], "overage");
        assert_eq!(personal["overage"]["state"], "active");
        assert_eq!(personal["overage"]["spent"], 12.5);
        assert_eq!(personal["overage"]["cap"], 50.0);
        assert_eq!(personal["overage"]["unit"], "unid. provedor");
        assert!(personal["overage"]["currency"].is_null());
        assert_eq!(personal["access"]["overage"], "provider_units");
        assert_eq!(personal["access"]["level"], "undocumented");

        // The failure keeps its own report and age; the old values travel
        // apart, with their original time, and never feed binding or pace.
        assert_eq!(work["report"]["status"], "error");
        assert_eq!(work["age_seconds"], 60);
        assert_eq!(work["stale"], true);
        assert_eq!(work["last_good"]["age_seconds"], 42 * 60);
        assert_eq!(work["last_good"]["metrics"][0]["used"], 12.5);
        assert!(work["binding"].is_null());
        assert_eq!(work["class"], "unmeasured");

        assert!(work["overage"].is_null() && groq["overage"].is_null());
        assert_eq!(groq["stale"], false);
        assert!(groq["last_good"].is_null());

        assert_eq!(pending["key"], "kimi:Kimi");
        assert_eq!(pending["class"], "unmeasured");
        for field in ["report", "age_seconds", "freshness", "last_good", "binding"] {
            assert!(pending[field].is_null(), "{field}");
        }
        assert_eq!(pending["pace"], serde_json::json!([]));
    }

    /// PRD 0031 RF13: with recorded points the export projects the pace the
    /// way the app does (`pace.source` says `recent`); without them it keeps
    /// the average since the window start.
    #[test]
    fn pace_uses_the_recent_samples_when_history_is_given() {
        let (config, cycle) = reference();
        let identity = ConfigIdentity::Explicit("reference".into());
        let checked_at = minutes_ago(2);
        let points = |used: [f64; 3]| {
            vec![
                pace::Point {
                    taken_at: checked_at - chrono::TimeDelta::minutes(40),
                    used: used[0],
                },
                pace::Point {
                    taken_at: checked_at - chrono::TimeDelta::minutes(20),
                    used: used[1],
                },
                pace::Point {
                    taken_at: checked_at,
                    used: used[2],
                },
            ]
        };
        let recent = |index: usize, metrics: &[Metric], _: DateTime<Utc>| {
            let mut lists = vec![Vec::new(); metrics.len()];
            if index == 0 {
                lists[0] = points([10.0, 20.0, 30.0]);
            }
            lists
        };
        let with = Export::from_cycle_with_history(
            &config,
            &identity,
            &cycle,
            &[],
            now(),
            Options::default(),
            recent,
        );
        let without =
            Export::from_cycle(&config, &identity, &cycle, &[], now(), Options::default());
        let source = |export: &Export| {
            export.accounts[0].pace[0]
                .as_ref()
                .expect("a fresh 5h quota has a pace")
                .source
        };
        assert_eq!(source(&with), pace::PaceSource::Recent);
        assert_eq!(source(&without), pace::PaceSource::Average);
        // The same call the app makes gives the same pace.
        let report = cycle.account_report(0).unwrap();
        let app = pace::assess_reading(
            &report.metrics[0],
            report.checked_at,
            Duration::from_secs(
                config.accounts[0].effective_refresh_seconds(config.refresh_seconds),
            ),
            now(),
            Some(&points([10.0, 20.0, 30.0])),
        );
        assert_eq!(with.accounts[0].pace[0], app);
        // Other accounts and metrics without points are unchanged.
        assert_eq!(with.accounts[1].pace, without.accounts[1].pace);
    }

    /// PRD 0031 RF1.2: the `/v1/limits` bodies the status-bar recipes run
    /// against (`examples/integrations/fixtures`) follow the contract, so a
    /// schema change that invalidates them fails here before it breaks a bar.
    #[test]
    fn recipe_fixtures_follow_the_limits_schema() {
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(doc_path(SCHEMA_DOC)).unwrap()).unwrap();
        for name in ["limits-busy.json", "limits-blocked.json"] {
            let path = doc_path(&format!("examples/integrations/fixtures/{name}"));
            let body: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap())
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let mut errors = Vec::new();
            validate(&schema, &schema["$defs"]["Limits"], &body, "$", &mut errors);
            assert!(errors.is_empty(), "{name}: {errors:#?}");
        }
    }

    /// PRD 0024 FIX-11: the pace is evaluated at the reading's `checked_at`
    /// and is `null` for every metric of a reading that is not `Fresh`.
    #[test]
    fn pace_follows_the_reading_time_and_freshness() {
        let config = Config {
            accounts: vec![account("Personal", Provider::Claude)],
            refresh_seconds: 3600,
            ..Config::default()
        };
        let pace_of = |age_minutes: i64| {
            let mut cycle = RefreshCycle::new(&config, Instant::now());
            let mut ready = report(&config.accounts[0], Status::Ready, minutes_ago(age_minutes));
            // Half of the 5-hour window elapsed at `now()`, half used.
            ready.metrics = vec![Metric {
                resets_at: Some(now() + chrono::TimeDelta::minutes(150)),
                ..quota("Sessão (5h)", 50.0, MetricWindow::Window5h, 0)
            }];
            finish(&mut cycle, 0, ready);
            let json = export(&config, &cycle, Options::default());
            let account = &json["accounts"][0];
            (account["freshness"].clone(), account["pace"][0].clone())
        };
        // Just measured: on pace, as at the wall clock.
        let (freshness, pace) = pace_of(0);
        assert_eq!(freshness, "fresh");
        assert_eq!(pace["stage"], "on_pace");
        // 90 minutes old, still Fresh at a 1-hour interval: at its own time
        // only 20% of the window had elapsed, so it is above the pace.
        let (freshness, pace) = pace_of(90);
        assert_eq!(freshness, "fresh");
        assert_eq!(pace["stage"], "ahead");
        assert_eq!(pace["elapsed_fraction"], 0.2);
        // Aging and Stale readings carry no pace.
        for age in [91, 181] {
            let (freshness, pace) = pace_of(age);
            assert_ne!(freshness, "fresh");
            assert!(pace.is_null(), "{age} minutes");
        }
    }

    /// PRD 0015 RF4.3: `local_cost` travels beside the accounts, never in a
    /// report; a model without a price stays unknown, and an export built
    /// without the cost index says `null`, never an empty estimate.
    #[test]
    fn local_cost_stays_apart_from_the_reports() {
        let (config, cycle) = reference();
        let json = export(&config, &cycle, Options::default());
        let cost = &json["local_cost"];
        assert_eq!(cost["currency"], "USD");
        let [claude, codex] = cost["sources"].as_array().unwrap().as_slice() else {
            panic!("every source");
        };
        assert_eq!(
            (&claude["state"], &codex["state"]),
            (&"ready".into(), &"off".into())
        );
        assert!(codex["usage"].is_null());
        let today = &claude["usage"]["today"];
        let mystery = today["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["model"] == "claude-mystery-9")
            .unwrap();
        assert!(mystery["cost"].is_null(), "unknown, never 0");
        assert_eq!(today["unknown_tokens"]["input"], 1_000);
        assert_eq!(claude["providers"], serde_json::json!(["claude"]));
        // Nothing of it enters an account.
        for account in json["accounts"].as_array().unwrap() {
            assert!(!account.to_string().contains("claude-mystery-9"));
        }

        let identity = ConfigIdentity::Explicit("reference".into());
        let bare = Export::from_cycle(&config, &identity, &cycle, &[], now(), Options::default());
        assert!(serde_json::to_value(bare).unwrap()["local_cost"].is_null());
    }

    /// PRD 0011 RF5: each account carries its own provider's status; an
    /// account whose provider was not read (status queries off, no verified
    /// source) has `null`, never an operational state.
    #[test]
    fn incident_is_the_accounts_own_provider_status() {
        let (config, cycle) = reference();
        let json = export(&config, &cycle, Options::default());
        let accounts = json["accounts"].as_array().unwrap();
        let claude = &accounts[0]["incident"];
        assert_eq!(claude["state"], "partial_outage");
        assert_eq!(claude["label"]["id"], "incident.state.partial_outage");
        assert_eq!(claude["event"]["title"], "Elevated errors on Claude Code");
        assert_eq!(
            claude["event"]["url"],
            "https://status.claude.com/incidents/abc123"
        );
        assert_eq!(claude["stale"], false);
        assert_eq!(claude["expired"], false);
        // OpenAI was not read; Groq's page never answered: unknown, stale.
        assert!(accounts[1]["incident"].is_null());
        let groq = &accounts[2]["incident"];
        assert_eq!(groq["state"], "unknown");
        assert_eq!(groq["stale"], true);
        assert!(groq["read_at"].is_null() && groq["event"].is_null());
        assert!(accounts[3]["incident"].is_null());

        let off = serde_json::to_value(build_with(&config, &cycle, &[], Options::default()));
        for entry in off.unwrap()["accounts"].as_array().unwrap() {
            assert!(entry["incident"].is_null(), "{entry}");
        }
    }

    /// RF1.4: unknown values are `null`, never `0`, including non-finite
    /// numbers an adapter might leave behind.
    #[test]
    fn missing_values_stay_null() {
        let (config, cycle) = reference();
        let json = export(&config, &cycle, Options::default());
        let credits = &json["accounts"][0]["report"]["metrics"][2];
        for field in [
            "used",
            "limit",
            "remaining",
            "resets_at",
            "window",
            "period",
        ] {
            assert!(credits[field].is_null(), "{field}: {}", credits[field]);
        }

        let config = Config {
            accounts: vec![account("Kimi", Provider::Kimi)],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let mut ready = report(&config.accounts[0], Status::Ready, minutes_ago(1));
        ready.metrics = vec![Metric {
            limit: Some(f64::NAN),
            remaining: Some(f64::INFINITY),
            ..Metric::usage("Saldo", f64::NAN, "CNY", MetricKind::Balance, None)
        }];
        finish(&mut cycle, 0, ready);
        let json = export(&config, &cycle, Options::default());
        let metric = &json["accounts"][0]["report"]["metrics"][0];
        for field in ["used", "limit", "remaining"] {
            assert!(metric[field].is_null(), "{field}: {}", metric[field]);
        }
    }

    /// RF1.3: a fixture account whose notes and source carry every kind of
    /// secret a provider could leak; none of them reaches the export.
    #[test]
    fn export_carries_no_secrets() {
        const TOKEN: &str = "sk-live-0123456789abcdef";
        let mut account = account("Leaky", Provider::GeminiAi);
        account.credential_env = Some("my_private_key".into());
        account
            .params
            .insert("session_env".into(), "GEMINI_SESSION".into());
        account
            .params
            .insert("email".into(), "leaky@example.com".into());
        let config = Config {
            accounts: vec![account.clone()],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let mut failed = report(&account, Status::Error, minutes_ago(1));
        failed.source = format!(
            "https://user:{TOKEN}@api.example.com/v1/usage?key={TOKEN}#frag ; https://docs.example.com/usage"
        );
        note(
            &mut failed,
            Localized::with_params(
                "query.router_existing.missing_gemini_session",
                params([
                    ("env", ParamValue::from("GEMINI_SESSION")),
                    (
                        "nested",
                        ParamValue::l10n(Localized::with_param(
                            "query.missing_credential",
                            "name",
                            "my_private_key",
                            "Credencial ausente: my_private_key",
                        )),
                    ),
                ]),
                format!(
                    "Token ausente em GEMINI_SESSION (my_private_key); GEMINI_API_KEY não \
                     autentica leaky@example.com. Veja https://api.example.com/x?access_token={TOKEN}."
                ),
            ),
        );
        // An address the provider's response put in a note (Cursor's
        // "Conta: …", Perplexity's member filter), not the account's own.
        note(
            &mut failed,
            Localized::with_param(
                "note.cursor_personal.account_email",
                "email",
                "owner.name+cuotai@corp.example.com",
                "Conta: owner.name+cuotai@corp.example.com",
            ),
        );
        finish(&mut cycle, 0, failed);

        let json = export(&config, &cycle, Options::default());
        let text = json.to_string();
        for needle in [
            TOKEN,
            "user:",
            "key=",
            "access_token",
            "#frag",
            "my_private_key",
            "GEMINI_SESSION",
            "GEMINI_API_KEY",
            "leaky@example.com",
            "corp.example.com",
            "credential_env",
            "session_env",
            "Bearer",
        ] {
            assert!(!text.contains(needle), "{needle} leaked: {text}");
        }
        assert!(json["accounts"][0].get("email").is_none());
        let report = &json["accounts"][0]["report"];
        assert_eq!(
            report["source"],
            "https://api.example.com/v1/usage ; https://docs.example.com/usage"
        );
        assert_eq!(report["notes_l10n"][0]["params"]["env"], REDACTED);
        assert_eq!(report["notes"][0], report["notes_l10n"][0]["fallback"]);
        // Ordinary text and public URLs survive the redaction.
        assert!(report["notes"][0]
            .as_str()
            .unwrap()
            .contains("Veja https://api.example.com/x."));
    }

    #[test]
    fn identity_appears_only_when_included() {
        let (config, cycle) = reference();
        let options = Options {
            include_identity: true,
        };
        let json = export(&config, &cycle, options);
        assert_eq!(json["accounts"][0]["email"], "person@example.com");
        // Included but unknown is `null`, not absent.
        assert!(json["accounts"][1]["email"].is_null());
        assert!(json["accounts"][1]
            .as_object()
            .unwrap()
            .contains_key("email"));

        let without = export(&config, &cycle, Options::default()).to_string();
        assert!(!without.contains("person@example.com"));
        assert!(!without.contains("\"email\""));
    }

    #[test]
    fn redaction_keeps_ordinary_words_and_names_the_account_uses() {
        let mut account = account("A", Provider::Kimi);
        account.credential_env = Some("work".into());
        let scrub = Scrub::new(&account, false);
        assert_eq!(
            scrub.text("HTTP 429 at work, network down; KIMI_API_KEY or _X_ missing"),
            "HTTP 429 at [redacted], network down; [redacted] or _X_ missing"
        );
        assert_eq!(scrub.text("teamwork"), "teamwork");
        assert_eq!(
            scrub.text("http://:80/x"),
            "[redacted]",
            "an unparsable URL is dropped"
        );
        assert_eq!(
            scrub.text("mailto:a.b+c@x.io, at @home or a@b and me@host.dev."),
            "mailto:[redacted], at @home or a@b and [redacted].",
            "every e-mail address goes, other @ words stay"
        );
        let identity = Scrub::new(&account, true);
        assert_eq!(identity.text("Conta: a@x.io"), "Conta: a@x.io");
    }

    /// RF1.2: the documented JSON Schema declares every field the export
    /// emits (an undeclared field fails here) and accepts the reference and
    /// the identity variant.
    #[test]
    fn json_schema_describes_the_export() {
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(doc_path(SCHEMA_DOC)).unwrap())
                .expect("docs/export-v1.schema.json is JSON");
        let (config, cycle) = reference();
        for options in [
            Options::default(),
            Options {
                include_identity: true,
            },
        ] {
            let json = export(&config, &cycle, options);
            let mut errors = Vec::new();
            validate(&schema, &schema, &json, "$", &mut errors);
            assert!(errors.is_empty(), "{errors:#?}");
        }

        // The validator itself rejects what the schema forbids.
        let mut broken = export(&config, &cycle, Options::default());
        broken["accounts"][0]["undeclared"] = Value::Bool(true);
        broken["accounts"][1]["stale"] = Value::from(1);
        broken["accounts"][2]
            .as_object_mut()
            .unwrap()
            .remove("access");
        let mut errors = Vec::new();
        validate(&schema, &schema, &broken, "$", &mut errors);
        assert_eq!(errors.len(), 3, "{errors:#?}");
    }

    /// PRD 0031 RF5.2: every enum of the schema is open (`anyOf` the known
    /// values and any string), so a value added within schema 1 validates;
    /// a type mismatch is still rejected.
    #[test]
    fn schema_enums_are_open() {
        fn closed(at: &str, schema: &Value, inside_any_of: bool, found: &mut Vec<String>) {
            match schema {
                Value::Object(map) => {
                    if map.contains_key("enum") && !inside_any_of {
                        found.push(at.to_owned());
                    }
                    for (key, value) in map {
                        closed(&format!("{at}/{key}"), value, false, found);
                    }
                }
                Value::Array(items) => {
                    let open = items
                        .iter()
                        .any(|item| item == &serde_json::json!({ "type": "string" }));
                    for (index, item) in items.iter().enumerate() {
                        let in_open_any_of = at.ends_with("/anyOf") && open;
                        closed(&format!("{at}/{index}"), item, in_open_any_of, found);
                    }
                }
                _ => {}
            }
        }
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(doc_path(SCHEMA_DOC)).unwrap())
                .expect("docs/export-v1.schema.json is JSON");
        let mut found = Vec::new();
        closed("#", &schema, false, &mut found);
        assert!(found.is_empty(), "closed enums: {found:#?}");

        let (config, cycle) = reference();
        let mut json = export(&config, &cycle, Options::default());
        let first = &mut json["accounts"][0];
        first["class"] = "future_class".into();
        first["freshness"] = "future_freshness".into();
        first["report"]["status"] = "future_status".into();
        first["report"]["metrics"][0]["kind"] = "future_kind".into();
        first["report"]["metrics"][0]["window"] = "future_window".into();
        first["binding"]["reason"] = "future_reason".into();
        first["pace"][0]["stage"] = "future_stage".into();
        first["access"]["level"] = "future_level".into();
        first["access"]["scope"] = "future_scope".into();
        let mut errors = Vec::new();
        validate(&schema, &schema, &json, "$", &mut errors);
        assert!(errors.is_empty(), "{errors:#?}");
        json["accounts"][0]["class"] = Value::from(7);
        let mut errors = Vec::new();
        validate(&schema, &schema, &json, "$", &mut errors);
        assert_eq!(errors.len(), 1, "{errors:#?}");
    }

    /// PRD 0031 RF5.4: `showing_last_good` is the documented alias of
    /// `stale`, on every account.
    #[test]
    fn showing_last_good_mirrors_stale() {
        let (config, cycle) = reference();
        let json = export(&config, &cycle, Options::default());
        let accounts = json["accounts"].as_array().unwrap();
        assert!(accounts.iter().any(|account| account["stale"] == true));
        for account in accounts {
            assert_eq!(account["showing_last_good"], account["stale"]);
        }
    }

    /// PRD 0012 RF2.2: the `/v1/limits` summary is declared in the same
    /// schema (`$defs/Limits`) and carries, per account, the binding metric
    /// and the age the export already computed.
    #[test]
    fn limits_summary_follows_its_schema() {
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(doc_path(SCHEMA_DOC)).unwrap())
                .expect("docs/export-v1.schema.json is JSON");
        let (config, cycle) = reference();
        let limits = serde_json::to_value(Limits::from_export(&build(
            &config,
            &cycle,
            Options::default(),
        )))
        .unwrap();
        let mut errors = Vec::new();
        validate(
            &schema,
            &schema["$defs"]["Limits"],
            &limits,
            "$",
            &mut errors,
        );
        assert!(errors.is_empty(), "{errors:#?}");

        let accounts = limits["accounts"].as_array().unwrap();
        assert_eq!(accounts.len(), 4, "every account, in configuration order");
        assert_eq!(accounts[0]["key"], "claude:Personal");
        assert_eq!(accounts[0]["binding"]["label"], "Semana");
        assert_eq!(accounts[0]["age_seconds"], 120);
        // A failure with a preserved reading: no binding, stale, its own age.
        assert!(accounts[1]["binding"].is_null());
        assert_eq!(accounts[1]["stale"], true);
        assert_eq!(accounts[1]["age_seconds"], 60);
        // Still pending: nothing to summarize, never zero.
        assert!(accounts[3]["age_seconds"].is_null() && accounts[3]["binding"].is_null());

        // RF4.2: the digest agrees with the accounts it summarizes.
        let summary = &limits["summary"];
        assert!(summary["worst_class"].is_string());
        let max_ratio = accounts
            .iter()
            .filter_map(|account| account["binding"]["used_ratio"].as_f64())
            .fold(f64::MIN, f64::max);
        assert_eq!(summary["tightest"]["used_ratio"].as_f64(), Some(max_ratio));
        // PRD 0029: the digest names the account by its stable id too.
        let tightest_id = summary["tightest"]["id"].as_str().expect("tightest id");
        assert!(accounts
            .iter()
            .any(|account| account["id"].as_str() == Some(tightest_id)));
        assert!(accounts
            .iter()
            .any(|account| account["incident"].is_object()));

        let mut broken = limits.clone();
        broken["accounts"][0]["report"] = Value::Null;
        let mut errors = Vec::new();
        validate(
            &schema,
            &schema["$defs"]["Limits"],
            &broken,
            "$",
            &mut errors,
        );
        assert_eq!(errors.len(), 1, "the summary never carries the report");
    }

    /// The subset of JSON Schema 2020-12 the document uses, strict about
    /// undeclared properties so the schema cannot fall behind the code.
    fn connection(id: &str, upstream: &str, status: Status, metrics: Vec<Metric>) -> SubReport {
        SubReport {
            id: id.into(),
            upstream: upstream.into(),
            provider: crate::providers::nine_router::map_upstream(upstream),
            label: id.into(),
            status,
            metrics,
            notes_l10n: Vec::new(),
            checked_at: minutes_ago(1),
            as_of: None,
            enabled_upstream: Some(true),
            duplicate_of: None,
            plan: None,
            via: None,
        }
    }

    /// An aggregator account with three connections (one available, one
    /// exhausted, one failed after a good reading) after a direct Claude
    /// account.
    fn aggregator_fixture() -> (Config, RefreshCycle) {
        let direct = account("Personal", Provider::Claude);
        let router = account("home", Provider::NineRouter);
        let config = Config {
            accounts: vec![direct, router],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let mut ready = report(&config.accounts[0], Status::Ready, minutes_ago(2));
        ready.metrics = vec![quota("Semana", 50.0, MetricWindow::Weekly, 48)];
        finish(&mut cycle, 0, ready);
        let instance = |subs| Report {
            sub_reports: subs,
            ..report(&config.accounts[1], Status::Ready, minutes_ago(1))
        };
        let mut first = instance(vec![
            connection(
                "a1",
                "claude",
                Status::Ready,
                vec![quota("weekly (7d)", 20.0, MetricWindow::Weekly, 24)],
            ),
            connection(
                "b2",
                "claude",
                Status::Ready,
                vec![quota("weekly (7d)", 100.0, MetricWindow::Weekly, 5)],
            ),
            connection(
                "c3",
                "glm",
                Status::Ready,
                vec![quota("monthly", 10.0, MetricWindow::Monthly, 100)],
            ),
        ]);
        first.checked_at = minutes_ago(10);
        for sub in &mut first.sub_reports {
            sub.checked_at = minutes_ago(10);
        }
        finish(&mut cycle, 1, first);
        let mut c3 = connection("c3", "glm", Status::Error, Vec::new());
        c3.label = "me@example.com".into();
        let mut b2 = connection(
            "b2",
            "claude",
            Status::Ready,
            vec![quota("weekly (7d)", 100.0, MetricWindow::Weekly, 5)],
        );
        b2.label = "Work".into();
        let mut second = instance(vec![
            connection(
                "a1",
                "claude",
                Status::Ready,
                vec![quota("weekly (7d)", 30.0, MetricWindow::Weekly, 24)],
            ),
            b2,
            c3,
        ]);
        note(
            &mut second,
            Localized::with_params(
                "note.nine_router.version",
                params([("version", ParamValue::from("0.5.109"))]),
                "9router 0.5.109.",
            ),
        );
        finish(&mut cycle, 1, second);
        (config, cycle)
    }

    /// PRD 0036 RF7.1: connections are nested under their aggregator,
    /// never added to `accounts`; each carries its own class, binding and
    /// stale treatment; the aggregator block has the version and pools;
    /// labels go through the same redaction; the schema accepts it all.
    #[test]
    fn aggregator_accounts_export_their_connections_additively() {
        let (config, cycle) = aggregator_fixture();
        let built = Export::from_cycle(
            &config,
            &ConfigIdentity::Explicit("reference".into()),
            &cycle,
            &statuses(),
            now(),
            Options::default(),
        );
        let json = serde_json::to_value(&built).unwrap();
        assert_eq!(json["accounts"].as_array().unwrap().len(), 2);
        // A direct account keeps its exact shape.
        assert!(json["accounts"][0].get("sub_accounts").is_none());
        assert!(json["accounts"][0].get("aggregator").is_none());
        let router = &json["accounts"][1];
        let subs = router["sub_accounts"].as_array().unwrap();
        assert_eq!(subs.len(), 3);
        assert_eq!(subs[0]["key"], "nine_router:home/a1");
        assert_eq!(subs[0]["provider"], "claude");
        assert_eq!(subs[0]["class"], "available");
        assert_eq!(subs[0]["binding"]["label"], "weekly (7d)");
        assert_eq!(subs[0]["stale"], false);
        // RF6.6: the mapped provider's public status.
        assert_eq!(subs[0]["incident"]["state"], "partial_outage");
        assert_eq!(subs[1]["class"], "exhausted");
        // RF4.2: the failed connection keeps its last good reading.
        assert_eq!(subs[2]["provider"], Value::Null);
        assert_eq!(subs[2]["upstream"], "glm");
        assert_eq!(subs[2]["stale"], true);
        assert_eq!(subs[2]["last_good"]["age_seconds"], 600);
        assert_eq!(subs[2]["name"], REDACTED);
        let aggregator = &router["aggregator"];
        assert_eq!(aggregator["kind"], "9router");
        assert_eq!(aggregator["version"], "0.5.109");
        assert_eq!(aggregator["pools"][0]["upstream"], "claude");
        assert_eq!(aggregator["pools"][0]["available"], 1);
        assert_eq!(aggregator["pools"][0]["total"], 2);
        let text = json.to_string();
        assert!(!text.contains("me@example.com"));

        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(doc_path(SCHEMA_DOC)).unwrap()).unwrap();
        let mut errors = Vec::new();
        validate(&schema, &schema, &json, "$", &mut errors);
        assert!(errors.is_empty(), "{errors:#?}");

        // The instance fails: every connection is stale with no report.
        let mut cycle = cycle;
        finish(
            &mut cycle,
            1,
            report(&config.accounts[1], Status::Error, minutes_ago(0)),
        );
        let failed = Export::from_cycle(
            &config,
            &ConfigIdentity::Explicit("reference".into()),
            &cycle,
            &[],
            now(),
            Options::default(),
        );
        let subs = &failed.accounts[1].sub_accounts;
        assert_eq!(subs.len(), 3);
        assert!(subs.iter().all(|sub| sub.report.is_none()));
        assert!(subs[0].stale && subs[1].stale);
        // c3 had a good reading before failing: it is still offered.
        assert!(subs[2].stale);
        assert_eq!(
            failed.accounts[1].aggregator.as_ref().unwrap().pools.len(),
            2
        );
    }

    /// PRD 0036 RF7.2 and RF6.5: `/v1/limits` lists each connection after
    /// its aggregator with `parent`, and "best now" may name a connection.
    #[test]
    fn limits_list_connections_with_parent_and_best_may_name_one() {
        let (config, cycle) = aggregator_fixture();
        let built = build_with(&config, &cycle, &[], Options::default());
        let limits = Limits::from_export(&built);
        let keys: Vec<_> = limits.accounts.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "claude:Personal",
                "nine_router:home",
                "nine_router:home/a1",
                "nine_router:home/b2",
                "nine_router:home/c3"
            ]
        );
        assert_eq!(limits.accounts[0].parent, None);
        assert_eq!(
            limits.accounts[2].parent.as_deref(),
            Some("nine_router:home")
        );
        // Personal has 50% left, the connection a1 70%: a1 wins for Claude
        // (one best, PRD 0024 RF7.4 with PRD 0036 RF6.5), keyed by ids.
        let parent = config.accounts[1].state_key();
        assert_eq!(limits.best, built.best);
        assert_eq!(limits.best.len(), 1);
        assert_eq!(limits.best[0].group, "provider:claude");
        assert_eq!(limits.best[0].account, format!("{parent}/a1"));
        assert_eq!(limits.best[0].parent.as_deref(), Some(parent.as_str()));
    }

    /// PRD 0036 RF7.3: connections have events of their own, with
    /// `parent`; a whole pool running out is `pool_exhausted` on the
    /// aggregator.
    #[test]
    fn connection_events_carry_parent_and_pools_have_their_own() {
        use crate::events::{EventKind, EventState};
        use crate::watch::Watcher;
        let (config, mut cycle) = aggregator_fixture();
        let identity = ConfigIdentity::Explicit("reference".into());
        let mut watcher = Watcher::new(EventState::new([75, 90]).unwrap(), None);
        let first = Export::from_cycle(&config, &identity, &cycle, &[], now(), Options::default());
        assert!(watcher.observe(&first, now()).is_empty(), "baseline");
        let mut all_out = report(&config.accounts[1], Status::Ready, now());
        all_out.sub_reports = ["a1", "b2"]
            .into_iter()
            .map(|id| SubReport {
                checked_at: now(),
                ..connection(
                    id,
                    "claude",
                    Status::Ready,
                    vec![quota("weekly (7d)", 100.0, MetricWindow::Weekly, 5)],
                )
            })
            .collect();
        finish(&mut cycle, 1, all_out);
        let next = Export::from_cycle(&config, &identity, &cycle, &[], now(), Options::default());
        let events = watcher.observe(&next, now());
        let pool = events
            .iter()
            .find(|event| event.event == EventKind::PoolExhausted)
            .expect("pool_exhausted");
        assert_eq!(pool.account_key, "nine_router:home");
        assert_eq!(pool.parent, None);
        assert_eq!(pool.metric.as_ref().unwrap().label, "Claude");
        let exhausted = events
            .iter()
            .find(|event| event.event == EventKind::QuotaExhausted)
            .expect("a1 ran out");
        assert_eq!(exhausted.account_key, "nine_router:home/a1");
        assert_eq!(exhausted.parent.as_deref(), Some("nine_router:home"));
        assert_eq!(exhausted.provider, Provider::Claude);
        let json = serde_json::to_value(exhausted).unwrap();
        assert_eq!(json["parent"], "nine_router:home");
        // A direct account's event has no `parent` at all.
        assert!(events
            .iter()
            .filter(|event| event.account_key == "claude:Personal")
            .all(|event| serde_json::to_value(event).unwrap().get("parent").is_none()));
    }

    /// PRD 0036 RF7.4: connections are widget items marked with `parent`.
    #[test]
    fn widget_items_include_connections_with_parent() {
        let (config, cycle) = aggregator_fixture();
        let built = build_with(&config, &cycle, &[], Options::default());
        let widgets = crate::widget_snapshot::WidgetSnapshot::from_export(&built, 10);
        let item = widgets
            .accounts
            .iter()
            .find(|item| item.key == "nine_router:home/a1")
            .expect("a1 is an item");
        assert_eq!(item.parent.as_deref(), Some("nine_router:home"));
        assert_eq!(item.provider, Provider::Claude);
        let direct = widgets
            .accounts
            .iter()
            .find(|item| item.key == "claude:Personal")
            .unwrap();
        assert!(serde_json::to_value(direct)
            .unwrap()
            .get("parent")
            .is_none());
        // The failed connection shows its preserved reading as stale.
        let c3 = widgets
            .accounts
            .iter()
            .find(|item| item.key == "nine_router:home/c3")
            .unwrap();
        assert_eq!(c3.state, crate::widget_snapshot::WidgetState::Stale);
    }

    fn validate(root: &Value, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = reference
                .strip_prefix("#/$defs/")
                .unwrap_or_else(|| panic!("unsupported $ref {reference}"));
            let target = &root["$defs"][name];
            assert!(!target.is_null(), "missing $defs/{name}");
            return validate(root, target, value, at, errors);
        }
        const KEYWORDS: [&str; 16] = [
            "$schema",
            "$id",
            "$ref",
            "$defs",
            "title",
            "description",
            "type",
            "properties",
            "additionalProperties",
            "required",
            "items",
            "enum",
            "const",
            "anyOf",
            "format",
            "minimum",
        ];
        for key in schema.as_object().into_iter().flatten().map(|(k, _)| k) {
            assert!(
                KEYWORDS.contains(&key.as_str()),
                "unsupported keyword {key} at {at}"
            );
        }
        if let Some(options) = schema.get("anyOf").and_then(Value::as_array) {
            let matches = options.iter().any(|option| {
                let mut inner = Vec::new();
                validate(root, option, value, at, &mut inner);
                inner.is_empty()
            });
            if !matches {
                errors.push(format!("{at}: matches no anyOf branch: {value}"));
            }
        }
        if let Some(types) = schema.get("type") {
            let types: Vec<&str> = match types {
                Value::String(one) => vec![one.as_str()],
                Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
                _ => panic!("bad type at {at}"),
            };
            let actual = match value {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(n) if n.is_u64() || n.is_i64() => "integer",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            let ok = types.contains(&actual) || (actual == "integer" && types.contains(&"number"));
            if !ok {
                errors.push(format!("{at}: {actual} is not {types:?}"));
                return;
            }
        }
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
            if !allowed.contains(value) {
                errors.push(format!("{at}: {value} not in enum"));
            }
        }
        if let Some(expected) = schema.get("const") {
            if expected != value {
                errors.push(format!("{at}: {value} is not {expected}"));
            }
        }
        if let (Some(minimum), Some(number)) = (
            schema.get("minimum").and_then(Value::as_f64),
            value.as_f64(),
        ) {
            if number < minimum {
                errors.push(format!("{at}: {number} < {minimum}"));
            }
        }
        if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
            for (i, item) in array.iter().enumerate() {
                validate(root, items, item, &format!("{at}[{i}]"), errors);
            }
        }
        if let Some(object) = value.as_object() {
            for name in schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !object.contains_key(name) {
                    errors.push(format!("{at}: missing required {name}"));
                }
            }
            // An object schema declares each property by name or through
            // `additionalProperties` (maps); anything else is undeclared.
            if schema.get("type").is_some() {
                let properties = schema.get("properties").and_then(Value::as_object);
                let extra = schema.get("additionalProperties");
                for (name, item) in object {
                    match properties.and_then(|p| p.get(name)).or(extra) {
                        Some(property) => {
                            validate(root, property, item, &format!("{at}.{name}"), errors)
                        }
                        None => errors.push(format!("{at}: undeclared property {name}")),
                    }
                }
            }
        }
    }
}
