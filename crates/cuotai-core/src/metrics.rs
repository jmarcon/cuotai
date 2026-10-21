//! Behavioral classification of report quotas and per-metric status.
//!
//! These helpers describe *what* a `Report` or `Metric` means, not how to
//! render it. The TUI and any other consumer (CLI, GUI, web, scripts) can
//! drive their own colors, labels, and layouts on top of the same answers.
//! Style, ordering, and presentation live outside this module.
//!
//! Three primitives are exposed:
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
//! * [`binding`] picks the eligible quota that restricts the account most,
//!   while [`binding_ratio`] and [`cmp_binding`] provide allocation-free sort
//!   keys for consumers.
//!
//! These primitives intentionally avoid colors or anything tied to terminal
//! widgets; only [`Binding::summary`] produces presentation-neutral text.

use crate::model::{Metric, MetricKind, MetricWindow, Report, Status};
use crate::text::{params, Localized, ParamValue};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::cmp::Ordering;

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

/// Why a metric is the account's binding one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingReason {
    /// The metric is consumed; the account is blocked.
    Exhausted,
    /// Nothing is consumed; this metric has the smallest remaining fraction.
    Tightest,
}

/// The metric that restricts the account the most right now. An emphasis,
/// not a filter: every other metric stays in the report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Binding {
    /// Position in `Report::metrics`.
    pub index: usize,
    pub label: String,
    pub window: Option<MetricWindow>,
    /// Consumed fraction in `[0.0, 1.0]`; `1.0` when exhausted.
    pub used_ratio: f64,
    /// Remaining amount in the metric's unit (`0.0` when exhausted).
    pub remaining: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub reason: BindingReason,
    /// When the account is usable again: set only when blocked and the
    /// latest return among the exhausted metrics is known. Never "now".
    pub blocked_until: Option<DateTime<Utc>>,
}

/// The account's binding metric, using the eligibility of [`classify_metric`]:
/// a `Quota` with a finite positive cap and a finite signal; `RateLimit` only
/// when no `Quota` qualifies; other kinds never. Blocked accounts return the
/// exhausted metric with the latest (or unknown) return; otherwise the
/// smallest remaining fraction wins, ties going to the shortest window and
/// then to the adapter's order. Non-`Ready` or unmeasured reports: `None`.
pub fn binding(report: &Report) -> Option<Binding> {
    binding_where(report, |_| true)
}

/// [`binding`] among the metrics whose index `keep` accepts (e.g. the ones
/// with a pace, PRD 0007 RF3.2), with the same order and ties.
pub fn binding_where(report: &Report, keep: impl Fn(usize) -> bool) -> Option<Binding> {
    let candidate = binding_candidate(report, &keep)?;
    let metric = &report.metrics[candidate.index];
    Some(Binding {
        index: candidate.index,
        label: metric.label.clone(),
        window: metric.window,
        used_ratio: 1.0 - candidate.ratio,
        remaining: metric.limit.unwrap_or_default() * candidate.ratio,
        resets_at: metric.resets_at,
        reason: if candidate.exhausted {
            BindingReason::Exhausted
        } else {
            BindingReason::Tightest
        },
        blocked_until: metric.resets_at.filter(|_| candidate.exhausted),
    })
}

#[derive(Clone, Copy)]
struct BindingCandidate {
    index: usize,
    ratio: f64,
    exhausted: bool,
}

fn binding_candidate(report: &Report, keep: &dyn Fn(usize) -> bool) -> Option<BindingCandidate> {
    if report.status != Status::Ready {
        return None;
    }
    select_kind(report, MetricKind::Quota, keep)
        .or_else(|| select_kind(report, MetricKind::RateLimit, keep))
}

fn select_kind(
    report: &Report,
    kind: MetricKind,
    keep: &dyn Fn(usize) -> bool,
) -> Option<BindingCandidate> {
    let mut selected: Option<BindingCandidate> = None;
    for (index, metric) in report.metrics.iter().enumerate() {
        if !keep(index)
            || metric.kind != kind
            || !metric.limit.is_some_and(|cap| cap.is_finite() && cap > 0.0)
        {
            continue;
        }
        let (ratio, exhausted) = match classify_metric(metric) {
            MetricState::Exhausted { no_cap: false } => (0.0, true),
            MetricState::Available {
                ratio,
                uncapped: false,
            } => (ratio, false),
            _ => continue,
        };
        let candidate = BindingCandidate {
            index,
            ratio,
            exhausted,
        };
        if selected.is_none_or(|current| better_candidate(report, candidate, current)) {
            selected = Some(candidate);
        }
    }
    selected
}

