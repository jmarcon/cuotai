//! PRD 0006 RF5/RF6 — what the front ends show of an account's history: the
//! recorded series of each of its metrics over a span, with the window
//! instances, the range summary (lowest, highest and last value), the placed
//! area chart the desktop draws and the levels of the TUI sparkline.
//! Network-free reads of the store only.

use super::{finite, group_windows, instant, untag, HistoryError, HistoryStore, WindowSample};
use super::{span, WindowUsage};
use crate::history::series_digest;
use crate::pace::Point;
use crate::{Account, Billing, MetricKind, MetricWindow};
use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use std::ops::RangeBounds;

/// The spans the account details offer (RF5.2, RF6.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryRange {
    /// The last 24 hours.
    #[default]
    Day,
    /// The last 7 days.
    Week,
    /// The last 30 days.
    Month,
}

impl HistoryRange {
    pub const ALL: [Self; 3] = [Self::Day, Self::Week, Self::Month];

    pub fn span(self) -> TimeDelta {
        match self {
            Self::Day => TimeDelta::days(1),
            Self::Week => TimeDelta::days(7),
            Self::Month => TimeDelta::days(30),
        }
    }

    /// Start of the span ending at `now`.
    pub fn since(self, now: DateTime<Utc>) -> DateTime<Utc> {
        now - self.span()
    }
}

/// The recorded history of one metric of an account within a span.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricHistory {
    /// [`super::metric_key`] of the metric.
    pub key: String,
    pub label: String,
    pub unit: String,
    pub kind: MetricKind,
    pub window: Option<MetricWindow>,
    /// Plan allowance or paid extra usage (PRD 0026 RF1.5).
    pub billing: Billing,
    /// A limit of one model, not of the account (PRD 0023 RF10): it does
    /// not count as a limit hit of the account.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scoped: bool,
    /// The last finite `limit` recorded in the span, if any.
    pub limit: Option<f64>,
    /// Samples with a finite `used`, in time order.
    pub points: Vec<Point>,
    /// Window instances in the span (see [`HistoryStore::windows`]).
    pub windows: Vec<WindowUsage>,
}

/// Lowest, highest and last `used` of a series.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Trend {
    pub min: f64,
    pub max: f64,
    pub last: Point,
}

/// Samples at most this far apart draw a continuous area: an unchanged
/// value is not stored again within 15 minutes (RF2.3) and refreshes may be
/// hourly. A longer silence (the app closed, the computer off) is a gap.
const CHART_JOIN: TimeDelta = TimeDelta::hours(1);

/// A compact chart of one metric over a span (RF5.2), already placed: the
/// front ends only draw it. Horizontal places run from 0 (the start of the
/// span) to 1 (its end); heights from 0 (`floor`) to 1 (`ceiling`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chart {
    /// The value axis: from zero (or the lowest value when negative) to the
    /// limit (or the highest value when above it) for a metric with a
    /// limit; from the lowest to the highest value without one. Equal
    /// bounds (a flat series) put every value on the floor.
    pub floor: f64,
    pub ceiling: f64,
    /// The highest `used` of each equal time bucket (the peak, as the
    /// resolution reduction keeps it) as a height. An empty bucket between
    /// two samples at most an hour apart carries the earlier value; any
    /// other empty bucket is `None` (a gap, never zero).
    pub heights: Vec<Option<f64>>,
    /// The last measured point of the span, highlighted.
    pub last: Option<ChartPoint>,
    /// Where each window instance after the first began in the span: a
    /// reset (a reset-sized drop or a new `resets_at`).
    pub resets: Vec<ChartReset>,
}

/// A measured point placed on a [`Chart`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ChartPoint {
    pub taken_at: DateTime<Utc>,
    pub used: f64,
    pub x: f64,
    pub y: f64,
}

/// A reset placed on a [`Chart`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ChartReset {
    pub at: DateTime<Utc>,
    pub x: f64,
}

