//! PRD 0026 RF1: usage statistics over the stored measurement history.
//!
//! [`summarize`] reads the history store (never a provider) and answers how
//! often the plan limits were hit, how close the tightest limit got each day
//! and how much history backs the answer. Honesty rules: a day without
//! samples is a gap, never a zero; a limit that is unknown is excluded and
//! counted; fewer than two days of data is reported as not enough.
//!
//! Only plan limits count: `MetricKind::Quota` metrics with a finite
//! positive limit (PRD 0023's `is_plan_limit` replaces the kind test when it
//! lands). The tightest limit of an account on a day is the plan limit with
//! the highest peak share that day, so shares never exceed 1.0 here even
//! when a provider reports use above the cap.
//!
//! Compaction safety: the hourly reduction of old samples keeps each
//! window's peak, so the figures are the same before and after it.
//!
//! "Extra spent" (RF1.5) waits for PRD 0023's overage amounts and is not
//! part of the summary yet.

use crate::history::{HistoryError, HistoryStore};
use crate::text::{params, Localized};
use crate::{Account, MetricKind};
use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, TimeZone, Utc};
use serde::Serialize;

/// Longest window a hit can span; samples this far before the range still
/// belong to a window that ends inside it.
const LOOKBACK: TimeDelta = TimeDelta::days(31);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatsRange {
    Days7,
    Days30,
}

impl StatsRange {
    pub fn days(self) -> u32 {
        match self {
            Self::Days7 => 7,
            Self::Days30 => 30,
        }
    }
}

