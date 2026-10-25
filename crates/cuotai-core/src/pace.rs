//! Consumption pace until the reset (PRD 0007).
//!
//! A pure, linear estimate over one metric, an injected `now` and optional
//! recent samples: no clock, no network. Expected use grows uniformly from
//! the window start to `resets_at`; the projection extends either the slope
//! of the recent samples or the average since the start. It is an estimate,
//! not a promise, and front ends present it as such.

use crate::freshness::{self, Freshness};
use crate::model::{Metric, MetricWindow};
use crate::text::{params, Localized};
use chrono::{DateTime, Months, TimeDelta, Utc};
use serde::Serialize;
use std::time::Duration;

/// One measurement of a metric at `taken_at` (a PRD 0006 history sample).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub taken_at: DateTime<Utc>,
    pub used: f64,
}

/// Real consumption against the uniform pace, with a ±5-point dead band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceStage {
    /// `delta_points ≤ −5`: below the pace.
    Reserve,
    OnPace,
    /// `delta_points ≥ +5`: above the pace.
    Ahead,
    /// `used ≥ limit`.
    Exhausted,
}

/// Semantic tone of the stage, so no interface picks a color from text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceTone {
    Good,
    Neutral,
    Warn,
    Bad,
}

/// What the projection extended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceSource {
    /// Average since the start of the window.
    Average,
    /// Slope of the recent samples.
    Recent,
}

/// Pace of one eligible metric at `now`. Fractions are in `[0, 1]` except
/// `used_fraction`, which exceeds 1 when the provider reports overuse.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pace {
    pub elapsed_fraction: f64,
    pub expected_used_fraction: f64,
    pub used_fraction: f64,
    /// Real minus expected consumption, in percentage points.
    pub delta_points: f64,
    /// PRD 0024 RF1.3: the deviation relative to the expected use,
    /// `(used − expected) / expected` (expected 50%, used 60% → `0.2`).
    /// `None` below a 10% expected share or when not finite.
    pub delta_ratio: Option<f64>,
    pub stage: PaceStage,
    pub tone: PaceTone,
    pub source: PaceSource,
    pub resets_at: DateTime<Utc>,
    /// Set only when the projection runs out strictly before `resets_at`.
    pub projected_exhaustion_at: Option<DateTime<Utc>>,
    pub lasts_until_reset: bool,
}

/// PRD 0033 RF4.1: a pace line text that names its metric ("Week: 8
/// points under pace"). A front end omits the label when the binding chip
/// beside the line already names the same metric.
pub fn labelled(label: &str, text: &str) -> String {
    if label.is_empty() {
        text.to_owned()
    } else {
        format!("{label}: {text}")
    }
}

const BAND_POINTS: f64 = 5.0;
/// Below this expected share [`Pace::delta_ratio`] is undefined.
const MIN_RATIO_EXPECTED: f64 = 0.10;
/// The largest percent a chip shows ("999% faster than pace").
const MAX_CHIP_PERCENT: f64 = 999.0;
/// PRD 0024 RF2.2.1 (FIX-12): an alert needs its recent samples to span
/// at least this long, or [`ALERT_MIN_ELAPSED`] of the window to elapse.
pub const ALERT_MIN_SPAN: TimeDelta = TimeDelta::minutes(30);
/// See [`ALERT_MIN_SPAN`].
pub const ALERT_MIN_ELAPSED: f64 = 0.20;
const MIN_ELAPSED_SECS: f64 = 600.0;
const MIN_LOOKBACK_SECS: f64 = 7_200.0;

