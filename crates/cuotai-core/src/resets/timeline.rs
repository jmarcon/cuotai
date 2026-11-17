//! PRD 0026 RF2: when each account's tightest limit comes back.
//!
//! [`timeline`] places one lane per account on a time axis from the
//! readings the front end already holds. It is pure: no clock, no network,
//! no history; the caller gives `now` and the time zone the clock texts
//! (`returns_clock`, `ends_clock`) are read in. The lane of an account is its binding metric (PRD 0009,
//! `metrics::binding`): when the account is blocked, the time it is usable
//! again; otherwise the reset of its tightest quota. Overage and
//! model-scoped metrics never form a lane because the binding metric never
//! is one.
//!
//! An account with a binding metric but no known return goes to `unknown`
//! ("Return unknown"), never plotted at an edge. An account with no binding
//! metric (no reading, an error, no capped quota) has nothing to come back
//! and is omitted. A return already in the past stays a lane at its time:
//! the reading predates the reset, which `stale` usually explains.

use crate::metrics::{binding, Binding, BindingReason};
use crate::{Provider, Report};
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use serde::Serialize;
use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineRange {
    Hours24,
    Days7,
}

impl TimelineRange {
    pub fn span(self) -> TimeDelta {
        match self {
            Self::Hours24 => TimeDelta::hours(24),
            Self::Days7 => TimeDelta::days(7),
        }
    }
}

/// What the timeline needs of one account, in configuration order.
#[derive(Debug, Clone, Copy)]
pub struct AccountView<'a> {
    pub name: &'a str,
    pub provider: Provider,
    /// The last reading, if any.
    pub report: Option<&'a Report>,
    /// The reading is older than the account's refresh interval allows
    /// (PRD 0010 `Freshness::Stale`).
    pub stale: bool,
}

/// Level of the remaining share (DS-ID-5): at least 70% left is good,
/// 30% to 70% warn, under 30% bad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareLevel {
    Good,
    Warn,
    Bad,
}

