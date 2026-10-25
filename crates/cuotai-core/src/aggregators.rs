//! Views over aggregator accounts (PRD 0036): the flattened list of units,
//! the pool summary and the connection test.
//!
//! An **aggregator** (today only 9router) holds credentials for several
//! upstream providers. Its [`Report`] describes the instance; every upstream
//! connection is a [`SubReport`] in `Report::sub_reports`. Nothing here
//! queries: these are pure functions over a snapshot.

use crate::metrics::{classify_parts, AccountClass};
use crate::model::{Account, Config, Metric, Provider, Report, Snapshot, Status, SubReport};
use crate::text::{params, Localized, ParamValue};
use chrono::{DateTime, Utc};

pub use crate::providers::nine_router::TestOutcome;

/// One thing the engines can evaluate: a direct account or a sub-account of
/// an aggregator (PRD 0036 RF1.4). Binding, pace, alerts, resets and
/// freshness read units, so none of them learns about nesting.
#[derive(Debug, Clone)]
pub struct Unit<'a> {
    /// `Report::account_key` of a direct account; `<parent key>/<sub id>`
    /// of a sub-account.
    pub key: String,
    /// The aggregator's key, for a sub-account.
    pub parent: Option<String>,
    /// The mapped provider; `None` for an upstream Cuotai does not know.
    pub provider: Option<Provider>,
    pub label: &'a str,
    pub status: Status,
    pub metrics: &'a [Metric],
    pub checked_at: DateTime<Utc>,
}

impl Snapshot {
    /// Every direct account and every sub-account, in report order (an
    /// aggregator's sub-accounts follow it). A sub-account the aggregator
    /// itself turned off, or that duplicates a direct account, is still
    /// listed: callers filter with [`unit_is_active`] when they rank.
    pub fn units(&self) -> Vec<Unit<'_>> {
        let mut units = Vec::with_capacity(self.reports.len());
        for report in &self.reports {
            let key = report.account_key();
            units.push(Unit {
                key: key.clone(),
                parent: None,
                provider: Some(report.provider),
                label: &report.account,
                status: report.status,
                metrics: &report.metrics,
                checked_at: report.checked_at,
            });
            for sub in &report.sub_reports {
                units.push(Unit {
                    key: format!("{key}/{}", sub.id),
                    parent: Some(key.clone()),
                    provider: sub.provider,
                    label: &sub.label,
                    status: sub.status,
                    metrics: &sub.metrics,
                    checked_at: sub.checked_at,
                });
            }
        }
        units
    }
}

/// One sub-account presented as a direct account (PRD 0036 RF1.4), so the
/// engines that take an `(Account, Report)` pair (binding, pace, alerts,
/// resets, history, events) evaluate it without knowing about nesting.
#[derive(Debug, Clone)]
pub struct SubUnit {
    /// Position of the aggregator account in the configuration.
    pub parent_index: usize,
    /// [`Account::key`] of the aggregator account.
    pub parent_key: String,
    /// The sub-account's own SubReport id.
    pub sub_id: String,
    /// Keyed form: the provider of the aggregator and the name
    /// `<aggregator name>/<sub id>`, so `account.key()` and
    /// `report.account_key()` are `<aggregator key>/<sub id>`, like
    /// [`Unit::key`]. Never carries a credential or a parameter.
    pub account: Account,
    /// Display form for notices: the mapped provider (the aggregator's
    /// when the upstream is unknown) and "<aggregator name> · <label>".
    pub display: Account,
    /// The sub-account's reading as a report of [`Self::account`].
    pub report: Report,
}

/// `<aggregator name>/<sub id>`: the name of a sub-account's keyed account.
pub fn sub_account_name(parent: &str, sub_id: &str) -> String {
    format!("{parent}/{sub_id}")
}

/// The keyed account of `sub` (see [`SubUnit::account`]).
pub fn sub_account(parent: &Account, sub: &SubReport) -> Account {
    Account {
        // PRD 0036 RF6.5: a group set on the aggregator applies to its
        // connections; without one a connection ranks under its mapped
        // provider.
        group: parent.group.clone(),
        // PRD 0029: the connection's state follows the aggregator's id.
        id: Some(crate::AccountId::connection(&parent.state_key(), &sub.id)),
        name: sub_account_name(&parent.name, &sub.id),
        provider: parent.provider,
        credential_env: None,
        oauth: None,
        session: None,
        params: std::collections::BTreeMap::new(),
        refresh_seconds: parent.refresh_seconds,
    }
}