/// Pace of `metric` at `now`, or `None` when it is not eligible: a `Quota`
/// in a `window_5h`/`daily`/`weekly`/`monthly` window with a finite positive
/// `limit`, a finite non-negative `used`, a future `resets_at`, and at least
/// 5% of the window (and 10 minutes) elapsed. `recent` with 3 or more samples
/// in the last 2 hours (or 10% of the window, whichever is greater) drives
/// the projection; otherwise the average since the window start does.
pub fn assess(metric: &Metric, now: DateTime<Utc>, recent: Option<&[Point]>) -> Option<Pace> {
    if !crate::metrics::is_plan_limit(metric) {
        return None;
    }
    let limit = metric.limit.filter(|cap| cap.is_finite() && *cap > 0.0)?;
    let used = metric
        .used
        .filter(|used| used.is_finite() && *used >= 0.0)?;
    let resets_at = metric.resets_at.filter(|at| *at > now)?;
    let start = window_start(metric, resets_at, now)?;
    let length = secs(resets_at - start);
    let elapsed = secs(now - start);
    if elapsed < (length * 0.05).max(MIN_ELAPSED_SECS) {
        return None;
    }
    // Percentages first, so round inputs land exactly on the band edges.
    let used_fraction = (used / limit).min(f64::MAX);
    let scaled_used = used * 100.0;
    let used_pct = if scaled_used.is_finite() {
        (scaled_used / limit).min(f64::MAX)
    } else {
        (used_fraction * 100.0).min(f64::MAX)
    };
    let expected_pct = elapsed * 100.0 / length;
    let delta_points = used_pct - expected_pct;
    let (stage, tone) = if used >= limit {
        (PaceStage::Exhausted, PaceTone::Bad)
    } else if delta_points <= -BAND_POINTS {
        (PaceStage::Reserve, PaceTone::Good)
    } else if delta_points >= BAND_POINTS {
        (PaceStage::Ahead, PaceTone::Warn)
    } else {
        (PaceStage::OnPace, PaceTone::Neutral)
    };
    // Seconds from `now` until `used` reaches `limit`; `None` = no consumption.
    let (source, secs_left) =
        match recent.and_then(|points| recent_slope(points, start, now, length, limit)) {
            Some(slope) => (
                PaceSource::Recent,
                (slope > 0.0).then(|| (1.0 - used / limit) / slope),
            ),
            None => (
                PaceSource::Average,
                (used > 0.0).then(|| (limit - used) / used * elapsed),
            ),
        };
    let exhausted = stage == PaceStage::Exhausted;
    let until_reset = secs(resets_at - now);
    // Bounded by `until_reset`, so the conversion cannot overflow.
    let projected_exhaustion_at = secs_left
        .filter(|left| !exhausted && *left < until_reset)
        .map(|left| now + TimeDelta::milliseconds((left * 1000.0) as i64));
    Some(Pace {
        elapsed_fraction: elapsed / length,
        expected_used_fraction: elapsed / length,
        used_fraction,
        delta_points,
        delta_ratio: delta_ratio(used_fraction, elapsed / length),
        stage,
        tone,
        source,
        resets_at,
        projected_exhaustion_at,
        lasts_until_reset: !exhausted && projected_exhaustion_at.is_none(),
    })
}

/// Pace of `metric` as measured by a reading taken at `checked_at`, for a
/// surface that shows it at `now` (PRD 0024 FIX-11, RF6.4). The one rule
/// every surface (dashboard, `--once`, `export::v1`, desktop) applies:
///
/// * the reading must be [`Freshness::Fresh`] against the account's
///   effective refresh `interval` at `now` (PRD 0010); an `Aging` or `Stale`
///   reading shows no pace, only its age;
/// * the pace is evaluated at `checked_at`, never at `now`, so an old `used`
///   is never paired with the current clock;
/// * a window that reset after the reading (`resets_at <= now`) has no pace.
///
/// Otherwise the same as [`assess`] at `checked_at`.
pub fn assess_reading(
    metric: &Metric,
    checked_at: DateTime<Utc>,
    interval: Duration,
    now: DateTime<Utc>,
    recent: Option<&[Point]>,
) -> Option<Pace> {
    if freshness::assess(checked_at, interval, now) != Freshness::Fresh {
        return None;
    }
    if metric.resets_at.is_some_and(|at| at <= now) {
        return None;
    }
    assess(metric, checked_at, recent)
}

/// Start of the span whose samples [`assess`] reads as `recent` for `metric`
/// at `now`: the later of the window start and `now` minus 2 hours (or 10%
/// of the window, whichever is greater). `None` when the metric has no pace
/// to project, so a caller can skip reading the history at all.
pub fn recent_since(metric: &Metric, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    assess(metric, now, None)?;
    let resets_at = metric.resets_at?;
    let start = window_start(metric, resets_at, now)?;
    let lookback = (secs(resets_at - start) * 0.1).max(MIN_LOOKBACK_SECS);
    let since = now - TimeDelta::milliseconds((lookback * 1000.0) as i64);
    Some(since.max(start))
}

/// `resets_at − length`; `monthly` is the calendar month ending at
/// `resets_at`. An RFC 3339 start in `period` wins when coherent: already
/// begun and not earlier than the nominal start.
fn window_start(
    metric: &Metric,
    resets_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let nominal = match metric.window? {
        MetricWindow::Window5h => resets_at - TimeDelta::hours(5),
        MetricWindow::Daily => resets_at - TimeDelta::days(1),
        MetricWindow::Weekly => resets_at - TimeDelta::weeks(1),
        MetricWindow::Monthly => resets_at.checked_sub_months(Months::new(1))?,
        MetricWindow::Other => return None,
    };
    let known = metric
        .period
        .as_deref()
        .and_then(|period| DateTime::parse_from_rfc3339(period).ok())
        .map(|start| start.with_timezone(&Utc))
        .filter(|start| *start >= nominal && *start <= now);
    Some(known.unwrap_or(nominal))
}

