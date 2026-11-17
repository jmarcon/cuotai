//! RF4.1: Codex CLI's rate-limit windows from its own session logs.
//!
//! Codex CLI writes `rollout-<time>-<id>.jsonl` under `<home>/sessions/`
//! (`<home>` is `CODEX_HOME` or `~/.codex`, the directories PRD 0015 already
//! discloses for local cost). An `event_msg` line whose `payload.type` is
//! `token_count` carries `payload.rate_limits` with `primary` and
//! `secondary` windows of `{ used_percent, window_minutes, resets_at }`
//! (`RateLimitSnapshot` and `RateLimitWindow` in
//! `codex-rs/protocol/src/protocol.rs` of openai/codex, verified
//! 2026-11-05 at commit 86ac091; older releases wrote `resets_in_seconds`
//! instead of `resets_at`, also accepted). The shape is not a published
//! API: a `rate_limits` object this reader does not recognise is refused
//! with a schema error, never read as zero.
//!
//! Only the logs modified within the last day are read, newest first and
//! at most [`MAX_LOGS`], and of each only its first line (the session id,
//! digested at once) and its last 256 KiB. Each line is parsed into a typed
//! struct that keeps only the fields read here, so messages, instructions,
//! working directories and tool output are never deserialized into a value;
//! and symbolic links are never followed, so no file outside the given
//! roots is opened.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;

use super::{session_digest, FeedReading, FeedSession, FeedSource, FeedWindow, WindowId, SCHEMA};

/// The evidence the access descriptor cites (PRD 0028).
pub const EVIDENCE: &str = "https://github.com/openai/codex/blob/86ac0916597244a2e67a197822a5483924660ff1/codex-rs/protocol/src/protocol.rs";

/// How much of the end of a log is read.
const TAIL: u64 = 256 * 1024;
/// How deep below a root logs are looked for (`YYYY/MM/DD/file`).
const MAX_DEPTH: usize = 4;

/// Why the logs gave no reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexError {
    /// A `rate_limits` object of a shape this reader does not know.
    UnknownShape,
}

/// Every `rollout-*.jsonl` below `roots`, without following links.
fn logs(roots: &[PathBuf]) -> Vec<(PathBuf, std::time::SystemTime)> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, std::time::SystemTime)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() && depth < MAX_DEPTH {
                walk(&path, depth + 1, out);
            } else if kind.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("rollout-") && name.ends_with(".jsonl") {
                    if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                        out.push((path, modified));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for root in roots {
        // A root that is itself a link is not followed either.
        if std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
            walk(root, 0, &mut out);
        }
    }
    out
}

/// The first line of `path` (at most 64 KiB).
fn first_line(path: &Path) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = vec![0; 64 * 1024];
    let read = file.read(&mut buffer).ok()?;
    buffer.truncate(read);
    let end = buffer
        .iter()
        .position(|b| *b == b'\n')
        .unwrap_or(buffer.len());
    buffer.truncate(end);
    Some(buffer)
}

/// The last [`TAIL`] bytes of `path`, from the first whole line.
fn tail(path: &Path) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;
    if start > 0 {
        let skip = buffer
            .iter()
            .position(|b| *b == b'\n')
            .map_or(buffer.len(), |i| i + 1);
        buffer.drain(..skip);
    }
    Some(buffer)
}

/// One window of a `rate_limits` object; `Err` for a shape this reader
/// does not know.
fn window(
    value: Option<&Value>,
    id: WindowId,
    at: DateTime<Utc>,
) -> Result<Option<FeedWindow>, CodexError> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let object = value.as_object().ok_or(CodexError::UnknownShape)?;
    let used = object
        .get("used_percent")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
        .ok_or(CodexError::UnknownShape)?;
    let resets_at = match (object.get("resets_at"), object.get("resets_in_seconds")) {
        (Some(absolute), _) if !absolute.is_null() => {
            Some(absolute.as_i64().ok_or(CodexError::UnknownShape)?)
        }
        (_, Some(relative)) if !relative.is_null() => {
            let seconds = relative.as_i64().ok_or(CodexError::UnknownShape)?;
            Some(at.timestamp().saturating_add(seconds))
        }
        _ => None,
    };
    let window_minutes = match object.get("window_minutes") {
        Some(minutes) if !minutes.is_null() => Some(
            minutes
                .as_u64()
                .and_then(|m| u32::try_from(m).ok())
                .filter(|m| (1..=525_600).contains(m))
                .ok_or(CodexError::UnknownShape)?,
        ),
        _ => None,
    };
    Ok(Some(FeedWindow {
        id,
        used_pct: used,
        resets_at,
        used_usd: None,
        limit_usd: None,
        period: None,
        window_minutes,
    }))
}

/// A string field, kept only when it is a string; any other value is
/// skipped without being read into memory (S-03).
#[derive(Debug, Default)]
struct Text(Option<String>);

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Skip;
        impl<'de> Visitor<'de> for Skip {
            type Value = Text;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_str<E>(self, value: &str) -> Result<Text, E> {
                Ok(Text(Some(value.to_owned())))
            }
            fn visit_bool<E>(self, _: bool) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_i64<E>(self, _: i64) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_u64<E>(self, _: u64) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_f64<E>(self, _: f64) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_unit<E>(self) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Text, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Text(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Text, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Text(None))
            }
        }
        deserializer.deserialize_any(Skip)
    }
}

