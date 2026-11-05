//! PRD 0040 RF3 and RF4: accounts answered by an agent's feed.
//!
//! `claude` with `query_mode = "claude_code"` reads the newest Claude Code
//! status-line feed ([`crate::feed`]); `chat_gpt` with `query_mode =
//! "codex_cli"` reads Codex CLI's own session logs. Neither makes a network
//! request or reads a credential. The hybrid modes (`*_or_api`) use the feed
//! while it is younger than the account's refresh interval and otherwise
//! fall back to the existing query, saying which one answered.
//!
//! The metrics are the ones the query mode produces (same labels, so the
//! history continues across a mode change), and the report's `as_of`
//! (PRD 0033) is the feed's time, so age counts from when the agent last
//! spoke. A window absent from the feed is left absent, never zero.

use chrono::{DateTime, TimeDelta, Utc};

use super::{Data, QueryError};
use crate::feed::{self, FeedReading, FeedSource, FeedStore, WindowId};
use crate::text::Notes;
use crate::{Billing, Metric, MetricKind, MetricWindow};

/// `Report::source` of a reading answered by the Claude Code feed.
pub(crate) const SOURCE_CLAUDE_CODE: &str = "feed:claude_code";
/// `Report::source` of a reading answered by the Codex CLI logs.
pub(crate) const SOURCE_CODEX_CLI: &str = "feed:codex_cli";

fn no_feed(source: FeedSource) -> QueryError {
    let note = match source {
        FeedSource::ClaudeCode => crate::text::Localized::fixed(
            "note.claude_code.no_feed",
            "No reading from Claude Code yet: add the status line (see docs) and send one prompt.",
        ),
        FeedSource::CodexCli => crate::text::Localized::fixed(
            "note.codex_cli.no_feed",
            "No rate-limit reading in the Codex CLI logs yet: send one prompt with Codex CLI.",
        ),
    };
    QueryError {
        diagnostic: None,
        status: crate::Status::NeedsConfiguration,
        note,
    }
}

fn no_consent() -> QueryError {
    QueryError {
        diagnostic: None,
        status: crate::Status::NeedsConfiguration,
        note: crate::text::Localized::fixed(
            "note.codex_cli.needs_consent",
            "Reading Codex CLI's logs needs the Codex CLI local source turned on (Local cost), which lists the folders read.",
        ),
    }
}

fn percent(
    label: &str,
    used: f64,
    resets_at: Option<DateTime<Utc>>,
    window: MetricWindow,
) -> Metric {
    let mut metric = Metric::usage(label, used, "%", MetricKind::Quota, Some(window));
    metric.limit = Some(100.0);
    metric.remaining = Some((100.0 - used).max(0.0));
    metric.resets_at = resets_at;
    metric
}

/// The spend-limit period as a metric window.
fn period_window(period: Option<&str>) -> MetricWindow {
    match period {
        Some("daily") => MetricWindow::Daily,
        Some("weekly") => MetricWindow::Weekly,
        Some("monthly") => MetricWindow::Monthly,
        _ => MetricWindow::Other,
    }
}

/// RF3.1/RF4.2: the metrics of `reading` at `now`. A window whose reset
/// already passed is dropped (the agent would have dropped it too); the
/// second value says whether an expected window was absent.
pub(crate) fn metrics(reading: &FeedReading, now: DateTime<Utc>) -> (Vec<Metric>, bool) {
    let mut metrics = Vec::new();
    let live = |window: &&feed::FeedWindow| window.resets_at().is_none_or(|at| at > now);
    let get = |id: WindowId| reading.window(id).filter(live);
    match reading.source {
        FeedSource::ClaudeCode => {
            if let Some(w) = get(WindowId::FiveHour) {
                metrics.push(percent(
                    "Sessão (5 horas) (%)",
                    w.used_pct,
                    w.resets_at(),
                    MetricWindow::Window5h,
                ));
            }
            if let Some(w) = get(WindowId::SevenDay) {
                metrics.push(percent(
                    "Semana (7 dias) (%)",
                    w.used_pct,
                    w.resets_at(),
                    MetricWindow::Weekly,
                ));
            }
            if let Some(w) = get(WindowId::SpendLimit) {
                let window = period_window(w.period.as_deref());
                match w.used_usd {
                    Some(used) => {
                        let mut metric = Metric::usage(
                            "Spend limit",
                            used,
                            "USD",
                            MetricKind::Spend,
                            Some(window),
                        );
                        metric.limit = w.limit_usd;
                        metric.remaining = w.limit_usd.map(|limit| (limit - used).max(0.0));
                        metric.resets_at = w.resets_at();
                        metric.billing = Billing::Included;
                        metric.period = w.period.clone();
                        metrics.push(metric);
                    }
                    None => {
                        // The dollars arrive later than the percentage.
                        let mut metric =
                            percent("Spend limit (%)", w.used_pct, w.resets_at(), window);
                        metric.remaining = Some((100.0 - w.used_pct).max(0.0));
                        metrics.push(metric);
                    }
                }
            }
            let absent = get(WindowId::FiveHour).is_none() || get(WindowId::SevenDay).is_none();
            (
                metrics,
                absent && reading.window(WindowId::SpendLimit).is_none(),
            )
        }
        FeedSource::CodexCli => {
            for id in [WindowId::Primary, WindowId::Secondary] {
                let Some(w) = get(id) else { continue };
                // The same labels and windows as the query mode.
                let seconds = w.window_minutes.map(|minutes| i64::from(minutes) * 60);
                let label = super::subscriptions::window_label("Codex", seconds);
                let window = super::subscriptions::codex_window(seconds);
                metrics.push(percent(&label, w.used_pct, w.resets_at(), window));
            }
            let absent = get(WindowId::Primary).is_none() || get(WindowId::Secondary).is_none();
            (metrics, absent)
        }
    }
}