fn better_candidate(
    report: &Report,
    candidate: BindingCandidate,
    current: BindingCandidate,
) -> bool {
    match (candidate.exhausted, current.exhausted) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => match (
            report.metrics[candidate.index].resets_at,
            report.metrics[current.index].resets_at,
        ) {
            (None, Some(_)) => true,
            (Some(_), None) | (None, None) => false,
            (Some(candidate), Some(current)) => candidate > current,
        },
        (false, false) => {
            candidate.ratio < current.ratio
                || (candidate.ratio == current.ratio
                    && window_rank(report.metrics[candidate.index].window)
                        < window_rank(report.metrics[current.index].window))
        }
    }
}

/// Shortest first; `Other` and windowless metrics have no known length.
fn window_rank(window: Option<MetricWindow>) -> u8 {
    match window {
        Some(MetricWindow::Window5h) => 0,
        Some(MetricWindow::Daily) => 1,
        Some(MetricWindow::Weekly) => 2,
        Some(MetricWindow::Monthly) => 3,
        Some(MetricWindow::Other) | None => 4,
    }
}

/// Remaining fraction of the account's binding metric, if any.
pub fn binding_ratio(report: &Report) -> Option<f64> {
    binding_candidate(report, &|_| true).map(|candidate| candidate.ratio)
}

