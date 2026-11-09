//! PRD 0040 — agent-fed readings.
//!
//! A **feed** is a reading an agent running on this computer handed to
//! Cuotai, as opposed to a query Cuotai makes over the network. Claude Code
//! hands its status-line JSON to `cuotai --feed claude-statusline` on
//! standard input ([`claude::parse`]); Codex CLI's rate-limit windows are
//! read from its own session logs ([`codex`]). Either way the reading lands
//! here as one small JSON file per source and session, under
//! `<data dir>/feed/<source>/<session digest>.json` (RF1.1), and the
//! adapters, the live-session block and the status line read it back.
//!
//! A feed file holds numbers, times, a model name and a one-way digest of
//! the agent's session id: never a working directory, a transcript path, a
//! prompt or a raw id (§6). Every field is validated on read (RF1.3); a
//! file that fails is ignored as a whole and counted by [`FeedStore::health`],
//! never turned into zero.

pub mod claude;
pub mod codex;
pub mod live;
pub mod statusline;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::text::Localized;
use crate::Provider;

/// The feed file format this release writes and reads.
pub const SCHEMA: u32 = 1;
/// RF1.2: files older than this are deleted (the newest file of a source is
/// kept, so a quiet agent ages instead of disappearing).
pub const RETENTION: TimeDelta = TimeDelta::hours(24);
/// RF1.2: the most files kept per source; the oldest go first.
pub const MAX_FILES: usize = 64;
/// RF2.1: the largest status-line object accepted on standard input.
pub const MAX_INPUT: usize = 64 * 1024;
/// RF1.3: a reading written further ahead of now than this is refused, so
/// a future-dated file can never pass for the newest (clock skew between
/// the agent and Cuotai stays well inside it).
pub const FUTURE_TOLERANCE: TimeDelta = TimeDelta::minutes(1);
/// RF1.3: a `resets_at` further than this from now is refused.
const RESET_HORIZON: TimeDelta = TimeDelta::days(400);
/// RF1.3: the spend limit may exceed 100 % and is kept up to this value.
const SPEND_MAX: f64 = 1_000.0;
/// The largest context window size kept (tokens).
const CONTEXT_MAX: u64 = 100_000_000;
/// The largest session cost or dollar amount kept.
const USD_MAX: f64 = 10_000_000.0;

/// The agent that wrote a feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedSource {
    ClaudeCode,
    CodexCli,
}

impl FeedSource {
    pub const ALL: [Self; 2] = [Self::ClaudeCode, Self::CodexCli];

    pub fn slug(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::CodexCli => "codex_cli",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.slug() == slug)
    }

    /// The agent's product name (never translated).
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::CodexCli => "Codex CLI",
        }
    }

    /// The provider whose account the feed can answer for.
    pub fn provider(self) -> Provider {
        match self {
            Self::ClaudeCode => Provider::Claude,
            Self::CodexCli => Provider::ChatGpt,
        }
    }

    /// The feed source of `provider`, if it has one.
    pub fn for_provider(provider: Provider) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.provider() == provider)
    }
}

/// One rate-limit window as the agent reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowId {
    /// Claude Code's rolling 5-hour window.
    FiveHour,
    /// Claude Code's weekly window.
    SevenDay,
    /// The spend limit of a Claude apps gateway.
    SpendLimit,
    /// Codex CLI's primary window.
    Primary,
    /// Codex CLI's secondary window.
    Secondary,
}

/// One window of a [`FeedReading`] (§7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedWindow {
    pub id: WindowId,
    /// Share consumed, in percent (`0..=100`; the spend limit up to 1 000).
    pub used_pct: f64,
    /// Unix seconds of the window's reset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_usd: Option<f64>,
    /// The spend limit's period: `daily`, `weekly` or `monthly`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    /// Codex CLI: the window's length in minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<u32>,
}

impl FeedWindow {
    /// The reset as a time, when stated.
    pub fn resets_at(&self) -> Option<DateTime<Utc>> {
        self.resets_at
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
    }
}

/// The session part of a [`FeedReading`] (§7): what the live-session block
/// shows. Every field but the digest is optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedSession {
    /// SHA-256 of the agent's session id, 16 hex characters.
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_used_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_remaining_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_size: Option<u64>,
    /// The agent's own session estimate at list price, not a charge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// One feed file (`schema: 1`, §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedReading {
    pub schema: u32,
    pub source: FeedSource,
    pub written_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    pub session: FeedSession,
    #[serde(default)]
    pub windows: Vec<FeedWindow>,
}

impl FeedReading {
    /// The window `id`, when the reading carries it.
    pub fn window(&self, id: WindowId) -> Option<&FeedWindow> {
        self.windows.iter().find(|window| window.id == id)
    }

