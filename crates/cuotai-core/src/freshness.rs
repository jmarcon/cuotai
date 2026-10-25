//! Measurement age and freshness of a report.
//!
//! Pure functions over `checked_at`, the account's effective refresh interval
//! (`Account::effective_refresh_seconds`, already floored for providers billed
//! per query) and an injected `now`. No clock, no network: every front end
//! passes its own `now` and renders the answer its own way.

use chrono::{DateTime, Utc};
use std::time::Duration;

/// How old a measurement is relative to the account's refresh interval.
///
/// `Fresh` up to 1.5× the interval, `Aging` up to 3×, `Stale` above that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Fresh,
    Aging,
    Stale,
}

/// Age of a measurement taken at `checked_at`. A measurement "from the
/// future" (the system clock went backwards) has age zero; see
/// [`clock_adjusted`].
pub fn age(checked_at: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
    (now - checked_at).to_std().unwrap_or(Duration::ZERO)
}

/// `true` when `checked_at` lies after `now`, i.e. the system clock was
/// adjusted backwards since the measurement. Not an error: the age reads zero.
pub fn clock_adjusted(checked_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    checked_at > now
}

/// Classify a measurement taken at `checked_at` against the account's
/// effective refresh `interval`.
pub fn assess(checked_at: DateTime<Utc>, interval: Duration, now: DateTime<Utc>) -> Freshness {
    // Compare 2·age with 3·interval and 6·interval to stay in integers.
    let doubled = age(checked_at, now).saturating_mul(2);
    if doubled <= interval.saturating_mul(3) {
        Freshness::Fresh
    } else if doubled <= interval.saturating_mul(6) {
        Freshness::Aging
    } else {
        Freshness::Stale
    }
}

/// PRD 0033 RF3.2: gap after which a card names the data time.
pub const AS_OF_LAG: Duration = Duration::from_secs(15 * 60);

/// Age of the source's data at `now` when the provider states its time
/// (`Report::as_of`); `None` otherwise.
pub fn data_age(report: &crate::Report, now: DateTime<Utc>) -> Option<Duration> {
    report.as_of.map(|as_of| age(as_of, now))
}

/// `Report::as_of` when it lies more than [`AS_OF_LAG`] before
/// `checked_at`, i.e. when "Checked ... · data as of ..." is worth saying.
pub fn lagging_as_of(report: &crate::Report) -> Option<DateTime<Utc>> {
    let as_of = report.as_of?;
    (report.checked_at - as_of > chrono::TimeDelta::from_std(AS_OF_LAG).ok()?).then_some(as_of)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(second: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + second, 0).unwrap()
    }

    #[test]
    fn thresholds_follow_the_effective_interval() {
        use Freshness::*;
        // (interval seconds, age seconds, expected)
        let table = [
            (30, 0, Fresh),
            (30, 45, Fresh),
            (30, 46, Aging),
            (30, 90, Aging),
            (30, 91, Stale),
            (300, 450, Fresh),
            (300, 451, Aging),
            (300, 900, Aging),
            (300, 901, Stale),
            (3600, 5400, Fresh),
            (3600, 5401, Aging),
            (3600, 10800, Aging),
            (3600, 10801, Stale),
        ];
        let now = at(20_000);
        for (interval, age_s, expected) in table {
            let checked_at = at(20_000 - age_s);
            assert_eq!(
                assess(checked_at, Duration::from_secs(interval), now),
                expected,
                "interval {interval}s, age {age_s}s"
            );
            assert_eq!(age(checked_at, now), Duration::from_secs(age_s as u64));
            assert!(!clock_adjusted(checked_at, now));
        }
    }

    #[test]
    fn subsecond_age_past_the_threshold_is_aging() {
        let checked_at = at(0);
        let now = checked_at + chrono::Duration::milliseconds(45_001);
        assert_eq!(
            assess(checked_at, Duration::from_secs(30), now),
            Freshness::Aging
        );
    }

    #[test]
    fn clock_going_backwards_reads_as_zero_age_not_an_error() {
        let checked_at = at(600);
        let now = at(0);
        assert_eq!(age(checked_at, now), Duration::ZERO);
        assert!(clock_adjusted(checked_at, now));
        assert_eq!(
            assess(checked_at, Duration::from_secs(300), now),
            Freshness::Fresh
        );
    }

    #[test]
    fn data_age_and_lag_follow_as_of_only() {
        let mut report = crate::Report {
            account: "a".into(),
            provider: crate::Provider::OpenAiApi,
            checked_at: at(10_000),
            status: crate::Status::Ready,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            as_of: None,
        };
        assert_eq!(data_age(&report, at(10_000)), None);
        assert_eq!(lagging_as_of(&report), None);
        report.as_of = Some(at(10_000 - 600));
        assert_eq!(data_age(&report, at(10_060)), Some(Duration::from_secs(660)));
        assert_eq!(lagging_as_of(&report), None, "10 minutes is not lag");
        report.as_of = Some(at(10_000 - 7_200));
        assert_eq!(lagging_as_of(&report), Some(at(10_000 - 7_200)));
    }
}
