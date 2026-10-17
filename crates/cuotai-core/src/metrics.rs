//! Behavioral classification of report quotas and per-metric status.
//!
//! These helpers describe *what* a `Report` or `Metric` means, not how to
//! render it. The TUI and any other consumer (CLI, GUI, web, scripts) can
//! drive their own colors, labels, and layouts on top of the same answers.
//! Style, ordering, and presentation live outside this module.
//!
//! Two primitives are exposed:
//!
//! * [`AccountClass`] and [`classify_report`] collapse a whole report (or its
//!   absence) into one of `Available`, `Exhausted`, or `Unmeasured`. The
//!   semantics preserve the existing dashboard behavior: limit-only metrics
//!   do not count as measured; `used = 0, limit = 0` with no `remaining`
//!   value is a sentinel for unknown and stays unmeasured; non-finite values
//!   (NaN, ±inf) are treated as missing signals.
//! * [`MetricState`] and [`classify_metric`] describe a single metric with the
//!   nuance the renderer needs (exhausted with or without a usable cap,
//!   available with a ratio and an uncapped flag).
//!
//! Both primitives intentionally avoid strings, colors, or anything tied to
//! terminal widgets.

use crate::model::{Metric, Report, Status};
use serde::Serialize;

/// High-level classification of an account card based on the latest report.
///
/// `Available` — at least one metric reports finite usage and no measurable
/// metric is fully consumed.
/// `Exhausted` — at least one measurable metric is fully consumed (used ≥ limit
/// with a positive limit, or remaining ≤ 0 including negative debt).
/// `Unmeasured` — no report, non-`Ready` status, or no metric with `used` or
/// `remaining` values (limit-only metrics do not count as measured).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountClass {
    Available,
    Exhausted,
    Unmeasured,
}

impl AccountClass {
    /// True when the class means "no measurable signal" (`Unmeasured`).
    /// Provided as a stable name so callers don't have to compare variants
    /// everywhere they need to decide whether to drop a row.
    pub fn is_unmeasured(self) -> bool {
        matches!(self, AccountClass::Unmeasured)
    }
}

/// Classify an account using the latest report, if any. A `None` report or a
/// non-`Ready` status is never classified as available.
pub fn classify_report(report: Option<&Report>) -> AccountClass {
    let Some(report) = report else {
        return AccountClass::Unmeasured;
    };
    if report.status != Status::Ready {
        return AccountClass::Unmeasured;
    }
    if report.metrics.is_empty() {
        return AccountClass::Unmeasured;
    }
    let has_quota = report.metrics.iter().any(|metric| {
        metric
            .limit
            .is_some_and(|limit| limit.is_finite() && limit > 0.0)
            && has_finite_signal(metric)
    });
    let mut measurable = false;
    for metric in &report.metrics {
        // Limit-only metrics carry no consumption signal; they're not measurable.
        if !has_finite_signal(metric) {
            continue;
        }
        // `used = 0, limit = 0` with no remaining value is a sentinel for
        // unknown — never classify as exhausted or available from it alone.
        let has_unknown_zero_cap =
            matches!((metric.used, metric.limit), (Some(u), Some(l)) if u == 0.0 && l == 0.0);
        if has_unknown_zero_cap && metric.remaining.is_none() {
            continue;
        }
        // Skip non-finite values when present alongside a finite signal —
        // they don't override other metrics' evidence.
        if let Some(remaining) = metric.remaining {
            if !remaining.is_finite() {
                continue;
            }
            if remaining <= 0.0
                && (!has_quota
                    || metric
                        .limit
                        .is_some_and(|limit| limit.is_finite() && limit > 0.0))
            {
                return AccountClass::Exhausted;
            }
        }
        if let (Some(used), Some(limit)) = (metric.used, metric.limit) {
            if !used.is_finite() || !limit.is_finite() {
                continue;
            }
            if limit > 0.0 && used >= limit {
                return AccountClass::Exhausted;
            }
        }
        measurable = true;
    }
    if !measurable {
        return AccountClass::Unmeasured;
    }
    AccountClass::Available
}

/// True when at least one of `used` or `remaining` is a finite number that
/// can drive classification or rendering. Non-finite values (NaN, ±inf) are
/// treated as missing signals.
fn has_finite_signal(metric: &Metric) -> bool {
    match (metric.used, metric.remaining) {
        (Some(u), _) if u.is_finite() => true,
        (_, Some(r)) if r.is_finite() => true,
        _ => false,
    }
}

