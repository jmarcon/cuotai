//! RF5.1: the one line `cuotai --statusline-claude` prints in Claude
//! Code's status line: Claude's own windows from the feed first ("5h 23% ·
//! Wk 41% ↺ 19:30"), then the tightest other account ("Codex 64% left"),
//! then the session's context share ("ctx 8%"). The words come from the
//! core catalogs so the terminal and the desktop say the same; the caller
//! only supplies the locale, the clock format and the other accounts.

use chrono::{DateTime, Utc};

use super::{FeedReading, WindowId};
use crate::text::{Localized, ParamValue};
use crate::tray::{self, AccountView, Level};

/// One other account for the line (a running server's view, or the last
/// recorded reading of the local history, RF5.2). A stale one carries its
/// age from `view.measured_at` ([`tray::stale_suffix`], the menu bar's
/// rule).
#[derive(Debug, Clone)]
pub struct OtherAccount {
    pub view: AccountView,
}

/// The composed line and the level that colours it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    /// The level of Claude's tightest window, `None` when none is known.
    pub level: Option<Level>,
    /// The byte length of Claude's own part at the start of `text` (0 when
    /// the line shows none): the only part a colour applies to (PRD 0040
    /// L-8, owner decision 2026-11-20).
    pub claude_len: usize,
}

impl Line {
    /// The line with Claude's part (only) wrapped in `start`…`end`, such
    /// as an ANSI colour and its reset; the other parts stay plain.
    pub fn painted(&self, start: &str, end: &str) -> String {
        if self.claude_len == 0 || self.level.is_none() {
            return self.text.clone();
        }
        let (claude, rest) = self.text.split_at(self.claude_len);
        format!("{start}{claude}{end}{rest}")
    }
}

/// "23%", the sign placed as the reader's language places it (V-17).
fn pct(value: f64) -> ParamValue {
    ParamValue::l10n(crate::text::number::percent(value.round()))
}

/// PRD 0040 re-validation V-04/V-05: the interval Claude's own part is
/// aged against: the effective interval of the first account the Claude
/// Code feed answers in `config`, else the configuration's (or the
/// default) global interval.
pub fn claude_interval(config: Option<&crate::Config>) -> std::time::Duration {
    let seconds = match config {
        Some(config) => config
            .accounts
            .iter()
            .find(|account| super::bound_source(account) == Some(super::FeedSource::ClaudeCode))
            .map_or(config.refresh_seconds, |account| {
                account.effective_refresh_seconds(config.refresh_seconds)
            }),
        None => crate::model::default_refresh(),
    };
    std::time::Duration::from_secs(seconds)
}

/// PRD 0040 re-validation V-04/V-05: the age Claude's windows show when
/// they were written (`measured_at`, the oldest window's time,
/// [`super::AccountReading`]) longer than `interval` ago; `None` while
/// they are recent.
pub fn claude_stale_age(
    measured_at: DateTime<Utc>,
    interval: std::time::Duration,
    now: DateTime<Utc>,
) -> Option<std::time::Duration> {
    let age = crate::freshness::age(measured_at, now);
    (age > interval).then_some(age)
}

/// Claude's part of the line: its windows ("5h 23%", "Wk 41%"), the reset
/// of the tightest one ("↺ 19:30") and that window's used share.
struct ClaudePart {
    windows: Vec<String>,
    reset: Option<String>,
    used: f64,
    /// " (2 h)" when the windows are old; never dropped by the width.
    stale: Option<String>,
}

impl ClaudePart {
    /// "5h 23% · Wk 41% ↺ 19:30" with the first `windows` windows, and the
    /// reset when `reset` (`compose` drops it before any window, as it
    /// belongs to the tightest one).
    fn text(&self, windows: usize, reset: bool) -> String {
        let mut text = self.windows[..windows].join(" · ");
        if let Some(at) = self.reset.as_ref().filter(|_| reset) {
            text.push(' ');
            text.push_str(at);
        }
        if let Some(stale) = &self.stale {
            text.push(' ');
            text.push_str(stale);
        }
        text
    }
}