impl ShareLevel {
    pub fn of(remaining_share: f64) -> Self {
        if remaining_share >= 0.7 {
            Self::Good
        } else if remaining_share >= 0.3 {
            Self::Warn
        } else {
            Self::Bad
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lane {
    /// Position in the `accounts` slice (the configuration order).
    pub account_index: usize,
    pub name: String,
    pub provider: Provider,
    /// Label of the binding metric.
    pub label: String,
    /// When the tightest limit comes back; `None` only in `unknown`.
    pub returns_at: Option<DateTime<Utc>>,
    /// PRD 0024 RF6.1 (re-validation V-10): the time until `returns_at` as
    /// the core words it ("1h 35m", [`crate::text::duration`]), for the
    /// lane's accessible name, whose catalog text adds "in"; `None` when the
    /// return is unknown.
    pub returns_in: Option<crate::text::Localized>,
    /// PRD 0026 re-validation (V-01): `returns_at` as a clock time
    /// ([`crate::text::clock`]: the time today, the weekday within six days,
    /// the date beyond), so the desktop, the TUI and the CLI agree; `None`
    /// when the return is unknown.
    pub returns_clock: Option<crate::text::Localized>,
    /// Share left of the binding metric, `0.0..=1.0`.
    pub remaining_share: f64,
    pub level: ShareLevel,
    pub exhausted: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Timeline {
    pub range: TimelineRange,
    pub now: DateTime<Utc>,
    /// `now` plus the range.
    pub ends_at: DateTime<Utc>,
    /// PRD 0026 re-validation (V-01): `ends_at` as a clock time, the label of
    /// the axis end (never read as "now").
    pub ends_clock: crate::text::Localized,
    /// Returns within the range, earliest first, ties in configuration order.
    pub lanes: Vec<Lane>,
    /// Returns after the range, earliest first.
    pub later: Vec<Lane>,
    /// Return unknown, in configuration order.
    pub unknown: Vec<Lane>,
}

fn lane<Tz>(
    index: usize,
    view: &AccountView<'_>,
    binding: &Binding,
    returns_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Lane
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let remaining_share = (1.0 - binding.used_ratio).clamp(0.0, 1.0);
    Lane {
        account_index: index,
        name: view.name.to_string(),
        provider: view.provider,
        label: binding.label.clone(),
        returns_at,
        returns_in: returns_at.map(|at| crate::text::duration(at - now)),
        returns_clock: returns_at.map(|at| crate::text::clock(at, now, tz)),
        remaining_share,
        level: ShareLevel::of(remaining_share),
        exhausted: binding.reason == BindingReason::Exhausted,
        stale: view.stale,
    }
}

/// [`timeline`] over the held readings of `config`'s accounts (`reports[i]`
/// belongs to `config.accounts[i]`; a missing entry has no reading): each
/// reading is stale when it is older than its account's refresh interval
/// allows (PRD 0010). What the front ends call, so they agree; clock texts
/// read in `tz`.
pub fn timeline_of_config<Tz>(
    config: &crate::Config,
    reports: &[Option<Report>],
    range: TimelineRange,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Timeline
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    use crate::freshness::{assess, Freshness};
    let views: Vec<AccountView<'_>> = config
        .accounts
        .iter()
        .enumerate()
        .map(|(index, account)| {
            let report = reports.get(index).and_then(Option::as_ref);
            let interval = std::time::Duration::from_secs(
                account.effective_refresh_seconds(config.refresh_seconds),
            );
            AccountView {
                name: &account.name,
                provider: account.provider,
                report,
                stale: report.is_some_and(|report| {
                    assess(report.checked_at, interval, now) == Freshness::Stale
                }),
            }
        })
        .collect();
    timeline(&views, range, now, tz)
}

/// The accounts' returns within `range` of `now`, clock texts read in `tz`.
pub fn timeline<Tz>(
    accounts: &[AccountView<'_>],
    range: TimelineRange,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Timeline
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let ends_at = now + range.span();
    let mut lanes = Vec::new();
    let mut later = Vec::new();
    let mut unknown = Vec::new();
    for (index, view) in accounts.iter().enumerate() {
        let Some(binding) = view.report.and_then(binding) else {
            continue;
        };
        // Blocked accounts return when the exhausted limit does; others when
        // the tightest quota resets.
        let returns_at = binding.blocked_until.or(binding.resets_at);
        let lane = lane(index, view, &binding, returns_at, now, tz);
        match returns_at {
            None => unknown.push(lane),
            Some(at) if at <= ends_at => lanes.push(lane),
            Some(_) => later.push(lane),
        }
    }
    // Stable sorts keep the configuration order of equal returns.
    lanes.sort_by_key(|lane| lane.returns_at);
    later.sort_by_key(|lane| lane.returns_at);
    Timeline {
        range,
        now,
        ends_at,
        ends_clock: crate::text::clock(ends_at, now, tz),
        lanes,
        later,
        unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Metric, MetricKind, MetricWindow, Status};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
    }

    fn quota(used: f64, limit: f64, resets: Option<DateTime<Utc>>) -> Metric {
        Metric {
            label: "5h".into(),
            used: Some(used),
            limit: Some(limit),
            remaining: Some(limit - used),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: None,
            resets_at: resets,
            billing: Default::default(),
            scope: None,
            derived_from: None,
            unlimited: false,
        }
    }

    fn report(metrics: Vec<Metric>) -> Report {
        Report {
            overage_hint: Default::default(),
            as_of: Default::default(),
            sub_reports: Vec::new(),
            diagnostic: Default::default(),
            account_id: None,
            account: "x".into(),
            provider: Provider::Claude,
            checked_at: now(),
            status: Status::Ready,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            breakdown: Vec::new(),
        }
    }

    fn view<'a>(name: &'a str, report: Option<&'a Report>, stale: bool) -> AccountView<'a> {
        AccountView {
            name,
            provider: Provider::Claude,
            report,
            stale,
        }
    }

    fn names(lanes: &[Lane]) -> Vec<&str> {
        lanes.iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn blocked_and_resetting_accounts_sort_by_return_with_ties_in_config_order() {
        let soon = now() + TimeDelta::minutes(95);
        let blocked = report(vec![quota(100.0, 100.0, Some(soon))]);
        let resetting = report(vec![quota(40.0, 100.0, Some(now() + TimeDelta::hours(1)))]);
        let tied_a = report(vec![quota(10.0, 100.0, Some(soon))]);
        let tied_b = report(vec![quota(20.0, 100.0, Some(soon))]);
        let views = [
            view("blocked", Some(&blocked), false),
            view("resetting", Some(&resetting), false),
            view("tied a", Some(&tied_a), false),
            view("tied b", Some(&tied_b), false),
        ];
        let t = timeline(&views, TimelineRange::Hours24, now(), &Utc);
        assert_eq!(
            names(&t.lanes),
            ["resetting", "blocked", "tied a", "tied b"]
        );
        assert!(t.later.is_empty() && t.unknown.is_empty());
        let blocked = &t.lanes[1];
        assert!(blocked.exhausted);
        assert_eq!(
            (blocked.remaining_share, blocked.level),
            (0.0, ShareLevel::Bad)
        );
        assert_eq!(blocked.returns_at, Some(soon));
        let resetting = &t.lanes[0];
        assert!(!resetting.exhausted);
        assert!((resetting.remaining_share - 0.6).abs() < 1e-9);
        assert_eq!(resetting.level, ShareLevel::Warn);
        assert_eq!(t.lanes[2].level, ShareLevel::Good);
        assert_eq!(t.ends_at, now() + TimeDelta::hours(24));
    }

    #[test]
    fn a_return_after_the_range_goes_to_later_and_an_unknown_one_to_unknown() {
        let far = report(vec![quota(30.0, 100.0, Some(now() + TimeDelta::days(3)))]);
        let none = report(vec![quota(30.0, 100.0, None)]);
        let edge = report(vec![quota(30.0, 100.0, Some(now() + TimeDelta::hours(24)))]);
        let views = [
            view("far", Some(&far), false),
            view("none", Some(&none), false),
            view("edge", Some(&edge), false),
        ];
        let day = timeline(&views, TimelineRange::Hours24, now(), &Utc);
        assert_eq!(names(&day.lanes), ["edge"], "exactly at the end is inside");
        assert_eq!(names(&day.later), ["far"]);
        assert_eq!(names(&day.unknown), ["none"]);
        assert_eq!(day.unknown[0].returns_at, None);
        let week = timeline(&views, TimelineRange::Days7, now(), &Utc);
        assert_eq!(names(&week.lanes), ["edge", "far"]);
        assert!(week.later.is_empty());
    }

    #[test]
    fn blocked_without_a_known_return_is_unknown_and_accounts_without_a_limit_are_omitted() {
        let blocked_unknown = report(vec![quota(100.0, 100.0, None)]);
        let errored = Report {
            status: Status::Error,
            ..report(vec![quota(10.0, 100.0, Some(now() + TimeDelta::hours(1)))])
        };
        let no_cap = report(vec![Metric {
            limit: None,
            ..quota(10.0, 100.0, Some(now() + TimeDelta::hours(1)))
        }]);
        let overage = report(vec![Metric {
            kind: MetricKind::Balance,
            ..quota(10.0, 100.0, Some(now() + TimeDelta::hours(1)))
        }]);
        let views = [
            view("blocked", Some(&blocked_unknown), false),
            view("errored", Some(&errored), false),
            view("no cap", Some(&no_cap), false),
            view("cost only", Some(&overage), false),
            view("never read", None, false),
        ];
        let t = timeline(&views, TimelineRange::Days7, now(), &Utc);
        assert!(t.lanes.is_empty() && t.later.is_empty());
        assert_eq!(names(&t.unknown), ["blocked"]);
        assert!(t.unknown[0].exhausted);
    }

    #[test]
    fn stale_readings_keep_their_lane_marked_and_several_accounts_of_one_provider_coexist() {
        let a = report(vec![quota(50.0, 100.0, Some(now() + TimeDelta::hours(2)))]);
        let b = report(vec![quota(80.0, 100.0, Some(now() + TimeDelta::hours(1)))]);
        let past = report(vec![quota(
            95.0,
            100.0,
            Some(now() - TimeDelta::minutes(5)),
        )]);
        let views = [
            view("Claude Work", Some(&a), true),
            view("Claude Home", Some(&b), false),
            view("Claude Old", Some(&past), true),
        ];
        let t = timeline(&views, TimelineRange::Hours24, now(), &Utc);
        assert_eq!(
            names(&t.lanes),
            ["Claude Old", "Claude Home", "Claude Work"]
        );
        assert_eq!(
            t.lanes.iter().map(|l| l.stale).collect::<Vec<_>>(),
            [true, false, true]
        );
        assert_eq!(t.lanes[2].account_index, 0);
        assert_eq!(t.lanes[1].label, "5h");
        assert_eq!(t.lanes[1].provider, Provider::Claude);
    }

    /// Re-validation V-01: a return more than six days away reads with its
    /// date, and the axis end is never worded as "now".
    #[test]
    fn returns_beyond_six_days_carry_their_date_and_the_axis_end_its_day() {
        let soon = report(vec![quota(50.0, 100.0, Some(now() + TimeDelta::hours(3)))]);
        let week = report(vec![quota(50.0, 100.0, Some(now() + TimeDelta::days(8)))]);
        let views = [
            view("soon", Some(&soon), false),
            view("week", Some(&week), false),
        ];
        let day = timeline(&views, TimelineRange::Hours24, now(), &Utc);
        let clock = |lane: &Lane| lane.returns_clock.clone().unwrap();
        assert_eq!(clock(&day.lanes[0]).id, crate::text::clock::TIME);
        assert_eq!(clock(&day.lanes[0]).fallback, "3:00\u{a0}PM");
        let later = clock(&day.later[0]);
        assert_eq!(later.id, crate::text::clock::DATE);
        assert_eq!(later.fallback, "10/28 12:00\u{a0}PM");
        assert_eq!(later.resolve("de"), "28.10., 12:00");
        // Now is a Tuesday noon: the 24 h end is tomorrow, the 7 d end next
        // Tuesday, each worded with its weekday.
        assert_eq!(day.ends_clock.fallback, "Wednesday 12:00\u{a0}PM");
        let seven = timeline(&views, TimelineRange::Days7, now(), &Utc);
        assert_eq!(seven.ends_clock.id, crate::text::clock::DATE);
        assert_eq!(seven.ends_clock.fallback, "10/27 12:00\u{a0}PM");
        let unknown = report(vec![quota(30.0, 100.0, None)]);
        let t = timeline(
            &[view("u", Some(&unknown), false)],
            TimelineRange::Days7,
            now(),
            &Utc,
        );
        assert_eq!(t.unknown[0].returns_clock, None);
    }

    #[test]
    fn the_binding_metric_of_an_account_decides_its_lane() {
        // Two quotas: the weekly one is tighter, so the lane returns with it.
        let five_hours = quota(10.0, 100.0, Some(now() + TimeDelta::hours(2)));
        let weekly = Metric {
            label: "weekly".into(),
            window: Some(MetricWindow::Weekly),
            ..quota(90.0, 100.0, Some(now() + TimeDelta::days(2)))
        };
        let r = report(vec![five_hours, weekly]);
        let views = [view("two", Some(&r), false)];
        let t = timeline(&views, TimelineRange::Days7, now(), &Utc);
        assert_eq!(t.lanes[0].label, "weekly");
        assert_eq!(t.lanes[0].returns_at, Some(now() + TimeDelta::days(2)));
        // V-10: the lane carries the core's countdown for its accessible name.
        assert_eq!(
            t.lanes[0].returns_in.as_ref().map(|c| c.resolve("en-US")),
            Some("2d".to_owned())
        );
        assert_eq!(
            timeline(&views, TimelineRange::Hours24, now(), &Utc)
                .later
                .len(),
            1
        );
    }
}
