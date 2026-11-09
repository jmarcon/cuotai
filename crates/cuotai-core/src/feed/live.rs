//! RF7 and RF8: the live-session block and the unbound-feed banner.
//!
//! Pure: front ends pass the readings of [`super::FeedStore::sessions`],
//! the configuration and `now`, and render what comes back. The block is
//! information only: it is never notified and never feeds an alert.

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;

use super::{FeedReading, FeedSource};
use crate::text::{params, Localized, ParamValue};
use crate::{Account, Config, Provider};

/// RF7.2: a session disappears this long after its last feed.
pub const LIVE_FOR: TimeDelta = TimeDelta::minutes(10);
/// RF7.1: the most sessions listed; the rest are counted.
pub const MAX_SHOWN: usize = 5;
/// RF7.1: the context meter turns `warn` from this share (display only).
pub const CONTEXT_WARN: f64 = 70.0;
/// RF7.1: the context meter turns `bad` from this share (display only).
pub const CONTEXT_BAD: f64 = 90.0;
/// RF8.1: how long a dismissed banner stays hidden.
pub const BANNER_SNOOZE: TimeDelta = TimeDelta::days(7);

/// The display role of the context meter (DS-CMP-MTR, display only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextLevel {
    Good,
    Warn,
    Bad,
}

impl ContextLevel {
    pub fn of(used_pct: f64) -> Self {
        if used_pct >= CONTEXT_BAD {
            Self::Bad
        } else if used_pct >= CONTEXT_WARN {
            Self::Warn
        } else {
            Self::Good
        }
    }
}

/// One live agent session (RF7.1).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveSession {
    pub source: FeedSource,
    pub model_name: Option<String>,
    pub context_used_pct: Option<f64>,
    pub context_level: Option<ContextLevel>,
    /// The agent's estimate at list price, not a charge.
    pub cost_usd: Option<f64>,
    pub written_at: DateTime<Utc>,
}

impl LiveSession {
    /// "ctx 8%" (`feed.live.ctx`), when the context share is known.
    pub fn context_chip(&self) -> Option<Localized> {
        self.context_used_pct.map(context_chip)
    }

    /// "≈ $0.42 this session" (`feed.live.cost`), when the agent stated it.
    pub fn cost_text(&self) -> Option<Localized> {
        self.cost_usd.map(|usd| {
            Localized::with_param(
                "feed.live.cost",
                "usd",
                format!("{usd:.2}").as_str(),
                "≈ ${usd} this session",
            )
        })
    }

    /// The compact terminal form: "ctx 8% · ≈$0.42" (RF7.3).
    pub fn compact(&self) -> Option<Localized> {
        let cost = self.cost_usd.map(|usd| {
            Localized::with_param(
                "feed.live.cost_short",
                "usd",
                format!("{usd:.2}").as_str(),
                "≈${usd}",
            )
        });
        let parts: Vec<Localized> = self.context_chip().into_iter().chain(cost).collect();
        join(parts)
    }
}

/// "ctx 8%".
pub fn context_chip(used_pct: f64) -> Localized {
    Localized::with_param(
        "feed.live.ctx",
        "pct",
        format!("{:.0}", used_pct.round()).as_str(),
        "ctx {pct}%",
    )
}

fn join(parts: Vec<Localized>) -> Option<Localized> {
    parts.into_iter().reduce(|first, rest| {
        Localized::with_params(
            "feed.live.join",
            params([
                ("first", ParamValue::l10n(first)),
                ("rest", ParamValue::l10n(rest)),
            ]),
            "{first} · {rest}",
        )
    })
}

/// RF7.1: the block's title, "Live session".
pub fn title() -> Localized {
    Localized::fixed("feed.live.title", "Live session")
}

/// RF7.1 (HON): what the cost figure is.
pub fn cost_caveat() -> Localized {
    Localized::fixed(
        "feed.live.caveat",
        "Agent estimate at list price, not a charge.",
    )
}

/// The live sessions of one account (RF7.1).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LiveBlock {
    /// Newest first, at most [`MAX_SHOWN`].
    pub sessions: Vec<LiveSession>,
    /// Live sessions left out by the cap.
    pub more: usize,
}

