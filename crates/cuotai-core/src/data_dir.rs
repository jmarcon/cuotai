//! PRD 0051 RF5.2/RF5.3: the one place every local store resolves its
//! directory. The root is `CUOTAI_DATA_DIR` when that is set (and not
//! empty), else the desktop app's own choice (`desktop.json` `data_dir`,
//! set with [`set_app_override`] before any store opens; the CLI and TUI
//! never read it), else `<OS local data dir>/cuotai`. An override must be an
//! absolute path to a private directory of the current user: it is
//! created as `0700`, a symbolic link, a file or another user's directory
//! is refused, and an invalid override never falls back to the OS default
//! (the stores are then unavailable and [`check_override`] says why).
//! The history, model cache, reset marks, cost index and settings, prices,
//! hook consent/log/work folders, extensions, serve token, last-config
//! records, vault index, metered records, canary records, update-check
//! cache and the desktop widget file (on Windows and Linux; on macOS it
//! stays at `widget_snapshot::MACOS_PATH`) all live under it.

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
    /// The desktop app's chosen folder is relative or unusable.
    Setting,
}

/// Which rule chose the data folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataDirSource {
    Environment,
    Desktop,
    Default,
}

/// The desktop app's chosen folder for this process (PRD 0051 RF5.2).
static APP_OVERRIDE: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// PRD 0051 RF5.2: the data folder the desktop app's settings chose, for
/// this process; below `CUOTAI_DATA_DIR`, above the OS default. Call it at
/// start, before any store opens. `None` clears it.
pub fn set_app_override(path: Option<PathBuf>) {
    *APP_OVERRIDE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = path;
}

fn app_override() -> Option<PathBuf> {
    APP_OVERRIDE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Pure precedence: `env` (when not empty), then `app`, then nothing.
fn resolve(
    env: Option<&std::ffi::OsStr>,
    app: Option<&Path>,
) -> Result<Option<(PathBuf, DataDirSource)>, DataDirError> {
    if let Some(value) = env.filter(|value| !value.is_empty()) {
        return open(Path::new(value)).map(|dir| Some((dir, DataDirSource::Environment)));
    }
    if let Some(app) = app {
        return open(app)
            .map(|dir| Some((dir, DataDirSource::Desktop)))
            .map_err(|_| DataDirError::Setting);
    }
    Ok(None)
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
            Self::Setting => Localized::fixed(
                "data_dir.error.setting",
                "The data folder chosen in Settings must be an absolute path to a directory of your own that is not a link; Cuotai keeps no data until it is fixed or reset.",
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

/// The override in effect (`CUOTAI_DATA_DIR`, else the desktop app's
/// choice): `Ok(None)` when there is none.
pub fn check_override() -> Result<Option<PathBuf>, DataDirError> {
    let env = std::env::var_os(ENV);
    resolve(env.as_deref(), app_override().as_deref()).map(|found| found.map(|(dir, _)| dir))
}

/// Which rule chose the data folder now (an invalid override still names
/// its rule).
pub fn source() -> DataDirSource {
    if std::env::var_os(ENV).is_some_and(|value| !value.is_empty()) {
        DataDirSource::Environment
    } else if app_override().is_some() {
        DataDirSource::Desktop
    } else {
        DataDirSource::Default
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
    fn the_environment_wins_over_the_desktop_choice_and_never_falls_back() {
        let root = tempfile::tempdir().unwrap();
        let env = root.path().join("env");
        let app = root.path().join("app");
        assert_eq!(
            resolve(Some(env.as_os_str()), Some(&app)).unwrap(),
            Some((env.clone(), DataDirSource::Environment))
        );
        assert_eq!(
            resolve(Some(std::ffi::OsStr::new("")), Some(&app)).unwrap(),
            Some((app.clone(), DataDirSource::Desktop)),
            "an empty variable is unset"
        );
        assert_eq!(resolve(None, None).unwrap(), None);
        assert_eq!(
            resolve(None, Some(Path::new("relative"))),
            Err(DataDirError::Setting)
        );
        assert_eq!(
            resolve(Some(std::ffi::OsStr::new("relative")), Some(&app)),
            Err(DataDirError::Relative),
            "an invalid variable does not fall back to the desktop choice"
        );
    }

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
