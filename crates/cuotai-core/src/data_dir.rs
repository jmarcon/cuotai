//! PRD 0051 RF5.2/RF5.3: the one place every local store resolves its
//! directory. The root is `CUOTAI_DATA_DIR` when that is set (and not
//! empty), else the desktop app's own choice (`desktop.json` `data_dir`,
//! set with [`set_app_override`] before any store opens; the CLI and TUI
//! never store into it), else `<OS local data dir>/cuotai`.
//!
//! An override goes through [`plan`] (PRD 0051 re-validation S-01/S-02):
//! a path that does not exist is created (`0700`) and used as is; a folder
//! that holds the ownership marker ([`MARKER`]) or Cuotai's own files is
//! used as is and never narrowed again; any other folder keeps Cuotai's
//! files in a `Cuotai` folder inside it ([`SUBFOLDER`]), so Cuotai never
//! deletes, narrows or writes beside files it did not create. A relative
//! path, a symbolic link, a file or another user's directory is refused,
//! and an invalid override never falls back to the OS default (the stores
//! are then unavailable and [`check_override`] says why). Deleting code
//! asks [`owned`] first: only the OS default and a marked folder qualify.
//! The history, model cache, reset marks, cost index and settings, prices,
//! hook consent/log/work folders, extensions, serve token, last-config
//! records, vault index, canary records, update-check (the metered records
//! stay in one fixed per-user place: `metered_record::Store::default_dir`)
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
    /// The folder already holds a `Cuotai` folder that Cuotai did not
    /// create (not empty, no marker, none of Cuotai's files).
    Occupied,
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

/// The file that marks a folder as Cuotai's own data folder: Cuotai
/// deletes or narrows only in a folder that holds it (or the OS default).
pub const MARKER: &str = ".cuotai-data";
/// The folder Cuotai keeps its files in inside a chosen folder that is not
/// already Cuotai's.
pub const SUBFOLDER: &str = "Cuotai";
/// The marker's first line, which identifies it.
const MARKER_HEAD: &str =
    "Cuotai data folder (PRD 0051). Cuotai deletes files only in folders that hold this file.";
/// A marker larger than this is not Cuotai's.
const MARKER_MAX_BYTES: u64 = 4096;

/// Files whose presence (as regular files, never links) shows that a
/// folder is a data folder an older Cuotai (0.83.0 to 0.87.x) used before
/// the marker existed. Only distinctive names: generic ones (`widget.json`,
/// `serve/token`, the `feed` or `canary` folders) do not count.
pub(crate) const RECOGNISED: [&str; 8] = [
    "logs/cuotai.log",
    "history/history.sqlite3",
    "models/models.sqlite3",
    "cost/cost.sqlite3",
    "last-config-tui.json",
    "last-config-desktop.json",
    "vault-index.json",
    "notified.json",
];

/// How [`plan`] recognised the folder it chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// It does not exist yet (or is an empty `Cuotai` folder): Cuotai
    /// creates it, marks it `origin=created` and keeps it `0700`.
    Created,
    /// It holds the marker: used as is.
    Marked,
    /// An older Cuotai's folder without a marker: used as is, marked
    /// `origin=adopted`, never narrowed.
    Adopted,
}

/// The folder a data-folder value resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub dir: PathBuf,
    pub origin: Origin,
}

/// What a path holds, for [`plan`].
enum Found {
    Missing,
    Marked,
    Recognised,
    Other { empty: bool },
}

/// Inspect `dir` without changing anything (links are never followed).
fn inspect(dir: &Path) -> Result<Found, DataDirError> {
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Found::Missing),
        Err(_) => return Err(DataDirError::Unusable),
    };
    if !meta.is_dir() || crate::private_fs::foreign_owner(&meta) {
        return Err(DataDirError::Unusable);
    }
    if marker_origin(dir).is_some() {
        return Ok(Found::Marked);
    }
    let regular = |rel: &str| {
        let path = rel
            .split('/')
            .fold(dir.to_path_buf(), |p, part| p.join(part));
        std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
    };
    if RECOGNISED.iter().any(|rel| regular(rel)) {
        return Ok(Found::Recognised);
    }
    let mut entries = std::fs::read_dir(dir).map_err(|_| DataDirError::Unusable)?;
    Ok(Found::Other {
        empty: entries.next().is_none(),
    })
}

