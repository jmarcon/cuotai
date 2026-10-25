//! PRD 0024 RF7: the best account now inside each group of comparable
//! accounts. Pure functions over the cycle's current readings and an
//! injected clock, advisory only: nothing here writes configuration,
//! credentials or the cycle.
//!
//! A **group** is the accounts that share a `group` value (`Account::group`);
//! an account without one is compared only with the other ungrouped
//! accounts of its provider ([`group_key`]). A group needs at least two
//! candidates for a best account to exist.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};

use crate::alerts;
use crate::freshness::{self, Freshness};
use crate::metrics;
use crate::text::{params, Localized};
use crate::{Account, Config, Provider, Report, Status};

/// A limit that renews within this time is credited toward full (RF7.2).
pub const RENEWAL_CREDIT: TimeDelta = TimeDelta::minutes(30);

/// What the ranking reads of one configured account.
#[derive(Debug, Clone, Copy)]
pub struct AccountView<'a> {
    /// Position in the configuration.
    pub index: usize,
    pub provider: Provider,
    pub group: Option<&'a str>,
    /// Turned on in the front end (a disabled account is never compared).
    pub enabled: bool,
    /// The account's current reading; `None` while it has none.
    pub report: Option<&'a Report>,
    /// [`freshness::assess`] of that reading; `None` without one.
    pub freshness: Option<Freshness>,
}

/// The key of the group `account` belongs to: its `group`, or
/// `provider:<slug>` for an ungrouped account (a group slug never contains
/// a colon, so the two never collide).
pub fn group_key(provider: Provider, group: Option<&str>) -> String {
    match group {
        Some(group) => group.to_owned(),
        None => format!("provider:{}", provider.slug()),
    }
}

/// [`AccountView`]s of `config` at `now`: `reading(index)` is the account's
/// current report and `enabled(index)` whether it is turned on. The
/// freshness uses each account's effective refresh interval.
pub fn views<'a>(
    config: &'a Config,
    reading: impl Fn(usize) -> Option<&'a Report>,
    enabled: impl Fn(usize) -> bool,
    now: DateTime<Utc>,
) -> Vec<AccountView<'a>> {
    config
        .accounts
        .iter()
        .enumerate()
        .map(|(index, account)| {
            let report = reading(index);
            let interval =
                Duration::from_secs(account.effective_refresh_seconds(config.refresh_seconds));
            AccountView {
                index,
                provider: account.provider,
                group: config.group_of(account),
                enabled: enabled(index),
                report,
                freshness: report.map(|report| freshness::assess(report.checked_at, interval, now)),
            }
        })
        .collect()
}

/// One account that can be compared, best first within [`GroupBest::ranked`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub index: usize,
    pub freshness: Freshness,
    /// The score of RF7.2, `0.0..=1.0`: the least effective remaining share
    /// over the account's plan limits.
    pub score: f64,
    /// The least plain remaining share of those limits, `0.0..=1.0`
    /// ("64% left").
    pub remaining: f64,
}

/// The ranking of one group with at least two candidates.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupBest {
    /// [`group_key`] of the members.
    pub group: String,
    /// The `index` of the best candidate (`ranked[0]`).
    pub best: usize,
    /// Every candidate, best first.
    pub ranked: Vec<Candidate>,
}

impl GroupBest {
    /// The candidate for the account at `index`, if it is one.
    pub fn candidate(&self, index: usize) -> Option<&Candidate> {
        self.ranked
            .iter()
            .find(|candidate| candidate.index == index)
    }
}

/// `r + (1 − r) × max(0, 1 − t / 30 min)` for a limit with remaining share
/// `remaining` renewing in `until_reset` (`None`: no reset known, no credit;
/// a reset already past counts as renewed).
fn effective_remaining(remaining: f64, until_reset: Option<TimeDelta>) -> f64 {
    let credit = until_reset.map_or(0.0, |left| {
        let left = left.num_milliseconds().max(0) as f64;
        (1.0 - left / RENEWAL_CREDIT.num_milliseconds() as f64).max(0.0)
    });
    remaining + (1.0 - remaining) * credit
}