/// Least-squares slope (fraction of `limit` per second) of the samples of
/// the current window taken in the lookback span; `None` below 3 distinct
/// samples.
fn recent_slope(
    points: &[Point],
    start: DateTime<Utc>,
    now: DateTime<Utc>,
    length: f64,
    limit: f64,
) -> Option<f64> {
    let lookback = (length * 0.1).max(MIN_LOOKBACK_SECS);
    let samples = || {
        points
            .iter()
            .filter(move |p| p.used.is_finite() && p.taken_at >= start && p.taken_at <= now)
            .map(move |p| (secs(p.taken_at - now), p.used / limit))
            .filter(move |(x, y)| *x >= -lookback && y.is_finite())
    };
    let (n, sum_x, sum_y) = samples().fold((0.0, 0.0, 0.0), |(n, sx, sy), (x, y)| {
        (n + 1.0, sx + x, sy + y)
    });
    if n < 3.0 {
        return None;
    }
    let (mean_x, mean_y) = (sum_x / n, sum_y / n);
    let (sxx, sxy) = samples().fold((0.0, 0.0), |(sxx, sxy), (x, y)| {
        (
            sxx + (x - mean_x).powi(2),
            sxy + (x - mean_x) * (y - mean_y),
        )
    });
    (sxx > 0.0).then(|| sxy / sxx)
}

fn delta_ratio(used: f64, expected: f64) -> Option<f64> {
    if !expected.is_finite() || expected < MIN_RATIO_EXPECTED || !used.is_finite() {
        return None;
    }
    let ratio = (used - expected) / expected;
    ratio.is_finite().then_some(ratio)
}

/// PRD 0024 RF2.2.1 (FIX-12): whether `pace`, assessed for `metric` at
/// `at` with `recent`, is grounded enough to warn about: projected from the
/// recent slope of at least 3 samples spanning [`ALERT_MIN_SPAN`], or with
/// [`ALERT_MIN_ELAPSED`] of the window elapsed. An average-only projection
/// never is. The displayed pace keeps the plain 3-sample rule (decision D).
pub fn alert_grade(metric: &Metric, pace: &Pace, at: DateTime<Utc>, recent: &[Point]) -> bool {
    if pace.source != PaceSource::Recent {
        return false;
    }
    if pace.elapsed_fraction >= ALERT_MIN_ELAPSED {
        return true;
    }
    let Some(start) = window_start(metric, pace.resets_at, at) else {
        return false;
    };
    let lookback = (secs(pace.resets_at - start) * 0.1).max(MIN_LOOKBACK_SECS);
    let taken: Vec<DateTime<Utc>> = recent
        .iter()
        .filter(|p| p.used.is_finite() && p.taken_at >= start && p.taken_at <= at)
        .filter(|p| secs(p.taken_at - at) >= -lookback)
        .map(|p| p.taken_at)
        .collect();
    match (taken.iter().min(), taken.iter().max()) {
        (Some(first), Some(last)) if taken.len() >= 3 => *last - *first >= ALERT_MIN_SPAN,
        _ => false,
    }
}

fn secs(delta: TimeDelta) -> f64 {
    delta.num_milliseconds() as f64 / 1000.0
}

impl Pace {
    /// Stage text: `pace.reserve`/`pace.ahead` (param `delta`, whole points),
    /// `pace.on_pace`, `pace.exhausted`. Front ends join it with
    /// [`Pace::outlook_label`] as `stage · outlook`.
    pub fn stage_label(&self) -> Localized {
        match self.stage {
            PaceStage::Reserve => {
                let delta = format!("{:.0}", self.delta_points.abs());
                Localized::with_param(
                    "pace.reserve",
                    "delta",
                    delta.as_str(),
                    "{delta} points in reserve",
                )
            }
            PaceStage::OnPace => Localized::fixed("pace.on_pace", "On pace"),
            PaceStage::Ahead => {
                let delta = format!("{:.0}", self.delta_points.abs());
                Localized::with_param(
                    "pace.ahead",
                    "delta",
                    delta.as_str(),
                    "{delta} points above the pace",
                )
            }
            PaceStage::Exhausted => {
                Localized::fixed("pace.exhausted", "Limit reached until the reset")
            }
        }
    }

    /// Outlook text: `pace.lasts_until_reset`, or `pace.runs_out_at` with
    /// `at` (the projected time) and `margin` (how long before the reset),
    /// both formatted by the caller's clock and style. `None` when exhausted.
    pub fn outlook_label(
        &self,
        at: impl FnOnce(DateTime<Utc>) -> String,
        margin: impl FnOnce(TimeDelta) -> String,
    ) -> Option<Localized> {
        if self.lasts_until_reset {
            return Some(Localized::fixed(
                "pace.lasts_until_reset",
                "lasts until the reset",
            ));
        }
        let when = self.projected_exhaustion_at?;
        let (at, margin) = (at(when), margin(self.resets_at - when));
        let params = params([("at", at.into()), ("margin", margin.into())]);
        Some(Localized::with_params(
            "pace.runs_out_at",
            params,
            "runs out at {at}, {margin} before the reset",
        ))
    }
}