/// Where `at` falls in `start..=end`: its millisecond offset and the span's
/// length, `None` outside it (or for an empty span).
fn span_offset(at: DateTime<Utc>, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<(i64, i64)> {
    let total = (end - start).num_milliseconds();
    let offset = (at - start).num_milliseconds();
    (total > 0 && (0..=total).contains(&offset)).then_some((offset, total))
}

/// `at` as a horizontal place in `start..=end` (0 to 1).
fn place(at: DateTime<Utc>, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<f64> {
    // Millisecond counts of a span stay far below 2^53, so the casts are exact.
    span_offset(at, start, end).map(|(offset, total)| offset as f64 / total as f64)
}

impl Chart {
    /// `used` as a height on this chart's axis (0 to 1).
    fn height(&self, used: f64) -> f64 {
        if self.ceiling > self.floor {
            ((used - self.floor) / (self.ceiling - self.floor)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

impl MetricHistory {
    /// The range summary; `None` without a measured point.
    pub fn trend(&self) -> Option<Trend> {
        let last = *self.points.last()?;
        let (min, max) = self
            .points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), point| {
                (min.min(point.used), max.max(point.used))
            });
        Some(Trend { min, max, last })
    }

    /// The chart of `start..=end` in `cells` equal time buckets (see
    /// [`Chart`]). Only samples within the span count, for the peaks and
    /// for the scale; with none, every bucket is a gap.
    pub fn chart(&self, start: DateTime<Utc>, end: DateTime<Utc>, cells: usize) -> Chart {
        self.plot(start, end, cells, Some(CHART_JOIN))
    }

    /// [`Self::chart`]; with `join`, the empty buckets between two samples
    /// at most `join` apart carry the earlier value instead of a gap.
    fn plot(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        cells: usize,
        join: Option<TimeDelta>,
    ) -> Chart {
        let mut peaks: Vec<Option<f64>> = vec![None; cells];
        let mut range: Option<(f64, f64)> = None;
        let mut last = None;
        let mut previous: Option<(DateTime<Utc>, usize, f64)> = None;
        for point in &self.points {
            let Some((offset, total)) = span_offset(point.taken_at, start, end) else {
                continue;
            };
            if cells > 0 {
                let bucket = usize::try_from(
                    i128::from(offset) * i128::try_from(cells).unwrap_or(i128::MAX)
                        / i128::from(total),
                )
                .unwrap_or(usize::MAX)
                .min(cells - 1);
                if let (Some(join), Some((at, from, used))) = (join, previous) {
                    if point.taken_at - at <= join {
                        // Points come in time order, so these buckets are empty.
                        for peak in peaks.iter_mut().take(bucket).skip(from + 1) {
                            *peak = Some(used);
                        }
                    }
                }
                let peak = &mut peaks[bucket];
                *peak = Some(peak.map_or(point.used, |peak: f64| peak.max(point.used)));
                previous = Some((point.taken_at, bucket, point.used));
            }
            let x = offset as f64 / total as f64;
            range = Some(range.map_or((point.used, point.used), |(min, max)| {
                (min.min(point.used), max.max(point.used))
            }));
            if last.is_none_or(|(at, _, _)| point.taken_at >= at) {
                last = Some((point.taken_at, point.used, x));
            }
        }
        let (min, max) = range.unwrap_or((0.0, 0.0));
        let (floor, ceiling) = match self.limit.filter(|limit| *limit > 0.0) {
            Some(limit) => (min.min(0.0), limit.max(max)),
            None => (min, max),
        };
        let mut chart = Chart {
            floor,
            ceiling,
            heights: Vec::new(),
            last: None,
            resets: self
                .windows
                .iter()
                .skip(1)
                .filter_map(|window| {
                    let x = place(window.started_at, start, end)?;
                    Some(ChartReset {
                        at: window.started_at,
                        x,
                    })
                })
                .collect(),
        };
        chart.heights = peaks
            .into_iter()
            .map(|peak| peak.map(|used| chart.height(used)))
            .collect();
        chart.last = last.map(|(taken_at, used, x)| ChartPoint {
            taken_at,
            used,
            x,
            y: chart.height(used),
        });
        chart
    }

    /// Levels of a compact chart of `start..=end` in `cells` equal time
    /// buckets: each bucket's peak, placed as in [`Chart::heights`], as a
    /// level from `0` to `levels - 1`; `None` for every bucket without a
    /// sample (no joining: each cell stands for its own readings).
    pub fn levels(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        cells: usize,
        levels: u8,
    ) -> Vec<Option<u8>> {
        if levels == 0 {
            return vec![None; cells];
        }
        let top = f64::from(levels - 1);
        self.plot(start, end, cells, None)
            .heights
            .into_iter()
            // A height is within 0..=1, so the cast cannot truncate.
            .map(|height| height.map(|height| (height * top).round() as u8))
            .collect()
    }
}

impl HistoryStore {
    /// RF5.1/RF6: every recorded metric of `account` with samples within
    /// `range`, ordered as the account last reported them, each with its
    /// points (only finite `used`) and window instances. Empty when nothing
    /// was recorded in the span; the file is never created by this read.
    pub fn account_history(
        &self,
        account: &Account,
        range: impl RangeBounds<DateTime<Utc>>,
    ) -> Result<Vec<MetricHistory>, HistoryError> {
        let Some(db) = self.open_existing()? else {
            return Ok(Vec::new());
        };
        let (start, end) = span(range);
        let account_digest = series_digest(&db, &self.config_key, account)?;
        // The series of the account first (a few rows), then the samples of
        // each one through the primary key, already in time order. One join
        // over the account's whole span would walk the samples of every
        // account in it through `samples_by_time` and sort them in a
        // temporary tree: an order of magnitude slower on a month of data.
        let mut series_query = db.prepare(
            "SELECT metric_key, label, billing, scope IS NOT NULL FROM series
             WHERE config_key = ?1 AND account_digest = ?2
             ORDER BY position, seen_at DESC, metric_key",
        )?;
        let series: Vec<(String, String, String, bool)> = series_query
            .query_map(rusqlite::params![self.config_key, account_digest], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut samples_query = db.prepare(
            "SELECT unit, kind, \"window\", taken_at, used, \"limit\", resets_at FROM samples
             WHERE config_key = ?1 AND account_digest = ?2 AND metric_key = ?3
               AND taken_at BETWEEN ?4 AND ?5
             ORDER BY taken_at",
        )?;
        let mut metrics: Vec<MetricHistory> = Vec::new();
        for (key, label, billing, scoped) in series {
            let mut rows = samples_query.query(rusqlite::params![
                self.config_key,
                account_digest,
                key,
                start,
                end
            ])?;
            let mut metric: Option<MetricHistory> = None;
            let mut samples = Vec::new();
            while let Some(row) = rows.next()? {
                let sample = WindowSample {
                    taken_at: instant(row.get(3)?)?,
                    used: row.get(4)?,
                    limit: row.get(5)?,
                    resets_at: row.get::<_, Option<i64>>(6)?.map(instant).transpose()?,
                };
                let metric = match &mut metric {
                    Some(metric) => metric,
                    None => metric.insert(MetricHistory {
                        key: key.clone(),
                        label: label.clone(),
                        unit: row.get(0)?,
                        kind: untag(row.get(1)?)?,
                        window: row
                            .get::<_, Option<String>>(2)?
                            .map(untag::<MetricWindow>)
                            .transpose()?,
                        billing: untag(billing.clone())?,
                        scoped,
                        limit: None,
                        points: Vec::new(),
                        windows: Vec::new(),
                    }),
                };
                metric.limit = finite(sample.limit).or(metric.limit);
                if let Some(used) = finite(sample.used) {
                    metric.points.push(Point {
                        taken_at: sample.taken_at,
                        used,
                    });
                }
                samples.push(sample);
            }
            // A series without samples in the span is left out, as before.
            if let Some(metric) = metric {
                metrics.push(MetricHistory {
                    windows: group_windows(samples),
                    ..metric
                });
            }
        }
        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::super::metric_key;
    use super::super::tests::{config, quota, store, t0, CONFIG};
    use super::*;
    use crate::Metric;

    fn history(points: &[(i64, f64)], limit: Option<f64>) -> MetricHistory {
        MetricHistory {
            key: "k".into(),
            label: "Sessão".into(),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            billing: Billing::Included,
            scoped: false,
            limit,
            points: points
                .iter()
                .map(|&(hours, used)| Point {
                    taken_at: t0() + TimeDelta::hours(hours),
                    used,
                })
                .collect(),
            windows: Vec::new(),
        }
    }

    #[test]
    fn ranges_span_a_day_a_week_and_a_month() {
        let spans: Vec<i64> = HistoryRange::ALL
            .iter()
            .map(|range| (t0() - range.since(t0())).num_hours())
            .collect();
        assert_eq!(spans, [24, 24 * 7, 24 * 30]);
        assert_eq!(HistoryRange::default(), HistoryRange::Day);
    }

    #[test]
    fn trend_reports_lowest_highest_and_last() {
        let trend = history(&[(0, 30.0), (1, 5.0), (2, 80.0), (3, 40.0)], Some(100.0))
            .trend()
            .unwrap();
        assert_eq!((trend.min, trend.max, trend.last.used), (5.0, 80.0, 40.0));
        assert_eq!(trend.last.taken_at, t0() + TimeDelta::hours(3));
        assert_eq!(history(&[], Some(100.0)).trend(), None);
    }

    #[test]
    fn levels_take_each_bucket_peak_against_the_limit_and_keep_gaps() {
        // 4 buckets of 1 h over 4 h: two samples in the first (the peak
        // wins), none in the second, a full and a half reading after.
        let metric = history(&[(0, 10.0), (0, 50.0), (2, 100.0), (3, 50.0)], Some(100.0));
        let start = t0();
        let end = t0() + TimeDelta::hours(4);
        assert_eq!(
            metric.levels(start, end, 4, 8),
            [Some(4), None, Some(7), Some(4)]
        );
        // Without a limit the scale runs from the lowest to the highest
        // value, so a growing spend shows its trend.
        let spend = history(&[(0, 2.0), (3, 4.0)], None);
        assert_eq!(
            spend.levels(start, end, 4, 8),
            [Some(0), None, None, Some(7)]
        );
        // Use above the limit is never cut: the scale grows to it.
        let over = history(&[(0, 50.0), (3, 200.0)], Some(100.0));
        assert_eq!(
            over.levels(start, end, 4, 8),
            [Some(2), None, None, Some(7)]
        );
        // A flat zero without a limit reads the lowest level; no sample at
        // all is only gaps; samples outside the span are ignored.
        let flat = history(&[(0, 0.0), (3, 0.0)], None);
        assert_eq!(flat.levels(start, end, 2, 8), [Some(0), Some(0)]);
        assert_eq!(history(&[], None).levels(start, end, 3, 8), [None; 3]);
        let outside = history(&[(-2, 90.0), (5, 90.0), (1, 10.0)], Some(100.0));
        assert_eq!(outside.levels(start, end, 2, 8), [Some(1), None]);
        // Only the span sets the scale.
        let spend = history(&[(-2, 1000.0), (1, 10.0), (3, 20.0)], None);
        assert_eq!(spend.levels(start, end, 2, 8), [Some(0), Some(7)]);
    }

    #[test]
    fn chart_places_bucket_peaks_and_the_last_point_on_the_axis() {
        let start = t0();
        let end = t0() + TimeDelta::hours(4);
        // Against the limit: each bucket's peak, a gap, the end highlighted.
        let chart = history(&[(0, 10.0), (0, 50.0), (2, 100.0), (3, 25.0)], Some(100.0))
            .chart(start, end, 4);
        assert_eq!((chart.floor, chart.ceiling), (0.0, 100.0));
        assert_eq!(chart.heights, [Some(0.5), None, Some(1.0), Some(0.25)]);
        let last = chart.last.expect("the end point");
        assert_eq!(
            (last.taken_at, last.used, last.x, last.y),
            (t0() + TimeDelta::hours(3), 25.0, 0.75, 0.25)
        );
        // A point after the span neither draws nor ends the chart.
        let outside = history(&[(1, 40.0), (6, 90.0)], Some(100.0)).chart(start, end, 4);
        assert_eq!(outside.last.map(|p| (p.used, p.x)), Some((40.0, 0.25)));
        assert_eq!(outside.heights, [None, Some(0.4), None, None]);
        // Readings an hour apart draw a continuous area (the earlier value
        // carries on); two hours of silence stay a gap.
        let joined = history(&[(0, 20.0), (1, 40.0), (3, 60.0)], Some(100.0)).chart(start, end, 8);
        assert_eq!(
            joined.heights,
            [
                Some(0.2),
                Some(0.2),
                Some(0.4),
                None,
                None,
                None,
                Some(0.6),
                None
            ]
        );
        // Without a limit the axis runs from the lowest to the highest
        // value; a lone value sits on the floor.
        let spend = history(&[(0, 2.0), (4, 4.0)], None).chart(start, end, 2);
        assert_eq!((spend.floor, spend.ceiling), (2.0, 4.0));
        assert_eq!(spend.heights, [Some(0.0), Some(1.0)]);
        let lone = history(&[(1, 7.0)], None).chart(start, end, 2);
        assert_eq!(lone.heights, [Some(0.0), None]);
        assert_eq!(lone.last.map(|p| p.y), Some(0.0));
        // Nothing in the span: only gaps and no end point, never zero.
        let empty = history(&[(-3, 50.0)], Some(100.0)).chart(start, end, 3);
        assert_eq!(empty.heights, [None; 3]);
        assert_eq!((empty.last, empty.floor, empty.ceiling), (None, 0.0, 100.0));
    }

    #[test]
    fn chart_marks_each_reset_where_a_new_window_began() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir.path().join("history.sqlite3"), "one");
        let alpha = &config(CONFIG).accounts[0];
        for (hours, used) in [(0, 10.0), (1, 60.0), (2, 5.0), (3, 30.0)] {
            let at = t0() + TimeDelta::hours(hours);
            history
                .record(alpha, at, &[quota("Sessão", Some(used), Some(100.0))])
                .unwrap();
        }
        let session = &history.account_history(alpha, ..).unwrap()[0];
        let chart = session.chart(t0(), t0() + TimeDelta::hours(4), 8);
        // The drop from 60 to 5 of 100 opened the second window.
        assert_eq!(
            chart.resets,
            [ChartReset {
                at: t0() + TimeDelta::hours(2),
                x: 0.5
            }]
        );
        // A reset outside the drawn span is not placed on it.
        let late = session.chart(t0() + TimeDelta::hours(3), t0() + TimeDelta::hours(4), 8);
        assert_eq!(late.resets, []);
        // A single window has no reset.
        let first = history
            .account_history(alpha, ..=t0() + TimeDelta::hours(1))
            .unwrap();
        assert_eq!(
            first[0].chart(t0(), t0() + TimeDelta::hours(1), 4).resets,
            []
        );
    }

    #[test]
    fn account_history_lists_each_metric_with_points_windows_and_limit() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let accounts = config(CONFIG).accounts;
        let (alpha, beta) = (&accounts[0], &accounts[1]);
        // Never creates the file to say there is nothing.
        assert_eq!(history.account_history(alpha, ..), Ok(Vec::new()));
        assert!(!db.exists());

        let week = |used: Option<f64>| Metric {
            label: "Semana".into(),
            window: Some(MetricWindow::Weekly),
            resets_at: Some(t0() + TimeDelta::days(3)),
            ..quota("Semana", used, None)
        };
        let reading = |hours: i64, session: f64, weekly: Option<f64>| {
            let at = t0() + TimeDelta::hours(hours);
            history
                .record(
                    alpha,
                    at,
                    &[quota("Sessão", Some(session), Some(100.0)), week(weekly)],
                )
                .unwrap();
        };
        reading(0, 10.0, Some(1.0));
        reading(1, 60.0, None);
        // The 5-hour window reset: the drop opens a new window.
        reading(2, 5.0, Some(3.0));
        history
            .record(beta, t0(), &[quota("Sessão", Some(99.0), Some(100.0))])
            .unwrap();

        let all = history.account_history(alpha, ..).unwrap();
        let labels: Vec<&str> = all.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["Sessão", "Semana"], "the reported order");
        let session = &all[0];
        assert_eq!(
            session.key,
            metric_key(&quota("Sessão", Some(1.0), Some(100.0)))
        );
        assert_eq!(
            (session.kind, session.window, session.unit.as_str()),
            (MetricKind::Quota, Some(MetricWindow::Window5h), "%")
        );
        assert_eq!(session.limit, Some(100.0));
        let used: Vec<f64> = session.points.iter().map(|p| p.used).collect();
        assert_eq!(used, [10.0, 60.0, 5.0]);
        assert_eq!(session.windows.len(), 2);
        assert_eq!(session.windows[0].peak_used, 60.0);
        // A missing value is left out of the points, never zero, and a
        // metric without a limit reports none.
        let weekly = &all[1];
        let used: Vec<f64> = weekly.points.iter().map(|p| p.used).collect();
        assert_eq!(used, [1.0, 3.0]);
        assert_eq!(weekly.limit, None);

        // The span bounds the samples; a span without any is empty.
        let late = history
            .account_history(alpha, t0() + TimeDelta::minutes(90)..)
            .unwrap();
        assert_eq!(late[0].points.len(), 1);
        assert_eq!(
            history
                .account_history(alpha, ..t0() - TimeDelta::hours(1))
                .unwrap(),
            Vec::new()
        );
        // Another account and another configuration are not mixed in.
        assert_eq!(history.account_history(beta, ..).unwrap().len(), 1);
        assert_eq!(store(&db, "two").account_history(alpha, ..), Ok(Vec::new()));
    }
}
