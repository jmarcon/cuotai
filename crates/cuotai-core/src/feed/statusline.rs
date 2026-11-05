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

fn pct(value: f64) -> String {
    format!("{:.0}%", value.round())
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

/// Claude's windows: "5h 23% · Wk 41% ↺ 19:30", the reset being the one of
/// the tightest window. `None` without a window.
fn claude_part(
    reading: &FeedReading,
    now: DateTime<Utc>,
    at: &dyn Fn(DateTime<Utc>) -> String,
    locale: &str,
) -> Option<(String, f64)> {
    let mut parts = Vec::new();
    let mut tightest: Option<(f64, Option<DateTime<Utc>>)> = None;
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
                pct(window.used_pct).as_str(),
                "5h {pct}",
            ),
            WindowId::SevenDay => Localized::with_param(
                "feed.line.week",
                "pct",
                pct(window.used_pct).as_str(),
                "Wk {pct}",
            ),
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
                    pct(window.used_pct).as_str(),
                    "spend {pct}",
                ),
            },
            WindowId::Primary | WindowId::Secondary => continue,
        };
        parts.push(text.resolve(locale));
        if tightest.is_none_or(|(used, _)| window.used_pct > used) {
            tightest = Some((window.used_pct, resets));
        }
    }
    let (used, resets) = tightest?;
    let mut text = parts.join(" · ");
    if let Some(when) = resets {
        text.push(' ');
        text.push_str(
            &Localized::with_param("feed.line.reset", "time", at(when).as_str(), "↺ {time}")
                .resolve(locale),
        );
    }
    Some((text, used))
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
            ("pct", ParamValue::from(tray::percent(remaining))),
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

/// RF5.1/RF5.3: compose the line. `max_width` truncates by dropping the
/// context share first, then the other account, and finally cutting
/// Claude's windows with an ellipsis (they go last).
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
        .map(|(_, used)| Level::of((1.0 - used / 100.0).clamp(0.0, 1.0)));
    let mut parts: Vec<String> = claude.map(|(text, _)| text).into_iter().collect();
    parts.extend(other.clone());
    parts.extend(ctx.clone());
    let mut text = parts.join(" · ");
    if let Some(width) = max_width {
        let count = |s: &str| s.chars().count();
        if count(&text) > width && ctx.is_some() {
            parts.pop();
            text = parts.join(" · ");
        }
        if count(&text) > width && other.is_some() && parts.len() > 1 {
            parts.pop();
            text = parts.join(" · ");
        }
        if count(&text) > width {
            text = if width == 0 {
                String::new()
            } else {
                let kept: String = text.chars().take(width - 1).collect();
                format!("{kept}…")
            };
        }
    }
    Line { text, level }
}