/// PRD 0051 re-validation S-01: where a data-folder value (`CUOTAI_DATA_DIR`
/// or the desktop's choice) keeps Cuotai's files. Reads metadata only;
/// [`open`] applies it.
///
/// 1. A relative path is refused.
/// 2. A path that does not exist is used as is (Cuotai creates it).
/// 3. A folder with the marker, or 4. with an older Cuotai's files, is
///    used as is.
/// 5. Anything else (an empty folder included) gives `<value>/Cuotai`,
///    under rules 2 to 4; a not-empty, unmarked, unrecognised
///    `<value>/Cuotai` is [`DataDirError::Occupied`], and an empty one is
///    taken. `value` itself is never created, narrowed or written.
pub fn plan(value: &Path) -> Result<Planned, DataDirError> {
    if !value.is_absolute() {
        return Err(DataDirError::Relative);
    }
    let planned = |dir: &Path, origin| Planned {
        dir: dir.to_path_buf(),
        origin,
    };
    match inspect(value)? {
        Found::Missing => Ok(planned(value, Origin::Created)),
        Found::Marked => Ok(planned(value, Origin::Marked)),
        Found::Recognised => Ok(planned(value, Origin::Adopted)),
        Found::Other { .. } => {
            let sub = value.join(SUBFOLDER);
            match inspect(&sub)? {
                Found::Missing | Found::Other { empty: true } => Ok(planned(&sub, Origin::Created)),
                Found::Marked => Ok(planned(&sub, Origin::Marked)),
                Found::Recognised => Ok(planned(&sub, Origin::Adopted)),
                Found::Other { empty: false } => Err(DataDirError::Occupied),
            }
        }
    }
}

/// The marker's `origin` (`created` or `adopted`) when `dir` holds a valid
/// marker: a regular file (not a link), small, starting with its header.
fn marker_origin(dir: &Path) -> Option<String> {
    use std::io::Read;
    let path = dir.join(MARKER);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MARKER_MAX_BYTES {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(&path)
        .ok()?
        .take(MARKER_MAX_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    let mut lines = text.lines();
    if lines.next() != Some(MARKER_HEAD) {
        return None;
    }
    Some(
        lines
            .find_map(|line| line.strip_prefix("origin="))
            .unwrap_or("adopted")
            .to_string(),
    )
}

/// Write the marker into `dir` (`0600`, atomically).
fn write_marker(dir: &Path, origin: &str) -> std::io::Result<()> {
    crate::hooks::write_atomically(
        &dir.join(MARKER),
        format!("{MARKER_HEAD}\norigin={origin}\n").as_bytes(),
    )
}

/// Test seam for other crates: mark `dir` as a Cuotai data folder Cuotai
/// created, as [`open`] does.
#[doc(hidden)]
pub fn mark_for_tests(dir: &Path) -> std::io::Result<()> {
    write_marker(dir, "created")
}

/// Whether `dir` is a data folder Cuotai adopted from an older build
/// (marker `origin=adopted`): [`crate::private_fs::ensure_dir`] never
/// narrows it.
pub(crate) fn adopted(dir: &Path) -> bool {
    marker_origin(dir).as_deref() == Some("adopted")
}

/// `<OS local data dir>/cuotai`, Cuotai's by name.
pub fn default_root() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("cuotai"))
}

/// Whether Cuotai may delete or narrow inside `root`: it is the OS
/// default folder, or it holds a valid marker.
pub fn owned(root: &Path) -> bool {
    default_root().as_deref() == Some(root) || marker_origin(root).is_some()
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
            .map_err(|error| match error {
                DataDirError::Occupied => DataDirError::Occupied,
                _ => DataDirError::Setting,
            });
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
            Self::Occupied => Localized::fixed(
                "data_dir.error.occupied",
                "The chosen data folder already holds a folder named Cuotai that Cuotai did not create; choose another folder. Cuotai keeps no data until it is fixed.",
            ),
        }
    }
}

/// Resolve `value` with [`plan`] and prepare the folder: one Cuotai creates
/// is made private (`0700`) and marked; a marked or adopted one is only
/// checked (a real directory of this user, never narrowed) and an adopted
/// one is marked.
fn open(value: &Path) -> Result<PathBuf, DataDirError> {
    let planned = plan(value)?;
    let dir = planned.dir;
    let narrow = match planned.origin {
        Origin::Created => true,
        Origin::Marked => marker_origin(&dir).as_deref() == Some("created"),
        Origin::Adopted => false,
    };
    if narrow {
        crate::private_fs::ensure_dir(&dir).map_err(|_| DataDirError::Unusable)?;
    } else {
        crate::private_fs::check_dir(&dir).map_err(|_| DataDirError::Unusable)?;
    }
    match planned.origin {
        Origin::Marked => {}
        Origin::Created => write_marker(&dir, "created").map_err(|_| DataDirError::Unusable)?,
        Origin::Adopted => write_marker(&dir, "adopted").map_err(|_| DataDirError::Unusable)?,
    }
    Ok(dir)
}

/// The last resolution, keyed by the variable and the desktop choice it
/// came from, so the many [`root`] calls do not repeat [`plan`]'s walk.
type Memo = (
    Option<std::ffi::OsString>,
    Option<PathBuf>,
    Result<Option<PathBuf>, DataDirError>,
);
static MEMO: std::sync::Mutex<Option<Memo>> = std::sync::Mutex::new(None);