/// PRD 0024 RF1.3 (decision M): how a chip words the deviation from the
/// pace. The choice is the user's; [`PaceUnit::Points`] by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceUnit {
    /// Percentage points: used share minus expected share.
    #[default]
    Points,
    /// Relative to the expected use: "20% faster than pace".
    Percent,
}

impl PaceUnit {
    /// `points` or `percent` (the CLI and `desktop.json` spelling).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "points" => Some(Self::Points),
            "percent" => Some(Self::Percent),
            _ => None,
        }
    }
}

/// PRD 0024 RF1 (DS-CMP-TIME-1..5): one compact time or pace chip: a glyph,
/// the full text (also the accessible name and tooltip) and, for narrow
/// places, the short value shown after the glyph ("19:30", "≈18:53",
/// "20%").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chip {
    /// One of ↓ → ↑ ✕ ↺.
    pub glyph: &'static str,
    pub text: Localized,
    /// The compact form after the glyph; `None` when the chip has none
    /// (the full text is short already).
    pub compact: Option<String>,
    /// A tooltip beyond the text: why the percent form fell back to points.
    pub hint: Option<Localized>,
}

/// ↓ below the pace, or lasting until the reset.
pub const GLYPH_RESERVE: &str = "↓";
/// → on pace.
pub const GLYPH_ON_PACE: &str = "→";
/// ↑ above the pace, or running out before the reset.
pub const GLYPH_AHEAD: &str = "↑";
/// ✕ exhausted.
pub const GLYPH_EXHAUSTED: &str = "✕";
/// ↺ when the quota returns.
pub const GLYPH_RETURNS: &str = "↺";

impl Pace {
    /// PRD 0024 RF1.1/RF1.3: the stage chip in `unit`. Points: "{d} pts in
    /// reserve", "On pace", "{d} pts ahead", "Exhausted", where `{d}` is the
    /// rounded absolute `delta_points` (a rounded 0 reads "On pace").
    /// Percent: "{p}% faster than pace"/"{p}% slower than pace" from
    /// [`Pace::delta_ratio`] (capped at 999, a rounded 0 reads "On pace");
    /// without a ratio (expected share below 10%) the points form, with the
    /// hint "Too early in the window for a percentage".
    pub fn chip(&self, unit: PaceUnit) -> Chip {
        if self.stage == PaceStage::Exhausted {
            return Chip {
                glyph: GLYPH_EXHAUSTED,
                text: Localized::fixed("pace.chip.exhausted", "Exhausted"),
                compact: None,
                hint: None,
            };
        }
        let on_pace = || Chip {
            glyph: GLYPH_ON_PACE,
            text: Localized::fixed("pace.chip.on_pace", "On pace"),
            compact: None,
            hint: None,
        };
        let mut hint = None;
        if unit == PaceUnit::Percent {
            match self.delta_ratio {
                Some(ratio) => {
                    let percent = (ratio.abs() * 100.0).round().min(MAX_CHIP_PERCENT);
                    if percent < 1.0 {
                        return on_pace();
                    }
                    let p = format!("{percent:.0}");
                    let (glyph, id, fallback) = if ratio > 0.0 {
                        (GLYPH_AHEAD, "pace.chip.ahead_pct", "{p}% faster than pace")
                    } else {
                        (
                            GLYPH_RESERVE,
                            "pace.chip.reserve_pct",
                            "{p}% slower than pace",
                        )
                    };
                    return Chip {
                        glyph,
                        text: Localized::with_param(id, "p", p.as_str(), fallback),
                        compact: Some(format!("{p}%")),
                        hint: None,
                    };
                }
                None => {
                    hint = Some(Localized::fixed(
                        "pace.chip.too_early",
                        "Too early in the window for a percentage",
                    ));
                }
            }
        }
        let points = self.delta_points.abs().round();
        if !points.is_finite() || points < 1.0 {
            return Chip { hint, ..on_pace() };
        }
        let d = format!("{points:.0}");
        let (glyph, id, fallback) = if self.delta_points > 0.0 {
            (GLYPH_AHEAD, "pace.chip.ahead", "{d} pts ahead")
        } else {
            (GLYPH_RESERVE, "pace.chip.reserve", "{d} pts in reserve")
        };
        Chip {
            glyph,
            text: Localized::with_param(id, "d", d.as_str(), fallback),
            compact: Some(format!("{d} pts")),
            hint,
        }
    }

