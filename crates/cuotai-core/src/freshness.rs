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

/// The instant `report`'s values were measured: the one time every pace,
/// allowance, forecast and history sample of the reading is taken at (PRD
/// 0024 FIX-11: an old `used` is never paired with a later clock).
///
/// * An agent-fed reading (Claude Code status line, Codex CLI logs; PRD
///   0040, `Report::source` starting with `feed:`) was measured when the
///   agent wrote it, its `as_of`, not when Cuotai read the file.
/// * Every other adapter was measured at its query, `checked_at`. Its
///   `as_of` (PRD 0033), when stated, is a completeness horizon (OpenAI
///   Costs: the end of the last complete daily bucket) while `used` still
///   counts the partial current day, so it is never the measurement time.
///
/// Never later than `checked_at`: a feed time ahead of it reads as
/// `checked_at`.
pub fn reading_time(report: &crate::Report) -> DateTime<Utc> {
    match report.as_of {
        Some(as_of) if report.source.starts_with(crate::feed::REPORT_SOURCE_PREFIX) => {
            as_of.min(report.checked_at)
        }
        _ => report.checked_at,
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

/// PRD 0034 RF2.2: a measurement age as the front ends word it, in whole
/// units with abbreviations ("42 s ago", "3 min ago", "2 h ago", "1 d ago");
/// zero (including a reading "from the future") reads "now". Ids
/// `freshness.age.{now,seconds,minutes,hours,days}` with the `n` param.
pub fn age_label(age: Duration) -> crate::text::Localized {
    use crate::text::Localized;
    let secs = age.as_secs();
    let (id, n, fallback) = match secs {
        0 => return Localized::fixed("freshness.age.now", "now"),
        1..=59 => ("freshness.age.seconds", secs, "{n} s ago"),
        60..=3_599 => ("freshness.age.minutes", secs / 60, "{n} min ago"),
        3_600..=86_399 => ("freshness.age.hours", secs / 3_600, "{n} h ago"),
        _ => ("freshness.age.days", secs / 86_400, "{n} d ago"),
    };
    Localized::with_param(id, "n", n.to_string().as_str(), fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn age_labels_round_down_to_whole_units() {
        let label = |secs| age_label(Duration::from_secs(secs));
        assert_eq!(label(0).text(), "now");
        assert_eq!(label(59).text(), "59 s ago");
        assert_eq!(label(60).text(), "1 min ago");
        assert_eq!(label(3_599).text(), "59 min ago");
        assert_eq!(label(3_600).text(), "1 h ago");
        assert_eq!(label(86_399).text(), "23 h ago");
        assert_eq!(label(86_400).text(), "1 d ago");
        assert_eq!(label(180).resolve("pt-BR"), "há 3 min");
        assert_eq!(label(180).resolve("es"), "hace 3 min");
        assert_eq!(label(180).resolve("it"), "3 min fa");
        assert_eq!(label(0).resolve("pt-BR"), "agora");
    }

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
            overage_hint: Default::default(),
            account_id: None,
            diagnostic: None,
            sub_reports: Vec::new(),
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
            breakdown: Vec::new(),
        };
        assert_eq!(data_age(&report, at(10_000)), None);
        assert_eq!(lagging_as_of(&report), None);
        report.as_of = Some(at(10_000 - 600));
        assert_eq!(
            data_age(&report, at(10_060)),
            Some(Duration::from_secs(660))
        );
        assert_eq!(lagging_as_of(&report), None, "10 minutes is not lag");
        report.as_of = Some(at(10_000 - 7_200));
        assert_eq!(lagging_as_of(&report), Some(at(10_000 - 7_200)));
    }

    #[test]
    fn reading_time_is_the_feed_time_only_for_agent_feeds() {
        let mut report = crate::Report {
            overage_hint: Default::default(),
            account_id: None,
            diagnostic: None,
            sub_reports: Vec::new(),
            account: "a".into(),
            provider: crate::Provider::OpenAiApi,
            checked_at: at(10_000),
            status: crate::Status::Ready,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: "api".into(),
            session_expires_at: None,
            as_of: None,
            breakdown: Vec::new(),
        };
        assert_eq!(reading_time(&report), at(10_000), "no as_of");
        // OpenAI Costs: `as_of` is the end of the last complete bucket, not
        // the measurement time, so the query time stays.
        report.as_of = Some(at(10_000 - 86_400));
        assert_eq!(reading_time(&report), at(10_000));
        for source in ["feed:claude_code", "feed:codex_cli"] {
            report.source = source.into();
            report.as_of = Some(at(10_000 - 2_400));
            assert_eq!(reading_time(&report), at(10_000 - 2_400), "{source}");
            report.as_of = Some(at(10_600));
            assert_eq!(reading_time(&report), at(10_000), "{source}: clamped");
            report.as_of = None;
            assert_eq!(reading_time(&report), at(10_000), "{source}: no as_of");
        }
    }
}
