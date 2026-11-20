//! PRD 0052 S-04: which Cuotai programs run now, so erasing all data can
//! refuse while one of them could write its files back.
//!
//! Every long-lived front end (the desktop app, `cuotai --serve`,
//! `cuotai --watch` and the interactive terminal dashboard) holds a
//! [`Presence`] for its whole life: an empty `<program>-<pid>.lock` in
//! `<OS local data>/cuotai/running/`, locked with the operating system's
//! whole-file lock ([`File::try_lock`]: `flock` on Unix, `LockFileEx` on
//! Windows), as [`crate::oauth`]'s process lock is. The folder is fixed per
//! user, like the metered records, so every front end meets there whatever
//! `CUOTAI_DATA_DIR` or the desktop's data folder says.
//!
//! [`running`] lists the folder and tries each lock: a lock it cannot take
//! belongs to a live process; a lock it can take is a crash's leftover and
//! is removed. The system releases the lock when the holder exits or
//! crashes, so a stale file never blocks anything. The files are empty: no
//! account, path or secret is written to them. Holding is best effort: a
//! folder that cannot be created logs `presence.unavailable` and never stops
//! the program.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

/// A Cuotai program that holds a presence lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Program {
    /// The desktop app (window, menu bar or system tray).
    Desktop,
    /// `cuotai --serve`.
    Serve,
    /// `cuotai --watch`.
    Watch,
    /// The interactive terminal dashboard.
    Dashboard,
}

impl Program {
    pub const ALL: [Program; 4] = [
        Program::Desktop,
        Program::Serve,
        Program::Watch,
        Program::Dashboard,
    ];

    /// The file-name prefix of its lock.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Serve => "serve",
            Self::Watch => "watch",
            Self::Dashboard => "dashboard",
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.slug() == slug)
    }
}

/// The presence of this process as one [`Program`], held until dropped.
#[derive(Debug)]
pub struct Presence {
    file: Option<File>,
    path: PathBuf,
}

impl Drop for Presence {
    fn drop(&mut self) {
        // Release the lock first, then remove the file: a check that runs in
        // between finds it unlocked and removes it itself.
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The fixed per-user folder: `<OS local data dir>/cuotai/running`.
/// Debug builds honour `CUOTAI_RUNNING_DIR` for manual smokes; release
/// builds ignore it.
pub fn default_dir() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os(DEBUG_ENV).filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    dirs::data_local_dir().map(|dir| dir.join("cuotai").join("running"))
}

#[cfg(debug_assertions)]
const DEBUG_ENV: &str = "CUOTAI_RUNNING_DIR";

/// Hold the presence of `program` in the default folder for as long as the
/// returned guard lives; `None` when it cannot (logged, never fatal).
pub fn hold(program: Program) -> Option<Presence> {
    let held = default_dir().and_then(|dir| hold_in(&dir, program));
    if held.is_none() {
        crate::diagnostics::logging::event(
            crate::diagnostics::logging::Level::Warn,
            "presence.unavailable",
            &[],
        );
    }
    held
}

/// [`hold`] in `dir` (tests inject a temporary folder).
pub fn hold_in(dir: &Path, program: Program) -> Option<Presence> {
    crate::private_fs::ensure_dir(dir).ok()?;
    let path = dir.join(format!("{}-{}.lock", program.slug(), std::process::id()));
    // A check that runs between the open and the lock may take the lock
    // first, believe the file stale and remove it: retry briefly, and make
    // sure the locked file is still the one at the path.
    for _ in 0..20 {
        let file = open(&path)?;
        match file.try_lock() {
            Ok(()) if is_regular(&path) => {
                return Some(Presence {
                    file: Some(file),
                    path,
                });
            }
            // Removed meanwhile: open a new one.
            Ok(()) => continue,
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(10)),
            // A file system without lock support.
            Err(TryLockError::Error(_)) => return None,
        }
    }
    None
}

/// Every other Cuotai program running now, sorted and once each; leftovers
/// of a crash are removed on the way. An unreadable folder lists nothing.
pub fn running() -> Vec<Program> {
    default_dir().map_or_else(Vec::new, |dir| running_in(&dir))
}

