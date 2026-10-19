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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}
