//! PRD 0044 RF3.3: the local delivery log,
//! `<OS local data dir>/cuotai/channels/log.jsonl` (`0600`, 7 days).
//!
//! One JSON line per attempt outcome: the time, the channel id, the
//! notification kind, the outcome, the HTTP status and the attempt. Never
//! the body, the URL, the host, the label or an error text. Every Cuotai
//! process appends to the same file (short lines appended in one write),
//! so the log is also how processes agree on the hourly cap's history, the
//! last delivery shown in Settings and a channel failing for 24 hours.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// How long entries are kept.
pub const KEEP: TimeDelta = TimeDelta::days(7);
/// The log is never read past this size (it stays far below it).
const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
/// A log past [`MAX_READ_BYTES`] is cut to its newest lines within this
/// size when pruned, so it can be read again.
const TRIM_TO_BYTES: usize = (MAX_READ_BYTES / 2) as usize;

/// What happened to one notification on one channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogOutcome {
    /// The destination answered 2xx.
    Delivered,
    /// The last attempt failed (a 4xx at once; network errors and 5xx
    /// after the retries).
    Failed,
    /// RF3.2: past the hourly cap, folded into the hour's summary.
    RateLimited,
    /// RF3.2: not critical within the first minute after start.
    Held,
    /// RF3.4: the "is failing" system notification was shown.
    FailingNotice,
}

/// One log line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub at: DateTime<Utc>,
    pub channel: String,
    pub kind: String,
    pub outcome: LogOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default)]
    pub attempt: u8,
    /// `network`, `timeout`, `vault` or `invalid_url`: a category, never a
    /// message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The log file of one profile.
#[derive(Debug, Clone)]
pub struct DeliveryLog {
    path: PathBuf,
}

impl DeliveryLog {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `<OS local data dir>/cuotai/channels/log.jsonl`.
    pub fn in_data_dir() -> Option<Self> {
        default_path().map(Self::at)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append `entry`; a log that cannot be written is ignored (a delivery
    /// never fails because of it).
    pub fn append(&self, entry: &Entry) {
        let Ok(mut line) = serde_json::to_vec(entry) else {
            return;
        };
        line.push(b'\n');
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            if crate::private_fs::ensure_dir(dir).is_err() {
                return;
            }
        }
        if let Ok(meta) = std::fs::symlink_metadata(&self.path) {
            if !meta.is_file() || crate::private_fs::foreign_owner(&meta) {
                return;
            }
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        if let Ok(mut file) = options.open(&self.path) {
            let _ = file.write_all(&line);
        }
    }

    /// Every readable entry, oldest first.
    pub fn read(&self) -> Vec<Entry> {
        let Ok(meta) = std::fs::symlink_metadata(&self.path) else {
            return Vec::new();
        };
        if !meta.is_file() || meta.len() > MAX_READ_BYTES {
            return Vec::new();
        }
        std::fs::read_to_string(&self.path)
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Drop the entries older than [`KEEP`] at `now`, under the shared
    /// lock; nothing happens when none is that old. A log grown past
    /// [`MAX_READ_BYTES`] (which [`Self::read`] refuses) also keeps only
    /// its newest lines within [`TRIM_TO_BYTES`], so it never grows
    /// without end.
    pub fn prune(&self, now: DateTime<Utc>) {
        let Ok(meta) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        let oversized = meta.is_file() && meta.len() > MAX_READ_BYTES;
        if !oversized {
            let entries = self.read();
            if entries.first().is_none_or(|first| now - first.at <= KEEP) {
                return;
            }
        }
        let _ = crate::private_fs::update_locked::<()>(&self.path, |bytes| {
            let text = bytes.map(|b| String::from_utf8_lossy(&b).into_owned());
            let kept: Vec<&str> = text
                .as_deref()
                .unwrap_or_default()
                .lines()
                .filter(|line| {
                    serde_json::from_str::<Entry>(line).is_ok_and(|entry| now - entry.at <= KEEP)
                })
                .collect();
            let mut size = 0;
            let start = kept
                .iter()
                .rposition(|line| {
                    size += line.len() + 1;
                    size > TRIM_TO_BYTES
                })
                .map_or(0, |past| past + 1);
            let mut out = Vec::new();
            for line in &kept[start..] {
                out.extend_from_slice(line.as_bytes());
                out.push(b'\n');
            }
            Ok(out)
        });
    }
}

/// `<OS local data dir>/cuotai/channels/log.jsonl`.
pub fn default_path() -> Option<PathBuf> {
    crate::data_dir::root().map(|dir| dir.join("channels").join("log.jsonl"))
}

/// The last delivery outcome of a channel, for the list (RF5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LastDelivery {
    pub at: DateTime<Utc>,
    pub outcome: LogOutcome,
    pub status: Option<u16>,
}

/// The last `delivered` or `failed` entry of `channel`.
pub fn last_delivery(entries: &[Entry], channel: &str) -> Option<LastDelivery> {
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.channel == channel
                && matches!(entry.outcome, LogOutcome::Delivered | LogOutcome::Failed)
        })
        .map(|entry| LastDelivery {
            at: entry.at,
            outcome: entry.outcome,
            status: entry.status,
        })
}

/// Since when `channel` has failed without a success in between, and the
/// last status; `None` when its last outcome was a success (or none).
pub fn failing_since(entries: &[Entry], channel: &str) -> Option<(DateTime<Utc>, Option<u16>)> {
    let mut since = None;
    let mut status = None;
    for entry in entries.iter().filter(|entry| entry.channel == channel) {
        match entry.outcome {
            LogOutcome::Delivered => {
                since = None;
                status = None;
            }
            LogOutcome::Failed => {
                since.get_or_insert(entry.at);
                status = entry.status;
            }
            _ => {}
        }
    }
    since.map(|since| (since, status))
}

/// Whether the failing notice was already shown for the failure that
/// started at `since`.
pub fn failing_noticed(entries: &[Entry], channel: &str, since: DateTime<Utc>) -> bool {
    entries.iter().any(|entry| {
        entry.channel == channel && entry.outcome == LogOutcome::FailingNotice && entry.at >= since
    })
}