/// The candidate `view` makes, or `None` when it cannot be compared: turned
/// off, no reading, a failed or stale reading, or no account-wide plan limit
/// with a measured use.
fn candidate(view: &AccountView, now: DateTime<Utc>) -> Option<Candidate> {
    let freshness = view
        .freshness
        .filter(|freshness| *freshness != Freshness::Stale)?;
    let report = view
        .report
        .filter(|report| report.status == Status::Ready)?;
    if !view.enabled {
        return None;
    }
    let mut score: Option<f64> = None;
    let mut remaining: Option<f64> = None;
    for metric in report.metrics.iter().filter(|m| metrics::limits_account(m)) {
        let Some(consumption) = alerts::consumption(metric) else {
            continue;
        };
        let left = (1.0 - consumption.percent / 100.0).clamp(0.0, 1.0);
        let effective = effective_remaining(left, metric.resets_at.map(|at| at - now));
        score = Some(score.map_or(effective, |best| best.min(effective)));
        remaining = Some(remaining.map_or(left, |best| best.min(left)));
    }
    Some(Candidate {
        index: view.index,
        freshness,
        score: score?,
        remaining: remaining?,
    })
}

/// RF7.2: the best account of each group with two or more candidates, in the
/// order of each group's first member. `Fresh` readings rank before every
/// `Aging` one; within a tier the higher score wins and ties go to
/// configuration order.
pub fn best(accounts: &[AccountView], now: DateTime<Utc>) -> Vec<GroupBest> {
    let mut groups: BTreeMap<String, (usize, Vec<Candidate>)> = BTreeMap::new();
    for view in accounts {
        let entry = groups
            .entry(group_key(view.provider, view.group))
            .or_insert((view.index, Vec::new()));
        entry.0 = entry.0.min(view.index);
        if let Some(candidate) = candidate(view, now) {
            entry.1.push(candidate);
        }
    }
    let mut out: Vec<(usize, GroupBest)> = groups
        .into_iter()
        .filter(|(_, (_, ranked))| ranked.len() >= 2)
        .map(|(group, (first, mut ranked))| {
            ranked.sort_by(|a, b| {
                let tier = |c: &Candidate| u8::from(c.freshness != Freshness::Fresh);
                tier(a)
                    .cmp(&tier(b))
                    .then(b.score.total_cmp(&a.score))
                    .then(a.index.cmp(&b.index))
            });
            (
                first,
                GroupBest {
                    group,
                    best: ranked[0].index,
                    ranked,
                },
            )
        })
        .collect();
    out.sort_by_key(|(first, _)| *first);
    out.into_iter().map(|(_, best)| best).collect()
}

/// The group of `account` for display: its configured display name when the
/// configuration names it, else the group id; an ungrouped account has none.
pub fn group_label<'a>(config: &'a Config, account: &'a Account) -> Option<&'a str> {
    let id = config.group_of(account)?;
    Some(
        config
            .groups
            .iter()
            .find(|group| group.id == id)
            .and_then(|group| group.label.as_deref())
            .unwrap_or(id),
    )
}

/// The "Best now" chip (`advice.best`).
pub fn best_chip() -> Localized {
    Localized::fixed("advice.best", "Best now")
}

/// RF7.3: what the details of an account that runs out add about the best
/// account of its group: "Work runs out ≈18:53 · Personal has 64% left"
/// (`advice.switch`, with `runs_out_at` formatted by the caller) or, when
/// the account is already exhausted, "Work is exhausted · Personal has 64%
/// left" (`advice.switch.exhausted`). `left` is the best account's
/// [`Candidate::remaining`].
pub fn switch_text(from: &str, to: &str, left: f64, runs_out_at: Option<&str>) -> Localized {
    let percent = (left * 100.0).round().clamp(0.0, 100.0);
    let pct = format!("{percent:.0}");
    match runs_out_at {
        Some(at) => Localized::with_params(
            "advice.switch",
            params([
                ("from", from.into()),
                ("at", at.into()),
                ("to", to.into()),
                ("pct", pct.as_str().into()),
            ]),
            "{from} runs out ≈{at} · {to} has {pct}% left",
        ),
        None => Localized::with_params(
            "advice.switch.exhausted",
            params([
                ("from", from.into()),
                ("to", to.into()),
                ("pct", pct.as_str().into()),
            ]),
            "{from} is exhausted · {to} has {pct}% left",
        ),
    }
}

#[cfg(test)]
#[path = "advice_tests.rs"]
mod tests;
