//! Plausible instants and panic-free local conversions.
//!
//! A provider can send any epoch chrono can hold (up to year 262142). Such
//! a value has no meaning as a reset, and converting it to a local clock
//! near chrono's limits panics (`naive_local()` overflows). The refresh
//! drops implausible instants at the provider boundary with
//! [`plausible_instant`], and every front end formats a local clock
//! through [`checked_local`] or [`format_local`], which fall back to UTC.

use chrono::{DateTime, FixedOffset, NaiveDate, Offset, TimeZone, Utc};
use std::fmt::Display;

/// The first instant accepted from a provider: 1970-01-01T00:00:00Z.
pub fn earliest() -> DateTime<Utc> {
    DateTime::UNIX_EPOCH
}

/// The last instant accepted from a provider: 9999-12-31T23:59:59.999Z.
/// Every local offset (at most ±24 h) keeps it inside chrono's range.
pub fn latest() -> DateTime<Utc> {
    NaiveDate::from_ymd_opt(9999, 12, 31)
        .and_then(|day| day.and_hms_milli_opt(23, 59, 59, 999))
        .map(|naive| naive.and_utc())
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// `at` when it falls between 1970 and 9999 (UTC), else `None`, which the
/// product reads as an unknown time, never as "now".
pub fn plausible_instant(at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    (earliest()..=latest()).contains(&at).then_some(at)
}

/// [`plausible_instant`] over an optional instant.
pub fn plausible(at: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    at.and_then(plausible_instant)
}

/// `at` in `tz`, or `None` when its local clock falls outside chrono's
/// range (formatting it would panic).
pub fn checked_local<Tz: TimeZone>(at: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Tz>> {
    let offset: FixedOffset = tz.offset_from_utc_datetime(&at.naive_utc()).fix();
    at.naive_utc()
        .checked_add_signed(chrono::TimeDelta::seconds(i64::from(
            offset.local_minus_utc(),
        )))
        .map(|_| at.with_timezone(tz))
}

/// `at` formatted with `fmt` on the clock of `tz`; when that clock is out
/// of range, the same format in UTC followed by " UTC".
pub fn format_local<Tz>(at: DateTime<Utc>, tz: &Tz, fmt: &str) -> String
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    match checked_local(at, tz) {
        Some(local) => local.format(fmt).to_string(),
        None => format!("{} UTC", at.format(fmt)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn year(y: i32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(y, 6, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn only_years_1970_to_9999_are_plausible() {
        assert_eq!(plausible_instant(DateTime::<Utc>::MAX_UTC), None);
        assert_eq!(plausible_instant(DateTime::<Utc>::MIN_UTC), None);
        assert_eq!(plausible_instant(year(10_000)), None);
        assert_eq!(plausible_instant(year(1969)), None);
        assert_eq!(plausible_instant(earliest()), Some(earliest()));
        assert_eq!(plausible_instant(latest()), Some(latest()));
        assert_eq!(plausible_instant(year(9999)), Some(year(9999)));
        assert_eq!(plausible(None), None);
        // Every local offset fits the last accepted instant.
        let east = FixedOffset::east_opt(14 * 3600).unwrap();
        let west = FixedOffset::west_opt(12 * 3600).unwrap();
        assert!(checked_local(latest(), &east).is_some());
        assert!(checked_local(earliest(), &west).is_some());
    }

    #[test]
    fn local_conversions_at_the_limits_fall_back_without_panicking() {
        let east = FixedOffset::east_opt(14 * 3600).unwrap();
        let west = FixedOffset::west_opt(11 * 3600).unwrap();
        assert!(checked_local(DateTime::<Utc>::MAX_UTC, &east).is_none());
        assert!(checked_local(DateTime::<Utc>::MIN_UTC, &west).is_none());
        let text = format_local(DateTime::<Utc>::MAX_UTC, &east, "%Y-%m-%d %H:%M");
        assert!(text.ends_with(" UTC"), "{text}");
        assert_eq!(
            format_local(year(2026), &east, "%Y-%m-%d %H:%M"),
            "2026-06-01 14:00"
        );
    }
}