/// The override in effect (`CUOTAI_DATA_DIR`, else the desktop app's
/// choice), as [`plan`] resolves it: `Ok(None)` when there is none.
pub fn check_override() -> Result<Option<PathBuf>, DataDirError> {
    let env = std::env::var_os(ENV).filter(|value| !value.is_empty());
    let app = app_override();
    let mut memo = MEMO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((key_env, key_app, result)) = memo.as_ref() {
        if *key_env == env && *key_app == app {
            return result.clone();
        }
    }
    let result = resolve(env.as_deref(), app.as_deref()).map(|found| found.map(|(dir, _)| dir));
    *memo = Some((env, app, result.clone()));
    result
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

/// The root of every local store: the valid override as [`plan`] resolved
/// it (always [`owned`]), else `<OS local data dir>/cuotai`. `None` when
/// the override is invalid or the platform has no data directory.
pub fn root() -> Option<PathBuf> {
    match check_override() {
        Ok(Some(dir)) => Some(dir),
        Ok(None) => default_root(),
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
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// S-01: a path that does not exist yet is Cuotai's: created `0700`,
    /// marked `origin=created` and used as is.
    #[test]
    fn a_missing_path_is_created_marked_and_used_as_is() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("new-profile");
        assert_eq!(
            plan(&dir).unwrap(),
            Planned {
                dir: dir.clone(),
                origin: Origin::Created
            }
        );
        assert_eq!(open(&dir).unwrap(), dir);
        assert_eq!(marker_origin(&dir).as_deref(), Some("created"));
        assert!(owned(&dir));
        #[cfg(unix)]
        {
            assert_eq!(mode(&dir), 0o700);
            assert_eq!(mode(&dir.join(MARKER)), 0o600);
        }
    }

    /// S-01/S-02: an existing folder that is not Cuotai's (empty, or with
    /// someone else's files) keeps Cuotai's files in `<folder>/Cuotai`; the
    /// folder, its files and its mode stay as they were.
    #[test]
    fn an_existing_folder_that_is_not_cuotais_gets_a_cuotai_subfolder() {
        let root = tempfile::tempdir().unwrap();
        let empty = root.path().join("empty");
        let busy = root.path().join("busy");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(busy.join("feed")).unwrap();
        std::fs::write(busy.join("feed").join("notes.txt"), b"mine").unwrap();
        std::fs::write(busy.join("widget.json"), b"generic name").unwrap();
        #[cfg(unix)]
        for dir in [&empty, &busy] {
            set_mode(dir, 0o755);
        }
        for dir in [&empty, &busy] {
            let planned = plan(dir).unwrap();
            assert_eq!(planned.dir, dir.join(SUBFOLDER));
            assert_eq!(planned.origin, Origin::Created);
            assert_eq!(open(dir).unwrap(), dir.join(SUBFOLDER));
            assert!(
                !dir.join(MARKER).exists(),
                "the chosen folder is not marked"
            );
            assert!(owned(&dir.join(SUBFOLDER)) && !owned(dir));
            #[cfg(unix)]
            {
                assert_eq!(mode(dir), 0o755, "the chosen folder is not narrowed");
                assert_eq!(mode(&dir.join(SUBFOLDER)), 0o700);
            }
        }
        assert_eq!(
            std::fs::read(busy.join("feed").join("notes.txt")).unwrap(),
            b"mine"
        );
    }

    /// S-02: an older Cuotai's folder (any one of the recognised files) is
    /// used as is, marked `origin=adopted` and never narrowed, also by a
    /// store that writes into it later.
    #[test]
    fn an_older_cuotai_folder_is_adopted_as_is_and_never_narrowed() {
        for rel in RECOGNISED {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("old");
            let file = rel.split('/').fold(dir.clone(), |p, part| p.join(part));
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"x").unwrap();
            #[cfg(unix)]
            set_mode(&dir, 0o755);
            assert_eq!(
                plan(&dir).unwrap(),
                Planned {
                    dir: dir.clone(),
                    origin: Origin::Adopted
                },
                "{rel}"
            );
            assert_eq!(open(&dir).unwrap(), dir);
            assert_eq!(marker_origin(&dir).as_deref(), Some("adopted"));
            assert_eq!(plan(&dir).unwrap().origin, Origin::Marked);
            crate::private_fs::write_file(&dir.join("update-check.json"), b"{}").unwrap();
            assert_eq!(open(&dir).unwrap(), dir);
            #[cfg(unix)]
            assert_eq!(mode(&dir), 0o755, "{rel}: never narrowed");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_recognised_name_that_is_a_link_does_not_count() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("linked");
        std::fs::create_dir_all(&dir).unwrap();
        let elsewhere = root.path().join("notified.json");
        std::fs::write(&elsewhere, b"{}").unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join("notified.json")).unwrap();
        assert_eq!(plan(&dir).unwrap().dir, dir.join(SUBFOLDER));
        // A linked marker does not count either.
        let marked = root.path().join("marked");
        mark_for_tests(&marked).unwrap();
        let fake = root.path().join("fake");
        std::fs::create_dir_all(&fake).unwrap();
        std::os::unix::fs::symlink(marked.join(MARKER), fake.join(MARKER)).unwrap();
        assert!(!owned(&fake));
        assert_eq!(plan(&fake).unwrap().dir, fake.join(SUBFOLDER));
    }

    #[test]
    fn a_marked_folder_is_used_as_is() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("marked");
        mark_for_tests(&dir).unwrap();
        std::fs::write(dir.join("someone-else.txt"), b"x").unwrap();
        assert_eq!(
            plan(&dir).unwrap(),
            Planned {
                dir: dir.clone(),
                origin: Origin::Marked
            }
        );
        assert_eq!(open(&dir).unwrap(), dir);
        // A file named like the marker without its header is not one.
        let other = root.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join(MARKER), b"not ours").unwrap();
        assert!(!owned(&other));
    }

    #[test]
    fn a_foreign_cuotai_subfolder_is_occupied_and_an_empty_one_is_taken() {
        let root = tempfile::tempdir().unwrap();
        let occupied = root.path().join("occupied");
        std::fs::create_dir_all(occupied.join(SUBFOLDER)).unwrap();
        std::fs::write(occupied.join(SUBFOLDER).join("theirs.txt"), b"x").unwrap();
        assert_eq!(plan(&occupied), Err(DataDirError::Occupied));
        assert_eq!(open(&occupied), Err(DataDirError::Occupied));
        assert!(!occupied.join(SUBFOLDER).join(MARKER).exists());
        assert_eq!(
            resolve(None, Some(&occupied)),
            Err(DataDirError::Occupied),
            "the desktop choice says why too"
        );

        let empty = root.path().join("empty-sub");
        std::fs::create_dir_all(empty.join(SUBFOLDER)).unwrap();
        std::fs::write(empty.join("theirs.txt"), b"x").unwrap();
        assert_eq!(open(&empty).unwrap(), empty.join(SUBFOLDER));
        assert!(owned(&empty.join(SUBFOLDER)));
    }

    #[test]
    fn planning_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let cases = [
            root.path().join("missing"),
            root.path().join("empty"),
            root.path().join("busy"),
            root.path().join("old"),
        ];
        std::fs::create_dir_all(&cases[1]).unwrap();
        std::fs::create_dir_all(&cases[2]).unwrap();
        std::fs::write(cases[2].join("theirs.txt"), b"x").unwrap();
        std::fs::create_dir_all(cases[3].join("logs")).unwrap();
        std::fs::write(cases[3].join("logs").join("cuotai.log"), b"x").unwrap();
        for value in &cases {
            let first = plan(value).unwrap().dir;
            assert_eq!(plan(&first).unwrap().dir, first, "before use");
            let used = open(value).unwrap();
            assert_eq!(used, first);
            assert_eq!(plan(&used).unwrap().dir, used, "after use");
            assert_eq!(open(value).unwrap(), used, "a second start agrees");
        }
    }

    /// The recognised names are the privacy registry's own data-root files,
    /// so the list cannot drift from what Cuotai writes.
    #[test]
    fn every_recognised_name_is_a_local_data_file_of_the_registry() {
        for rel in RECOGNISED {
            assert!(
                crate::privacy::artifacts()
                    .iter()
                    .any(|a| a.root == crate::privacy::Root::LocalData
                        && a.kind == crate::privacy::Kind::File
                        && a.path == rel),
                "{rel}"
            );
        }
        assert!(crate::privacy::artifact("data.marker")
            .is_some_and(|a| a.path == MARKER && a.root == crate::privacy::Root::LocalData));
    }

    /// Whatever the override, the root it resolves to is owned: Cuotai
    /// deletes only in folders it can prove are its own.
    #[test]
    fn an_override_root_always_carries_the_marker() {
        let root = tempfile::tempdir().unwrap();
        let busy = root.path().join("busy");
        std::fs::create_dir_all(&busy).unwrap();
        std::fs::write(busy.join("theirs.txt"), b"x").unwrap();
        for value in [root.path().join("missing"), busy] {
            let (dir, _) = resolve(Some(value.as_os_str()), None).unwrap().unwrap();
            assert!(owned(&dir), "{}", dir.display());
            let (dir, _) = resolve(None, Some(&value)).unwrap().unwrap();
            assert!(owned(&dir), "{}", dir.display());
        }
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
