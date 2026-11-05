//! PRD 0043 — usage insights over readings already recorded: limit-change
//! detection (RF2) and the hourly/daily rhythm (RF1). Re-exported by
//! [`crate::stats`] as `stats::limit_changes` and `stats::rhythm`.
//!
//! Nothing here queries a provider: limit changes compare two readings the
//! refresh already has, and the rhythm reads the history store. Honesty
//! rules: a bucket without samples is `None` (a gap), never `0`; the typical
//! profile states its sample count.

use crate::model::{Metric, MetricKind, MetricWindow, Report};
use crate::text::{params, Localized, ParamValue};
use chrono::{DateTime, Utc};
use serde::Serialize;

/// RF2.1: a relative change at most this large is noise (rounding).
pub const LIMIT_CHANGE_MIN_RATIO: f64 = 0.005;
/// RF2.2: how long a limit change shows as a card note.
pub const LIMIT_NOTE_SPAN: chrono::TimeDelta = chrono::TimeDelta::hours(48);

/// RF2.1: a metric's `limit` changed between two readings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LimitChange {
    pub label: String,
    /// PRD 0034 RF2: display text of `label`.
    pub label_text: Localized,
    pub window: Option<MetricWindow>,
    pub from: f64,
    pub to: f64,
    pub unit: String,
    /// When the change was observed (the newer reading's `checked_at`).
    pub observed_at: DateTime<Utc>,
}

fn finite_limit(metric: &Metric) -> Option<f64> {
    metric
        .limit
        .filter(|limit| limit.is_finite() && *limit > 0.0)
}

/// RF2.1: whether a limit moving from `from` to `to` is a change, not a
/// rounding: more than 0.5% of `from`, or at least one unit when both are
/// whole numbers.
pub fn is_limit_change(from: f64, to: f64) -> bool {
    let delta = (to - from).abs();
    if from.fract() == 0.0 && to.fract() == 0.0 {
        return delta >= 1.0 && delta > from.abs() * LIMIT_CHANGE_MIN_RATIO;
    }
    delta > from.abs() * LIMIT_CHANGE_MIN_RATIO
}

/// RF2.1: the limit changes between `previous` and `current` (two readings
/// of one account): for each provider metric with a finite positive limit
/// in both, a change beyond [`is_limit_change`]. A metric that just renewed
/// (collapsed, PRD 0033) has no limit and is compared only once the new
/// window reports one, so a reset alone is never a change. Budget metrics
/// (PRD 0041) are the person's numbers and never count.
pub fn limit_changes(previous: &Report, current: &Report) -> Vec<LimitChange> {
    current
        .metrics
        .iter()
        .filter(|metric| metric.derived_from.is_none() && metric.kind != MetricKind::RateLimit)
        .filter_map(|metric| {
            let to = finite_limit(metric)?;
            let before = previous.metrics.iter().find(|old| {
                old.label == metric.label
                    && old.derived_from.is_none()
                    && old.unit == metric.unit
                    && old.scope == metric.scope
            })?;
            let from = finite_limit(before)?;
            is_limit_change(from, to).then(|| LimitChange {
                label: metric.label.clone(),
                label_text: crate::metric_text::metric_text(&metric.label),
                window: metric.window,
                from,
                to,
                unit: metric.unit.clone(),
                observed_at: current.checked_at,
            })
        })
        .collect()
}

/// A number without trailing zeros (`150`, `12.5`).
pub(crate) fn plain(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl LimitChange {
    /// RF2.2: the card note "Weekly limit changed 100 → 150 points on Oct
    /// 25"; `date` formats the observation day in the viewer's zone and
    /// locale.
    pub fn note(&self, date: impl FnOnce(DateTime<Utc>) -> String) -> Localized {
        Localized::with_params(
            "insights.limit_changed.note",
            params([
                ("metric", ParamValue::l10n(self.label_text.clone())),
                ("from", ParamValue::Text(plain(self.from))),
                ("to", ParamValue::Text(plain(self.to))),
                (
                    "unit",
                    ParamValue::l10n(crate::metric_text::metric_text(&self.unit)),
                ),
                ("date", ParamValue::Text(date(self.observed_at))),
            ]),
            "{metric} limit changed {from} → {to} {unit} on {date}",
        )
    }

    /// Whether the card note still shows at `now` ([`LIMIT_NOTE_SPAN`]).
    pub fn shows_at(&self, now: DateTime<Utc>) -> bool {
        now >= self.observed_at && now - self.observed_at <= LIMIT_NOTE_SPAN
    }
}

/// RF1.1: the span a [`Rhythm`] covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RhythmRange {
    /// 24 hourly buckets ending with the current hour.
    Last24h,
    /// 7 daily buckets ending today, days off marked.
    Last7d,
    /// 24 hour-of-day buckets averaged over the last 30 days.
    Typical30d,
}

