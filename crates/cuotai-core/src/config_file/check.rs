//! PRD 0051 RF2.3/RF2.4: `cuotai --check-config`, the strict, offline check
//! of a configuration file. It reads the file, runs the strict parser and
//! the semantic validation, and reports redacted diagnostics: a kind, a
//! position and a key path, never a value. Nothing here touches the
//! network, the vault, the model cache or the update check.

use std::path::Path;

use serde::Serialize;

use super::{parse, position, ConfigParseMode};
use crate::edit_error::config_error_note;
use crate::text::Localized;
use crate::ConfigError;

/// One problem found by [`check`].
#[derive(Debug, Clone, Serialize)]
pub struct ConfigDiagnostic {
    /// Stable machine-readable class: `not_found`, `unreadable`,
    /// `not_utf8`, `parse`, or the semantic rule (`bounds`,
    /// `account_names`, `metered_refresh_floor`, ...).
    pub kind: &'static str,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub path: Option<String>,
    /// The localized sentence for human output; not part of the JSON.
    #[serde(skip)]
    pub note: Localized,
}

/// The result of [`check`]: `{ "valid": bool, "diagnostics": [...] }`.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigCheck {
    pub valid: bool,
    pub diagnostics: Vec<ConfigDiagnostic>,
}

/// The stable kind of a [`ConfigError`].
pub fn error_kind(error: &ConfigError) -> &'static str {
    match error {
        ConfigError::Parse { .. } => "parse",
        ConfigError::Bounds => "bounds",
        ConfigError::MeteredRefreshFloor { .. } => "metered_refresh_floor",
        ConfigError::AccountNames => "account_names",
        ConfigError::AccountIds { .. } => "account_ids",
        ConfigError::Client => "client",
        ConfigError::OAuth => "oauth",
        ConfigError::Session => "session",
        ConfigError::Extension => "extension",
        ConfigError::Region => "region",
        ConfigError::BaseUrl => "base_url",
        ConfigError::Tls => "tls",
        ConfigError::Group => "group",
        ConfigError::OpenRouter => "open_router",
        ConfigError::Budget { .. } => "budget",
    }
}

fn diagnostic(error: &ConfigError) -> ConfigDiagnostic {
    let (line, column, path) = match error {
        ConfigError::Parse { line, column, path } => (
            Some(*line),
            Some(*column),
            path.as_ref().map(ToString::to_string),
        ),
        ConfigError::MeteredRefreshFloor { field, .. }
        | ConfigError::AccountIds { field }
        | ConfigError::Budget { field, .. } => (None, None, Some(field.clone())),
        _ => (None, None, None),
    };
    ConfigDiagnostic {
        kind: error_kind(error),
        line,
        column,
        path,
        note: config_error_note(error),
    }
}

fn failed(diagnostic: ConfigDiagnostic) -> ConfigCheck {
    ConfigCheck {
        valid: false,
        diagnostics: vec![diagnostic],
    }
}

/// Check the bytes of a configuration file strictly.
pub fn check_bytes(bytes: &[u8]) -> ConfigCheck {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let (line, column) = position(
                &String::from_utf8_lossy(&bytes[..error.valid_up_to()]),
                usize::MAX,
            );
            let mut found = diagnostic(&ConfigError::Parse {
                line,
                column,
                path: None,
            });
            found.kind = "not_utf8";
            return failed(found);
        }
    };
    let result = parse(text, ConfigParseMode::Strict).and_then(|parsed| parsed.config.validate());
    match result {
        Ok(()) => ConfigCheck {
            valid: true,
            diagnostics: Vec::new(),
        },
        Err(error) => failed(diagnostic(&error)),
    }
}

/// Check the configuration file at `path` strictly. A missing or unreadable
/// file is a diagnostic too: `--check-config` needs an existing file.
pub fn check(path: &Path) -> ConfigCheck {
    match std::fs::read(path) {
        Ok(bytes) => check_bytes(&bytes),
        Err(error) => {
            let not_found = error.kind() == std::io::ErrorKind::NotFound;
            failed(ConfigDiagnostic {
                kind: if not_found { "not_found" } else { "unreadable" },
                line: None,
                column: None,
                path: None,
                note: if not_found {
                    Localized::with_param(
                        "config.error.not_found",
                        "path",
                        crate::text::home_relative(path),
                        "There is no configuration file at {path}.",
                    )
                } else {
                    Localized::with_param(
                        "config.error.unreadable",
                        "path",
                        crate::text::home_relative(path),
                        "The configuration file at {path} could not be read.",
                    )
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_file_has_no_diagnostics() {
        let check = check_bytes(b"[[accounts]]\nprovider = \"groq\"\ncredential_env = \"K\"\n");
        assert!(check.valid);
        assert_eq!(
            serde_json::to_value(&check).unwrap(),
            serde_json::json!({ "valid": true, "diagnostics": [] })
        );
    }

    #[test]
    fn each_kind_is_reported_without_the_value() {
        let cases: [(&[u8], &str, Option<&str>); 5] = [
            (b"accounts = [\n", "parse", None),
            (
                b"[[accounts]]\nprovider = \"groq\"\ncredential_env = \"K\"\nsecret_hint = \"sk-live-123\"\n",
                "parse",
                Some("accounts[0].secret_hint"),
            ),
            (
                b"[[accounts]]\nprovider = \"sk-live-123\"\n",
                "parse",
                Some("accounts[0].provider"),
            ),
            (b"refresh_seconds = 5\naccounts = []\n", "bounds", None),
            (b"accounts = []\n\xff\n", "not_utf8", None),
        ];
        for (bytes, kind, path) in cases {
            let check = check_bytes(bytes);
            assert!(!check.valid);
            let found = &check.diagnostics[0];
            assert_eq!(found.kind, kind);
            assert_eq!(found.path.as_deref(), path);
            let json = serde_json::to_string(&check).unwrap();
            assert!(!json.contains("sk-live"), "{json}");
            assert!(
                !json.contains("fallback") && !json.contains("message"),
                "{json}"
            );
            assert!(!found.note.resolve("en-US").contains("sk-live"));
        }
    }

    #[test]
    fn a_missing_file_is_a_diagnostic() {
        let dir = tempfile::tempdir().unwrap();
        let check = check(&dir.path().join("absent.toml"));
        assert!(!check.valid);
        assert_eq!(check.diagnostics[0].kind, "not_found");
    }
}
