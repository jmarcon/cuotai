//! PRD 0006 RF5/RF6 — what the front ends show of an account's history: the
//! recorded series of each of its metrics over a span, with the window
//! instances, the range summary (lowest, highest and last value) and the
//! levels of a compact chart. Network-free reads of the store only.

use super::{finite, group_windows, instant, untag, HistoryError, HistoryStore, WindowSample};
use super::{span, WindowUsage};
use crate::models::account_key;
use crate::pace::Point;
use crate::{Account, MetricKind, MetricWindow};
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

    /// Levels of a compact chart of `start..=end` in `cells` equal time
    /// buckets: each bucket shows its highest `used` (the peak, as the
    /// resolution reduction keeps it) as a level from `0` to `levels - 1`.
    /// A metric with a limit is scaled from zero (or its lowest value when
    /// negative) to the limit (or its highest value when above it); one
    /// without a limit from its lowest to its highest value, so a growing
    /// spend shows its trend. `None` for a bucket without a sample (a gap
    /// is never zero); a flat series without a scale reads the lowest level.
    pub fn levels(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        cells: usize,
        levels: u8,
    ) -> Vec<Option<u8>> {
        let mut peaks: Vec<Option<f64>> = vec![None; cells];
        let total = (end - start).num_milliseconds();
        if cells == 0 || levels == 0 || total <= 0 {
            return vec![None; cells];
        }
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for point in &self.points {
            let offset = (point.taken_at - start).num_milliseconds();
            if !(0..=total).contains(&offset) {
                continue;
            }
            let bucket = usize::try_from(
                i128::from(offset) * i128::try_from(cells).unwrap_or(i128::MAX) / i128::from(total),
            )
            .unwrap_or(usize::MAX)
            .min(cells - 1);
            let peak = &mut peaks[bucket];
            *peak = Some(peak.map_or(point.used, |peak: f64| peak.max(point.used)));
            (min, max) = (min.min(point.used), max.max(point.used));
        }
        let (floor, ceiling) = match self.limit.filter(|limit| *limit > 0.0) {
            Some(limit) => (min.min(0.0), limit.max(max)),
            None => (min, max),
        };
        let top = f64::from(levels - 1);
        peaks
            .into_iter()
            .map(|peak| {
                peak.map(|used| {
                    if ceiling > floor {
                        // Bounded by the clamp, so the cast cannot truncate.
                        ((used - floor) / (ceiling - floor) * top)
                            .round()
                            .clamp(0.0, top) as u8
                    } else {
                        0
                    }
                })
            })
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
        let mut query = db.prepare(
            "SELECT x.metric_key, s.label, x.unit, x.kind, x.\"window\", x.taken_at,
                    x.used, x.\"limit\", x.resets_at
             FROM samples x JOIN series s ON s.config_key = x.config_key
               AND s.account_digest = x.account_digest AND s.metric_key = x.metric_key
             WHERE x.config_key = ?1 AND x.account_digest = ?2 AND x.taken_at BETWEEN ?3 AND ?4
             ORDER BY s.position, s.seen_at DESC, x.metric_key, x.taken_at",
        )?;
        let mut rows = query.query(rusqlite::params![
            self.config_key,
            account_key(account),
            start,
            end
        ])?;
        let mut metrics: Vec<(MetricHistory, Vec<WindowSample>)> = Vec::new();
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            let sample = WindowSample {
                taken_at: instant(row.get(5)?)?,
                used: row.get(6)?,
                limit: row.get(7)?,
                resets_at: row.get::<_, Option<i64>>(8)?.map(instant).transpose()?,
            };
            if metrics.last().is_none_or(|(metric, _)| metric.key != key) {
                metrics.push((
                    MetricHistory {
                        key,
                        label: row.get(1)?,
                        unit: row.get(2)?,
                        kind: untag(row.get(3)?)?,
                        window: row
                            .get::<_, Option<String>>(4)?
                            .map(untag::<MetricWindow>)
                            .transpose()?,
                        limit: None,
                        points: Vec::new(),
                        windows: Vec::new(),
                    },
                    Vec::new(),
                ));
            }
            let (metric, samples) = metrics.last_mut().expect("pushed above");
            metric.limit = finite(sample.limit).or(metric.limit);
            if let Some(used) = finite(sample.used) {
                metric.points.push(Point {
                    taken_at: sample.taken_at,
                    used,
                });
            }
            samples.push(sample);
        }
        Ok(metrics
            .into_iter()
            .map(|(metric, samples)| MetricHistory {
                windows: group_windows(samples),
                ..metric
            })
            .collect())
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