impl RhythmRange {
    pub const ALL: [Self; 3] = [Self::Last24h, Self::Last7d, Self::Typical30d];

    /// `24h`, `7d` or `typical` (the CLI spelling).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "24h" => Some(Self::Last24h),
            "7d" => Some(Self::Last7d),
            "typical" => Some(Self::Typical30d),
            _ => None,
        }
    }
}

/// RF1.1: the typical profile needs this many days of data per hour.
pub const TYPICAL_MIN_DAYS: usize = 3;

/// RF1.1/RF1.2: one bar of a [`Rhythm`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RhythmBucket {
    /// The bucket's start (for `typical`, the hour of `now`'s day).
    pub start: DateTime<Utc>,
    /// The local hour (0–23) of an hourly bucket.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hour: Option<u8>,
    /// The local date of a daily bucket.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<chrono::NaiveDate>,
    /// Consumption in percentage points of the limit (the increase of
    /// `used`, resets dropped); `None` without a sample (a gap, never 0).
    pub consumption: Option<f64>,
    /// Samples in the bucket; for `typical`, the days with data.
    pub samples: usize,
    /// A quota reset happened in the bucket.
    pub reset: bool,
    /// A daily bucket on one of the person's days off.
    pub day_off: bool,
}

/// RF1.1: how use spread over hours or days, from the local history only.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rhythm {
    pub range: RhythmRange,
    pub buckets: Vec<RhythmBucket>,
    /// The metrics summed, as `label` per account in scope (the binding
    /// plan limit of each one, RF §12).
    pub metrics: Vec<String>,
}

/// Recorded samples of one series: `(taken_at, used)` and its limit.
struct Series {
    points: Vec<crate::pace::Point>,
    limit: f64,
}

/// The plan limit of an account that drives its rhythm: an included,
/// unscoped quota with a known limit, the one with the highest peak share
/// in the span (the binding one, PRD 0009), or the one whose label is
/// `chosen`.
fn rhythm_series(
    histories: &[crate::history::MetricHistory],
    chosen: Option<&str>,
) -> Option<(String, Series)> {
    histories
        .iter()
        .filter(|metric| {
            metric.kind == MetricKind::Quota
                && metric.billing == crate::Billing::Included
                && !metric.scoped
                && metric.window != Some(MetricWindow::Other)
                && chosen.is_none_or(|label| metric.label == label)
        })
        .filter_map(|metric| {
            let limit = metric.limit.filter(|l| l.is_finite() && *l > 0.0)?;
            let peak = metric.points.iter().map(|p| p.used).fold(0.0_f64, f64::max) / limit;
            Some((peak, metric, limit))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, metric, limit)| {
            (
                metric.label.clone(),
                Series {
                    points: metric.points.clone(),
                    limit,
                },
            )
        })
}

/// RF1.1: what a [`rhythm`] covers.
#[derive(Debug, Clone, Copy)]
pub struct RhythmQuery<'a> {
    /// The account's position in `accounts`; `None` sums all of them.
    pub scope: Option<usize>,
    /// Another plan limit of a single account, by label.
    pub metric: Option<&'a str>,
    pub range: RhythmRange,
    pub days_off: DaysOff,
}