    /// PRD 0024 RF1.2 (DS-CMP-TIME-4): ↑ "Runs out ≈{at}" when the core
    /// projects exhaustion before the reset, otherwise ↓ "Lasts to reset";
    /// `None` for an exhausted metric. `at` formats the projected time in
    /// the caller's locale and clock.
    pub fn outlook_chip(&self, at: impl FnOnce(DateTime<Utc>) -> String) -> Option<Chip> {
        if self.stage == PaceStage::Exhausted {
            return None;
        }
        match self.projected_exhaustion_at {
            Some(when) => {
                let at = at(when);
                Some(Chip {
                    glyph: GLYPH_AHEAD,
                    text: Localized::with_param(
                        "pace.chip.runs_out",
                        "at",
                        at.as_str(),
                        "Runs out ≈{at}",
                    ),
                    compact: Some(format!("≈{at}")),
                    hint: None,
                })
            }
            None => Some(Chip {
                glyph: GLYPH_RESERVE,
                text: Localized::fixed("pace.chip.lasts", "Lasts to reset"),
                compact: None,
                hint: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labelled_text_names_the_metric_or_stays_bare() {
        assert_eq!(
            labelled("5h", "8 points in reserve"),
            "5h: 8 points in reserve"
        );
        assert_eq!(labelled("", "8 points in reserve"), "8 points in reserve");
    }

    use super::*;
    use crate::model::MetricKind;
    use chrono::TimeZone;

    fn ts(y: i32, mo: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, 0, 0).unwrap()
    }

    fn now() -> DateTime<Utc> {
        ts(2026, 2, 15, 0)
    }

    fn mins(n: i64) -> TimeDelta {
        TimeDelta::minutes(n)
    }

    fn quota(window: MetricWindow, used: f64, resets_at: DateTime<Utc>) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: "plan".into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(window),
            period: None,
            resets_at: Some(resets_at),
        }
    }

    fn five_h(used: f64) -> Metric {
        // Half of the 5-hour window elapsed.
        quota(MetricWindow::Window5h, used, now() + mins(150))
    }

    #[test]
    fn overage_metric_has_no_pace() {
        let now = ts(2026, 2, 15, 0);
        let mut m = quota(MetricWindow::Monthly, 50.0, ts(2026, 3, 1, 0));
        assert!(assess(&m, now, None).is_some());
        m.billing = crate::model::Billing::Overage;
        assert!(assess(&m, now, None).is_none());
    }

    #[test]
    fn windows_and_projection() {
        use MetricWindow::*;
        // (case, metric, now, elapsed_fraction, stage, minutes until exhaustion)
        let cases = [
            (
                "5h on pace, ends at reset",
                five_h(50.0),
                now(),
                0.5,
                PaceStage::OnPace,
                None,
            ),
            (
                "daily ahead, before reset",
                quota(Daily, 40.0, now() + mins(18 * 60)),
                now(),
                0.25,
                PaceStage::Ahead,
                Some(9 * 60),
            ),
            (
                "weekly reserve, after reset",
                quota(Weekly, 20.0, now() + mins(84 * 60)),
                now(),
                0.5,
                PaceStage::Reserve,
                None,
            ),
            (
                "february month",
                quota(Monthly, 50.0, ts(2026, 3, 1, 0)),
                now(),
                0.5,
                PaceStage::OnPace,
                None,
            ),
            (
                "31-day month",
                quota(Monthly, 50.0, ts(2026, 2, 1, 0)),
                ts(2026, 1, 16, 12),
                0.5,
                PaceStage::OnPace,
                None,
            ),
            (
                "unused",
                quota(Weekly, 0.0, now() + mins(84 * 60)),
                now(),
                0.5,
                PaceStage::Reserve,
                None,
            ),
            (
                "exhausted",
                quota(Daily, 100.0, now() + mins(12 * 60)),
                now(),
                0.5,
                PaceStage::Exhausted,
                None,
            ),
            (
                "overused",
                quota(Daily, 130.0, now() + mins(12 * 60)),
                now(),
                0.5,
                PaceStage::Exhausted,
                None,
            ),
        ];
        for (case, metric, now, elapsed, stage, runs_out) in cases {
            let pace = assess(&metric, now, None).unwrap_or_else(|| panic!("{case}: no pace"));
            assert!(
                (pace.elapsed_fraction - elapsed).abs() < 1e-9,
                "{case}: {}",
                pace.elapsed_fraction
            );
            assert_eq!(pace.expected_used_fraction, pace.elapsed_fraction, "{case}");
            assert_eq!(pace.stage, stage, "{case}");
            assert_eq!(pace.source, PaceSource::Average, "{case}");
            assert_eq!(
                pace.projected_exhaustion_at,
                runs_out.map(|m| now + mins(m)),
                "{case}"
            );
            let lasts = stage != PaceStage::Exhausted && runs_out.is_none();
            assert_eq!(pace.lasts_until_reset, lasts, "{case}");
        }
    }

    #[test]
    fn missing_or_invalid_data_yields_nothing() {
        let base = || quota(MetricWindow::Daily, 40.0, now() + mins(12 * 60));
        type Edit = fn(&mut Metric);
        let cases: [(&str, Edit); 13] = [
            ("zero limit", |m| m.limit = Some(0.0)),
            ("missing limit", |m| m.limit = None),
            ("infinite limit", |m| m.limit = Some(f64::INFINITY)),
            ("NaN used", |m| m.used = Some(f64::NAN)),
            ("missing used", |m| m.used = None),
            ("negative used", |m| m.used = Some(-1.0)),
            ("missing reset", |m| m.resets_at = None),
            ("past reset", |m| m.resets_at = Some(now() - mins(1))),
            ("reset now", |m| m.resets_at = Some(now())),
            ("other window", |m| m.window = Some(MetricWindow::Other)),
            ("no window", |m| m.window = None),
            ("rate limit", |m| m.kind = MetricKind::RateLimit),
            ("spend", |m| m.kind = MetricKind::Spend),
        ];
        assert!(assess(&base(), now(), None).is_some());
        for (case, edit) in cases {
            let mut metric = base();
            edit(&mut metric);
            assert_eq!(assess(&metric, now(), None), None, "{case}");
        }
    }

    #[test]
    fn finite_values_keep_pace_finite() {
        let mut exhausted = quota(MetricWindow::Daily, f64::MAX, now() + mins(12 * 60));
        exhausted.limit = Some(f64::MAX);
        let pace = assess(&exhausted, now(), None).unwrap();
        assert!(pace.used_fraction.is_finite());
        assert!(pace.delta_points.is_finite());
        assert_eq!(pace.stage, PaceStage::Exhausted);

        let mut disproportionate = exhausted.clone();
        disproportionate.limit = Some(f64::MIN_POSITIVE);
        let pace = assess(&disproportionate, now(), None).unwrap();
        assert_eq!(pace.stage, PaceStage::Exhausted);
        assert!(pace.delta_points.is_finite());
        let json = serde_json::to_value(&pace).unwrap();
        assert!(json["used_fraction"].is_number());
        assert!(json["delta_points"].is_number());

        let mut ahead = quota(MetricWindow::Daily, f64::MAX * 0.75, now() + mins(12 * 60));
        ahead.limit = Some(f64::MAX);
        let pace = assess(&ahead, now(), None).unwrap();
        assert_eq!(
            pace.projected_exhaustion_at,
            Some(now() + TimeDelta::hours(4))
        );
        let recent = [
            Point {
                taken_at: now() - TimeDelta::hours(2),
                used: f64::MAX * 0.45,
            },
            Point {
                taken_at: now() - TimeDelta::hours(1),
                used: f64::MAX * 0.60,
            },
            Point {
                taken_at: now(),
                used: f64::MAX * 0.75,
            },
        ];
        let pace = assess(&ahead, now(), Some(&recent)).unwrap();
        assert_eq!(pace.source, PaceSource::Recent);
        assert_eq!(pace.projected_exhaustion_at, Some(now() + mins(100)));
    }

    #[test]
    fn freshly_started_window_is_not_assessed() {
        // 5% of 5 h is 15 min; 10 min is the floor for every window.
        let at = |elapsed: i64| quota(MetricWindow::Window5h, 1.0, now() + mins(300 - elapsed));
        assert_eq!(assess(&at(9), now(), None), None);
        assert_eq!(assess(&at(14), now(), None), None);
        assert!(assess(&at(15), now(), None).is_some());
        // Weekly: 10 minutes is far below 5% (8.4 h).
        let weekly = quota(
            MetricWindow::Weekly,
            1.0,
            now() + TimeDelta::weeks(1) - mins(8 * 60),
        );
        assert_eq!(assess(&weekly, now(), None), None);
    }

    #[test]
    fn hysteresis_at_the_thresholds() {
        for (used, stage, tone) in [
            (44.9, PaceStage::Reserve, PaceTone::Good),
            (45.0, PaceStage::Reserve, PaceTone::Good),
            (45.1, PaceStage::OnPace, PaceTone::Neutral),
            (54.9, PaceStage::OnPace, PaceTone::Neutral),
            (55.0, PaceStage::Ahead, PaceTone::Warn),
            (100.0, PaceStage::Exhausted, PaceTone::Bad),
        ] {
            let pace = assess(&five_h(used), now(), None).unwrap();
            assert_eq!((pace.stage, pace.tone), (stage, tone), "used {used}");
        }
    }

    #[test]
    fn recent_slope_versus_average() {
        let metric = five_h(50.0);
        let point = |ago: i64, used| Point {
            taken_at: now() - mins(ago),
            used,
        };
        // Average: 50 in 150 min ends exactly at the reset.
        let steep = [point(60, 20.0), point(30, 35.0), point(0, 50.0)];
        let pace = assess(&metric, now(), Some(&steep)).unwrap();
        assert_eq!(pace.source, PaceSource::Recent);
        // 30 per hour → the 50 left last 100 min, 50 min before the reset.
        assert_eq!(pace.projected_exhaustion_at, Some(now() + mins(100)));
        assert!(!pace.lasts_until_reset);

        // Flat recent use lasts even though the average would not.
        let ahead = five_h(70.0);
        let flat = [point(90, 70.0), point(45, 70.0), point(0, 70.0)];
        let pace = assess(&ahead, now(), Some(&flat)).unwrap();
        assert_eq!(
            (pace.source, pace.lasts_until_reset),
            (PaceSource::Recent, true)
        );
        assert!(assess(&ahead, now(), None)
            .unwrap()
            .projected_exhaustion_at
            .is_some());

        // Fewer than 3 samples in the window, or outside the lookback: average.
        let sparse = [point(200, 0.0), point(30, 35.0), point(0, 50.0)];
        let daily = quota(MetricWindow::Daily, 50.0, now() + mins(12 * 60));
        let stale = [point(300, 10.0), point(250, 20.0), point(200, 30.0)];
        for (metric, points) in [
            (&metric, &sparse[..]),
            (&daily, &stale[..]),
            (&metric, &[][..]),
        ] {
            let pace = assess(metric, now(), Some(points)).unwrap();
            assert_eq!(pace.source, PaceSource::Average);
        }
    }

    #[test]
    fn known_start_in_period_wins_when_coherent() {
        let mut metric = quota(MetricWindow::Daily, 10.0, now() + mins(6 * 60));
        metric.period = Some((now() - mins(12 * 60)).to_rfc3339());
        let pace = assess(&metric, now(), None).unwrap();
        assert!((pace.elapsed_fraction - 2.0 / 3.0).abs() < 1e-9);
        // Before the nominal start, after `now`, or free text: nominal window.
        for period in [now() - mins(30 * 60), now() + mins(60)]
            .map(|t| t.to_rfc3339())
            .into_iter()
            .chain(["semanal".into()])
        {
            metric.period = Some(period);
            assert_eq!(assess(&metric, now(), None).unwrap().elapsed_fraction, 0.75);
        }
    }

    #[test]
    fn pace_serializes_stably() {
        let pace = assess(
            &quota(MetricWindow::Daily, 40.0, now() + mins(18 * 60)),
            now(),
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&pace).unwrap(),
            serde_json::json!({
                "elapsed_fraction": 0.25,
                "expected_used_fraction": 0.25,
                "used_fraction": 0.4,
                "delta_points": 15.0,
                "delta_ratio": 0.6000000000000001,
                "stage": "ahead",
                "tone": "warn",
                "source": "average",
                "resets_at": "2026-02-15T18:00:00Z",
                "projected_exhaustion_at": "2026-02-15T09:00:00Z",
                "lasts_until_reset": false,
            })
        );
    }