impl LiveBlock {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// "and 2 more" (`feed.live.more`), when sessions were left out.
    pub fn more_text(&self) -> Option<Localized> {
        (self.more > 0)
            .then(|| Localized::with_param("feed.live.more", "n", self.more as u64, "and {n} more"))
    }

    /// The newest session's context chip, for the tray row (RF7.2).
    pub fn tray_chip(&self) -> Option<Localized> {
        self.sessions.first().and_then(LiveSession::context_chip)
    }

    /// RF7.3: the `--once` line, "Live session: ctx 8% · ≈$0.42".
    pub fn once_line(&self) -> Option<Localized> {
        let newest = self.sessions.first()?;
        let model = newest
            .model_name
            .as_deref()
            .map(|name| Localized::with_param("feed.live.model", "model", name, "{model}"));
        let compact = join(model.into_iter().chain(newest.compact()).collect())?;
        Some(Localized::with_param(
            "feed.live.line",
            "parts",
            compact,
            "Live session: {parts}",
        ))
    }
}

/// RF7.1/RF7.2: the sessions of `readings` younger than [`LIVE_FOR`] at
/// `now`, newest first, capped at [`MAX_SHOWN`].
pub fn block(readings: &[FeedReading], now: DateTime<Utc>) -> LiveBlock {
    let mut live: Vec<&FeedReading> = readings
        .iter()
        .filter(|reading| {
            now - reading.written_at < LIVE_FOR
                && reading.written_at <= now + super::FUTURE_TOLERANCE
        })
        .collect();
    live.sort_by_key(|reading| std::cmp::Reverse(reading.written_at));
    let more = live.len().saturating_sub(MAX_SHOWN);
    let sessions = live
        .into_iter()
        .take(MAX_SHOWN)
        .map(|reading| LiveSession {
            source: reading.source,
            model_name: reading.session.model_name.clone(),
            context_used_pct: reading.session.context_used_pct,
            context_level: reading.session.context_used_pct.map(ContextLevel::of),
            cost_usd: reading.session.cost_usd,
            written_at: reading.written_at,
        })
        .collect();
    LiveBlock { sessions, more }
}

/// RF8.1: the banner offered when an agent sends readings no account uses.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnboundBanner {
    pub source: FeedSource,
    pub text: Localized,
    pub action: Localized,
    /// The provider the **Add account** action prefills.
    pub provider: Provider,
    /// The `query_mode` it prefills.
    pub query_mode: &'static str,
}

/// RF8.1: the banner state machine. `Some` when `source` sent a reading in
/// the last day, no account is bound to it (feed mode or `feed` binding),
/// and the banner was not dismissed in the last [`BANNER_SNOOZE`].
pub fn unbound_banner(
    source: FeedSource,
    config: &Config,
    latest: Option<&FeedReading>,
    dismissed_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<UnboundBanner> {
    let latest = latest?;
    if now - latest.written_at > super::RETENTION {
        return None;
    }
    if config
        .accounts
        .iter()
        .any(|account| super::bound_source(account) == Some(source))
    {
        return None;
    }
    if dismissed_at.is_some_and(|at| now - at < BANNER_SNOOZE) {
        return None;
    }
    let (text, provider_label, query_mode) = match source {
        FeedSource::ClaudeCode => (
            Localized::fixed(
                "feed.banner.claude_code",
                "Claude Code is sending readings; add a Claude account fed by Claude Code.",
            ),
            Provider::Claude,
            super::MODE_CLAUDE_CODE,
        ),
        FeedSource::CodexCli => (
            Localized::fixed(
                "feed.banner.codex_cli",
                "Codex CLI has rate-limit readings; add a ChatGPT / Codex account fed by Codex CLI.",
            ),
            Provider::ChatGpt,
            super::MODE_CODEX_CLI,
        ),
    };
    Some(UnboundBanner {
        source,
        text,
        action: Localized::fixed("feed.banner.add", "Add account"),
        provider: provider_label,
        query_mode,
    })
}

/// RF7.1/RF8: the live block of `account`, from the sessions of the feed it
/// is bound to; empty for an account without a feed.
pub fn block_for(account: &Account, store: &super::FeedStore, now: DateTime<Utc>) -> LiveBlock {
    match super::bound_source(account) {
        Some(source) => block(&store.sessions(source, now), now),
        None => LiveBlock::default(),
    }
}
