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
    /// Advisories of a valid file, never a reason to fail it (PRD 0036
    /// re-validation: an aggregator whose password or key travels over
    /// plain HTTP on the private network). Left out of the JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ConfigDiagnostic>,
}

/// The stable kind of a [`ConfigError`].
pub fn error_kind(error: &ConfigError) -> &'static str {
    match error {
        ConfigError::Parse { .. } => "parse",
        ConfigError::Bounds => "bounds",
        ConfigError::MeteredRefreshFloor { .. } => "metered_refresh_floor",
        // V-05 split one error in two; the stable kind stays the same.
        ConfigError::AccountNameRequired { .. } | ConfigError::AccountNameRepeated { .. } => {
            "account_names"
        }
        ConfigError::AccountNameCharacters => "account_name_characters",
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
        ConfigError::Breakdown { .. } => "breakdown",
        ConfigError::ReservedName { .. } => "reserved_name",
        ConfigError::Feed => "feed",
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
        | ConfigError::AccountIds { field, .. }
        | ConfigError::AccountNameRequired { field, .. }
        | ConfigError::AccountNameRepeated { field, .. }
        | ConfigError::Budget { field, .. }
        | ConfigError::Breakdown { field }
        | ConfigError::ReservedName { field } => (None, None, Some(field.clone())),
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
        warnings: Vec::new(),
    }
}

/// Check the bytes of a configuration file strictly.
pub fn check_bytes(bytes: &[u8]) -> ConfigCheck {
    check_bytes_at(bytes, None)
}

/// [`check_bytes`] for the file whose identity is `identity`, so the ids
/// computed for accounts without an `id` line are the ones a reader of that
/// file derives (PRD 0029 F3).
fn check_bytes_at(bytes: &[u8], identity: Option<&super::ConfigIdentity>) -> ConfigCheck {
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
    let result = parse(text, ConfigParseMode::Strict).and_then(|parsed| {
        parsed.config.check_ids(identity)?;
        parsed.config.validate()?;
        Ok(parsed.config)
    });
    match result {
        Ok(config) => ConfigCheck {
            valid: true,
            diagnostics: Vec::new(),
            warnings: warnings(&config),
        },
        Err(error) => failed(diagnostic(&error)),
    }
}

/// The advisories of a valid configuration: every aggregator account whose
/// password or key would travel over plain HTTP on the private network
/// (`allow_private_http`), with HTTPS and a pinned certificate offered.
fn warnings(config: &crate::Config) -> Vec<ConfigDiagnostic> {
    config
        .accounts
        .iter()
        .enumerate()
        .filter(|(_, account)| crate::providers::private_http_in_effect(account))
        .map(|(index, _)| {
            let path = format!("accounts[{index}].params.allow_private_http");
            ConfigDiagnostic {
                kind: "private_http",
                line: None,
                column: None,
                note: Localized::with_param(
                    "config.warning.private_http",
                    "path",
                    path.clone(),
                    "Warning: {path} sends the account's password or key unencrypted over plain HTTP on your private network. Prefer HTTPS with a pinned certificate.",
                ),
                path: Some(path),
            }
        })
        .collect()
}

/// Check the configuration file at `path` strictly. A missing or unreadable
/// file is a diagnostic too: `--check-config` needs an existing file.
pub fn check(path: &Path) -> ConfigCheck {
    match std::fs::read(path) {
        Ok(bytes) => check_bytes_at(&bytes, super::ConfigIdentity::of_path(path).ok().as_ref()),
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

    /// PRD 0036 re-validation S-06: a valid file still warns when an
    /// aggregator's password or key would travel over plain HTTP on the
    /// private network; JSON keeps `warnings` out when there are none.
    #[test]
    fn a_secret_over_private_http_is_a_warning_of_a_valid_file() {
        let check = check_bytes(
            b"[[accounts]]\nname = \"Home\"\nprovider = \"nine_router\"\n[accounts.params]\nbase_url = \"http://192.168.1.10:20128\"\nallow_private_http = \"1\"\n\n[[accounts]]\nname = \"Proxy\"\nprovider = \"litellm\"\ncredential_env = \"LITELLM_API_KEY\"\n[accounts.params]\nbase_url = \"http://10.0.0.7:4000\"\nallow_private_http = \"true\"\n\n[[accounts]]\nname = \"Local\"\nprovider = \"nine_router\"\n",
        );
        assert!(check.valid, "{:?}", check.diagnostics);
        let paths: Vec<_> = check
            .warnings
            .iter()
            .map(|warning| (warning.kind, warning.path.as_deref().unwrap_or_default()))
            .collect();
        assert_eq!(
            paths,
            [
                ("private_http", "accounts[0].params.allow_private_http"),
                ("private_http", "accounts[1].params.allow_private_http"),
            ]
        );
        let text = check.warnings[0].note.resolve("en-US");
        assert!(
            text.contains("unencrypted") && text.contains("HTTPS"),
            "{text}"
        );
        assert!(!text.contains("192.168"), "{text}");
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["warnings"][0]["kind"], "private_http");
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