/// Display width in terminal cells: CJK and other wide characters take two,
/// combining marks none (the same approximation as the catalog budget test;
/// the core carries no width table).
fn cells(text: &str) -> usize {
    text.chars()
        .map(|c| match u32::from(c) {
            0x0300..=0x036F
            | 0x0900..=0x0902
            | 0x093A
            | 0x093C
            | 0x0941..=0x0948
            | 0x094D
            | 0x0951..=0x0957
            | 0x0962..=0x0963
            | 0x200B..=0x200D
            | 0xFE00..=0xFE0F => 0,
            0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD => 2,
            _ => 1,
        })
        .sum()
}

/// Claude's windows: "5h 23% · Wk 41% ↺ 19:30", the reset being the one of
/// the tightest window that states one. `None` without a window.
fn claude_part(
    reading: &FeedReading,
    now: DateTime<Utc>,
    at: &dyn Fn(DateTime<Utc>) -> String,
    locale: &str,
) -> Option<ClaudePart> {
    let mut parts = Vec::new();
    let mut tightest: Option<f64> = None;
    let mut next_reset: Option<(f64, DateTime<Utc>)> = None;
    for window in &reading.windows {
        let resets = window.resets_at().filter(|t| *t > now);
        if window.resets_at.is_some() && resets.is_none() {
            // Past its reset: Claude Code would have dropped it.
            continue;
        }
        let text = match window.id {
            WindowId::FiveHour => Localized::with_param(
                "feed.line.five_hour",
                "pct",
                pct(window.used_pct),
                "5h {pct}",
            ),
            WindowId::SevenDay => {
                Localized::with_param("feed.line.week", "pct", pct(window.used_pct), "Wk {pct}")
            }
            WindowId::SpendLimit => match (window.used_usd, window.limit_usd) {
                (Some(used), Some(limit)) => Localized::with_params(
                    "feed.line.spend_usd",
                    crate::text::params([
                        (
                            "used",
                            ParamValue::l10n(crate::text::number::money(used, "USD")),
                        ),
                        (
                            "limit",
                            ParamValue::l10n(crate::text::number::money(limit, "USD")),
                        ),
                    ]),
                    "{used}/{limit}",
                ),
                _ => Localized::with_param(
                    "feed.line.spend",
                    "pct",
                    pct(window.used_pct),
                    "spend {pct}",
                ),
            },
            WindowId::Primary | WindowId::Secondary => continue,
        };
        parts.push(text.resolve(locale));
        if tightest.is_none_or(|used| window.used_pct > used) {
            tightest = Some(window.used_pct);
        }
        if let Some(at) = resets {
            if next_reset.is_none_or(|(used, _)| window.used_pct > used) {
                next_reset = Some((window.used_pct, at));
            }
        }
    }
    let used = tightest?;
    let reset = next_reset.map(|(_, when)| {
        Localized::with_param("feed.line.reset", "time", at(when).as_str(), "↺ {time}")
            .resolve(locale)
    });
    Some(ClaudePart {
        windows: parts,
        reset,
        used,
        stale: None,
    })
}

/// RF5.1 (PRD 0040 re-validation V-02): the reading the line shows. The
/// context share is the agent's own (`own`, its standard input, when there
/// is one); Claude's windows are the account's ([`super::AccountReading`]),
/// so a new Claude Code window that has not reported its limits yet still
/// shows the last known ones. Their times stay in `account.window_times`.
pub fn line_reading(
    own: Option<&FeedReading>,
    account: Option<&super::AccountReading>,
) -> Option<FeedReading> {
    match (own, account) {
        (Some(own), Some(account)) => Some(FeedReading {
            windows: account.reading.windows.clone(),
            ..own.clone()
        }),
        (Some(own), None) => Some(own.clone()),
        (None, account) => account.map(|account| account.reading.clone()),
    }
}

