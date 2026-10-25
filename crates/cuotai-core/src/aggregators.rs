//! Views over aggregator accounts (PRD 0036): the flattened list of units,
//! the pool summary and the connection test.
//!
//! An **aggregator** (today only 9router) holds credentials for several
//! upstream providers. Its [`Report`] describes the instance; every upstream
//! connection is a [`SubReport`] in `Report::sub_reports`. Nothing here
//! queries: these are pure functions over a snapshot.

use crate::metrics::{classify_parts, AccountClass};
use crate::model::{Metric, Provider, Report, Snapshot, Status, SubReport};
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
                    format!("{name} · todas esgotadas · volta em {}", at.to_rfc3339()),
                ),
                None => Localized::with_param(
                    "pool.exhausted",
                    "upstream",
                    name.as_str(),
                    format!("{name} · todas esgotadas"),
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
                format!("{name} · {} conexões sem uso conhecido", self.total),
            );
        }
        Localized::with_params(
            "pool.available",
            params([
                ("upstream", ParamValue::from(name.clone())),
                ("available", ParamValue::from(self.available.to_string())),
                ("total", ParamValue::from(self.total.to_string())),
            ]),
            format!("{name} · {} de {} disponíveis", self.available, self.total),
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
    pub(crate) fn new(outcome: TestOutcome) -> Self {
        let message = crate::providers::nine_router::outcome_text(&outcome);
        Self { outcome, message }
    }

    /// True when the instance is reachable and authorized.
    pub fn is_ok(&self) -> bool {
        matches!(self.outcome, TestOutcome::Connected { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }

    fn report(status: Status, subs: Vec<SubReport>) -> Report {
        Report {
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
        assert_eq!(pools[0].summary().fallback, "Claude · 2 de 3 disponíveis");
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
