//! Quota reset planner (PRD 0003 RF1).
//!
//! A pure function over the configuration, the last reports, an injected
//! `now` and the notification preferences: no clock, no network. It only
//! uses `resets_at` already reported by the provider; front ends schedule
//! and deliver the notices.

use crate::metrics::{classify_metric, MetricState};
use crate::model::{Config, Metric, MetricKind, MetricWindow, Report};
use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use std::collections::BTreeSet;

/// Two `resets_at` of the same metric this close are the same reset
/// (rolling windows drift between queries).
pub const DRIFT_TOLERANCE: TimeDelta = TimeDelta::minutes(2);
/// Metrics of one account resetting this close become one grouped event.
const GROUP_SPAN: TimeDelta = TimeDelta::minutes(1);

/// When a reset is worth a notice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotifyWhen {
    /// Some quota was consumed (`used > 0` or `remaining < limit`).
    #[default]
    OnlyIfUsed,
    OnlyIfExhausted,
    Always,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefs {
    /// Notice this long before `resets_at`; zero notifies at the reset.
    pub lead: TimeDelta,
    pub when: NotifyWhen,
    /// Let `MetricWindow::Other` quotas in.
    pub include_other: bool,
    /// [`crate::Account::key`]s whose notices are off.
    pub disabled: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResetMetric {
    pub label: String,
    pub window: MetricWindow,
    pub resets_at: DateTime<Utc>,
}

/// One notice: the metrics of one account resetting within the same minute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResetEvent {
    /// Position in `Config::accounts` (and the snapshot).
    pub account_index: usize,
    pub account_key: String,
    /// Sorted by `resets_at`; never empty.
    pub metrics: Vec<ResetMetric>,
    /// Earliest `resets_at` of the group.
    pub resets_at: DateTime<Utc>,
    /// `resets_at − lead`; may already be past when the lead window opened
    /// before this query.
    pub notify_at: DateTime<Utc>,
}

/// Identity of one metric reset: account key + metric label + `resets_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResetKey {
    pub account_key: String,
    pub label: String,
    pub resets_at: DateTime<Utc>,
}

impl ResetKey {
    /// Same account and metric with `resets_at` within [`DRIFT_TOLERANCE`].
    /// A reset postponed beyond it is a new reset: the old notice is
    /// cancelled and the new one scheduled.
    pub fn same_reset(&self, other: &ResetKey) -> bool {
        self.account_key == other.account_key
            && self.label == other.label
            && (self.resets_at - other.resets_at).abs() <= DRIFT_TOLERANCE
    }
}

impl ResetEvent {
    pub fn keys(&self) -> impl Iterator<Item = ResetKey> + '_ {
        self.metrics.iter().map(|metric| ResetKey {
            account_key: self.account_key.clone(),
            label: metric.label.clone(),
            resets_at: metric.resets_at,
        })
    }
}

/// Upcoming reset notices, sorted by `notify_at` then account order.
/// `reports` follow `config.accounts` (a `Snapshot`); a report that no
/// longer matches its account (provider and name) is skipped. Eligible: a
/// `Quota` metric with a future `resets_at` in a `window_5h`/`daily`/
/// `weekly`/`monthly` window (`other` only with `include_other`) passing
/// `prefs.when`; a missing value is neither used nor exhausted.
pub fn upcoming(
    config: &Config,
    reports: &[Report],
    now: DateTime<Utc>,
    prefs: &Prefs,
) -> Vec<ResetEvent> {
    let mut events = Vec::new();
    for (account_index, (account, report)) in config.accounts.iter().zip(reports).enumerate() {
        if report.provider != account.provider || report.account != account.name {
            continue;
        }
        let account_key = account.key();
        if prefs.disabled.contains(&account_key) {
            continue;
        }
        let mut due: Vec<ResetMetric> = report
            .metrics
            .iter()
            .filter_map(|metric| eligible(metric, now, prefs))
            .collect();
        due.sort_by_key(|metric| metric.resets_at);
        let mut rest = due.into_iter().peekable();
        while let Some(first) = rest.next() {
            let resets_at = first.resets_at;
            let mut metrics = vec![first];
            while let Some(next) = rest.next_if(|m| m.resets_at - resets_at < GROUP_SPAN) {
                metrics.push(next);
            }
            events.push(ResetEvent {
                account_index,
                account_key: account_key.clone(),
                metrics,
                resets_at,
                notify_at: resets_at - prefs.lead,
            });
        }
    }
    events.sort_by_key(|event| (event.notify_at, event.account_index));
    events
}