/// "Codex 64% left (3 min)": the tightest of `others` with a known share;
/// an exhausted one says when it returns ("Codex 0% until 14:30", the
/// view's `resets_at` as the menu bar's figure uses it); a stale one ends
/// with its age ([`tray::stale_suffix`]).
fn other_part(
    others: &[OtherAccount],
    now: DateTime<Utc>,
    at: &dyn Fn(DateTime<Utc>) -> String,
    locale: &str,
) -> Option<String> {
    let other = others
        .iter()
        .filter(|other| other.view.remaining.is_some() || other.view.exhausted)
        .min_by(|a, b| {
            let share = |o: &OtherAccount| {
                if o.view.exhausted {
                    0.0
                } else {
                    o.view.remaining.unwrap_or(1.0)
                }
            };
            share(a).total_cmp(&share(b))
        })?;
    let name = if other.view.name.trim().is_empty() {
        other.view.provider_label.clone()
    } else {
        other.view.name.clone()
    };
    let returns = other
        .view
        .resets_at
        .filter(|when| other.view.exhausted && *when > now);
    let mut text = match returns {
        Some(when) => Localized::with_params(
            "feed.line.left_exhausted",
            crate::text::params([
                ("name", ParamValue::from(name)),
                ("pct", ParamValue::l10n(tray::percent_l10n(0.0))),
                ("time", ParamValue::from(at(when))),
            ]),
            "{name} {pct} until {time}",
        ),
        None => {
            let remaining = if other.view.exhausted {
                0.0
            } else {
                other.view.remaining.unwrap_or(0.0)
            };
            Localized::with_params(
                "feed.line.left",
                crate::text::params([
                    ("name", ParamValue::from(name)),
                    ("pct", ParamValue::l10n(tray::percent_l10n(remaining))),
                ]),
                "{name} {pct} left",
            )
        }
    }
    .resolve(locale);
    if other.view.stale {
        let age = other
            .view
            .measured_at
            .map(|measured| crate::freshness::age(measured, now));
        text.push(' ');
        text.push_str(&tray::stale_suffix(age, locale));
    }
    Some(text)
}

/// RF5.1/RF5.3: compose the line. `max_width` (in terminal cells) drops
/// whole parts, never a piece of one (PRD 0034 re-validation V-01): the
/// context share first, then the other account, then Claude's reset time,
/// then Claude's windows from the last; a line with no room is empty.
/// `claude_age` ([`claude_stale_age`]) ends Claude's part with the age of
/// old windows, which the width never drops (re-validation V-04/V-05).
pub fn compose(
    reading: Option<&FeedReading>,
    claude_age: Option<std::time::Duration>,
    others: &[OtherAccount],
    now: DateTime<Utc>,
    at: impl Fn(DateTime<Utc>) -> String,
    max_width: Option<usize>,
    locale: &str,
) -> Line {
    let claude = reading
        .and_then(|r| claude_part(r, now, &at, locale))
        .map(|part| ClaudePart {
            stale: claude_age.map(|age| tray::stale_suffix(Some(age), locale)),
            ..part
        });
    let other = other_part(others, now, &at, locale);
    let ctx = reading
        .and_then(|r| r.session.context_used_pct)
        .map(|used| super::live::context_chip(used).resolve(locale));
    let level = claude
        .as_ref()
        .map(|part| Level::of((1.0 - part.used / 100.0).clamp(0.0, 1.0)));
    let windows = claude.as_ref().map_or(0, |part| part.windows.len());
    // The candidates, longest first: (Claude's windows kept, its reset,
    // the other account, the context share).
    let mut candidates = vec![(windows, true, true, true), (windows, true, true, false)];
    candidates.push((windows, true, false, false));
    candidates.push((windows, false, false, false));
    candidates.extend((0..windows).rev().map(|kept| (kept, false, false, false)));
    let compose_with = |(kept, reset, with_other, with_ctx): (usize, bool, bool, bool)| {
        let mut parts: Vec<String> = Vec::new();
        let mut claude_len = 0;
        if let Some(part) = claude.as_ref().filter(|_| kept > 0) {
            let text = part.text(kept, reset);
            claude_len = text.len();
            parts.push(text);
        }
        if with_other || claude.is_none() {
            parts.extend(other.clone());
        }
        if with_ctx || (claude.is_none() && other.is_none()) {
            parts.extend(ctx.clone());
        }
        (parts.join(" · "), claude_len)
    };
    let (text, claude_len) = match max_width {
        None => compose_with(candidates[0]),
        Some(width) => candidates
            .into_iter()
            .map(compose_with)
            .find(|(text, _)| cells(text) <= width)
            .unwrap_or_default(),
    };
    Line {
        text,
        level,
        claude_len,
    }
}
