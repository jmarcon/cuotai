//! PRD 0051 RF5.2/RF5.3: the one place every local store resolves its
//! directory. The root is `CUOTAI_DATA_DIR` when that is set (and not
//! empty), else `<OS local data dir>/cuotai`. An override must be an
//! absolute path to a private directory of the current user: it is
//! created as `0700`, a symbolic link, a file or another user's directory
//! is refused, and an invalid override never falls back to the OS default
//! (the stores are then unavailable and [`check_override`] says why).
//! The history, model cache, reset marks, cost index and settings, prices,
//! hook consent/log/work folders, extensions, serve token, last-config
//! records, vault index, metered records, canary records, update-check
//! cache and the desktop widget fallback all live under it.

use std::path::{Path, PathBuf};

use crate::text::Localized;

/// The environment variable that moves every local store.
pub const ENV: &str = "CUOTAI_DATA_DIR";

/// Why a `CUOTAI_DATA_DIR` value cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataDirError {
    /// A relative path: its meaning would change with the working directory.
    Relative,
    /// Not a directory this user owns privately (a link, a file, another
    /// user's directory), or it could not be created.
    Unusable,
}

impl DataDirError {
    pub fn l10n(&self) -> Localized {
        match self {
            Self::Relative => Localized::fixed(
                "data_dir.error.relative",
                "CUOTAI_DATA_DIR must be an absolute path; Cuotai keeps no data until it is fixed or unset.",
            ),
            Self::Unusable => Localized::fixed(
                "data_dir.error.unusable",
                "CUOTAI_DATA_DIR must be a directory of your own that is not a link; Cuotai keeps no data until it is fixed or unset.",
            ),
        }
    }
}

/// Validate `value` as a data-directory override and make it private.
fn open(value: &Path) -> Result<PathBuf, DataDirError> {
    if !value.is_absolute() {
        return Err(DataDirError::Relative);
    }
    crate::private_fs::ensure_dir(value).map_err(|_| DataDirError::Unusable)?;
    Ok(value.to_path_buf())
}

/// The override of `CUOTAI_DATA_DIR`: `Ok(None)` when unset or empty.
pub fn check_override() -> Result<Option<PathBuf>, DataDirError> {
    match std::env::var_os(ENV).filter(|value| !value.is_empty()) {
        Some(value) => open(Path::new(&value)).map(Some),
        None => Ok(None),
    }
}

/// The root of every local store: the valid override, else
/// `<OS local data dir>/cuotai`. `None` when the override is invalid or
/// the platform has no data directory.
pub fn root() -> Option<PathBuf> {
    match check_override() {
        Ok(Some(dir)) => Some(dir),
        Ok(None) => dirs::data_local_dir().map(|dir| dir.join("cuotai")),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_override_is_refused() {
        assert_eq!(open(Path::new("relative/dir")), Err(DataDirError::Relative));
    }

    #[test]
    fn an_absolute_override_is_created_private() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("profile").join("data");
        assert_eq!(open(&dir).unwrap(), dir);
        assert!(dir.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        let file = root.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(open(&file), Err(DataDirError::Unusable));
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_override_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(open(&link), Err(DataDirError::Unusable));
    }
}
