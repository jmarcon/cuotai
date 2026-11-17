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

/// RF1.3: the largest count one field of one request may hold, a thousand
/// times the largest context window. A line above it is not a real request:
/// it is skipped and counted as invalid, so it can never overflow the
/// index's sums.
pub(crate) const MAX_TOKENS_PER_FIELD: u64 = 1_000_000_000;

impl Line {
    /// `Usage(entry)`, or `Invalid` when a count of `entry` is impossible
    /// (above [`MAX_TOKENS_PER_FIELD`]). Every adapter builds usage here.
    pub(crate) fn usage(entry: Entry) -> Self {
        let t = &entry.tokens;
        let counts = [
            t.input,
            t.output,
            t.cache_read,
            t.cache_write,
            t.cache_write_1h,
        ];
        if counts.iter().any(|&count| count > MAX_TOKENS_PER_FIELD) {
            Line::Invalid
        } else {
            Line::Usage(entry)
        }
    }
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
    /// PRD 0042 RF1.1: the one-way fingerprint of the project folder
    /// ([`project_digest`]); `None` when the log does not name it.
    pub project: Option<String>,
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
    /// PRD 0042 RF1.1: the project fingerprint in effect (a digest, never
    /// the path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// PRD 0042 RF4.3: the last folder name of the project just seen, for
    /// the opt-in label hint. Never serialized, so it never reaches the
    /// `files` table; the scanner takes it after each line and drops it
    /// unless the source keeps folder names.
    #[serde(skip)]
    pub folder: Option<String>,
    /// S-03: the key of this installation's project fingerprints, set by
    /// the scanner before each read. Never serialized.
    #[serde(skip)]
    pub key: Option<ProjectKey>,
    /// PRD 0042 S-06: the unkeyed session digest sealed with the key
    /// ([`ProjectKey::seal`]), so the request keys that include it stay
    /// the ones stored before the session digests were keyed. Unreadable
    /// without the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seal: Option<String>,
}

impl FileState {
    /// PRD 0042 S-06: record the session whose unkeyed digest is `plain`:
    /// `session` holds it keyed to this computer, `seal` the sealed
    /// original.
    pub(crate) fn set_session(&mut self, plain: String) {
        match &self.key {
            Some(key) => {
                let keyed = key.keyed(&plain);
                self.seal = Some(key.seal(&plain, &keyed));
                self.session = Some(keyed);
            }
            None => {
                self.seal = None;
                self.session = Some(plain);
            }
        }
    }

    /// The unkeyed digest of the session in effect (empty without one),
    /// as the request keys of Codex CLI and Gemini CLI include it.
    pub(crate) fn plain_session(&self) -> String {
        let Some(session) = &self.session else {
            return String::new();
        };
        match (&self.key, &self.seal) {
            (Some(key), Some(seal)) => key.unseal(seal, session).unwrap_or_else(|| session.clone()),
            _ => session.clone(),
        }
    }

    /// The key of a request the tool may write twice: [`anonymous`],
    /// keyed to this computer.
    pub(crate) fn request_key(&self, source: SourceId, value: &str) -> String {
        keyed_anonymous(self.key.as_ref(), source, value)
    }
}

/// S-03: the per-installation key of the project fingerprints
/// (`cost/fingerprint.key`). A fingerprint is
/// `HMAC-SHA256(key, sha256(normalized folder)[..16])[..16]`, so a guessed
/// folder path cannot be confirmed from it without the key. PRD 0042 S-06:
/// the same key turns the session, request and log-file digests into
/// `HMAC-SHA256(key, digest)` ([`ProjectKey::keyed`]), so a known session id,
/// message id or log path cannot be matched either.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProjectKey(std::sync::Arc<[u8; 32]>);

impl std::fmt::Debug for ProjectKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProjectKey(..)")
    }
}

impl ProjectKey {
    pub(crate) fn new(bytes: [u8; 32]) -> Self {
        Self(std::sync::Arc::new(bytes))
    }

    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    fn mac(&self, parts: &[&[u8]]) -> [u8; 32] {
        use hmac::{Hmac, Mac};
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(self.0.as_slice())
            .expect("HMAC takes a key of any length");
        for part in parts {
            mac.update(part);
        }
        mac.finalize().into_bytes().into()
    }

    /// The keyed fingerprint of an unkeyed one (`legacy`, 16 hex).
    pub(crate) fn fingerprint(&self, legacy: &str) -> String {
        let mut text = hex(&self.mac(&[legacy.as_bytes()]));
        text.truncate(16);
        text
    }

