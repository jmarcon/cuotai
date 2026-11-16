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

/// One other account for the line, with the age of its reading when it
/// comes from the local history rather than a running server (RF5.2).
#[derive(Debug, Clone)]
pub struct OtherAccount {
    pub view: AccountView,
    pub age: Option<std::time::Duration>,
}

/// The composed line and the level that colours it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    /// The level of Claude's tightest window, `None` when none is known.
    pub level: Option<Level>,
}

/// "23%", the sign placed as the reader's language places it (V-17).
fn pct(value: f64) -> ParamValue {
    ParamValue::l10n(crate::text::number::percent(value.round()))
}

/// "3m", "2h", "1d": the compact age of a history reading.
fn compact_age(age: std::time::Duration) -> String {
    let secs = age.as_secs();
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3_599 => format!("{}m", secs / 60),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// Claude's part of the line: its windows ("5h 23%", "Wk 41%"), the reset
/// of the tightest one ("↺ 19:30") and that window's used share.
struct ClaudePart {
    windows: Vec<String>,
    reset: Option<String>,
    used: f64,
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
                        ("used", ParamValue::from(format!("{used:.0}"))),
                        ("limit", ParamValue::from(format!("{limit:.0}"))),
                    ]),
                    "${used}/${limit}",
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
    })
}

/// "Codex 64% left (3m)": the tightest of `others` with a known share.
fn other_part(others: &[OtherAccount], locale: &str) -> Option<String> {
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
    let remaining = if other.view.exhausted {
        0.0
    } else {
        other.view.remaining.unwrap_or(0.0)
    };
    let mut text = Localized::with_params(
        "feed.line.left",
        crate::text::params([
            ("name", ParamValue::from(name)),
            ("pct", ParamValue::l10n(tray::percent_l10n(remaining))),
        ]),
        "{name} {pct} left",
    )
    .resolve(locale);
    if let Some(age) = other.age {
        text.push_str(&format!(" ({})", compact_age(age)));
    } else if other.view.stale {
        text.push(' ');
        text.push_str(tray::STALE_MARK);
    }
    Some(text)
}

/// RF5.1/RF5.3: compose the line. `max_width` (in terminal cells) drops
/// whole parts, never a piece of one (PRD 0034 re-validation V-01): the
/// context share first, then the other account, then Claude's reset time,
/// then Claude's windows from the last; a line with no room is empty.
pub fn compose(
    reading: Option<&FeedReading>,
    others: &[OtherAccount],
    now: DateTime<Utc>,
    at: impl Fn(DateTime<Utc>) -> String,
    max_width: Option<usize>,
    locale: &str,
) -> Line {
    let claude = reading.and_then(|r| claude_part(r, now, &at, locale));
    let other = other_part(others, locale);
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
        if let Some(part) = claude.as_ref().filter(|_| kept > 0) {
            parts.push(part.text(kept, reset));
        }
        if with_other || claude.is_none() {
            parts.extend(other.clone());
        }
        if with_ctx || (claude.is_none() && other.is_none()) {
            parts.extend(ctx.clone());
        }
        parts.join(" · ")
    };
    let text = match max_width {
        None => compose_with(candidates[0]),
        Some(width) => candidates
            .into_iter()
            .map(compose_with)
            .find(|text| cells(text) <= width)
            .unwrap_or_default(),
    };
    Line { text, level }
}