    #[test]
    fn labels_are_translated_in_every_catalog() {
        let hhmm = |t: DateTime<Utc>| t.format("%H:%M").to_string();
        let hours = |d: TimeDelta| format!("{} h", d.num_hours());
        let label = |used, resets| {
            let pace = assess(&quota(MetricWindow::Daily, used, resets), now(), None).unwrap();
            (pace.stage_label(), pace.outlook_label(hhmm, hours))
        };
        let runs_out = label(40.0, now() + mins(18 * 60));
        assert_eq!(runs_out.0.resolve("en-US"), "15 points above the pace");
        assert_eq!(
            runs_out.1.as_ref().unwrap().resolve("en-US"),
            "runs out at 09:00, 9 h before the reset"
        );
        assert_eq!(
            runs_out.1.as_ref().unwrap().resolve("pt-BR"),
            "acaba às 09:00, 9 h antes da renovação"
        );
        let reserve = label(17.0, now() + mins(18 * 60));
        assert_eq!(reserve.0.resolve("en-US"), "8 points in reserve");
        assert_eq!(reserve.0.resolve("pt-BR"), "8 pontos de reserva");
        let on_pace = label(50.0, now() + mins(12 * 60));
        assert_eq!(
            on_pace.1.as_ref().unwrap().resolve("es"),
            "dura hasta el reinicio"
        );
        let exhausted = label(100.0, now() + mins(12 * 60));
        assert_eq!(exhausted.1, None);

        let texts = [runs_out, reserve, on_pace, exhausted]
            .into_iter()
            .flat_map(|(stage, outlook)| std::iter::once(stage).chain(outlook))
            .collect::<Vec<_>>();
        let ids: std::collections::BTreeSet<&str> = texts.iter().map(|t| &*t.id).collect();
        assert_eq!(ids.len(), 6, "{ids:?}");
        for locale in ["pt-BR", "es", "it"] {
            let catalog = crate::text::core_catalog(locale).unwrap();
            for text in &texts {
                assert!(
                    catalog.contains_key(&*text.id),
                    "{locale} lacks {}",
                    text.id
                );
                assert!(!text.resolve(locale).contains('{'), "{locale} {}", text.id);
            }
        }
    }