/// Per-metric rendering state used to drive a gauge bar.
///
/// `Unmeasured` — no usable signal was found.
/// `Exhausted` — fully consumed; `no_cap` is true when exhaustion was inferred
/// from a zero `remaining` value without a usable cap, so the bar should be
/// suppressed.
/// `Available` — partially or not consumed; `ratio` is the remaining fraction
/// in `[0.0, 1.0]`, and `uncapped` is true when no usable cap was provided
/// (balance/cost-only metrics). Never invent a "100% free" reading in that
/// case.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MetricState {
    Unmeasured,
    Exhausted {
        /// When true, the exhaustion was inferred from a zero `remaining`
        /// value without a usable cap; callers should skip the gauge bar and
        /// show the textual label instead.
        no_cap: bool,
    },
    Available {
        ratio: f64,
        /// True when no usable cap was provided; the bar should be suppressed
        /// and the label should read "limite não informado" rather than a free
        /// percentage.
        uncapped: bool,
    },
}

/// Reduce a single metric to one of `Unmeasured` / `Exhausted` / `Available`.
/// See [`MetricState`] for the exact semantics.
pub fn classify_metric(metric: &Metric) -> MetricState {
    let used = metric.used;
    let remaining = metric.remaining;
    let limit = metric.limit;
    if !has_finite_signal(metric) {
        return MetricState::Unmeasured;
    }
    let has_unknown_zero_cap = matches!((used, limit), (Some(u), Some(l)) if u == 0.0 && l == 0.0);
    if has_unknown_zero_cap && remaining.is_none() {
        return MetricState::Unmeasured;
    }
    let usable_limit = match limit {
        Some(cap) if cap.is_finite() && cap > 0.0 => Some(cap),
        _ => None,
    };
    if let Some(value) = remaining {
        if value.is_finite() && value <= 0.0 {
            return MetricState::Exhausted {
                no_cap: usable_limit.is_none(),
            };
        }
    }
    if let (Some(consumed), Some(cap)) = (used, usable_limit) {
        if consumed.is_finite() && consumed >= cap {
            return MetricState::Exhausted { no_cap: false };
        }
    }
    if let Some(cap) = usable_limit {
        if let Some(consumed) = used {
            if consumed.is_finite() {
                let remaining = (cap - consumed).max(0.0);
                return MetricState::Available {
                    ratio: (remaining / cap).clamp(0.0, 1.0),
                    uncapped: false,
                };
            }
        }
        if let Some(value) = remaining {
            if value.is_finite() {
                return MetricState::Available {
                    ratio: (value / cap).clamp(0.0, 1.0),
                    uncapped: false,
                };
            }
        }
    }
    // Balance/cost or usage without a usable cap — never invent a
    // "100% free" reading.
    MetricState::Available {
        ratio: 0.0,
        uncapped: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn metric(
        label: &str,
        used: Option<f64>,
        limit: Option<f64>,
        remaining: Option<f64>,
    ) -> Metric {
        Metric {
            label: label.into(),
            used,
            limit,
            remaining,
            unit: "req".into(),
            kind: crate::MetricKind::Quota,
            window: None,
            period: None,
            resets_at: None,
        }
    }

    fn report(status: Status, metrics: Vec<Metric>) -> Report {
        Report {
            account: "acc".into(),
            provider: crate::model::Provider::OpenAiApi,
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            status,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
        }
    }

    #[test]
    fn classifications_serialize_with_stable_names_and_fields() {
        use serde_json::json;

        assert_eq!(
            serde_json::to_value(AccountClass::Unmeasured).unwrap(),
            json!("unmeasured")
        );
        assert_eq!(
            serde_json::to_value(MetricState::Unmeasured).unwrap(),
            json!({"state": "unmeasured"})
        );
        assert_eq!(
            serde_json::to_value(MetricState::Exhausted { no_cap: true }).unwrap(),
            json!({"state": "exhausted", "no_cap": true})
        );
        assert_eq!(
            serde_json::to_value(MetricState::Available {
                ratio: 0.25,
                uncapped: false
            })
            .unwrap(),
            json!({"state": "available", "ratio": 0.25, "uncapped": false})
        );
    }

    #[test]
    fn missing_report_is_unmeasured() {
        assert_eq!(classify_report(None), AccountClass::Unmeasured);
    }

    #[test]
    fn non_ready_status_is_unmeasured() {
        let report = report(
            Status::AuthenticationError,
            vec![metric("m", Some(1.0), Some(10.0), Some(9.0))],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn limit_only_metric_is_unmeasured() {
        let report = report(Status::Ready, vec![metric("m", None, Some(10.0), None)]);
        assert_eq!(classify_report(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn used_zero_limit_zero_is_unmeasured() {
        let report = report(Status::Ready, vec![metric("m", Some(0.0), Some(0.0), None)]);
        assert_eq!(classify_report(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn remaining_only_with_zero_cap_is_available() {
        let report = report(
            Status::Ready,
            vec![metric("balance", Some(0.0), Some(0.0), Some(5.0))],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Available);
    }

    #[test]
    fn remaining_only_with_zero_cap_debt_is_exhausted() {
        let report = report(
            Status::Ready,
            vec![metric("balance", Some(0.0), Some(0.0), Some(-1.0))],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Exhausted);
    }

    #[test]
    fn fully_consumed_is_exhausted() {
        let report = report(
            Status::Ready,
            vec![metric("m", Some(10.0), Some(10.0), Some(0.0))],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Exhausted);
    }

    #[test]
    fn negative_debt_is_exhausted() {
        let report = report(
            Status::Ready,
            vec![metric("balance", None, None, Some(-1.5))],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Exhausted);
    }

    #[test]
    fn nan_used_is_not_measured() {
        let report = report(
            Status::Ready,
            vec![metric("m", Some(f64::NAN), Some(10.0), None)],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn nan_remaining_is_not_measured() {
        let report = report(
            Status::Ready,
            vec![metric("balance", None, None, Some(f64::NAN))],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Unmeasured);
    }

    #[test]
    fn used_without_limit_is_available() {
        let report = report(
            Status::Ready,
            vec![metric("balance", Some(3.5), None, None)],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Available);
    }

    #[test]
    fn mixed_metrics_one_exhausted_means_exhausted() {
        let report = report(
            Status::Ready,
            vec![
                metric("ok", Some(1.0), Some(10.0), Some(9.0)),
                metric("drained", Some(5.0), Some(5.0), Some(0.0)),
            ],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Exhausted);
    }

    #[test]
    fn empty_optional_credit_balance_does_not_hide_available_subscription_quota() {
        let report = report(
            Status::Ready,
            vec![
                metric("Sessão", Some(10.0), Some(100.0), Some(90.0)),
                metric("Créditos extras", None, None, Some(0.0)),
            ],
        );
        assert_eq!(classify_report(Some(&report)), AccountClass::Available);
    }

    #[test]
    fn account_class_is_unmeasured_helper() {
        assert!(AccountClass::Unmeasured.is_unmeasured());
        assert!(!AccountClass::Available.is_unmeasured());
        assert!(!AccountClass::Exhausted.is_unmeasured());
    }

    #[test]
    fn metric_state_unmeasured_without_signal() {
        let m = metric("m", None, None, None);
        assert_eq!(classify_metric(&m), MetricState::Unmeasured);
    }

    #[test]
    fn metric_state_unmeasured_when_unknown_zero_cap_and_no_remaining() {
        let m = metric("m", Some(0.0), Some(0.0), None);
        assert_eq!(classify_metric(&m), MetricState::Unmeasured);
    }

    #[test]
    fn metric_state_available_with_cap_and_used() {
        let m = metric("m", Some(3.0), Some(10.0), None);
        match classify_metric(&m) {
            MetricState::Available { ratio, uncapped } => {
                assert!(!uncapped);
                assert!((ratio - 0.7).abs() < 1e-9);
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn metric_state_available_with_cap_and_remaining() {
        let m = metric("m", None, Some(10.0), Some(4.0));
        match classify_metric(&m) {
            MetricState::Available { ratio, uncapped } => {
                assert!(!uncapped);
                assert!((ratio - 0.4).abs() < 1e-9);
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn metric_state_exhausted_when_consumed_equals_cap() {
        let m = metric("m", Some(10.0), Some(10.0), Some(0.0));
        assert_eq!(
            classify_metric(&m),
            MetricState::Exhausted { no_cap: false }
        );
    }

    #[test]
    fn metric_state_exhausted_when_consumed_exceeds_cap() {
        let m = metric("m", Some(15.0), Some(10.0), None);
        assert_eq!(
            classify_metric(&m),
            MetricState::Exhausted { no_cap: false }
        );
    }

    #[test]
    fn metric_state_exhausted_when_remaining_zero_without_cap() {
        let m = metric("balance", None, None, Some(0.0));
        assert_eq!(classify_metric(&m), MetricState::Exhausted { no_cap: true });
    }

    #[test]
    fn metric_state_exhausted_when_negative_debt_without_cap() {
        let m = metric("balance", None, None, Some(-1.5));
        assert_eq!(classify_metric(&m), MetricState::Exhausted { no_cap: true });
    }

    #[test]
    fn metric_state_available_uncapped_without_cap() {
        let m = metric("balance", Some(3.5), None, None);
        assert_eq!(
            classify_metric(&m),
            MetricState::Available {
                ratio: 0.0,
                uncapped: true,
            }
        );
    }

    #[test]
    fn metric_state_available_when_remaining_with_zero_cap() {
        let m = metric("balance", Some(0.0), Some(0.0), Some(5.0));
        assert_eq!(
            classify_metric(&m),
            MetricState::Available {
                ratio: 0.0,
                uncapped: true,
            }
        );
    }
}