/// The reading of `sub` as a report of [`sub_account`]: same status,
/// metrics, notes and time; never any sub-report of its own.
pub fn sub_as_report(parent: &Report, sub: &SubReport) -> Report {
    Report {
        account_id: Some(crate::AccountId::connection(&parent.state_key(), &sub.id)),
        account: sub_account_name(&parent.account, &sub.id),
        provider: parent.provider,
        checked_at: sub.checked_at,
        status: sub.status,
        metrics: sub.metrics.clone(),
        notes: sub
            .notes_l10n
            .iter()
            .map(|note| note.fallback.to_string())
            .collect(),
        notes_l10n: sub.notes_l10n.clone(),
        source: parent.source.clone(),
        session_expires_at: None,
        sub_reports: Vec::new(),
        diagnostic: None,
        as_of: sub.as_of,
        overage_hint: None,
    }
}

/// The active sub-accounts of `report` (the reading of `parent`, at
/// `parent_index`) as [`SubUnit`]s, in report order. Connections off in the
/// aggregator and duplicates of direct accounts are left out: they never
/// alert nor enter the history twice (RF5.3).
pub fn sub_units(parent_index: usize, parent: &Account, report: &Report) -> Vec<SubUnit> {
    let parent_key = parent.key();
    report
        .sub_reports
        .iter()
        .filter(|sub| sub_is_active(sub))
        .map(|sub| {
            let account = sub_account(parent, sub);
            let display = Account {
                name: format!("{} · {}", parent.name, sub.label),
                provider: sub.provider.unwrap_or(parent.provider),
                ..account.clone()
            };
            SubUnit {
                parent_index,
                parent_key: parent_key.clone(),
                sub_id: sub.id.clone(),
                account,
                display,
                report: sub_as_report(report, sub),
            }
        })
        .collect()
}

/// A configuration whose accounts are followed by every active
/// sub-account of the readings given (PRD 0036 RF1.4), for the engines
/// indexed by configuration position (`resets::plan`, the alert loops of
/// the front ends). Index `i < config.accounts.len()` of the original is
/// unchanged; [`Expanded::origin`] maps the extra ones back.
#[derive(Debug, Clone)]
pub struct Expanded {
    pub config: Config,
    /// One per `config.accounts`: the original readings, then each
    /// sub-account's own.
    pub readings: Vec<Option<Report>>,
    /// One per `config.accounts`: `None` for an original account, the
    /// sub-unit for an appended one (its `report` is also in `readings`).
    pub origin: Vec<Option<SubUnit>>,
}

impl Expanded {
    /// The readings as borrowed options, the shape the engines take.
    pub fn readings(&self) -> Vec<Option<&Report>> {
        self.readings.iter().map(Option::as_ref).collect()
    }

    /// The account a notice names: the display form for a sub-account.
    pub fn display(&self, index: usize) -> &Account {
        match self.origin.get(index).and_then(Option::as_ref) {
            Some(unit) => &unit.display,
            None => &self.config.accounts[index],
        }
    }
}

/// [`Expanded`] of `config` with `readings` (one per account, `None` for
/// an account without a reading).
pub fn expand(config: &Config, readings: &[Option<&Report>]) -> Expanded {
    let mut expanded = Expanded {
        config: config.clone(),
        readings: readings.iter().map(|r| r.cloned()).collect(),
        origin: vec![None; readings.len()],
    };
    expanded.readings.resize(config.accounts.len(), None);
    expanded.origin.resize(config.accounts.len(), None);
    for (index, account) in config.accounts.iter().enumerate() {
        let Some(report) = readings.get(index).copied().flatten() else {
            continue;
        };
        if !account.provider.is_aggregator() || report.provider != account.provider {
            continue;
        }
        for unit in sub_units(index, account, report) {
            expanded.config.accounts.push(unit.account.clone());
            expanded.readings.push(Some(unit.report.clone()));
            expanded.origin.push(Some(unit));
        }
    }
    expanded
}

/// PRD 0036 RF9.3: whether `report` counts as a failure for
/// `--strict-exit`: a failed account, or an aggregator whose instance is
/// ready but one of its active connections failed. (Without the strict
/// flag only the instance's own status counts.)
pub fn has_failure(report: &Report) -> bool {
    report.status != Status::Ready
        || report
            .sub_reports
            .iter()
            .any(|sub| sub_is_active(sub) && sub.status != Status::Ready)
}

