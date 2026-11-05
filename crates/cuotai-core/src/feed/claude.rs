//! RF2.1: Claude Code's status-line JSON to a [`FeedReading`].
//!
//! Claude Code runs the status-line command with one JSON object on
//! standard input (<https://code.claude.com/docs/en/statusline>, read
//! 2026-10-25). Only these fields are read: `rate_limits.five_hour`,
//! `rate_limits.seven_day` and `rate_limits.spend_limit` (`used_percentage`,
//! `resets_at`, `used_usd`, `limit_usd`, `period`), `context_window`
//! (`used_percentage`, `remaining_percentage`, `context_window_size`),
//! `cost.total_cost_usd`, `model.id` and `model.display_name` (when they
//! match the model-text pattern), `session_id` (digested at once) and
//! `version`. The input is walked as a `serde_json::Value` by explicit
//! paths: `cwd`, `workspace`, `transcript_path`, `output_style` and every
//! other field are never copied into a struct, written or logged.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{
    is_model_text, is_version_text, session_digest, FeedReading, FeedSession, FeedSource,
    FeedWindow, WindowId, MAX_INPUT, SCHEMA,
};

/// Why a status-line input was refused (exit `1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// More than [`MAX_INPUT`] bytes.
    TooLarge,
    /// Not one JSON object.
    Malformed,
}

impl ParseError {
    pub fn l10n(self) -> crate::text::Localized {
        match self {
            Self::TooLarge => crate::text::Localized::fixed(
                "feed.error.too_large",
                "The status-line input is larger than 64 KiB.",
            ),
            Self::Malformed => crate::text::Localized::fixed(
                "feed.error.malformed",
                "The status-line input is not a JSON object.",
            ),
        }
    }
}

/// A finite number at `value`, or `None` (absent, `null` or not a number).
fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64).filter(|n| n.is_finite())
}

fn percent(value: Option<&Value>, max: f64) -> Option<f64> {
    number(value).filter(|n| (0.0..=max).contains(n))
}

fn window(rate_limits: Option<&Value>, key: &str, id: WindowId) -> Option<FeedWindow> {
    let object = rate_limits?.get(key)?;
    let max = if id == WindowId::SpendLimit {
        1_000.0
    } else {
        100.0
    };
    // `used_percentage` null or out of range: the window is unknown, not 0.
    let used_pct = percent(object.get("used_percentage"), max)?;
    let usd = |name: &str| number(object.get(name)).filter(|n| *n >= 0.0 && *n <= 10_000_000.0);
    Some(FeedWindow {
        id,
        used_pct,
        resets_at: object.get("resets_at").and_then(Value::as_i64),
        used_usd: (id == WindowId::SpendLimit)
            .then(|| usd("used_usd"))
            .flatten(),
        limit_usd: (id == WindowId::SpendLimit)
            .then(|| usd("limit_usd"))
            .flatten(),
        period: (id == WindowId::SpendLimit)
            .then(|| object.get("period").and_then(Value::as_str))
            .flatten()
            .filter(|p| matches!(*p, "daily" | "weekly" | "monthly"))
            .map(str::to_owned),
        window_minutes: None,
    })
}

/// RF2.1: parse one status-line object received at `now`. A window without
/// a usable percentage is left out (absence is normal before the first API
/// response); a reset outside the accepted horizon drops only that reset.
pub fn parse(input: &[u8], now: DateTime<Utc>) -> Result<FeedReading, ParseError> {
    if input.len() > MAX_INPUT {
        return Err(ParseError::TooLarge);
    }
    let value: Value = serde_json::from_slice(input).map_err(|_| ParseError::Malformed)?;
    if !value.is_object() {
        return Err(ParseError::Malformed);
    }
    let rate_limits = value.get("rate_limits");
    let mut windows: Vec<FeedWindow> = [
        ("five_hour", WindowId::FiveHour),
        ("seven_day", WindowId::SevenDay),
        ("spend_limit", WindowId::SpendLimit),
    ]
    .into_iter()
    .filter_map(|(key, id)| window(rate_limits, key, id))
    .collect();
    let horizon = super::RESET_HORIZON;
    for window in &mut windows {
        if window
            .resets_at()
            .is_none_or(|at| (at - now).abs() > horizon)
        {
            window.resets_at = None;
        }
    }
    let context = value.get("context_window");
    let model = value.get("model");
    let text = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|text| is_model_text(text))
            .map(str::to_owned)
    };
    // Without a session id every input shares one file, digest of "".
    let session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let session = FeedSession {
        digest: session_digest(session_id),
        model_id: text(model.and_then(|m| m.get("id"))),
        model_name: text(model.and_then(|m| m.get("display_name"))),
        context_used_pct: percent(context.and_then(|c| c.get("used_percentage")), 100.0),
        context_remaining_pct: percent(context.and_then(|c| c.get("remaining_percentage")), 100.0),
        context_size: context
            .and_then(|c| c.get("context_window_size"))
            .and_then(Value::as_u64)
            .filter(|size| *size <= super::CONTEXT_MAX),
        cost_usd: number(value.get("cost").and_then(|c| c.get("total_cost_usd")))
            .filter(|n| *n >= 0.0 && *n <= super::USD_MAX),
    };
    Ok(FeedReading {
        schema: SCHEMA,
        source: FeedSource::ClaudeCode,
        written_at: now,
        agent_version: value
            .get("version")
            .and_then(Value::as_str)
            .filter(|v| is_version_text(v))
            .map(str::to_owned),
        session,
        windows,
    })
}

/// RF2.4: the `~/.claude/settings.json` snippets `--setup` prints and the
/// desktop offers with **Copy** (text only; Cuotai never edits the agent's
/// settings).
pub const SETTINGS_SNIPPET: &str = r#"{
  "statusLine": {
    "type": "command",
    "command": "cuotai --feed claude-statusline --print"
  }
}"#;

/// RF2.4: the chained variant that keeps the person's own status-line
/// script (`~/.claude/statusline.sh` stands for it).
pub const CHAINED_SNIPPET: &str = r#"{
  "statusLine": {
    "type": "command",
    "command": "sh -c 'input=$(cat); printf %s \"$input\" | cuotai --feed claude-statusline; printf %s \"$input\" | ~/.claude/statusline.sh'"
  }
}"#;
