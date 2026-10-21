//! RF1.2/RF1.3: the adapter every source implements, and what a parser may
//! return. A parser sees one line at a time and keeps only numbers, the
//! time, the model and digests: any other field of the line is skipped by
//! the deserializer without being stored.

use super::{SourceId, Tokens};
use crate::models::digest;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where a source's logs live and how one line becomes numbers.
pub(crate) trait Source: Send + Sync {
    fn id(&self) -> SourceId;

    /// The directories read when the user did not choose others, from the
    /// tool's documented environment variables or the home directory.
    fn default_roots(
        &self,
        home: Option<&Path>,
        var: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<PathBuf>;

    /// The file pattern of the tool's session logs, as shown to the user.
    fn pattern(&self) -> &'static str;

    /// Whether the file named `name` (under one of the roots, at any depth)
    /// matches [`Self::pattern`].
    fn is_log(&self, name: &str) -> bool;

    /// The usage on one line of a log, given what earlier lines of the same
    /// file established (`state`, kept by the index between scans).
    fn parse(&self, line: &[u8], state: &mut FileState) -> Line;
}

/// What one line holds for the index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Line {
    Usage(Entry),
    /// A valid line without usage (a prompt, a tool call, metadata).
    Skip,
    /// Not a line of the known format: skipped and counted.
    Invalid,
}

/// One request's usage: the only values a parser hands on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub at: DateTime<Utc>,
    pub model: String,
    pub tokens: Tokens,
    /// Digest of the tool's session identifier (never the identifier).
    pub session: String,
    /// Digest that identifies a request the tool may write twice (Claude
    /// Code copies earlier messages into a resumed session's file).
    pub dedup: Option<String>,
}

/// What a parser carries from one line of a file to the next, stored per
/// file between scans: the model in effect, the session digest and the
/// last cumulative counts (Codex CLI). Only those values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// The model name recorded for usage whose model the log did not name.
pub(crate) const UNKNOWN_MODEL: &str = "unknown";

/// A model name as logs write it (`claude-sonnet-4-5-20250929`,
/// `gpt-5-codex`): 1 to 96 characters of letters, digits and `.-_:/@+`.
/// Anything else is not a model name and makes the line invalid, so free
/// text can never reach the index through this field.
pub(crate) fn model_name(value: &str) -> Option<String> {
    let valid = (1..=96).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:/@+".contains(&b));
    valid.then(|| value.to_owned())
}

/// Whether a (trimmed) line is a JSON object, as every log line is; serde
/// would otherwise accept an array for a struct.
pub(crate) fn is_object(line: &[u8]) -> bool {
    line.first() == Some(&b'{')
}

/// An RFC 3339 time.
pub(crate) fn instant(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// The anonymous identifier of `value` for `source`: a SHA-256 digest, so
/// the index can count sessions without holding the tool's identifiers.
pub(crate) fn anonymous(source: SourceId, value: &str) -> String {
    digest(format!("{}\0{value}", source.slug()).as_bytes())
}

/// `<home>/<parts…>`, when the home directory is known.
pub(crate) fn under_home(home: Option<&Path>, parts: &[&str]) -> Option<PathBuf> {
    home.map(|home| {
        parts
            .iter()
            .fold(home.to_path_buf(), |path, part| path.join(part))
    })
}

/// The comma-separated directories of an environment variable (as the
/// tools accept them), with `~/` expanded; `None` when unset or empty.
pub(crate) fn listed_dirs(
    var: &dyn Fn(&str) -> Option<String>,
    name: &str,
    home: Option<&Path>,
) -> Option<Vec<PathBuf>> {
    let value = var(name).filter(|value| !value.trim().is_empty())?;
    let dirs: Vec<PathBuf> = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter_map(|part| expand_home(part, home))
        .collect();
    (!dirs.is_empty()).then_some(dirs)
}

/// `~` or `~/…` under `home`; other paths as written. `None` for a `~`
/// path without a known home.
pub(crate) fn expand_home(path: &str, home: Option<&Path>) -> Option<PathBuf> {
    match path.strip_prefix('~') {
        Some("") => home.map(Path::to_path_buf),
        Some(rest) if rest.starts_with(['/', '\\']) => {
            home.map(|home| home.join(rest.trim_start_matches(['/', '\\'])))
        }
        _ => Some(PathBuf::from(path)),
    }
}
