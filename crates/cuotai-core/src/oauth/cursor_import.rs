//! Explicit, user-authorized import of a Cursor IDE access token from its
//! local SQLite database (`state.vscdb`). Never called during startup,
//! refresh, or any background path; the TUI wizard invokes `import` only
//! after the user explicitly confirms which Cursor profile should be used.
//!
//! Security model:
//! - Read-only SQLite access: `mode=ro`, no journal/WAL writes, no
//!   `PRAGMA journal_mode=WAL`, no `CREATE`, no log-on-error pragmas.
//! - Bounded extraction: only the keys actually needed for the import are
//!   queried (`cursorAuth/accessToken`, `cursorAuth/token`,
//!   `storage.serviceMachineId`, `storage.machineId`,
//!   `telemetry.machineId`). The whole secrets table is never read.
//! - Bounded lengths (token ≤ 64 KiB, machine id ≤ 1 KiB) and control-char
//!   rejection prevent pathological values from reaching Keychain or
//!   project params.
//! - The source file bytes are never mutated. SQLite is opened in a
//!   dedicated thread via `spawn_blocking`, with a short busy timeout
//!   and a single read transaction so the snapshot is internally
//!   consistent without blocking a live Cursor editor.
//! - The token never leaves the secure store; only the non-secret
//!   `auth_method`, `device_id`, and Cursor session `user_id` (when an
//!   unverified JWT `sub` is present and valid) are projected into metadata.

use super::{OAuthError, OAuthToken};
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

/// Maximum length of an extracted access token. Cursor tokens are
/// typically a few hundred bytes; anything above 64 KiB is not a real
/// token and indicates corruption or a misconfigured database.
const MAX_TOKEN_LEN: usize = 64 * 1024;
/// Keep import acceptance aligned with `OAuthSession::credentials`: a
/// non-refreshable Cursor token inside the refresh skew is already unusable.
const MIN_TOKEN_VALIDITY_SECS: i64 = 60;

/// Maximum length of an extracted machine id. The reference stores
/// UUID-shaped strings well under 256 bytes.
const MAX_MACHINE_LEN: usize = 1024;

/// Token keys, in priority order. The reference documentation names
/// `cursorAuth/accessToken` as primary; `cursorAuth/token` is a
/// legacy/fallback observed in some builds. Only the first match is
/// consumed.
const TOKEN_KEYS: &[&str] = &["cursorAuth/accessToken", "cursorAuth/token"];

/// Machine id keys, in priority order. The reference names
/// `storage.serviceMachineId`; the other two are observed in older or
/// Insiders builds.
const MACHINE_KEYS: &[&str] = &[
    "storage.serviceMachineId",
    "storage.machineId",
    "telemetry.machineId",
];

/// Subdirectory under which Cursor keeps its `state.vscdb`.
const STATE_SUBDIR: &str = "User/globalStorage/state.vscdb";

/// Build a profile path under the platform's config dir.
///
/// `profile` is the full directory name: `Cursor` for the stable build,
/// `Cursor - Insiders` for the Insiders build (the leading "Cursor"
/// prefix is part of the directory name itself, not a sibling folder).
fn profile_path(config_dir: &Path, profile: &str) -> PathBuf {
    config_dir.join(profile).join(STATE_SUBDIR)
}

/// Return the well-known `state.vscdb` locations for the current
/// platform, in stable-then-Insiders order. The function is metadata
/// only: it never opens, stats, or otherwise touches the filesystem,
/// so the wizard can call it freely before the user confirms a path.
///
/// `dirs::config_dir()` already resolves to the right per-OS config
/// directory: `~/Library/Application Support` on macOS, `~/.config` on
/// Linux, `%APPDATA%` on Windows. On Linux we also list the lowercase
/// `cursor` variant that some distros use.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(config) = dirs::config_dir() {
        out.push(profile_path(&config, "Cursor"));
        out.push(profile_path(&config, "Cursor - Insiders"));
    }
    out
}