/// RF5.2: an e-mail as compared for duplicates: trimmed and case-folded.
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Comma-separated values of `key` in `account.params`, trimmed and
/// lowercased; empty entries are dropped.
fn list_param(account: &Account, key: &str) -> Vec<String> {
    account
        .params
        .get(key)
        .map(|raw| {
            raw.split(',')
                .map(|item| item.trim().to_ascii_lowercase())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether duplicate detection is on for the aggregator `account`
/// (`dedup = "auto"`, the default; `"off"` shows every connection).
pub fn dedup_enabled(account: &Account) -> bool {
    !account
        .params
        .get("dedup")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("off"))
}

/// PRD 0036 RF5.2/RF5.3: marks every connection of `report` (the reading
/// of the aggregator `parent`) that is the same as a direct account of
/// `config`: same mapped provider and the same normalized e-mail, exactly.
/// Never by name and never a guess: a connection or a direct account
/// without an e-mail is never a duplicate. The direct account wins, so
/// `duplicate_of` names its key. A connection listed in the aggregator's
/// `show_both` parameter (sub ids) stays in both places; `dedup = "off"`
/// turns the detection off. Pure: it reads only the configuration.
pub fn mark_duplicates(config: &Config, parent: &Account, report: &mut Report) {
    if !parent.provider.is_aggregator() {
        return;
    }
    let enabled = dedup_enabled(parent);
    let show_both = list_param(parent, "show_both");
    for sub in &mut report.sub_reports {
        sub.duplicate_of = None;
        if !enabled || show_both.contains(&sub.id.to_ascii_lowercase()) {
            continue;
        }
        let (Some(provider), Some(email)) = (sub.provider, sub.identity.as_deref()) else {
            continue;
        };
        sub.duplicate_of = config
            .accounts
            .iter()
            .filter(|direct| !direct.provider.is_aggregator() && direct.provider == provider)
            .find(|direct| {
                crate::identity::email(direct).is_some_and(|own| normalize_email(own) == email)
            })
            .map(Account::key);
    }
}

/// PRD 0036 RF1/RF10.3: what differs between aggregator adapters. Every
/// view in this module (units, pools, dedup, the dashboard link) works on
/// any shape; an adapter adds only its contract, mapping table, access arm
/// and fixtures.
pub trait AggregatorShape: Sync {
    /// Display name of the aggregator kind ("9router", "LiteLLM proxy").
    fn kind(&self) -> &'static str;
    /// The validated base address of an account of this kind.
    fn base(&self, account: &Account) -> Option<url::Url>;
    /// Path of the aggregator's own usage page under the base address, if
    /// it has one ("Open dashboard").
    fn dashboard_path(&self) -> Option<&'static str>;
    /// Note id carrying the instance's version, if the adapter reads one.
    fn version_note(&self) -> Option<&'static str>;
}

struct NineRouterShape;
impl AggregatorShape for NineRouterShape {
    fn kind(&self) -> &'static str {
        Provider::NineRouter.label()
    }
    fn base(&self, account: &Account) -> Option<url::Url> {
        crate::providers::nine_router::account_base(account).ok()
    }
    fn dashboard_path(&self) -> Option<&'static str> {
        Some("dashboard/usage")
    }
    fn version_note(&self) -> Option<&'static str> {
        Some("note.nine_router.version")
    }
}

struct LiteLlmShape;
impl AggregatorShape for LiteLlmShape {
    fn kind(&self) -> &'static str {
        Provider::LiteLlm.label()
    }
    fn base(&self, account: &Account) -> Option<url::Url> {
        crate::providers::litellm::account_base(account).ok()
    }
    fn dashboard_path(&self) -> Option<&'static str> {
        // The LiteLLM admin UI is served under `/ui`.
        Some("ui")
    }
    fn version_note(&self) -> Option<&'static str> {
        None
    }
}

/// The shape of an aggregator provider; `None` for every other provider.
pub fn shape(provider: Provider) -> Option<&'static dyn AggregatorShape> {
    match provider {
        Provider::NineRouter => Some(&NineRouterShape),
        Provider::LiteLlm => Some(&LiteLlmShape),
        _ => None,
    }
}

/// PRD 0036 RF8.2: the usage page of the aggregator's own dashboard
/// (`<base_url>/dashboard/usage` for 9router), from the validated base
/// address; `None` for another provider or an invalid address. Front ends
/// open it in the browser ("Open dashboard"); Cuotai never requests it.
pub fn dashboard_url(account: &Account) -> Option<String> {
    let shape = shape(account.provider)?;
    let base = shape.base(account)?;
    base.join(shape.dashboard_path()?).ok().map(String::from)
}

