//! Typed edit/validation errors shared between `config_file` and
//! `account_edit`. The variants let frontends distinguish conflict (the
//! file changed on disk and the in-memory snapshot is stale) from
//! validation problems (the user typed something the schema refuses)
//! and from I/O failures (disk full, permission, broken symlink).
//!
//! RF8: every error also carries a [`Localized`] note — a stable id plus
//! structured parameters, with the Portuguese message as the fallback —
//! so a frontend translates via its catalog (`edit_error.duplicate_name`,
//! `edit_error.conflict`, …) instead of matching on the wording.
//!
//! Messages are kept as fixed strings or path-only diagnostics. The
//! error type intentionally does **not** carry the user-typed draft,
//! raw token material, or TOML body — those are secret-bearing or
//! schema-confusing and the RFC explicitly forbids echoing them.

use crate::text::{params, Localized, ParamValue};
use std::path::PathBuf;

/// Typed error returned by `ConfigFile` mutators and `build_account`.
///
/// Variants are mutually exclusive so a UI can branch on the cause
/// without parsing text. The `Display` impl produces the same fixed
/// Portuguese message in every locale; frontends that translate use
/// [`EditError::l10n`] and never the string.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// The proposed configuration violates a schema rule. `field`
    /// points at the offending schema field name when known (and is
    /// `None` for document-wide rules); `note` carries the stable id,
    /// parameters and the Portuguese fallback describing the cause.
    #[error("{note}")]
    Validation {
        field: Option<&'static str>,
        note: Localized,
    },

    /// The file on disk changed since the last successful load or save
    /// (external edit, unexpected creation, deletion, or rename). The
    /// in-memory `ConfigFile` is unchanged and the caller may `reload`
    /// before retrying.
    #[error("arquivo {path} mudou desde o carregamento; recarregue antes de salvar.")]
    Conflict { path: PathBuf },

    /// Filesystem failure (read, write, rename, fsync, permission,
    /// missing parent directory, broken symlink). The `path` is the
    /// file the operation was attempting to read or write; the cause
    /// is the raw I/O error.
    #[error("falha em {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The combination of fields the caller asked for is not supported
    /// (e.g. AWS Bedrock with both an OAuth credential and an
    /// env-var credential, or Alibaba Token Plan with an explicit
    /// env-var). Distinct so a UI can tell the user "this provider
    /// does not accept that combination" instead of treating it as
    /// a regular validation error.
    #[error("{note}")]
    Unsupported { note: Localized },
}

impl EditError {
    /// Convenience predicate so a UI can branch on the conflict variant
    /// without matching the full enum.
    pub fn is_conflict(&self) -> bool {
        matches!(self, EditError::Conflict { .. })
    }

    /// Test-only helper to assert the validation variant. Most
    /// production callers only need `is_conflict`; the rest are
    /// behind `cfg(test)` to keep the public surface minimal.
    #[cfg(test)]
    pub fn is_validation(&self) -> bool {
        matches!(self, EditError::Validation { .. })
    }

    /// Test-only helper to assert the I/O variant.
    #[cfg(test)]
    pub fn is_io(&self) -> bool {
        matches!(self, EditError::Io { .. })
    }

    /// Convenience: which schema field the error names, when known.
    pub fn field(&self) -> Option<&'static str> {
        match self {
            EditError::Validation { field, .. } => *field,
            _ => None,
        }
    }

    /// RF8 structured form of the error: stable id + parameters +
    /// Portuguese fallback. Never carries user input, token material,
    /// or TOML body — only the `path` (for I/O and conflict) and fixed
    /// descriptions.
    pub fn l10n(&self) -> Localized {
        match self {
            EditError::Validation { note, .. } | EditError::Unsupported { note } => note.clone(),
            EditError::Conflict { path } => Localized::with_param(
                "edit_error.conflict",
                "path",
                path.display().to_string(),
                self.to_string(),
            ),
            EditError::Io { path, source } => Localized::with_params(
                "edit_error.io",
                params([
                    ("path", ParamValue::from(path.display().to_string())),
                    ("detail", ParamValue::from(source.to_string())),
                ]),
                self.to_string(),
            ),
        }
    }

    /// Convert to a stable user-facing message (the Portuguese fallback).
    /// The string never carries user input, token material, or TOML body.
    pub fn message(&self) -> String {
        self.to_string()
    }
}

/// Build a `Validation` error with a static id and message. Avoids the
/// [`Localized`] boilerplate at fixed-text call sites.
pub(crate) fn validation(
    field: Option<&'static str>,
    id: &'static str,
    message: &'static str,
) -> EditError {
    EditError::Validation {
        field,
        note: Localized::fixed(id, message),
    }
}