/// [`running`] over `dir` (tests inject a temporary folder).
pub fn running_in(dir: &Path) -> Vec<Program> {
    let own = std::process::id();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some((program, pid)) = name.to_str().and_then(parse_name) else {
            continue;
        };
        if pid == own {
            continue;
        }
        let path = entry.path();
        // Regular files only: a link or a folder is never opened.
        if !is_regular(&path) {
            continue;
        }
        let Some(file) = open(&path) else {
            continue;
        };
        match file.try_lock() {
            Err(TryLockError::WouldBlock) => found.push(program),
            Ok(()) => {
                // Nobody holds it: a crash's leftover.
                let _ = std::fs::remove_file(&path);
                drop(file);
            }
            Err(TryLockError::Error(_)) => {}
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Whether `name` is a presence lock file name (the erase removes only
/// those from the folder).
pub(crate) fn is_lock_name(name: &str) -> bool {
    parse_name(name).is_some()
}

/// `<program>-<digits>.lock` → the program and the process id.
fn parse_name(name: &str) -> Option<(Program, u32)> {
    let stem = name.strip_suffix(".lock")?;
    let (slug, pid) = stem.split_once('-')?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((Program::from_slug(slug)?, pid.parse().ok()?))
}

fn is_regular(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
}

/// Open (or create, `0600`) a lock file without following a final link.
fn open(path: &Path) -> Option<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_parsed_strictly() {
        assert_eq!(parse_name("serve-42.lock"), Some((Program::Serve, 42)));
        assert_eq!(
            parse_name("dashboard-7.lock"),
            Some((Program::Dashboard, 7))
        );
        for bad in [
            "serve-.lock",
            "serve-4x.lock",
            "serve-42",
            "other-42.lock",
            "serve42.lock",
            "serve--42.lock",
            ".serve-42.lock",
        ] {
            assert_eq!(parse_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_held_lock_is_reported_and_a_released_one_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        // Another process's lock, held by a second open file description
        // (two descriptions conflict for `flock`/`LockFileEx` as two
        // processes do).
        let other = dir.path().join("serve-1.lock");
        std::fs::write(&other, b"").unwrap();
        let holder = File::options().read(true).write(true).open(&other).unwrap();
        holder.lock().unwrap();
        let stale = dir.path().join("watch-2.lock");
        std::fs::write(&stale, b"").unwrap();
        assert_eq!(running_in(dir.path()), vec![Program::Serve]);
        assert!(other.exists(), "a live lock stays");
        assert!(!stale.exists(), "a crash's leftover goes");
        drop(holder);
        assert!(running_in(dir.path()).is_empty());
        assert!(!other.exists());
    }

    #[test]
    fn the_own_pid_is_never_reported_and_the_guard_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let held = hold_in(dir.path(), Program::Desktop).expect("held");
        let path = dir
            .path()
            .join(format!("desktop-{}.lock", std::process::id()));
        assert!(path.exists());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0, "empty");
        assert!(running_in(dir.path()).is_empty());
        assert!(path.exists(), "its own live lock is not removed");
        drop(held);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_and_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let running = dir.path().join("running");
        let _held = hold_in(&running, Program::Watch).expect("held");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&running), 0o700);
        let file = running.join(format!("watch-{}.lock", std::process::id()));
        assert_eq!(mode(&file), 0o600);
    }

    #[test]
    fn foreign_names_and_folders_in_running_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"keep").unwrap();
        std::fs::write(dir.path().join("other-3.lock"), b"keep").unwrap();
        std::fs::create_dir(dir.path().join("serve-4.lock")).unwrap();
        assert!(running_in(dir.path()).is_empty());
        assert!(dir.path().join("notes.txt").exists());
        assert!(dir.path().join("other-3.lock").exists());
        assert!(dir.path().join("serve-4.lock").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn links_in_running_are_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"keep").unwrap();
        let running = dir.path().join("running");
        std::fs::create_dir(&running).unwrap();
        std::os::unix::fs::symlink(&target, running.join("serve-5.lock")).unwrap();
        assert!(running_in(&running).is_empty());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        assert!(std::fs::symlink_metadata(running.join("serve-5.lock")).is_ok());
    }

    #[test]
    fn a_missing_folder_lists_nothing_and_an_unusable_one_holds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(running_in(&dir.path().join("absent")).is_empty());
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, b"").unwrap();
        assert!(hold_in(&blocker.join("running"), Program::Serve).is_none());
    }

    #[test]
    fn programs_are_sorted_and_listed_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut holders = Vec::new();
        for name in ["watch-10.lock", "serve-11.lock", "watch-12.lock"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"").unwrap();
            let file = File::options().read(true).write(true).open(&path).unwrap();
            file.lock().unwrap();
            holders.push(file);
        }
        assert_eq!(running_in(dir.path()), vec![Program::Serve, Program::Watch]);
    }
}