/// PRD 0036 RF8.5: the host (and port) of the aggregator's address, the only
/// part of it the details show.
pub fn instance_host(account: &Account) -> Option<String> {
    let base = shape(account.provider)?.base(account)?;
    let host = base.host_str()?.to_owned();
    Some(match base.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// The instance's version an aggregator report noted ("0.5.109"), if any.
pub fn instance_version(report: &Report) -> Option<String> {
    report
        .notes_l10n
        .iter()
        .find(|note| {
            Some(note.id.as_ref()) == shape(report.provider).and_then(|s| s.version_note())
        })
        .and_then(|note| match note.params.get("version")? {
            ParamValue::Text(text) => Some(text.clone()),
            ParamValue::L10n(_) => None,
        })
}

/// True for a sub-account that takes part in rankings: on in the aggregator
/// and not a duplicate of a direct account (PRD 0036 RF5.3).
pub fn sub_is_active(sub: &SubReport) -> bool {
    sub.enabled_upstream != Some(false) && sub.duplicate_of.is_none()
}

/// The class of one sub-account, with the rules of a direct account.
pub fn sub_class(sub: &SubReport) -> AccountClass {
    classify_parts(sub.status, &sub.metrics)
}

/// The class of an aggregator account (PRD 0036 RF1.5): `Available` when at
/// least one active sub-account is, `Exhausted` when the measured ones all
/// are, `Unmeasured` otherwise (including a failed instance).
pub(crate) fn pool_class(report: &Report) -> AccountClass {
    if report.status != Status::Ready {
        return AccountClass::Unmeasured;
    }
    let mut exhausted = false;
    for sub in report.sub_reports.iter().filter(|sub| sub_is_active(sub)) {
        match sub_class(sub) {
            AccountClass::Available => return AccountClass::Available,
            AccountClass::Exhausted => exhausted = true,
            AccountClass::Unmeasured => {}
        }
    }
    if exhausted {
        AccountClass::Exhausted
    } else {
        AccountClass::Unmeasured
    }
}

/// The connections that share one upstream (PRD 0036 RF6.3): since the
/// aggregator falls back between them, "N of M available" is the figure that
/// matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    /// The aggregator's upstream slug, verbatim.
    pub upstream: String,
    pub provider: Option<Provider>,
    /// Active connections with at least some quota left.
    pub available: usize,
    /// Active connections (on in the aggregator, not duplicates).
    pub total: usize,
    /// Active connections whose measured quota is used up.
    pub exhausted: usize,
    /// When every connection is exhausted: the earliest time one returns.
    pub returns_at: Option<DateTime<Utc>>,
}

impl Pool {
    /// Display name: the mapped provider's, or the upstream slug.
    pub fn name(&self) -> String {
        self.provider.map_or_else(
            || self.upstream.clone(),
            |provider| provider.label().to_string(),
        )
    }

    /// True when every active connection is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.total > 0 && self.exhausted == self.total
    }

    /// "Claude · 2 of 3 available", "Claude · all exhausted · returns …".
    pub fn summary(&self) -> Localized {
        let name = self.name();
        if self.is_exhausted() {
            return match self.returns_at {
                Some(at) => Localized::with_params(
                    "pool.exhausted_returns",
                    params([
                        ("upstream", ParamValue::from(name.clone())),
                        ("at", ParamValue::from(at.to_rfc3339())),
                    ]),
                    "{upstream} · all exhausted · returns {at}",
                ),
                None => Localized::with_param(
                    "pool.exhausted",
                    "upstream",
                    name.as_str(),
                    "{upstream} · all exhausted",
                ),
            };
        }
        if self.available == 0 {
            return Localized::with_params(
                "pool.unmeasured",
                params([
                    ("upstream", ParamValue::from(name.clone())),
                    ("total", ParamValue::from(self.total.to_string())),
                ]),
                "{upstream} · {total} connections with unknown usage",
            );
        }
        Localized::with_params(
            "pool.available",
            params([
                ("upstream", ParamValue::from(name.clone())),
                ("available", ParamValue::from(self.available.to_string())),
                ("total", ParamValue::from(self.total.to_string())),
            ]),
            "{upstream} · {available} of {total} available",
        )
    }
}

/// When a sub-account's exhausted quotas are all back: the latest reset of
/// its exhausted metrics, or `None` if one of them has no reset time.
fn returns_at(sub: &SubReport) -> Option<DateTime<Utc>> {
    use crate::metrics::{classify_metric, MetricState};
    let mut latest: Option<DateTime<Utc>> = None;
    for metric in &sub.metrics {
        if matches!(classify_metric(metric), MetricState::Exhausted { .. }) {
            let at = metric.resets_at?;
            latest = Some(latest.map_or(at, |known| known.max(at)));
        }
    }
    latest
}