/// The highest share of the tightest plan limit used on one local day.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DayPeak {
    pub day: NaiveDate,
    /// `0.0..=1.0`; `None` when no sample of a known limit fell on the day.
    pub share: Option<f64>,
    pub is_today: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AccountStats {
    /// Position in the `accounts` slice passed to [`summarize`].
    pub account_index: usize,
    pub name: String,
    /// Window instances of its plan limits that ended in the range at the
    /// limit; two metrics hitting at once count twice.
    pub hits: u32,
    /// Mean of the daily peaks over the days with data.
    pub average_peak: Option<f64>,
    pub days_with_data: u32,
    /// One entry per day of the range, oldest first.
    pub days: Vec<DayPeak>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Coverage {
    Full,
    Partial {
        days_with_data: u32,
    },
    /// Fewer than two days have data: show "Not enough history".
    NotEnough,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsSummary {
    pub range: StatsRange,
    /// Limit hits of every account.
    pub hits: u32,
    /// Mean over the accounts that have data of their average peak.
    pub average_peak: Option<f64>,
    pub accounts: Vec<AccountStats>,
    /// Highest peak across accounts per day, oldest first.
    pub daily: Vec<DayPeak>,
    /// Plan-limit metrics left out because their limit is unknown.
    pub unknown_limits: u32,
    pub retention_days: u16,
    pub first_sample_at: Option<DateTime<Utc>>,
    pub coverage: Coverage,
}

impl StatsSummary {
    /// "History kept {days} days on this device".
    pub fn history_kept(&self) -> Localized {
        Localized::with_params(
            "stats.history_kept",
            params([("days", self.retention_days.into())]),
            format!(
                "Histórico mantido por {} dias neste dispositivo",
                self.retention_days
            ),
        )
    }

    /// Shown instead of figures when [`Coverage::NotEnough`].
    pub fn not_enough() -> Localized {
        Localized::fixed("stats.not_enough", "Histórico insuficiente")
    }
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.fold((0.0, 0u32), |(sum, count), v| (sum + v, count + 1));
    (count > 0).then(|| sum / f64::from(count))
}

/// Start of the local day `date` as a UTC instant.
fn day_start<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> DateTime<Utc> {
    let local = date.and_time(NaiveTime::MIN);
    tz.from_local_datetime(&local)
        .earliest()
        // Midnight skipped by a DST jump: the day starts at the first
        // instant that exists after it.
        .or_else(|| {
            tz.from_local_datetime(&(local + TimeDelta::hours(1)))
                .earliest()
        })
        .map_or_else(|| local.and_utc(), |at| at.with_timezone(&Utc))
}

/// Statistics of `accounts` over the last `range` local days ending today
/// (`now` in `tz`). Reads the history only; a missing store is an empty
/// history.
pub fn summarize<Tz: TimeZone>(
    store: &HistoryStore,
    accounts: &[Account],
    range: StatsRange,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Result<StatsSummary, HistoryError> {
    let today = now.with_timezone(tz).date_naive();
    let dates: Vec<NaiveDate> = (0..i64::from(range.days()))
        .rev()
        .map(|back| today - TimeDelta::days(back))
        .collect();
    let range_start = day_start(tz, dates[0]);
    let day_index = |at: DateTime<Utc>| {
        let date = at.with_timezone(tz).date_naive();
        dates.iter().position(|d| *d == date)
    };

    let mut unknown_limits = 0;
    let mut stats = Vec::with_capacity(accounts.len());
    for (account_index, account) in accounts.iter().enumerate() {
        let mut best: Vec<Option<f64>> = vec![None; dates.len()];
        let mut hits = 0;
        let histories = store.account_history(account, range_start - LOOKBACK..=now)?;
        for metric in histories.iter().filter(|m| m.kind == MetricKind::Quota) {
            let Some(limit) = metric.limit.filter(|l| l.is_finite() && *l > 0.0) else {
                unknown_limits += 1;
                continue;
            };
            for window in &metric.windows {
                let cap = window
                    .limit
                    .filter(|l| l.is_finite() && *l > 0.0)
                    .unwrap_or(limit);
                if window.ended_at >= range_start
                    && window.ended_at <= now
                    && window.peak_used >= cap
                {
                    hits += 1;
                }
            }
            for point in metric.points.iter().filter(|p| p.taken_at >= range_start) {
                if let Some(index) = day_index(point.taken_at) {
                    let share = (point.used / limit).clamp(0.0, 1.0);
                    best[index] = Some(best[index].map_or(share, |known: f64| known.max(share)));
                }
            }
        }
        let days: Vec<DayPeak> = dates
            .iter()
            .zip(&best)
            .map(|(day, share)| DayPeak {
                day: *day,
                share: *share,
                is_today: *day == today,
            })
            .collect();
        stats.push(AccountStats {
            account_index,
            name: account.name.clone(),
            hits,
            average_peak: mean(best.iter().flatten().copied()),
            days_with_data: best.iter().flatten().count() as u32,
            days,
        });
    }

    let daily: Vec<DayPeak> = dates
        .iter()
        .enumerate()
        .map(|(index, day)| DayPeak {
            day: *day,
            share: stats
                .iter()
                .filter_map(|account| account.days[index].share)
                .reduce(f64::max),
            is_today: *day == today,
        })
        .collect();
    let days_with_data = daily.iter().filter(|d| d.share.is_some()).count() as u32;
    let coverage = if days_with_data < 2 {
        Coverage::NotEnough
    } else if days_with_data as usize == daily.len() {
        Coverage::Full
    } else {
        Coverage::Partial { days_with_data }
    };
    Ok(StatsSummary {
        range,
        hits: stats.iter().map(|a| a.hits).sum(),
        average_peak: mean(stats.iter().filter_map(|a| a.average_peak)),
        accounts: stats,
        daily,
        unknown_limits,
        retention_days: store.retention().keep_days(),
        first_sample_at: store.recorded_bounds()?.map(|bounds| bounds.first),
        coverage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::ConfigIdentity;
    use crate::history::Retention;
    use crate::{Config, Metric, MetricWindow};
    use chrono::{Datelike, FixedOffset};
    use std::path::Path;

    const CONFIG: &str = r#"
[[accounts]]
name = "alpha"
provider = "groq"
credential_env = "GROQ_KEY_ALPHA"
[[accounts]]
name = "beta"
provider = "groq"
credential_env = "GROQ_KEY_BETA"
"#;

    fn accounts() -> Vec<Account> {
        Config::from_toml(CONFIG).unwrap().accounts
    }

    fn store(dir: &Path) -> HistoryStore {
        HistoryStore::new(
            dir.join("history.sqlite3"),
            &ConfigIdentity::Explicit("stats".into()),
        )
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    fn metric(label: &str, used: f64, limit: Option<f64>, resets: DateTime<Utc>) -> Metric {
        Metric {
            label: label.into(),
            used: Some(used),
            limit,
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Daily),
            period: None,
            resets_at: Some(resets),
            billing: Default::default(),
            scope: None,
        }
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    /// One reading of a "daily" metric of 100: its window resets at the end
    /// of the day it is taken.
    fn record(store: &HistoryStore, account: &Account, when: DateTime<Utc>, used: f64) {
        let resets = at(when.day(), 0) + TimeDelta::days(1);
        store
            .record(account, when, &[metric("daily", used, Some(100.0), resets)])
            .unwrap();
    }

    fn summary(store: &HistoryStore, range: StatsRange, now: DateTime<Utc>) -> StatsSummary {
        summarize(store, &accounts(), range, now, &utc()).unwrap()
    }

    #[test]
    fn no_history_is_not_enough_and_creates_no_store() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let s = summary(&history, StatsRange::Days7, at(20, 12));
        assert_eq!(s.coverage, Coverage::NotEnough);
        assert_eq!((s.hits, s.average_peak, s.first_sample_at), (0, None, None));
        assert_eq!(s.daily.len(), 7);
        assert!(s.daily.iter().all(|d| d.share.is_none()));
        assert_eq!(s.daily.iter().filter(|d| d.is_today).count(), 1);
        assert!(s.daily[6].is_today);
        assert_eq!(s.retention_days, Retention::DEFAULT_DAYS);
        assert!(!dir.path().join("history.sqlite3").exists());
    }

    #[test]
    fn one_day_of_data_is_not_enough_but_keeps_its_figure() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let alpha = &accounts()[0];
        record(&history, alpha, at(20, 9), 40.0);
        record(&history, alpha, at(20, 11), 60.0);
        let s = summary(&history, StatsRange::Days7, at(20, 12));
        assert_eq!(s.coverage, Coverage::NotEnough);
        assert_eq!(s.daily[6].share, Some(0.6));
        assert_eq!(s.accounts[0].days_with_data, 1);
        assert_eq!(s.first_sample_at, Some(at(20, 9)));
        assert_eq!(s.accounts[1].average_peak, None);
        assert_eq!(
            s.average_peak,
            Some(0.6),
            "accounts without data are not zeros"
        );
    }

    #[test]
    fn a_hit_exactly_at_the_limit_counts_once_and_gaps_are_not_zeros() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let alpha = &accounts()[0];
        record(&history, alpha, at(16, 10), 50.0);
        record(&history, alpha, at(16, 12), 100.0); // exactly the limit
        record(&history, alpha, at(18, 10), 20.0); // 17th is a gap
        record(&history, alpha, at(20, 10), 80.0);
        let s = summary(&history, StatsRange::Days7, at(20, 12));
        assert_eq!(s.hits, 1);
        assert_eq!(s.accounts[0].hits, 1);
        let shares: Vec<_> = s.daily.iter().map(|d| d.share).collect();
        assert_eq!(
            shares,
            [None, None, Some(1.0), None, Some(0.2), None, Some(0.8)]
        );
        assert_eq!(s.accounts[0].days_with_data, 3);
        let average = s.accounts[0].average_peak.unwrap();
        assert!((average - (1.0 + 0.2 + 0.8) / 3.0).abs() < 1e-9);
        assert_eq!(s.coverage, Coverage::Partial { days_with_data: 3 });
    }

    #[test]
    fn a_window_straddling_the_range_start_counts_once_by_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let alpha = &accounts()[0];
        // One window resetting on the 15th at midnight: its peak (the limit)
        // was reached on the 13th, before the 7 day range starting 14th.
        let resets = at(15, 0);
        let hit = |when, used| {
            history
                .record(alpha, when, &[metric("weekly", used, Some(100.0), resets)])
                .unwrap();
        };
        hit(at(13, 12), 100.0);
        hit(at(14, 12), 100.0);
        hit(at(14, 18), 100.0);
        // Next window, below the limit.
        let next = at(22, 0);
        history
            .record(
                alpha,
                at(16, 12),
                &[metric("weekly", 30.0, Some(100.0), next)],
            )
            .unwrap();
        let s = summary(&history, StatsRange::Days7, at(20, 12));
        assert_eq!(s.hits, 1, "the window ended on the 14th, inside the range");
        // A window that ended before the range is not counted.
        let later = summary(&history, StatsRange::Days7, at(24, 12));
        assert_eq!(later.hits, 0);
    }

    #[test]
    fn unknown_limits_are_excluded_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let alpha = &accounts()[0];
        let resets = at(21, 0);
        history
            .record(
                alpha,
                at(19, 10),
                &[
                    metric("known", 50.0, Some(100.0), resets),
                    metric("unknown", 500.0, None, resets),
                ],
            )
            .unwrap();
        history
            .record(
                alpha,
                at(20, 10),
                &[
                    metric("known", 70.0, Some(100.0), resets),
                    metric("unknown", 900.0, None, resets),
                ],
            )
            .unwrap();
        let s = summary(&history, StatsRange::Days7, at(20, 12));
        assert_eq!(s.unknown_limits, 1);
        assert_eq!(s.daily[6].share, Some(0.7));
        assert_eq!(s.coverage, Coverage::Partial { days_with_data: 2 });
    }

    #[test]
    fn several_accounts_report_the_highest_peak_and_the_mean_of_averages() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let accounts = accounts();
        record(&history, &accounts[0], at(19, 10), 40.0);
        record(&history, &accounts[0], at(20, 10), 60.0);
        record(&history, &accounts[1], at(19, 10), 100.0);
        record(&history, &accounts[1], at(20, 10), 20.0);
        let s = summary(&history, StatsRange::Days7, at(20, 12));
        assert_eq!(s.hits, 1);
        assert_eq!((s.accounts[0].hits, s.accounts[1].hits), (0, 1));
        assert_eq!(s.daily[5].share, Some(1.0));
        assert_eq!(s.daily[6].share, Some(0.6));
        let (a, b) = (0.5, 0.6);
        assert!((s.average_peak.unwrap() - (a + b) / 2.0).abs() < 1e-9);
        assert_eq!(s.accounts[1].name, "beta");
    }

    #[test]
    fn use_above_the_limit_is_clamped_to_the_full_share() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let alpha = &accounts()[0];
        record(&history, alpha, at(19, 10), 150.0);
        record(&history, alpha, at(20, 10), 10.0);
        let s = summary(&history, StatsRange::Days7, at(20, 12));
        assert_eq!(s.daily[5].share, Some(1.0));
        assert_eq!(s.hits, 1);
    }

    /// A zone at UTC-3 that moves to UTC-2 at `jump` (a DST start).
    #[derive(Clone, Copy)]
    struct Dst {
        jump: DateTime<Utc>,
    }

    impl Dst {
        fn offset_at(&self, utc: DateTime<Utc>) -> FixedOffset {
            let hours = if utc >= self.jump { -2 } else { -3 };
            FixedOffset::east_opt(hours * 3600).unwrap()
        }
    }

    impl TimeZone for Dst {
        type Offset = FixedOffset;
        fn from_offset(offset: &FixedOffset) -> Self {
            // Only used for offsets of this zone; the jump is irrelevant.
            Dst {
                jump: if offset.local_minus_utc() == -2 * 3600 {
                    DateTime::<Utc>::MIN_UTC
                } else {
                    DateTime::<Utc>::MAX_UTC
                },
            }
        }
        fn offset_from_local_date(&self, local: &NaiveDate) -> chrono::LocalResult<FixedOffset> {
            self.offset_from_local_datetime(&local.and_time(NaiveTime::MIN))
        }
        fn offset_from_local_datetime(
            &self,
            local: &chrono::NaiveDateTime,
        ) -> chrono::LocalResult<FixedOffset> {
            let before = FixedOffset::east_opt(-3 * 3600).unwrap();
            let after = FixedOffset::east_opt(-2 * 3600).unwrap();
            let as_before =
                local.and_utc() - TimeDelta::seconds(i64::from(before.local_minus_utc()));
            let as_after = local.and_utc() - TimeDelta::seconds(i64::from(after.local_minus_utc()));
            match (as_before < self.jump, as_after >= self.jump) {
                (true, true) => chrono::LocalResult::Ambiguous(before, after),
                (true, false) => chrono::LocalResult::Single(before),
                (false, true) => chrono::LocalResult::Single(after),
                (false, false) => chrono::LocalResult::None,
            }
        }
        fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
            self.offset_at(utc.and_time(NaiveTime::MIN).and_utc())
        }
        fn offset_from_utc_datetime(&self, utc: &chrono::NaiveDateTime) -> FixedOffset {
            self.offset_at(utc.and_utc())
        }
    }

    #[test]
    fn days_follow_the_callers_time_zone_across_a_dst_change() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let alpha = &accounts()[0];
        // Local midnight of the 18th is 03:00 UTC before the jump; the zone
        // moves forward one hour on the 18th at 03:00 UTC (00:00 -> 01:00
        // local), so the 18th has only 23 hours.
        let zone = Dst { jump: at(18, 3) };
        record(&history, alpha, at(18, 2), 30.0); // 17th 23:00 local
        record(&history, alpha, at(18, 4), 60.0); // 18th 02:00 local
        record(&history, alpha, at(19, 1), 90.0); // 18th 23:00 local (UTC-2)
        record(&history, alpha, at(19, 3), 10.0); // 19th 01:00 local
        let s = summarize(&history, &accounts(), StatsRange::Days7, at(19, 12), &zone).unwrap();
        let day = |d: u32| {
            s.daily
                .iter()
                .find(|x| x.day == NaiveDate::from_ymd_opt(2026, 10, d).unwrap())
                .unwrap()
                .share
        };
        assert_eq!(day(17), Some(0.3));
        assert_eq!(day(18), Some(0.9));
        assert_eq!(day(19), Some(0.1));
        assert_eq!(s.daily.len(), 7);
        assert!(s.daily[6].is_today);
    }

    #[test]
    fn hourly_reduction_keeps_every_figure() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path());
        let alpha = &accounts()[0];
        // Several readings per hour more than a week old, with a hit.
        for (hour, minute, used) in [
            (9u32, 0i64, 20.0),
            (9, 20, 70.0),
            (9, 40, 100.0),
            (9, 50, 40.0),
            (14, 0, 30.0),
            (14, 30, 55.0),
        ] {
            record(
                &history,
                alpha,
                at(3, hour) + TimeDelta::minutes(minute),
                used,
            );
        }
        record(&history, alpha, at(4, 10), 10.0);
        let now = at(20, 12);
        let before = summarize(&history, &accounts(), StatsRange::Days30, now, &utc()).unwrap();
        let report = history.prune(now).unwrap();
        assert!(report.reduced > 0, "the fixture exercises the reduction");
        let after = summarize(&history, &accounts(), StatsRange::Days30, now, &utc()).unwrap();
        assert_eq!(
            StatsSummary {
                first_sample_at: None,
                ..before
            },
            StatsSummary {
                first_sample_at: None,
                ..after.clone()
            }
        );
        assert_eq!(after.hits, 1);
        assert_eq!(after.coverage, Coverage::Partial { days_with_data: 2 });
        let third = after
            .daily
            .iter()
            .find(|d| d.day == NaiveDate::from_ymd_opt(2026, 10, 3).unwrap())
            .unwrap();
        assert_eq!(third.share, Some(1.0));
    }

    #[test]
    fn texts_name_the_retention_and_resolve_in_every_language() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(dir.path()).with_retention(Retention::days(30).unwrap());
        let s = summary(&history, StatsRange::Days30, at(20, 12));
        assert_eq!(s.retention_days, 30);
        assert_eq!(
            s.history_kept().resolve("en-US"),
            "History kept 30 days on this device"
        );
        assert!(s.history_kept().resolve("pt-BR").contains("30 dias"));
        for locale in ["es", "it"] {
            assert!(!s.history_kept().resolve(locale).contains('{'), "{locale}");
            assert!(!StatsSummary::not_enough()
                .resolve(locale)
                .contains("stats."));
        }
        assert_eq!(
            StatsSummary::not_enough().resolve("en-US"),
            "Not enough history"
        );
    }
}