/// Wire entry point invoked by `OAuthSession::import_cursor` after the
/// user has confirmed the import.
///
/// `database_path`:
/// - `Some(path)` → read only that path. The caller is expected to have
///   surfaced this to the user explicitly (e.g. via `candidates()`).
/// - `None` → scan default locations, prefer stable over Insiders; if
///   the stable file exists but holds no token, the user is told to log
///   in or pick a path explicitly (we do NOT silently fall back to an
///   Insiders session).
pub(super) async fn import(database_path: Option<PathBuf>) -> Result<OAuthToken, OAuthError> {
    // Pick the first existing candidate. The single read below decides
    // whether the file actually holds a session; we do not pre-probe
    // the SQLite file or open it twice.
    let is_stable_first = database_path.is_none();
    let chosen = match database_path {
        Some(p) => p,
        None => first_existing(&candidates())
            .await
            .ok_or_else(not_found_error)?,
    };
    tokio::task::spawn_blocking(move || read_token_from(&chosen, is_stable_first))
        .await
        .map_err(|_| join_error())?
}

async fn first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    for path in paths {
        if path_is_file(path).await {
            return Some(path.to_path_buf());
        }
    }
    None
}

async fn path_is_file(path: &Path) -> bool {
    // `tokio::fs` is not in the workspace feature set; do the stat off
    // the runtime thread so we don't add a new dependency for a single
    // metadata call.
    let owned = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::metadata(owned).map(|m| m.is_file()))
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(false)
}

fn not_found_error() -> OAuthError {
    OAuthError::Configuration(crate::text::Localized::fixed(
        "oauth_error.cursor.database_not_found",
        "Cursor database not found; open Cursor, sign in, and confirm the state.vscdb path in the wizard.",
    ))
}

fn join_error() -> OAuthError {
    OAuthError::Configuration(crate::text::Localized::fixed(
        "oauth_error.cursor.database_read_failed",
        "Could not read the Cursor database in the background; try again.",
    ))
}

fn read_token_from(path: &Path, default_was_stable: bool) -> Result<OAuthToken, OAuthError> {
    // Read-only; no journal_mode pragma, no CREATE. A short busy timeout
    // avoids a deadlocked wait when Cursor has the file open exclusively.
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.cursor.database_open_failed",
            "Could not open the Cursor database; confirm the state.vscdb path and try again.",
        ))
    })?;
    conn.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| storage_unavailable())?;
    // Single read transaction gives an internally consistent snapshot of
    // the keys we extract without acquiring write locks.
    let tx = conn
        .unchecked_transaction()
        .map_err(|_| storage_unavailable())?;
    let token_value = first_present(&tx, TOKEN_KEYS)?;
    let machine_value = first_present(&tx, MACHINE_KEYS)?;
    drop(tx);
    // Connection drops here, closing the file handle.
    drop(conn);

    let token = match token_value {
        Some(t) => t,
        // When the wizard reached us via the default-pick path and the
        // stable file held no token, refuse to silently fall back to
        // another signed-in profile the user did not pick.
        None if default_was_stable => {
            return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                "oauth_error.cursor.stable_no_session",
                "Cursor is installed but has no session; sign in to the IDE or supply the state.vscdb path explicitly.",
            )))
        }
        None => {
            return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                "oauth_error.cursor.no_access_token",
                "No access token found in Cursor; sign in to the IDE or supply an explicit state.vscdb path.",
            )))
        }
    };
    let machine = match machine_value {
        Some(m) => m,
        None => {
            return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                "oauth_error.cursor.machine_id_missing",
                "Cursor machine id is missing; sign in again to the IDE or supply a valid state.vscdb path.",
            )))
        }
    };

    let token = clean_value(&token, MAX_TOKEN_LEN)?;
    let machine = clean_value(&machine, MAX_MACHINE_LEN)?;

    if token.len() < 50 {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.cursor.token_unexpected_format",
            "Cursor token in an unexpected format; sign in again to the IDE and try again.",
        )));
    }
    // The reference service names the machine id "UUID-like" but the
    // actual stored value is whatever Cursor's serializer wrote; smoke
    // fixtures (and several builds) use a short ASCII identifier. We
    // therefore enforce only the bounded-length and control-char rules
    // already applied by `clean_value`.

    // Cursor's local access token is not refreshable by Cuotai. Match the
    // session manager's refresh skew so import cannot succeed with a token
    // that `credentials` will immediately reject as expired.
    let expires_at = parse_jwt_exp_unverified(&token);
    if expires_at.is_some_and(|expiry| {
        expiry <= Utc::now() + chrono::Duration::seconds(MIN_TOKEN_VALIDITY_SECS)
    }) {
        return Err(OAuthError::Expired);
    }

    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("auth_method".to_string(), "imported".to_string());
    metadata.insert("device_id".to_string(), machine);
    if let Some(user_id) = session_user_id_from_jwt(&token) {
        metadata.insert("user_id".to_string(), user_id);
    }
    if let Some(expiry) = expires_at {
        metadata.insert("token_expires_at".to_string(), expiry.to_rfc3339());
    }

    Ok(OAuthToken {
        access_token: token,
        refresh_token: None,
        expires_at,
        metadata,
    })
}