/// The pools of an aggregator report, in order of first appearance.
/// Connections off in the aggregator and duplicates of direct accounts are
/// not counted.
pub fn pools(report: &Report) -> Vec<Pool> {
    let mut pools: Vec<Pool> = Vec::new();
    for sub in report.sub_reports.iter().filter(|sub| sub_is_active(sub)) {
        let index = pools
            .iter()
            .position(|pool| pool.upstream == sub.upstream)
            .unwrap_or_else(|| {
                pools.push(Pool {
                    upstream: sub.upstream.clone(),
                    provider: sub.provider,
                    available: 0,
                    total: 0,
                    exhausted: 0,
                    returns_at: None,
                });
                pools.len() - 1
            });
        let pool = &mut pools[index];
        pool.total += 1;
        match sub_class(sub) {
            AccountClass::Available => pool.available += 1,
            AccountClass::Exhausted => pool.exhausted += 1,
            AccountClass::Unmeasured => {}
        }
    }
    for pool in &mut pools {
        if pool.is_exhausted() {
            pool.returns_at = report
                .sub_reports
                .iter()
                .filter(|sub| sub_is_active(sub) && sub.upstream == pool.upstream)
                .filter_map(returns_at)
                .min();
        }
    }
    pools
}

/// Result of testing an aggregator connection before saving it (PRD 0036
/// RF2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregatorTest {
    pub outcome: TestOutcome,
    /// One sentence for the outcome; never a body, URL or token.
    pub message: Localized,
}

impl AggregatorTest {
    /// The test result of `outcome`, with its core sentence.
    pub fn new(outcome: TestOutcome) -> Self {
        let message = crate::providers::nine_router::outcome_text(&outcome);
        Self { outcome, message }
    }

    /// The test result of `outcome` for an instance of `provider`: a
    /// LiteLLM proxy gets its own sentences, never the 9router ones.
    pub fn for_provider(provider: crate::Provider, outcome: TestOutcome) -> Self {
        if provider == crate::Provider::LiteLlm {
            if let Some(message) = crate::providers::nine_router::litellm_outcome_text(&outcome) {
                return Self { outcome, message };
            }
        }
        Self::new(outcome)
    }

    /// True when the instance is reachable and authorized.
    pub fn is_ok(&self) -> bool {
        matches!(self.outcome, TestOutcome::Connected { .. })
    }

    /// PRD 0036 RF2.3: the SHA-256 fingerprint ("AB:CD:…") of a
    /// certificate the user may choose to trust (pin) after checking it on
    /// the server: an untrusted or a changed one. Saving it is the front
    /// end's explicit, confirmed step; the core never pins by itself.
    pub fn fingerprint(&self) -> Option<&str> {
        match &self.outcome {
            TestOutcome::UntrustedCertificate { fingerprint }
            | TestOutcome::CertificateChanged { fingerprint } => Some(fingerprint),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn litellm_test_sentences_never_name_nine_router() {
        for outcome in [
            TestOutcome::Connected {
                version: None,
                connections: 3,
            },
            TestOutcome::NotNineRouter,
            TestOutcome::PasswordWrong,
            TestOutcome::PasswordRequired,
            TestOutcome::Unreachable,
        ] {
            let test = AggregatorTest::for_provider(crate::Provider::LiteLlm, outcome);
            assert!(
                !test.message.id.contains("nine_router")
                    || test.message.id == "test.nine_router.unreachable",
                "{}",
                test.message.id
            );
            assert!(
                !test.message.fallback.contains("9router"),
                "{}",
                test.message.fallback
            );
        }
    }
    use crate::model::MetricKind;
    use chrono::TimeZone;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 25, hour, 0, 0).unwrap()
    }