/// RF3.1-RF3.3: the [`Data`] of a feed reading.
pub(crate) fn data(reading: &FeedReading, now: DateTime<Utc>, hybrid: bool) -> Data {
    let (metrics, absent) = metrics(reading, now);
    let mut notes = Notes::new();
    match reading.source {
        FeedSource::ClaudeCode => {
            if hybrid {
                notes.push(
                    "note.claude_code.answered_by_feed",
                    "Answered by Claude Code's own status line (no network request).",
                );
            } else {
                notes.push(
                    "note.claude_code.fed",
                    "Fed by Claude Code's documented status line: no network request, no token reuse.",
                );
            }
            if absent {
                notes.push(
                    "note.claude_code.window_absent",
                    "Claude Code drops a window once it resets and sends it again after the next prompt; until then it is unknown, not zero.",
                );
            }
        }
        FeedSource::CodexCli => {
            if hybrid {
                notes.push(
                    "note.codex_cli.answered_by_feed",
                    "Answered by Codex CLI's own session logs (no network request).",
                );
            } else {
                notes.push(
                    "note.codex_cli.fed",
                    "Read from Codex CLI's local session logs (undocumented format): no network request, no token reuse.",
                );
            }
            if absent {
                notes.push(
                    "note.codex_cli.window_absent",
                    "A window missing from the latest Codex CLI reading is unknown, not zero.",
                );
            }
        }
    }
    let source = match reading.source {
        FeedSource::ClaudeCode => SOURCE_CLAUDE_CODE,
        FeedSource::CodexCli => SOURCE_CODEX_CLI,
    };
    Data::new(metrics, notes, source).with_as_of(Some(reading.written_at))
}

/// The newest reading of `source`: the store for Claude Code; for Codex
/// CLI, its logs (gated on the `codex_cli` local-source consent, RF4.3),
/// stored in the feed directory so the live block can read it too.
pub(crate) fn latest(
    source: FeedSource,
    store: Option<&FeedStore>,
    now: DateTime<Utc>,
) -> Result<Option<FeedReading>, QueryError> {
    match source {
        FeedSource::ClaudeCode => Ok(store.and_then(|store| store.latest(source, now))),
        FeedSource::CodexCli => {
            let settings = crate::cost::SettingsStore::in_data_dir()
                .and_then(|store| store.load())
                .unwrap_or_default();
            let env = crate::cost::Env::system();
            if !settings.enabled(crate::cost::SourceId::CodexCli, &env) {
                return Err(no_consent());
            }
            let roots = settings.roots(crate::cost::SourceId::CodexCli, &env);
            let reading = codex_latest(&roots)?;
            if let (Some(reading), Some(store)) = (&reading, store) {
                // Best effort: the live block reads it from there.
                let _ = store.write(reading, now);
            }
            Ok(reading)
        }
    }
}

/// RF4.1: the Codex CLI reading below `roots`, an unknown shape being a
/// schema error.
pub(crate) fn codex_latest(
    roots: &[std::path::PathBuf],
) -> Result<Option<FeedReading>, QueryError> {
    feed::codex::latest(roots).map_err(|_| QueryError::schema())
}

/// RF3.4: whether a hybrid account uses `reading` rather than the query:
/// the feed is younger than the account's refresh interval.
pub(crate) fn fresh_enough(
    reading: &FeedReading,
    interval_seconds: u64,
    now: DateTime<Utc>,
) -> bool {
    now - reading.written_at <= TimeDelta::seconds(interval_seconds as i64)
}

/// RF3.1/RF4.2: answer a feed-mode account.
pub(crate) fn fetch(
    source: FeedSource,
    store: Option<&FeedStore>,
    now: DateTime<Utc>,
) -> Result<Data, QueryError> {
    match latest(source, store, now)? {
        Some(reading) => Ok(data(&reading, now, false)),
        None => Err(no_feed(source)),
    }
}

/// RF3.4: the note added when a hybrid account fell back to the query.
pub(crate) fn answered_by_api(source: FeedSource) -> crate::text::Localized {
    match source {
        FeedSource::ClaudeCode => crate::text::Localized::fixed(
            "note.claude_code.answered_by_api",
            "Answered by the Claude query: no recent reading from Claude Code.",
        ),
        FeedSource::CodexCli => crate::text::Localized::fixed(
            "note.codex_cli.answered_by_api",
            "Answered by the Codex query: no recent reading in the Codex CLI logs.",
        ),
    }
}