fn first_present(
    tx: &rusqlite::Transaction<'_>,
    keys: &[&str],
) -> Result<Option<String>, OAuthError> {
    for key in keys {
        let mut stmt = tx
            .prepare("SELECT value FROM itemTable WHERE key = ?1 LIMIT 1")
            .map_err(|_| storage_unavailable())?;
        let mut rows = stmt.query([key]).map_err(|_| storage_unavailable())?;
        if let Some(row) = rows.next().map_err(|_| storage_unavailable())? {
            let raw: String = row.get(0).map_err(|_| storage_unavailable())?;
            let cleaned = unwrap_quoted_or_raw(&raw);
            if !cleaned.is_empty() {
                return Ok(Some(cleaned));
            }
        }
    }
    Ok(None)
}

/// Cursor stores some values as either a raw string or a JSON-quoted
/// string (`"value"`). Strip one layer of JSON quoting if present.
fn unwrap_quoted_or_raw(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

fn clean_value(input: &str, max_len: usize) -> Result<String, OAuthError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.cursor.field_value_missing",
            "Field value missing in the Cursor database.",
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.cursor.field_invalid_chars",
            "Field value contains invalid characters in the Cursor database.",
        )));
    }
    if trimmed.len() > max_len {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.cursor.field_too_long",
            "Field value exceeds the expected limit in the Cursor database.",
        )));
    }
    Ok(trimmed.to_string())
}

fn storage_unavailable() -> OAuthError {
    OAuthError::Configuration(crate::text::Localized::fixed(
        "oauth_error.cursor.database_unreadable",
        "Cursor database unreadable; check the IDE is closed and the file is accessible.",
    ))
}

/// Parse the unverified `exp` claim from a JWT-shaped token. Decoding is
/// best-effort: a missing or malformed claim remains "unknown". A known
/// expiry is retained even when stale so the import boundary can reject it.
fn parse_jwt_exp_unverified(token: &str) -> Option<DateTime<Utc>> {
    let payload = jwt_payload_b64(token)?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    let exp = json.get("exp")?.as_i64()?;
    Utc.timestamp_opt(exp, 0).single()
}

/// Cursor's session-cookie user ID derived from the unverified JWT subject.
///
/// Cursor.app uses the final non-empty `|`-delimited subject component for
/// legacy Auth0 subjects, but preserves its spelling exactly. Modern subjects
/// such as `user_AbC123` therefore remain intact instead of being lowercased.
pub(crate) fn session_user_id_from_jwt(token: &str) -> Option<String> {
    let payload = jwt_payload_b64(token)?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    let sub = json.get("sub")?.as_str()?.trim();
    let user_id = sub.split('|').rfind(|part| !part.is_empty())?;
    if !is_session_user_id(user_id) {
        return None;
    }
    Some(user_id.to_string())
}

fn is_session_user_id(value: &str) -> bool {
    (1..=256).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn jwt_payload_b64(token: &str) -> Option<Vec<u8>> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let sig = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if sig.is_empty() {
        return None;
    }
    base64_url_decode(payload)
}

fn base64_url_decode(input: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.decode(input).ok()
}

#[cfg(test)]
#[path = "cursor_import_tests.rs"]
mod tests;