    fn quota(remaining: f64, resets: Option<DateTime<Utc>>) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: "weekly (7d)".into(),
            used: Some(100.0 - remaining),
            limit: Some(100.0),
            remaining: Some(remaining),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: None,
            period: None,
            resets_at: resets,
        }
    }

    fn sub(id: &str, upstream: &str, status: Status, metrics: Vec<Metric>) -> SubReport {
        SubReport {
            id: id.into(),
            upstream: upstream.into(),
            provider: crate::providers::nine_router::map_upstream(upstream),
            label: id.into(),
            status,
            metrics,
            notes_l10n: Vec::new(),
            checked_at: at(12),
            as_of: None,
            enabled_upstream: Some(true),
            duplicate_of: None,
            plan: None,
            via: None,
            identity: None,
        }
    }

    fn report(status: Status, subs: Vec<SubReport>) -> Report {
        Report {
            overage_hint: Default::default(),
            as_of: Default::default(),
            account_id: None,
            diagnostic: None,
            account: "home".into(),
            provider: Provider::NineRouter,
            checked_at: at(12),
            status,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: subs,
        }
    }

    #[test]
    fn units_flatten_direct_accounts_and_sub_accounts_in_order() {
        let direct = Report {
            account: "work".into(),
            provider: Provider::Claude,
            sub_reports: Vec::new(),
            ..report(Status::Ready, Vec::new())
        };
        let router = report(
            Status::Ready,
            vec![
                sub("a1", "claude", Status::Ready, vec![quota(50.0, None)]),
                sub("b2", "glm", Status::Error, Vec::new()),
            ],
        );
        let snapshot = Snapshot {
            started_at: at(12),
            finished_at: at(12),
            reports: vec![direct, router],
        };
        let units = snapshot.units();
        let keys: Vec<_> = units.iter().map(|unit| unit.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "claude:work",
                "nine_router:home",
                "nine_router:home/a1",
                "nine_router:home/b2"
            ]
        );
        assert_eq!(units[0].parent, None);
        assert_eq!(units[2].parent.as_deref(), Some("nine_router:home"));
        assert_eq!(units[2].provider, Some(Provider::Claude));
        // An upstream Cuotai does not know has no provider.
        assert_eq!(units[3].provider, None);
        // A failed sub-account keeps its own status.
        assert_eq!(units[3].status, Status::Error);
    }

    fn router_account() -> Account {
        Account {
            group: Default::default(),
            id: None,
            name: "home".into(),
            provider: Provider::NineRouter,
            credential_env: Some("NINEROUTER_PASSWORD".into()),
            oauth: None,
            session: None,
            params: [("base_url".to_string(), "http://127.0.0.1:20128".to_string())]
                .into_iter()
                .collect(),
            refresh_seconds: Some(300),
        }
    }

    #[test]
    fn sub_units_present_active_connections_as_direct_accounts() {
        let mut off = sub("c3", "claude", Status::Ready, vec![quota(10.0, None)]);
        off.enabled_upstream = Some(false);
        let mut dup = sub("d4", "claude", Status::Ready, vec![quota(10.0, None)]);
        dup.duplicate_of = Some("claude:work".into());
        let router = report(
            Status::Ready,
            vec![
                sub("a1", "claude", Status::Ready, vec![quota(50.0, None)]),
                sub("b2", "glm", Status::Error, Vec::new()),
                off,
                dup,
            ],
        );
        let units = sub_units(3, &router_account(), &router);
        let keys: Vec<_> = units.iter().map(|unit| unit.account.key()).collect();
        assert_eq!(keys, ["nine_router:home/a1", "nine_router:home/b2"]);
        let unit = &units[0];
        // Report and account keys agree with `Snapshot::units()`.
        assert_eq!(unit.report.account_key(), unit.account.key());
        assert_eq!(unit.parent_key, "nine_router:home");
        assert_eq!(unit.parent_index, 3);
        // No credential or parameter is carried over.
        assert!(unit.account.credential_env.is_none());
        assert!(unit.account.params.is_empty());
        assert!(unit.report.sub_reports.is_empty());
        // Notices name the mapped provider and the connection label.
        assert_eq!(unit.display.provider, Provider::Claude);
        assert_eq!(unit.display.name, "home · a1");
        // An unknown upstream keeps the aggregator as provider.
        assert_eq!(units[1].display.provider, Provider::NineRouter);
        // Engines see the same binding a direct account would have.
        assert_eq!(
            crate::metrics::binding(&unit.report).map(|b| b.label),
            Some("weekly (7d)".to_string())
        );
    }

    #[test]
    fn expand_appends_sub_accounts_after_the_configuration() {
        let direct = Account {
            name: "work".into(),
            provider: Provider::Claude,
            ..router_account()
        };
        let config = Config {
            groups: Default::default(),
            refresh_seconds: 60,
            concurrency: 2,
            timeout_seconds: 5,
            accounts: vec![direct, router_account()],
        };
        let router = report(
            Status::Ready,
            vec![sub("a1", "claude", Status::Ready, vec![quota(50.0, None)])],
        );
        let expanded = expand(&config, &[None, Some(&router)]);
        assert_eq!(expanded.config.accounts.len(), 3);
        assert_eq!(expanded.readings().len(), 3);
        assert!(expanded.readings[0].is_none());
        assert!(expanded.origin[1].is_none());
        assert_eq!(expanded.config.accounts[2].key(), "nine_router:home/a1");
        assert_eq!(expanded.display(2).name, "home · a1");
        assert_eq!(expanded.display(0).name, "work");
        // Without readings nothing is appended.
        assert_eq!(expand(&config, &[]).config.accounts.len(), 2);
    }

    fn dedup_config(router: Account) -> Config {
        let direct = Account {
            name: "work".into(),
            provider: Provider::Claude,
            credential_env: None,
            params: [("email".to_string(), " Ana@Example.com ".to_string())]
                .into_iter()
                .collect(),
            ..router_account()
        };
        Config {
            groups: Default::default(),
            refresh_seconds: 60,
            concurrency: 2,
            timeout_seconds: 5,
            accounts: vec![direct, router],
        }
    }

    fn with_identity(id: &str, upstream: &str, email: Option<&str>) -> SubReport {
        SubReport {
            identity: email.map(normalize_email),
            ..sub(id, upstream, Status::Ready, vec![quota(10.0, None)])
        }
    }

    #[test]
    fn duplicates_match_provider_and_exact_email_only() {
        let config = dedup_config(router_account());
        let mut router = report(
            Status::Ready,
            vec![
                with_identity("same", "claude", Some("ana@example.com")),
                with_identity("other", "claude", Some("bia@example.com")),
                with_identity("none", "claude", None),
                with_identity("codex", "codex", Some("ana@example.com")),
            ],
        );
        mark_duplicates(&config, &config.accounts[1], &mut router);
        let dup: Vec<_> = router
            .sub_reports
            .iter()
            .map(|s| s.duplicate_of.as_deref())
            .collect();
        assert_eq!(dup, [Some("claude:work"), None, None, None]);
        // The duplicate is not an active unit: it never alerts or ranks.
        let units = sub_units(1, &config.accounts[1], &router);
        assert_eq!(units.len(), 3);
        assert!(units.iter().all(|unit| unit.sub_id != "same"));
        // The e-mail never reaches the serialized report.
        let json = serde_json::to_string(&router).unwrap();
        assert!(!json.contains("example.com"));
    }

    #[test]
    fn show_both_and_dedup_off_keep_the_connection() {
        let mut both = router_account();
        both.params.insert("show_both".into(), "same".into());
        let config = dedup_config(both);
        let mut router = report(
            Status::Ready,
            vec![with_identity("same", "claude", Some("ana@example.com"))],
        );
        mark_duplicates(&config, &config.accounts[1], &mut router);
        assert_eq!(router.sub_reports[0].duplicate_of, None);

        let mut off = router_account();
        off.params.insert("dedup".into(), "off".into());
        let config = dedup_config(off);
        mark_duplicates(&config, &config.accounts[1], &mut router);
        assert_eq!(router.sub_reports[0].duplicate_of, None);
    }

    #[test]
    fn a_failed_active_connection_is_a_strict_failure() {
        let ok = report(
            Status::Ready,
            vec![sub("a", "claude", Status::Ready, Vec::new())],
        );
        assert!(!has_failure(&ok));
        let mut bad = report(
            Status::Ready,
            vec![sub("a", "claude", Status::Error, Vec::new())],
        );
        assert!(has_failure(&bad));
        bad.sub_reports[0].duplicate_of = Some("claude:work".into());
        assert!(!has_failure(&bad), "a duplicate never counts");
        assert!(has_failure(&report(Status::Error, Vec::new())));
    }

    #[test]
    fn a_group_on_the_aggregator_applies_to_its_connections() {
        let mut parent = router_account();
        assert_eq!(
            sub_account(&parent, &sub("a", "claude", Status::Ready, Vec::new())).group,
            None
        );
        parent.group = Some("team".into());
        let account = sub_account(&parent, &sub("a", "claude", Status::Ready, Vec::new()));
        assert_eq!(account.group.as_deref(), Some("team"));
    }

    #[test]
    fn a_failed_instance_has_no_sub_accounts_and_is_unmeasured() {
        let failed = report(Status::AuthenticationError, Vec::new());
        assert_eq!(pool_class(&failed), AccountClass::Unmeasured);
        let snapshot = Snapshot {
            started_at: at(12),
            finished_at: at(12),
            reports: vec![failed],
        };
        assert_eq!(snapshot.units().len(), 1);
    }

    #[test]
    fn pool_class_follows_the_active_measured_connections() {
        let ready = |remaining| sub("x", "claude", Status::Ready, vec![quota(remaining, None)]);
        // One available connection is enough.
        let mixed = report(Status::Ready, vec![ready(0.0), ready(40.0)]);
        assert_eq!(pool_class(&mixed), AccountClass::Available);
        // All measured connections exhausted.
        let all_out = report(Status::Ready, vec![ready(0.0), ready(0.0)]);
        assert_eq!(pool_class(&all_out), AccountClass::Exhausted);
        // An unmeasured connection does not hide the exhaustion of the others.
        let with_unknown = report(
            Status::Ready,
            vec![ready(0.0), sub("y", "claude", Status::Error, Vec::new())],
        );
        assert_eq!(pool_class(&with_unknown), AccountClass::Exhausted);
        // Nothing measured.
        let none = report(
            Status::Ready,
            vec![sub("y", "claude", Status::Error, Vec::new())],
        );
        assert_eq!(pool_class(&none), AccountClass::Unmeasured);
        // Off connections and duplicates do not count.
        let mut off = ready(80.0);
        off.enabled_upstream = Some(false);
        let mut dup = ready(80.0);
        dup.duplicate_of = Some("claude:work".into());
        let hidden = report(Status::Ready, vec![off, dup]);
        assert_eq!(pool_class(&hidden), AccountClass::Unmeasured);
        // The dispatcher in metrics uses the pool class for aggregators.
        assert_eq!(
            crate::metrics::classify_report(Some(&mixed)),
            AccountClass::Available
        );
    }

    #[test]
    fn pools_count_available_connections_per_upstream() {
        let router = report(
            Status::Ready,
            vec![
                sub("a", "claude", Status::Ready, vec![quota(40.0, None)]),
                sub("b", "claude", Status::Ready, vec![quota(0.0, Some(at(20)))]),
                sub("c", "claude", Status::Ready, vec![quota(10.0, None)]),
                sub("d", "glm", Status::Ready, vec![quota(10.0, None)]),
            ],
        );
        let pools = pools(&router);
        assert_eq!(pools.len(), 2);
        assert_eq!(pools[0].upstream, "claude");
        assert_eq!((pools[0].available, pools[0].total), (2, 3));
        assert_eq!(pools[0].summary().id, "pool.available");
        assert_eq!(pools[0].summary().fallback, "Claude · 2 of 3 available");
        // An upstream Cuotai does not know keeps the verbatim slug.
        assert_eq!(pools[1].name(), "glm");
    }

    #[test]
    fn an_exhausted_pool_returns_when_the_earliest_connection_does() {
        let router = report(
            Status::Ready,
            vec![
                sub("a", "claude", Status::Ready, vec![quota(0.0, Some(at(20)))]),
                sub("b", "claude", Status::Ready, vec![quota(0.0, Some(at(18)))]),
            ],
        );
        let pool = &pools(&router)[0];
        assert!(pool.is_exhausted());
        assert_eq!(pool.returns_at, Some(at(18)));
        assert_eq!(pool.summary().id, "pool.exhausted_returns");

        // One connection without a reset time: still the known earliest.
        let router = report(
            Status::Ready,
            vec![
                sub("a", "claude", Status::Ready, vec![quota(0.0, None)]),
                sub("b", "claude", Status::Ready, vec![quota(0.0, Some(at(18)))]),
            ],
        );
        assert_eq!(pools(&router)[0].returns_at, Some(at(18)));

        // No reset time at all.
        let router = report(
            Status::Ready,
            vec![sub("a", "claude", Status::Ready, vec![quota(0.0, None)])],
        );
        let pool = &pools(&router)[0];
        assert_eq!(pool.returns_at, None);
        assert_eq!(pool.summary().id, "pool.exhausted");
    }

    #[test]
    fn a_pool_with_nothing_measured_says_so_instead_of_zero() {
        let router = report(
            Status::Ready,
            vec![sub("a", "claude", Status::Error, Vec::new())],
        );
        let pool = &pools(&router)[0];
        assert_eq!((pool.available, pool.exhausted, pool.total), (0, 0, 1));
        assert!(!pool.is_exhausted());
        assert_eq!(pool.summary().id, "pool.unmeasured");
    }

    #[test]
    fn the_dashboard_and_host_come_from_the_validated_address() {
        let mut account = router_account();
        assert_eq!(
            dashboard_url(&account).as_deref(),
            Some("http://127.0.0.1:20128/dashboard/usage")
        );
        assert_eq!(instance_host(&account).as_deref(), Some("127.0.0.1:20128"));
        account
            .params
            .insert("base_url".into(), "https://router.example.com/9r/".into());
        assert_eq!(
            dashboard_url(&account).as_deref(),
            Some("https://router.example.com/9r/dashboard/usage")
        );
        assert_eq!(
            instance_host(&account).as_deref(),
            Some("router.example.com")
        );
        // An address the rules refuse, or another provider, has neither.
        account
            .params
            .insert("base_url".into(), "http://user:pw@10.0.0.1/".into());
        assert!(dashboard_url(&account).is_none() && instance_host(&account).is_none());
        let direct = Account {
            provider: Provider::Claude,
            ..router_account()
        };
        assert!(dashboard_url(&direct).is_none());
    }

    #[test]
    fn sub_reports_are_additive_in_json() {
        let plain = report(Status::Ready, Vec::new());
        let json = serde_json::to_value(&plain).unwrap();
        assert!(json.get("sub_reports").is_none(), "empty must be skipped");
        let router = report(
            Status::Ready,
            vec![sub("a", "claude", Status::Ready, vec![quota(40.0, None)])],
        );
        let json = serde_json::to_value(&router).unwrap();
        assert_eq!(json["sub_reports"][0]["upstream"], "claude");
        assert_eq!(json["sub_reports"][0]["provider"], "claude");
        let back: Report = serde_json::from_value(json).unwrap();
        assert_eq!(back.sub_reports.len(), 1);
    }
}