/// RF1.1: the rhythm of the account at `scope` in `accounts` (or of all of
/// them summed, `None`) over `range` at `now`, in `tz`. Buckets hold the
/// consumption in percentage points of the binding plan limit (summed over
/// accounts); a bucket without samples is `None`. `metric` picks another
/// plan limit of a single account by label. Reads the history only.
pub fn rhythm<Tz: chrono::TimeZone>(
    store: &crate::history::HistoryStore,
    accounts: &[crate::Account],
    query: &RhythmQuery<'_>,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Result<Rhythm, crate::history::HistoryError> {
    use chrono::{Datelike, TimeDelta, Timelike};
    let RhythmQuery {
        scope,
        metric,
        range,
        days_off,
    } = *query;
    let local_now = now.with_timezone(tz);
    let hour_start = now
        - TimeDelta::minutes(i64::from(local_now.minute()))
        - TimeDelta::seconds(i64::from(local_now.second()))
        - TimeDelta::nanoseconds(i64::from(local_now.nanosecond()));
    let today = local_now.date_naive();
    // Bucket starts (with the label of each) and the span read.
    let (starts, since): (Vec<DateTime<Utc>>, DateTime<Utc>) = match range {
        RhythmRange::Last24h => {
            let starts: Vec<_> = (0..24)
                .map(|i| hour_start - TimeDelta::hours(23 - i))
                .collect();
            let since = starts[0];
            (starts, since)
        }
        RhythmRange::Last7d => {
            let starts: Vec<_> = (0..7)
                .filter_map(|i| crate::budget::local_midnight(today - TimeDelta::days(6 - i), tz))
                .collect();
            let since = starts.first().copied().unwrap_or(now);
            (starts, since)
        }
        RhythmRange::Typical30d => {
            let since = crate::budget::local_midnight(today - TimeDelta::days(29), tz)
                .unwrap_or(now - TimeDelta::days(30));
            let starts = (0..24)
                .map(|h| {
                    crate::budget::local_midnight(today, tz).unwrap_or(now) + TimeDelta::hours(h)
                })
                .collect();
            (starts, since)
        }
    };
    let selected: Vec<&crate::Account> = match scope {
        Some(index) => accounts.get(index).into_iter().collect(),
        None => accounts.iter().collect(),
    };
    // Per-bucket sums, sample counts and resets; for `typical`, per
    // (date, hour) cells first.
    let n = starts.len();
    let mut sums: Vec<Option<f64>> = vec![None; n];
    let mut counts = vec![0usize; n];
    let mut resets = vec![false; n];
    let mut cells: std::collections::BTreeMap<(chrono::NaiveDate, u32), f64> = Default::default();
    let mut labels = Vec::new();
    let index_of = |at: DateTime<Utc>| -> Option<usize> {
        if at < since || at > now {
            return None;
        }
        match range {
            RhythmRange::Last24h => {
                let i = (at - starts[0]).num_hours();
                usize::try_from(i).ok().filter(|i| *i < n)
            }
            RhythmRange::Last7d => {
                let date = at.with_timezone(tz).date_naive();
                let first = today - TimeDelta::days(6);
                usize::try_from((date - first).num_days())
                    .ok()
                    .filter(|i| *i < n)
            }
            RhythmRange::Typical30d => Some(at.with_timezone(tz).hour() as usize),
        }
    };
    for account in selected {
        let histories = store.account_history(account, since..=now)?;
        let chosen = metric.filter(|_| scope.is_some());
        let Some((label, series)) = rhythm_series(&histories, chosen) else {
            continue;
        };
        labels.push(label);
        let mut previous: Option<f64> = None;
        for point in &series.points {
            let delta = previous.map(|before| point.used - before);
            previous = Some(point.used);
            let Some(index) = index_of(point.taken_at) else {
                continue;
            };
            counts[index] += 1;
            let consumed = match delta {
                Some(delta) if delta < 0.0 => {
                    resets[index] = true;
                    0.0
                }
                Some(delta) => delta * 100.0 / series.limit,
                None => 0.0,
            };
            if range == RhythmRange::Typical30d {
                let local = point.taken_at.with_timezone(tz);
                *cells.entry((local.date_naive(), local.hour())).or_default() += consumed;
            } else {
                sums[index] = Some(sums[index].unwrap_or(0.0) + consumed);
            }
        }
    }
    if range == RhythmRange::Typical30d {
        for (hour, slot) in sums.iter_mut().enumerate() {
            let days: Vec<f64> = cells
                .iter()
                .filter(|((_, h), _)| *h as usize == hour)
                .map(|(_, value)| *value)
                .collect();
            counts[hour] = days.len();
            *slot = (days.len() >= TYPICAL_MIN_DAYS)
                .then(|| days.iter().sum::<f64>() / days.len() as f64);
        }
    }
    let buckets = starts
        .iter()
        .enumerate()
        .map(|(i, start)| {
            let local = start.with_timezone(tz);
            let daily = range == RhythmRange::Last7d;
            RhythmBucket {
                start: *start,
                hour: (!daily).then(|| local.hour() as u8),
                day: daily.then(|| local.date_naive()),
                consumption: sums[i],
                samples: counts[i],
                reset: resets[i],
                day_off: daily && days_off.contains(local.weekday()),
            }
        })
        .collect();
    Ok(Rhythm {
        range,
        buckets,
        metrics: labels,
    })
}

/// RF3.1: the consumption (percentage points of each account's binding
/// plan limit) per local hour of day between `start` and `end`, summed
/// over the days and the accounts in `scope` (all with `None`).
pub fn hour_totals<Tz: chrono::TimeZone>(
    store: &crate::history::HistoryStore,
    accounts: &[crate::Account],
    scope: Option<usize>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    tz: &Tz,
) -> Result<[f64; 24], crate::history::HistoryError> {
    use chrono::Timelike;
    let mut hours = [0.0_f64; 24];
    let selected: Vec<&crate::Account> = match scope {
        Some(index) => accounts.get(index).into_iter().collect(),
        None => accounts.iter().collect(),
    };
    for account in selected {
        let histories = store.account_history(account, start..=end)?;
        let Some((_, series)) = rhythm_series(&histories, None) else {
            continue;
        };
        for pair in series.points.windows(2) {
            let delta = pair[1].used - pair[0].used;
            if delta > 0.0 {
                let hour = pair[1].taken_at.with_timezone(tz).hour() as usize;
                hours[hour] += delta * 100.0 / series.limit;
            }
        }
    }
    Ok(hours)
}