    /// RF1.3: whether every field is within its contract at `now`,
    /// including `written_at` no later than [`FUTURE_TOLERANCE`] past it.
    pub fn is_valid(&self, now: DateTime<Utc>) -> bool {
        let pct = |value: f64| value.is_finite() && (0.0..=100.0).contains(&value);
        let usd = |value: f64| value.is_finite() && (0.0..=USD_MAX).contains(&value);
        let reset_ok = |seconds: i64| {
            DateTime::from_timestamp(seconds, 0).is_some_and(|at| (at - now).abs() <= RESET_HORIZON)
        };
        let session = &self.session;
        self.schema == SCHEMA
            && self.written_at <= now + FUTURE_TOLERANCE
            && is_digest(&session.digest)
            && session.model_id.as_deref().is_none_or(is_model_text)
            && session.model_name.as_deref().is_none_or(is_model_text)
            && self.agent_version.as_deref().is_none_or(is_version_text)
            && session.context_used_pct.is_none_or(pct)
            && session.context_remaining_pct.is_none_or(pct)
            && session.context_size.is_none_or(|size| size <= CONTEXT_MAX)
            && session.cost_usd.is_none_or(usd)
            && self.windows.len() <= 8
            && self.windows.iter().all(|window| {
                let allowed = matches!(
                    (self.source, window.id),
                    (FeedSource::ClaudeCode, WindowId::FiveHour)
                        | (FeedSource::ClaudeCode, WindowId::SevenDay)
                        | (FeedSource::ClaudeCode, WindowId::SpendLimit)
                        | (FeedSource::CodexCli, WindowId::Primary)
                        | (FeedSource::CodexCli, WindowId::Secondary)
                );
                let used = if window.id == WindowId::SpendLimit {
                    window.used_pct.is_finite() && (0.0..=SPEND_MAX).contains(&window.used_pct)
                } else {
                    pct(window.used_pct)
                };
                allowed
                    && used
                    && window.resets_at.is_none_or(reset_ok)
                    && window.used_usd.is_none_or(usd)
                    && window.limit_usd.is_none_or(usd)
                    && window.period.as_deref().is_none_or(is_period)
                    && window
                        .window_minutes
                        .is_none_or(|minutes| (1..=525_600).contains(&minutes))
            })
            && {
                // Two windows with the same id would be ambiguous.
                self.windows.iter().enumerate().all(|(index, window)| {
                    self.windows[..index]
                        .iter()
                        .all(|other| other.id != window.id)
                })
            }
    }
}

/// RF1.1: the one-way digest of an agent's session id (16 hex characters).
pub fn session_digest(session_id: &str) -> String {
    let digest = Sha256::digest(session_id.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_digest(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// RF2.1: a model id or display name kept only when it matches
/// `^[A-Za-z0-9 ._-]{1,64}$`.
pub(crate) fn is_model_text(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b'_' | b'-'))
}

fn is_version_text(value: &str) -> bool {
    (1..=32).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}

fn is_period(value: &str) -> bool {
    matches!(value, "daily" | "weekly" | "monthly")
}

/// What [`FeedStore::health`] found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FeedHealth {
    /// Valid files.
    pub readable: usize,
    /// Files ignored by RF1.3's validation.
    pub unreadable: usize,
}

impl FeedHealth {
    /// "1 unreadable feed file", or `None` when every file was readable.
    pub fn note(&self) -> Option<Localized> {
        (self.unreadable > 0).then(|| {
            Localized::with_param(
                "feed.health.unreadable",
                "n",
                self.unreadable as u64,
                "{n} unreadable feed file(s) ignored.",
            )
        })
    }
}

/// The feed directory: one subdirectory per source.
#[derive(Debug, Clone)]
pub struct FeedStore {
    dir: PathBuf,
}

impl FeedStore {
    /// The store at `dir` (tests and tools).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `<data dir>/feed` ([`crate::data_dir::root`], honouring
    /// `CUOTAI_DATA_DIR`); `None` when there is no usable data directory.
    pub fn in_data_dir() -> Option<Self> {
        crate::data_dir::root().map(|root| Self::at(root.join("feed")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn source_dir(&self, source: FeedSource) -> PathBuf {
        self.dir.join(source.slug())
    }

    /// RF1.1/RF1.4: write `reading` atomically as a `0600` file in a `0700`
    /// directory, then apply the retention and the cap. An invalid reading
    /// is refused rather than written.
    pub fn write(&self, reading: &FeedReading, now: DateTime<Utc>) -> std::io::Result<()> {
        if !reading.is_valid(now) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "feed reading out of contract",
            ));
        }
        crate::private_fs::ensure_dir(&self.dir)?;
        let dir = self.source_dir(reading.source);
        let bytes = serde_json::to_vec(reading).map_err(std::io::Error::other)?;
        crate::private_fs::write_file(
            &dir.join(format!("{}.json", reading.session.digest)),
            &bytes,
        )?;
        self.prune(reading.source, now);
        Ok(())
    }

    /// Every valid reading of `source`, newest first, plus the count of
    /// files that failed validation. Expired files are deleted on the way.
    fn scan(&self, source: FeedSource, now: DateTime<Utc>) -> (Vec<FeedReading>, usize) {
        self.prune(source, now);
        let mut readings = Vec::new();
        let mut unreadable = 0;
        for path in json_files(&self.source_dir(source)) {
            match read_file(&path) {
                Some(reading) if reading.source == source && reading.is_valid(now) => {
                    readings.push(reading);
                }
                _ => unreadable += 1,
            }
        }
        readings.sort_by_key(|reading| std::cmp::Reverse(reading.written_at));
        (readings, unreadable)
    }