    /// PRD 0042 S-06: a session, request or log-file digest keyed to this
    /// computer: `HMAC-SHA256(key, unkeyed)` in full (64 hex). Empty stays
    /// empty (usage without a session).
    pub(crate) fn keyed(&self, unkeyed: &str) -> String {
        if unkeyed.is_empty() {
            return String::new();
        }
        hex(&self.mac(&[unkeyed.as_bytes()]))
    }

    /// The pad that seals the unkeyed session digest behind `keyed`: unique
    /// per session, unreadable without the key.
    fn pad(&self, keyed: &str) -> [u8; 32] {
        self.mac(&[b"cuotai session seal\0", keyed.as_bytes()])
    }

    /// PRD 0042 S-06: `plain` (a 64-hex digest) XORed with a pad derived
    /// from its keyed form, as hex. [`Self::unseal`] reverses it with the
    /// key; without the key it reveals nothing of `plain`.
    pub(crate) fn seal(&self, plain: &str, keyed: &str) -> String {
        let pad = self.pad(keyed);
        let bytes: Vec<u8> = unhex(plain)
            .unwrap_or_default()
            .iter()
            .zip(pad.iter().cycle())
            .map(|(b, p)| b ^ p)
            .collect();
        hex(&bytes)
    }

    /// The unkeyed digest [`Self::seal`] sealed, when `seal` belongs to
    /// `keyed` under this key.
    pub(crate) fn unseal(&self, seal: &str, keyed: &str) -> Option<String> {
        let pad = self.pad(keyed);
        let bytes: Vec<u8> = unhex(seal)?
            .iter()
            .zip(pad.iter().cycle())
            .map(|(b, p)| b ^ p)
            .collect();
        let plain = hex(&bytes);
        (self.keyed(&plain) == keyed).then_some(plain)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// [`project_digest`] keyed with `key` (unkeyed only without one, which
/// only the adapters' unit tests do).
pub(crate) fn keyed_project(key: Option<&ProjectKey>, cwd: &str) -> Option<String> {
    let legacy = project_digest(cwd)?;
    Some(key.map_or(legacy.clone(), |key| key.fingerprint(&legacy)))
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
/// The index stores it keyed ([`keyed_anonymous`]).
pub(crate) fn anonymous(source: SourceId, value: &str) -> String {
    digest(format!("{}\0{value}", source.slug()).as_bytes())
}

/// PRD 0042 S-06: [`anonymous`] keyed with `key` (unkeyed only without
/// one, which only the adapters' unit tests do), as [`keyed_project`].
pub(crate) fn keyed_anonymous(key: Option<&ProjectKey>, source: SourceId, value: &str) -> String {
    let plain = anonymous(source, value);
    key.map_or(plain.clone(), |key| key.keyed(&plain))
}

/// PRD 0042 RF1.1: the project fingerprint of a working directory: the
/// SHA-256 of the normalized path (separators as `/`, no trailing one),
/// truncated to 16 hex characters. Not salted by source, so the same folder
/// is one project for every tool. The path itself is dropped by the caller.
pub(crate) fn project_digest(cwd: &str) -> Option<String> {
    let normalized = normalized_dir(cwd)?;
    let mut full = digest(normalized.as_bytes());
    full.truncate(16);
    Some(full)
}

fn normalized_dir(cwd: &str) -> Option<String> {
    let unified = cwd.trim().replace('\\', "/");
    let trimmed = unified.trim_end_matches('/');
    let normalized = if trimmed.is_empty() && unified.starts_with('/') {
        "/"
    } else {
        trimmed
    };
    (!normalized.is_empty()).then(|| normalized.to_owned())
}

/// PRD 0042 RF4.3: the last segment of a working directory (at most 64
/// characters, control characters dropped), offered only to prefill a label
/// when the person turned the folder-name hint on.
pub(crate) fn folder_name(cwd: &str) -> Option<String> {
    let normalized = normalized_dir(cwd)?;
    let last = normalized.rsplit('/').next().unwrap_or_default();
    // S-05: no control, direction or other invisible character.
    let clean: String = last
        .chars()
        .filter(|c| !crate::text::is_hidden(*c))
        .take(64)
        .collect();
    (!clean.trim().is_empty()).then_some(clean)
}

/// PRD 0042 RF1.1: record `cwd` in `state` as a fingerprint (and the
/// transient folder name); the path is not kept.
pub(crate) fn note_project(state: &mut FileState, cwd: Option<&str>) {
    if let Some(project) = cwd.and_then(|cwd| keyed_project(state.key.as_ref(), cwd)) {
        state.project = Some(project);
        state.folder = cwd.and_then(folder_name);
    }
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