fn eligible(metric: &Metric, now: DateTime<Utc>, prefs: &Prefs) -> Option<ResetMetric> {
    let resets_at = metric.resets_at.filter(|at| *at > now)?;
    let window = metric.window.filter(|window| match window {
        MetricWindow::Other => prefs.include_other,
        _ => true,
    })?;
    if metric.kind != MetricKind::Quota {
        return None;
    }
    let exhausted = matches!(classify_metric(metric), MetricState::Exhausted { .. });
    let finite = |value: Option<f64>| value.filter(|v| v.is_finite());
    let used = finite(metric.used).is_some_and(|used| used > 0.0)
        || matches!(
            (finite(metric.remaining), finite(metric.limit)),
            (Some(remaining), Some(limit)) if remaining < limit
        );
    let wanted = match prefs.when {
        NotifyWhen::OnlyIfUsed => used,
        NotifyWhen::OnlyIfExhausted => exhausted,
        NotifyWhen::Always => true,
    };
    wanted.then(|| ResetMetric {
        label: metric.label.clone(),
        window,
        resets_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Account, Provider, Status};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 15, 12, 0, 0).unwrap()
    }

    fn mins(n: i64) -> TimeDelta {
        TimeDelta::minutes(n)
    }

    fn quota(label: &str, used: Option<f64>, resets_at: Option<DateTime<Utc>>) -> Metric {
        Metric {
            label: label.into(),
            used,
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: None,
            resets_at,
        }
    }

    fn setup(metrics: Vec<Vec<Metric>>) -> (Config, Vec<Report>) {
        let accounts: Vec<Account> = (0..metrics.len())
            .map(|i| Account {
                name: format!("acc{i}"),
                provider: Provider::Claude,
                ..Account::default()
            })
            .collect();
        let reports = accounts
            .iter()
            .zip(metrics)
            .map(|(account, metrics)| Report {
                account: account.name.clone(),
                provider: account.provider,
                checked_at: now(),
                status: Status::Ready,
                metrics,
                notes: Vec::new(),
                notes_l10n: Vec::new(),
                source: String::new(),
            })
            .collect();
        let config = Config {
            accounts,
            ..Config::default()
        };
        (config, reports)
    }

    fn labels(metrics: Vec<Metric>, prefs: &Prefs) -> Vec<String> {
        let (config, reports) = setup(vec![metrics]);
        upcoming(&config, &reports, now(), prefs)
            .into_iter()
            .flat_map(|event| event.metrics.into_iter().map(|m| m.label))
            .collect()
    }

    #[test]
    fn eligibility_table() {
        let soon = Some(now() + mins(30));
        let with = |mut metric: Metric, edit: fn(&mut Metric)| {
            edit(&mut metric);
            metric
        };
        let used = quota("used", Some(10.0), soon);
        let idle = quota("idle", Some(0.0), soon);
        let spent = quota("spent", Some(100.0), soon);
        let below = with(quota("below", None, soon), |m| m.remaining = Some(60.0));
        let full = with(quota("full", None, soon), |m| m.remaining = Some(100.0));
        let exhausted_without_limit = with(quota("exhausted-no-limit", None, soon), |m| {
            m.limit = None;
            m.remaining = Some(0.0);
        });
        let unknown = quota("unknown", None, soon);
        let base = vec![
            used.clone(),
            idle.clone(),
            spent.clone(),
            below.clone(),
            full.clone(),
            unknown.clone(),
            exhausted_without_limit.clone(),
        ];
        let prefs = |when| Prefs {
            when,
            ..Prefs::default()
        };
        let other = with(quota("other", Some(10.0), soon), |m| {
            m.window = Some(MetricWindow::Other)
        });
        let cases: Vec<(&str, Vec<Metric>, Prefs, Vec<&str>)> = vec![
            (
                "only if used: used > 0 or remaining < limit; missing limit is not used",
                base.clone(),
                Prefs::default(),
                vec!["used", "spent", "below"],
            ),
            (
                "only if exhausted, including remaining zero without a reported limit",
                base.clone(),
                prefs(NotifyWhen::OnlyIfExhausted),
                vec!["spent", "exhausted-no-limit"],
            ),
            (
                "always",
                base,
                prefs(NotifyWhen::Always),
                vec![
                    "used",
                    "idle",
                    "spent",
                    "below",
                    "full",
                    "unknown",
                    "exhausted-no-limit",
                ],
            ),
            (
                "no resets_at",
                vec![quota("none", Some(10.0), None)],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "past or current resets_at",
                vec![
                    quota("past", Some(10.0), Some(now() - mins(1))),
                    quota("now", Some(10.0), Some(now())),
                ],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "rate limit and balance never enter",
                vec![
                    with(used.clone(), |m| m.kind = MetricKind::RateLimit),
                    with(used.clone(), |m| m.kind = MetricKind::Balance),
                ],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "no window",
                vec![with(used.clone(), |m| m.window = None)],
                prefs(NotifyWhen::Always),
                vec![],
            ),
            (
                "other window off",
                vec![other.clone()],
                Prefs::default(),
                vec![],
            ),
            (
                "other window on",
                vec![other],
                Prefs {
                    include_other: true,
                    ..Prefs::default()
                },
                vec!["other"],
            ),
        ];
        for (case, metrics, prefs, expected) in cases {
            let mut got = labels(metrics, &prefs);
            got.sort();
            let mut expected: Vec<String> = expected.into_iter().map(String::from).collect();
            expected.sort();
            assert_eq!(got, expected, "{case}");
        }
    }

    #[test]
    fn groups_same_minute_per_account_and_applies_lead() {
        let at = now() + mins(60);
        let (config, reports) = setup(vec![
            vec![
                quota("weekly", Some(5.0), Some(at + TimeDelta::seconds(40))),
                quota("5h", Some(5.0), Some(at)),
                quota("later", Some(5.0), Some(at + mins(5))),
            ],
            vec![quota("5h", Some(5.0), Some(at))],
        ]);
        let prefs = Prefs {
            lead: mins(15),
            ..Prefs::default()
        };
        let events = upcoming(&config, &reports, now(), &prefs);
        let shape: Vec<(usize, Vec<&str>, DateTime<Utc>)> = events
            .iter()
            .map(|e| {
                let labels = e.metrics.iter().map(|m| m.label.as_str()).collect();
                (e.account_index, labels, e.notify_at)
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (0, vec!["5h", "weekly"], at - mins(15)),
                (1, vec!["5h"], at - mins(15)),
                (0, vec!["later"], at + mins(5) - mins(15)),
            ]
        );
        assert_eq!(events[0].account_key, "claude:acc0");
        assert_eq!(events[0].resets_at, at);
    }

    #[test]
    fn disabled_or_mismatched_account_is_skipped() {
        let soon = Some(now() + mins(30));
        let (mut config, reports) = setup(vec![
            vec![quota("a", Some(5.0), soon)],
            vec![quota("b", Some(5.0), soon)],
        ]);
        let prefs = Prefs {
            disabled: BTreeSet::from(["claude:acc0".to_string()]),
            ..Prefs::default()
        };
        let events = upcoming(&config, &reports, now(), &prefs);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].account_index, 1);
        // A renamed account no longer matches its stale report.
        config.accounts[1].name = "renamed".into();
        assert!(upcoming(&config, &reports, now(), &prefs).is_empty());
    }

    #[test]
    fn drift_within_tolerance_is_the_same_reset() {
        let at = now() + mins(60);
        let key = |resets_at| {
            let (config, reports) = setup(vec![vec![quota("5h", Some(5.0), Some(resets_at))]]);
            let events = upcoming(&config, &reports, now(), &Prefs::default());
            let key = events[0].keys().next().unwrap();
            key
        };
        let first = key(at);
        for (case, later, same) in [
            ("drift below tolerance", at + TimeDelta::seconds(90), true),
            ("drift at tolerance", at - DRIFT_TOLERANCE, true),
            ("postponed beyond tolerance", at + mins(3), false),
        ] {
            assert_eq!(first.same_reset(&key(later)), same, "{case}");
        }
        let mut other = first.clone();
        other.label = "weekly".into();
        assert!(!first.same_reset(&other), "different metric");
    }
}