    /// RF1.2: the newest valid reading of `source` (by `written_at`).
    pub fn latest(&self, source: FeedSource, now: DateTime<Utc>) -> Option<FeedReading> {
        self.scan(source, now).0.into_iter().next()
    }

    /// RF1.2: every valid reading of `source` within the retention, newest
    /// first.
    pub fn sessions(&self, source: FeedSource, now: DateTime<Utc>) -> Vec<FeedReading> {
        self.scan(source, now).0
    }

    /// RF1.3: how many files of every source are readable.
    pub fn health(&self, now: DateTime<Utc>) -> FeedHealth {
        let mut health = FeedHealth::default();
        for source in FeedSource::ALL {
            let (readings, unreadable) = self.scan(source, now);
            health.readable += readings.len();
            health.unreadable += unreadable;
        }
        health
    }

    /// RF1.2: delete files older than [`RETENTION`] (except the newest) and
    /// keep at most [`MAX_FILES`] per source. Best effort: a file that
    /// cannot be removed stays.
    fn prune(&self, source: FeedSource, now: DateTime<Utc>) {
        let mut files: Vec<(PathBuf, std::time::SystemTime)> = json_files(&self.source_dir(source))
            .into_iter()
            .filter_map(|path| {
                let modified = std::fs::symlink_metadata(&path).ok()?.modified().ok()?;
                Some((path, modified))
            })
            .collect();
        files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
        let cutoff: std::time::SystemTime = (now - RETENTION).into();
        for (index, (path, modified)) in files.iter().enumerate() {
            if index >= MAX_FILES || (index > 0 && *modified < cutoff) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Remove every feed file (part of erasing the data).
    pub fn clear(&self) -> std::io::Result<()> {
        match std::fs::remove_dir_all(&self.dir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

/// The regular `*.json` files of `dir` (never through a link).
fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect()
}

/// A feed file, at most 16 KiB, parsed; `None` when it is not one.
fn read_file(path: &Path) -> Option<FeedReading> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// RF3.1/RF4.2: how an account with a feed mode is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// The provider query only.
    Query,
    /// The agent's feed only, no network request.
    Feed,
    /// The feed while it is recent, otherwise the query.
    Hybrid,
}

/// The `query_mode` values of the feed modes.
pub const MODE_CLAUDE_CODE: &str = "claude_code";
pub const MODE_CLAUDE_CODE_OR_API: &str = "claude_code_or_api";
pub const MODE_CODEX_CLI: &str = "codex_cli";
pub const MODE_CODEX_CLI_OR_API: &str = "codex_cli_or_api";
/// RF8: the account parameter that binds a feed to an account.
pub const BIND_PARAM: &str = "feed";
/// The prefix of `Report::source` for a reading answered by a feed.
pub const REPORT_SOURCE_PREFIX: &str = "feed:";

/// The feed source and how `provider` with `params` is read; `None` for a
/// provider or mode without a feed.
pub fn mode_of(
    provider: Provider,
    params: &std::collections::BTreeMap<String, String>,
) -> Option<(FeedSource, SourceKind)> {
    let mode = params.get("query_mode").map(String::as_str);
    match (provider, mode) {
        (Provider::Claude, Some(MODE_CLAUDE_CODE)) => {
            Some((FeedSource::ClaudeCode, SourceKind::Feed))
        }
        (Provider::Claude, Some(MODE_CLAUDE_CODE_OR_API)) => {
            Some((FeedSource::ClaudeCode, SourceKind::Hybrid))
        }
        (Provider::ChatGpt, Some(MODE_CODEX_CLI)) => Some((FeedSource::CodexCli, SourceKind::Feed)),
        (Provider::ChatGpt, Some(MODE_CODEX_CLI_OR_API)) => {
            Some((FeedSource::CodexCli, SourceKind::Hybrid))
        }
        _ => None,
    }
}

/// RF8: the feed bound to `account`: its feed mode's source, or the
/// explicit `feed` parameter of a query-mode account of the same provider.
pub fn bound_source(account: &crate::Account) -> Option<FeedSource> {
    if let Some((source, _)) = mode_of(account.provider, &account.params) {
        return Some(source);
    }
    account
        .params
        .get(BIND_PARAM)
        .and_then(|slug| FeedSource::from_slug(slug))
        .filter(|source| source.provider() == account.provider)
}

/// RF8.1: whether the `feed` parameter is valid for `provider` (absent, or
/// the provider's own source).
pub fn valid_binding(
    provider: Provider,
    params: &std::collections::BTreeMap<String, String>,
) -> bool {
    match params.get(BIND_PARAM) {
        None => true,
        Some(slug) => {
            FeedSource::from_slug(slug).is_some_and(|source| source.provider() == provider)
        }
    }
}