/// Most restrictive account first; accounts without a binding value last.
/// Equal accounts compare `Equal`, so a stable sort keeps the manual order.
pub fn cmp_binding(a: Option<&Report>, b: Option<&Report>) -> Ordering {
    match (a.and_then(binding_ratio), b.and_then(binding_ratio)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

impl Binding {
    /// One-line summary (`binding.blocked_until`, `binding.blocked`,
    /// `binding.tightest`). `at` formats `blocked_until` in the caller's
    /// clock and style (e.g. local `HH:MM`).
    pub fn summary(&self, at: impl FnOnce(DateTime<Utc>) -> String) -> Localized {
        let (window, window_pt) = window_param(self.window, &self.label);
        match (self.reason, self.blocked_until) {
            (BindingReason::Exhausted, Some(until)) => {
                let at = at(until);
                let fallback = format!("bloqueada até {at} pela janela {window_pt}");
                let params = params([("at", at.into()), ("window", window)]);
                Localized::with_params("binding.blocked_until", params, fallback)
            }
            (BindingReason::Exhausted, None) => Localized::with_param(
                "binding.blocked",
                "window",
                window,
                format!("bloqueada pela janela {window_pt}; horário de retorno desconhecido"),
            ),
            (BindingReason::Tightest, _) => {
                let used = format!("{:.0}", self.used_ratio * 100.0);
                let fallback = format!("mais restritiva: janela {window_pt} em {used}%");
                let params = params([("window", window), ("used", used.into())]);
                Localized::with_params("binding.tightest", params, fallback)
            }
        }
    }
}

/// Known windows travel as `metric_window.*`; others use the metric label.
/// Returns the parameter and its Portuguese rendering.
pub(crate) fn window_param(window: Option<MetricWindow>, label: &str) -> (ParamValue, String) {
    let (id, pt) = match window {
        Some(MetricWindow::Window5h) => ("metric_window.window_5h", "de 5 horas"),
        Some(MetricWindow::Daily) => ("metric_window.daily", "diária"),
        Some(MetricWindow::Weekly) => ("metric_window.weekly", "semanal"),
        Some(MetricWindow::Monthly) => ("metric_window.monthly", "mensal"),
        Some(MetricWindow::Other) | None => return (label.into(), label.to_string()),
    };
    (ParamValue::l10n(Localized::fixed(id, pt)), pt.to_string())
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
            session_expires_at: None,
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

    fn quota(
        window: Option<MetricWindow>,
        used: Option<f64>,
        limit: Option<f64>,
        reset_hour: Option<u32>,
    ) -> Metric {
        Metric {
            window,
            resets_at: reset_hour.map(at),
            ..metric(&format!("{window:?}"), used, limit, None)
        }
    }

    fn kind(kind: MetricKind, m: Metric) -> Metric {
        Metric { kind, ..m }
    }

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, hour, 30, 0).unwrap()
    }

    const H5: Option<MetricWindow> = Some(MetricWindow::Window5h);
    const WEEK: Option<MetricWindow> = Some(MetricWindow::Weekly);
    const MONTH: Option<MetricWindow> = Some(MetricWindow::Monthly);

    #[test]
    fn binding_table() {
        use BindingReason::{Exhausted as Ex, Tightest as Ti};
        let n = |v: f64| Some(v);
        // (case, metrics, expected (index, reason, blocked_until hour))
        type Expected = Option<(usize, BindingReason, Option<u32>)>;
        let cases: Vec<(&str, Vec<Metric>, Expected)> = vec![
            (
                "5h free, week exhausted: week blocks until its return",
                vec![
                    quota(H5, n(0.0), n(100.0), Some(5)),
                    quota(WEEK, n(100.0), n(100.0), Some(14)),
                ],
                Some((1, Ex, Some(14))),
            ),
            (
                "two exhausted: the latest return wins",
                vec![
                    quota(WEEK, n(120.0), n(100.0), Some(20)),
                    quota(H5, n(50.0), n(50.0), Some(9)),
                ],
                Some((0, Ex, Some(20))),
            ),
            (
                "exhausted without a return: blocked_until unknown",
                vec![
                    quota(H5, n(50.0), n(50.0), Some(9)),
                    quota(WEEK, n(100.0), n(100.0), None),
                ],
                Some((1, Ex, None)),
            ),
            (
                "three windows, none exhausted: smallest remaining",
                vec![
                    quota(H5, n(10.0), n(100.0), Some(5)),
                    quota(WEEK, n(62.0), n(100.0), Some(14)),
                    quota(MONTH, n(30.0), n(100.0), Some(20)),
                ],
                Some((1, Ti, None)),
            ),
            (
                "tie: shortest window first",
                vec![
                    quota(MONTH, n(50.0), n(100.0), None),
                    quota(H5, n(5.0), n(10.0), None),
                ],
                Some((1, Ti, None)),
            ),
            (
                "tie on the same window: adapter order",
                vec![
                    quota(WEEK, n(5.0), n(10.0), None),
                    quota(WEEK, n(50.0), n(100.0), None),
                ],
                Some((0, Ti, None)),
            ),
            (
                "rate limit only",
                vec![kind(
                    MetricKind::RateLimit,
                    quota(None, n(9.0), n(10.0), None),
                )],
                Some((0, Ti, None)),
            ),
            (
                "rate limit ignored when a quota is eligible",
                vec![
                    kind(MetricKind::RateLimit, quota(None, n(10.0), n(10.0), None)),
                    quota(WEEK, n(1.0), n(10.0), None),
                ],
                Some((1, Ti, None)),
            ),
            (
                "balance, credits, spend and usage never enter",
                vec![
                    kind(MetricKind::Balance, quota(None, n(10.0), n(10.0), None)),
                    kind(MetricKind::Credits, quota(None, n(10.0), n(10.0), None)),
                    kind(MetricKind::Spend, quota(None, n(10.0), n(10.0), None)),
                    kind(MetricKind::Usage, quota(None, n(10.0), n(10.0), None)),
                ],
                None,
            ),
            (
                "no cap: neither used-only nor remaining-only enters",
                vec![
                    quota(WEEK, n(10.0), None, None),
                    Metric {
                        remaining: n(0.0),
                        ..quota(H5, None, None, None)
                    },
                ],
                None,
            ),
            (
                "missing and non-finite values are skipped",
                vec![
                    quota(H5, None, n(100.0), None),
                    quota(H5, n(f64::NAN), n(100.0), None),
                    quota(H5, n(100.0), n(f64::INFINITY), None),
                    quota(WEEK, n(40.0), n(100.0), None),
                ],
                Some((3, Ti, None)),
            ),
            (
                "used = 0, limit = 0 is an unknown sentinel",
                vec![quota(WEEK, n(0.0), n(0.0), None)],
                None,
            ),
        ];
        for (case, metrics, expected) in cases {
            let got = binding(&report(Status::Ready, metrics))
                .map(|b| (b.index, b.reason, b.blocked_until));
            let expected = expected.map(|(i, r, h)| (i, r, h.map(at)));
            assert_eq!(got, expected, "{case}");
        }
    }

    #[test]
    fn binding_carries_the_metric_and_skips_unready_accounts() {
        let metrics = vec![quota(WEEK, Some(50.0), Some(200.0), Some(14))];
        let b = binding(&report(Status::Ready, metrics.clone())).unwrap();
        assert_eq!((b.label.as_str(), b.window), ("Some(Weekly)", WEEK));
        assert_eq!((b.used_ratio, b.remaining), (0.25, 150.0));
        assert_eq!(b.resets_at, Some(at(14)));
        assert_eq!(
            serde_json::to_value(&b).unwrap()["reason"],
            serde_json::json!("tightest")
        );
        for status in [Status::AuthenticationError, Status::Error] {
            assert_eq!(binding(&report(status, metrics.clone())), None);
        }
    }

    #[test]
    fn binding_where_picks_the_tightest_among_the_kept_metrics() {
        let r = report(
            Status::Ready,
            vec![
                quota(WEEK, Some(8.0), Some(8.0), Some(14)),
                quota(WEEK, Some(2.0), Some(8.0), None),
                quota(WEEK, Some(6.0), Some(8.0), None),
            ],
        );
        assert_eq!(binding(&r).unwrap().index, 0);
        // Without the blocked metric the 25%-left one binds, not the adapter's next.
        assert_eq!(binding_where(&r, |i| i != 0).unwrap().index, 2);
        assert_eq!(binding_where(&r, |_| false), None);
    }

    #[test]
    fn accounts_without_binding_sort_last() {
        let tight = report(Status::Ready, vec![quota(WEEK, Some(6.0), Some(8.0), None)]);
        let loose = report(Status::Ready, vec![quota(WEEK, Some(2.0), Some(8.0), None)]);
        let failed = report(Status::Error, Vec::new());
        let mut accounts = [None, Some(&failed), Some(&loose), Some(&tight)];
        accounts.sort_by(|a, b| cmp_binding(*a, *b));
        assert_eq!(binding_ratio(&tight), Some(0.25));
        assert!(std::ptr::eq(accounts[0].unwrap(), &tight));
        assert!(std::ptr::eq(accounts[1].unwrap(), &loose));
        assert!(accounts[2].is_none(), "manual order kept among the rest");
    }

    #[test]
    fn binding_summary_is_translated_in_every_catalog() {
        let blocked = |resets| Binding {
            index: 0,
            label: "Opus".into(),
            window: None,
            used_ratio: 1.0,
            remaining: 0.0,
            resets_at: resets,
            reason: BindingReason::Exhausted,
            blocked_until: resets,
        };
        let mut summaries = vec![blocked(Some(at(14))), blocked(None)];
        for window in [H5, Some(MetricWindow::Daily), WEEK, MONTH, None] {
            summaries.push(Binding {
                window,
                used_ratio: 0.62,
                reason: BindingReason::Tightest,
                blocked_until: None,
                ..blocked(None)
            });
        }
        let texts: Vec<Localized> = summaries
            .iter()
            .map(|b| b.summary(|t| t.format("%H:%M").to_string()))
            .collect();
        for locale in ["en-US", "es"] {
            let catalog = crate::text::core_catalog(locale).unwrap();
            for text in &texts {
                let nested = text.params.values().filter_map(|v| match v {
                    ParamValue::L10n(l) => Some(&*l.id),
                    ParamValue::Text(_) => None,
                });
                for id in std::iter::once(&*text.id).chain(nested) {
                    assert!(catalog.contains_key(id), "{locale} lacks {id}");
                }
            }
        }
        assert_eq!(
            texts[0].resolve("en-US"),
            "blocked until 14:30 by the Opus window"
        );
        assert_eq!(
            texts[1].fallback,
            "bloqueada pela janela Opus; horário de retorno desconhecido"
        );
        assert_eq!(
            texts[4].resolve("en-US"),
            "most restrictive: weekly window at 62%"
        );
        assert_eq!(
            texts[4].resolve("es"),
            "más restrictiva: ventana semanal al 62%"
        );
        assert_eq!(texts[4].fallback, "mais restritiva: janela semanal em 62%");
    }
}