    #[test]
    fn recent_since_spans_the_lookback_inside_the_window() {
        let hours = |n: f64| TimeDelta::milliseconds((n * 3_600_000.0) as i64);
        let cases = [
            // Half of 5 h elapsed: the 2-hour floor.
            ("5h", five_h(50.0), Some(now() - hours(2.0))),
            // A quarter of a day elapsed: 10% of 24 h beats 2 h.
            (
                "daily",
                quota(MetricWindow::Daily, 40.0, now() + mins(18 * 60)),
                Some(now() - hours(2.4)),
            ),
            // 9 h into a week: 10% (16.8 h) reaches back past the start.
            (
                "weekly, young",
                quota(
                    MetricWindow::Weekly,
                    5.0,
                    now() + TimeDelta::weeks(1) - hours(9.0),
                ),
                Some(now() - hours(9.0)),
            ),
            (
                "not eligible",
                quota(MetricWindow::Other, 5.0, now() + mins(60)),
                None,
            ),
        ];
        for (case, metric, expected) in cases {
            assert_eq!(recent_since(&metric, now()), expected, "{case}");
        }
    }

    /// PRD 0024 FIX-11: a reading's pace is evaluated at its `checked_at`
    /// and only while the reading is `Fresh` (PRD 0010 thresholds).
    #[test]
    fn reading_pace_is_evaluated_at_checked_at_and_only_when_fresh() {
        let secs = |n: i64| TimeDelta::seconds(n);
        // Half of the 5-hour window elapsed at `now()`, 50% used: on pace
        // at `now()`, but above the pace for a reading taken earlier.
        let metric = five_h(50.0);
        // (case, refresh interval seconds, reading age seconds, expected)
        let cases = [
            ("just measured", 300, 0, Some(PaceStage::OnPace)),
            ("fresh edge", 300, 450, Some(PaceStage::OnPace)),
            ("aging", 300, 451, None),
            ("aging edge", 300, 900, None),
            ("stale", 300, 901, None),
            // 90 minutes old with a 1-hour interval: still Fresh. At its
            // `checked_at` only 20% of the window had elapsed, so 50% used
            // is 30 points above the pace; the wall clock would say on pace.
            ("fresh, long interval", 3600, 5400, Some(PaceStage::Ahead)),
            ("stale, long interval", 3600, 10_801, None),
        ];
        for (case, interval, age, expected) in cases {
            let checked_at = now() - secs(age);
            let pace = assess_reading(
                &metric,
                checked_at,
                Duration::from_secs(interval),
                now(),
                None,
            );
            assert_eq!(pace.as_ref().map(|p| p.stage), expected, "{case}");
            if let Some(pace) = pace {
                assert_eq!(Some(pace), assess(&metric, checked_at, None), "{case}");
            }
        }
        let wall_clock = assess(&metric, now(), None).unwrap();
        assert_eq!(wall_clock.stage, PaceStage::OnPace);
    }

    #[test]
    fn reading_pace_ends_when_the_window_reset_after_the_reading() {
        let metric = quota(MetricWindow::Window5h, 50.0, now() - mins(1));
        let checked_at = now() - mins(2);
        let interval = Duration::from_secs(300);
        // The reading alone still has a pace; the window it measured is over.
        assert!(assess(&metric, checked_at, None).is_some());
        assert_eq!(
            assess_reading(&metric, checked_at, interval, now(), None),
            None
        );
    }

    #[test]
    fn reading_pace_uses_the_recent_samples_up_to_checked_at() {
        let checked_at = now() - mins(5);
        let metric = five_h(45.0);
        let point = |minutes: i64, used: f64| Point {
            taken_at: checked_at - mins(minutes),
            used,
        };
        let recent = [point(40, 15.0), point(20, 30.0), point(0, 45.0)];
        let pace = assess_reading(
            &metric,
            checked_at,
            Duration::from_secs(300),
            now(),
            Some(&recent),
        )
        .unwrap();
        assert_eq!(pace.source, PaceSource::Recent);
        assert_eq!(Some(pace), assess(&metric, checked_at, Some(&recent)));
    }
}

#[cfg(test)]
#[path = "pace_chip_tests.rs"]
mod chip_tests;