/// Build a `Validation` error from an already-structured [`Localized`]
/// note (ids and parameters produced by the `account_edit` helpers).
pub fn validation_note(field: Option<&'static str>, note: Localized) -> EditError {
    EditError::Validation { field, note }
}

/// Build an `Unsupported` error with a static id and message. Reserved
/// for callers that need to emit the variant; not yet reached by any
/// production code path.
#[allow(dead_code)]
pub(crate) fn unsupported(id: &'static str, message: &'static str) -> EditError {
    EditError::Unsupported {
        note: Localized::fixed(id, message),
    }
}

/// Shared "index out of range" validation error. The message carries
/// only the derived index and the account count — never user text.
pub fn index_out_of_range(index: usize, count: usize) -> EditError {
    EditError::Validation {
        field: None,
        note: Localized::with_params(
            "edit_error.index_out_of_range",
            params([
                ("index", ParamValue::from(index)),
                ("count", ParamValue::from(count)),
            ]),
            format!("Índice {index} fora de alcance ({count})."),
        ),
    }
}

/// Map a [`crate::ConfigError`] to its structured note. Single source for
/// the ids used by both `ConfigFile::save` and `Config::upsert_account`
/// when a document-wide rule fails.
pub fn config_error_note(error: &crate::ConfigError) -> Localized {
    let id = match error {
        crate::ConfigError::Parse => "edit_error.config_parse",
        crate::ConfigError::Bounds => "edit_error.config_bounds",
        crate::ConfigError::AccountNames => "edit_error.config_account_names",
        crate::ConfigError::Client => "edit_error.config_client",
        crate::ConfigError::OAuth => "edit_error.config_oauth",
        crate::ConfigError::Session => "edit_error.config_session",
    };
    Localized::fixed(id, error.to_string())
}

/// Convenience: validate that a numeric field fits in the TOML i64
/// range. Used by `ConfigFile::save` to surface its overflow guard as
/// a typed `EditError::Validation` rather than a bare string.
pub(crate) fn toml_int_in_range(label: &'static str, value: u64) -> Result<(), EditError> {
    if value > i64::MAX as u64 {
        return Err(EditError::Validation {
            field: Some(label),
            note: Localized::with_param(
                "edit_error.toml_int_range",
                "label",
                label,
                "valores numericos grandes demais para TOML",
            ),
        });
    }
    Ok(())
}

/// Lossless conversion used by callers that bubble typed errors
/// out of a function returning `Result<_, String>`. Prefer the
/// typed form for new code; this exists so desktop IPC and TUI
/// helpers can use `?` without an explicit `.map_err`.
impl From<EditError> for String {
    fn from(err: EditError) -> String {
        err.message()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_match_variants() {
        let conflict = EditError::Conflict {
            path: PathBuf::from("/tmp/cuotai.toml"),
        };
        assert!(conflict.is_conflict());

        let validation = validation(
            Some("refresh_seconds"),
            "edit_error.interval_range",
            "intervalo abaixo do minimo",
        );
        assert!(validation.is_validation());
        assert_eq!(validation.field(), Some("refresh_seconds"));

        let io = EditError::Io {
            path: PathBuf::from("/tmp/cuotai.toml"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "negado"),
        };
        assert!(io.is_io());
    }

    #[test]
    fn l10n_exposes_stable_ids_and_params_without_user_input() {
        let err = index_out_of_range(7, 3);
        let note = err.l10n();
        assert_eq!(note.id, "edit_error.index_out_of_range");
        assert_eq!(
            note.params.get("index"),
            Some(&ParamValue::Text("7".into()))
        );
        assert_eq!(note.fallback, "Índice 7 fora de alcance (3).");

        let conflict = EditError::Conflict {
            path: PathBuf::from("/tmp/cuotai.toml"),
        };
        let note = conflict.l10n();
        assert_eq!(note.id, "edit_error.conflict");
        assert_eq!(
            note.params.get("path"),
            Some(&ParamValue::Text("/tmp/cuotai.toml".into()))
        );

        let io = EditError::Io {
            path: PathBuf::from("/tmp/cuotai.toml"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "negado"),
        };
        assert_eq!(io.l10n().id, "edit_error.io");
    }

    #[test]
    fn config_error_note_covers_every_variant() {
        for (error, id) in [
            (crate::ConfigError::Parse, "edit_error.config_parse"),
            (crate::ConfigError::Bounds, "edit_error.config_bounds"),
            (
                crate::ConfigError::AccountNames,
                "edit_error.config_account_names",
            ),
            (crate::ConfigError::Client, "edit_error.config_client"),
            (crate::ConfigError::OAuth, "edit_error.config_oauth"),
            (crate::ConfigError::Session, "edit_error.config_session"),
        ] {
            let note = config_error_note(&error);
            assert_eq!(note.id, id);
            assert_eq!(note.fallback, error.to_string());
        }
    }
}