impl Text {
    fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

/// One log line, typed (L-2/S-03): only `type`, `timestamp` and the four
/// payload fields this reader uses are kept. Every other field (messages,
/// instructions, working directories, tool output) is skipped by serde
/// without being deserialized.
#[derive(Debug, Deserialize)]
struct LogLine {
    #[serde(default, rename = "type")]
    kind: Text,
    #[serde(default)]
    timestamp: Text,
    #[serde(default)]
    payload: Option<Payload>,
}

#[derive(Debug, Default, Deserialize)]
struct Payload {
    #[serde(default, rename = "type")]
    kind: Text,
    /// `turn_context`: the model id.
    #[serde(default)]
    model: Text,
    /// `session_meta`: the session id, digested at once.
    #[serde(default)]
    id: Text,
    /// `token_count`: the rate-limit windows (numbers only); its shape is
    /// checked by [`window`].
    #[serde(default)]
    rate_limits: Option<Value>,
}

/// The newest token-count event with rate limits in one log.
fn read_log(path: &Path) -> Result<Option<FeedReading>, CodexError> {
    let Some(bytes) = tail(path) else {
        return Ok(None);
    };
    let mut model: Option<String> = None;
    let mut found: Option<(DateTime<Utc>, Vec<FeedWindow>)> = None;
    for line in bytes.split(|b| *b == b'\n').rev() {
        if line.is_empty() || line[0] != b'{' {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<LogLine>(line) else {
            continue;
        };
        let payload = value.payload.as_ref();
        match value.kind.as_deref() {
            Some("turn_context") if found.is_some() && model.is_none() => {
                model = payload
                    .and_then(|p| p.model.as_deref())
                    .filter(|m| super::is_model_text(m))
                    .map(str::to_owned);
            }
            Some("event_msg") if found.is_none() => {
                let Some(payload) = payload else { continue };
                if payload.kind.as_deref() != Some("token_count") {
                    continue;
                }
                let Some(limits) = payload.rate_limits.as_ref().filter(|v| !v.is_null()) else {
                    continue;
                };
                let Some(at) = value
                    .timestamp
                    .as_deref()
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.with_timezone(&Utc))
                else {
                    continue;
                };
                if !limits.is_object() {
                    return Err(CodexError::UnknownShape);
                }
                let windows: Vec<FeedWindow> = [
                    window(limits.get("primary"), WindowId::Primary, at)?,
                    window(limits.get("secondary"), WindowId::Secondary, at)?,
                ]
                .into_iter()
                .flatten()
                .collect();
                if windows.is_empty()
                    && !(limits.get("primary").is_some() || limits.get("secondary").is_some())
                {
                    // Neither key at all: not the shape we verified.
                    return Err(CodexError::UnknownShape);
                }
                found = Some((at, windows));
            }
            _ => {}
        }
        if found.is_some() && model.is_some() {
            break;
        }
    }
    let Some((at, windows)) = found else {
        return Ok(None);
    };
    let session_id = first_line(path)
        .and_then(|line| serde_json::from_slice::<LogLine>(&line).ok())
        .filter(|value| value.kind.as_deref() == Some("session_meta"))
        .and_then(|value| value.payload.and_then(|p| p.id.0))
        .unwrap_or_else(|| {
            // Without a session id, the file name stands for the session.
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    Ok(Some(FeedReading {
        schema: SCHEMA,
        source: FeedSource::CodexCli,
        written_at: at,
        agent_version: None,
        session: FeedSession {
            digest: session_digest(&session_id),
            model_id: model.clone(),
            model_name: model,
            ..FeedSession::default()
        },
        windows,
    }))
}

/// How many of the most recently modified logs are looked at.
pub const MAX_LOGS: usize = 16;

/// RF4.1 (PRD 0040 re-validation V-02): the readings of the Codex CLI logs
/// below `roots` (the directories of the consented `codex_cli` local
/// source), newest first, for [`super::AccountReading::merge`]. Looks at
/// the logs modified within the feed retention (24 hours of `now`), newest
/// first and at most [`MAX_LOGS`], keeps each one's newest rate-limit event
/// that meets the feed contract at `now`, and stops once both windows are
/// known with their resets ahead. A log without a `token_count` event (a
/// session that has not reported its limits) is skipped, never taken as
/// "no limits".
pub fn readings(roots: &[PathBuf], now: DateTime<Utc>) -> Result<Vec<FeedReading>, CodexError> {
    let cutoff: std::time::SystemTime = (now - super::RETENTION).into();
    let mut files = logs(roots);
    files.retain(|(_, modified)| *modified >= cutoff);
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let mut found = Vec::new();
    let mut primary = false;
    let mut secondary = false;
    for (path, _) in files.into_iter().take(MAX_LOGS) {
        let Some(reading) = read_log(&path)?.filter(|reading| reading.is_valid(now)) else {
            continue;
        };
        let live = |id: WindowId| {
            reading
                .window(id)
                .is_some_and(|window| window.resets_at().is_none_or(|at| at > now))
        };
        primary |= live(WindowId::Primary);
        secondary |= live(WindowId::Secondary);
        found.push(reading);
        if primary && secondary {
            break;
        }
    }
    found.sort_by_key(|reading| std::cmp::Reverse(reading.written_at));
    Ok(found)
}
