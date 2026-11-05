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
//! Only the newest log is read, and only its first line (the session id,
//! digested at once) and its last 256 KiB. Messages, instructions, working
//! directories and tool output are never deserialized into a value, and
//! symbolic links are never followed, so no file outside the given roots
//! is opened.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
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
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let kind = value.get("type").and_then(Value::as_str);
        let payload = value.get("payload");
        match kind {
            Some("turn_context") if found.is_some() && model.is_none() => {
                model = payload
                    .and_then(|p| p.get("model"))
                    .and_then(Value::as_str)
                    .filter(|m| super::is_model_text(m))
                    .map(str::to_owned);
            }
            Some("event_msg") if found.is_none() => {
                let Some(payload) = payload else { continue };
                if payload.get("type").and_then(Value::as_str) != Some("token_count") {
                    continue;
                }
                let Some(limits) = payload.get("rate_limits").filter(|v| !v.is_null()) else {
                    continue;
                };
                let Some(at) = value
                    .get("timestamp")
                    .and_then(Value::as_str)
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
        .and_then(|line| serde_json::from_slice::<Value>(&line).ok())
        .filter(|value| value.get("type").and_then(Value::as_str) == Some("session_meta"))
        .and_then(|value| {
            value
                .get("payload")
                .and_then(|p| p.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
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

/// RF4.1: the newest reading of the Codex CLI logs below `roots` (the
/// directories of the consented `codex_cli` local source). Looks at the
/// three most recently modified logs, newest first, and returns the first
/// that carries rate limits.
pub fn latest(roots: &[PathBuf]) -> Result<Option<FeedReading>, CodexError> {
    let mut files = logs(roots);
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    for (path, _) in files.into_iter().take(3) {
        if let Some(reading) = read_log(&path)? {
            return Ok(Some(reading));
        }
    }
    Ok(None)
}