impl Rhythm {
    /// RF5: the text bar chart `cuotai --stats --rhythm` prints: one line
    /// per bucket, `HH` or the weekday and date, a bar scaled to the
    /// largest bucket, the points, `·` for a gap, `↺` for a reset and
    /// `(off)` for a day off.
    pub fn text_chart(&self, locale: &str, width: usize) -> String {
        let max = self
            .buckets
            .iter()
            .filter_map(|b| b.consumption)
            .fold(0.0_f64, f64::max);
        let mut out = String::new();
        for bucket in &self.buckets {
            let head = match (bucket.hour, bucket.day) {
                (_, Some(day)) => {
                    use chrono::Datelike;
                    let name =
                        crate::budget::weekday_text(weekday_name(day.weekday())).resolve(locale);
                    format!("{name:<10} {}", day.format("%m-%d"))
                }
                (Some(hour), None) => format!("{hour:02}"),
                _ => String::new(),
            };
            let line = match bucket.consumption {
                Some(value) => {
                    let len = if max > 0.0 {
                        ((value / max) * width as f64).round() as usize
                    } else {
                        0
                    };
                    format!("{} {value:.1}", "█".repeat(len))
                }
                None => "·".to_owned(),
            };
            let mut marks = String::new();
            if bucket.reset {
                marks.push_str(" ↺");
            }
            if bucket.day_off {
                marks.push_str(" (off)");
            }
            if self.range == RhythmRange::Typical30d && bucket.consumption.is_some() {
                marks.push_str(&format!(" n={}", bucket.samples));
            }
            out.push_str(&format!("{head:<16} {line}{marks}\n"));
        }
        out
    }
}

pub(crate) fn weekday_name(day: chrono::Weekday) -> crate::budget::WeekdayName {
    use crate::budget::WeekdayName::*;
    match day {
        chrono::Weekday::Mon => Monday,
        chrono::Weekday::Tue => Tuesday,
        chrono::Weekday::Wed => Wednesday,
        chrono::Weekday::Thu => Thursday,
        chrono::Weekday::Fri => Friday,
        chrono::Weekday::Sat => Saturday,
        chrono::Weekday::Sun => Sunday,
    }
}

/// The weekday slugs `days_off` is written with, Monday first.
pub const WEEKDAY_SLUGS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// PRD 0043 RF1.3: the person's days off, Monday first (`notify.days_off`,
/// default Saturday and Sunday), shared by the rhythm and the digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaysOff(pub [bool; 7]);

impl Default for DaysOff {
    fn default() -> Self {
        Self([false, false, false, false, false, true, true])
    }
}

impl DaysOff {
    /// Whether `day` is a day off.
    pub fn contains(&self, day: chrono::Weekday) -> bool {
        self.0[day.num_days_from_monday() as usize]
    }

    /// Lenient read of a list of weekday slugs; anything else is the default.
    pub fn from_value(value: &serde_json::Value) -> Self {
        let Some(list) = value.as_array() else {
            return Self::default();
        };
        let mut days = [false; 7];
        for slug in list.iter().filter_map(serde_json::Value::as_str) {
            if let Some(index) = WEEKDAY_SLUGS.iter().position(|known| *known == slug) {
                days[index] = true;
            }
        }
        Self(days)
    }

    /// The slugs of the days off, Monday first.
    pub fn slugs(&self) -> Vec<&'static str> {
        WEEKDAY_SLUGS
            .iter()
            .zip(self.0)
            .filter_map(|(slug, off)| off.then_some(*slug))
            .collect()
    }
}

impl Serialize for DaysOff {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.slugs().serialize(serializer)
    }
}

/// PRD 0043 RF3.2: the opt-in weekly digest (`notify.digest`): off by
/// default; Monday 09:00 local when on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DigestPrefs {
    pub enabled: bool,
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u8,
    /// 0 to 23, local time.
    pub hour: u8,
}

impl Default for DigestPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            weekday: 0,
            hour: 9,
        }
    }
}

impl DigestPrefs {
    /// Each field on its own: an unknown or out-of-range value is its default.
    pub fn from_value(value: &serde_json::Value) -> Self {
        let default = Self::default();
        let small = |key: &str, max: u8, default: u8| {
            value[key]
                .as_u64()
                .and_then(|v| u8::try_from(v).ok())
                .filter(|v| *v <= max)
                .unwrap_or(default)
        };
        Self {
            enabled: value["enabled"].as_bool().unwrap_or(default.enabled),
            weekday: small("weekday", 6, default.weekday),
            hour: small("hour", 23, default.hour),
        }
    }
}

#[cfg(test)]
#[path = "insights_tests.rs"]
mod tests;
