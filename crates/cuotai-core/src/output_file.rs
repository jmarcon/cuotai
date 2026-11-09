//! PRD 0006 RF4.4 / PRD 0046 RF2.1: the files a person names for Cuotai to
//! write (a history export, a backup). Such a path comes from a command
//! line, a typed field or a save dialog, so it is checked before anything
//! is written:
//!
//! - it may not name a folder or a special file;
//! - it may not land inside Cuotai's own data folder or configuration
//!   folder, nor on the accounts TOML and the files beside it that share
//!   its name (a lock, a `.bak`), so an export can never replace
//!   `history.sqlite3`, its `-wal`/`-shm` side files or its `.bak` copy;
//! - the file is written to a private temporary file (`0600` on Unix) in
//!   the same folder, synced and renamed over the target. A failure leaves
//!   the previous file intact, an existing file ends `0600` whatever its
//!   mode was, and a symbolic link at the target is replaced, never
//!   followed.
//!
//! The parent folder is resolved through its symbolic links before the
//! comparison, so a link to the data folder is refused too. Errors carry no
//! path.

use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// The folders and files an output must never replace.
#[derive(Debug, Clone, Default)]
pub struct Protected {
    dirs: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Protected {
    /// Nothing protected: tests and callers that add their own paths.
    pub fn none() -> Self {
        Self::default()
    }

    /// Cuotai's folders on this computer: the data root
    /// ([`crate::data_dir::root`], `CUOTAI_DATA_DIR` included) and
    /// `<OS config dir>/cuotai`, plus the accounts TOML `accounts` in use
    /// and the `hooks.toml` beside it.
    pub fn system(accounts: Option<&Path>) -> Self {
        let mut protected = Self::none();
        if let Some(dir) = crate::data_dir::root() {
            protected = protected.dir(dir);
        }
        if let Some(dir) = dirs::config_dir() {
            protected = protected.dir(dir.join("cuotai"));
        }
        if let Some(accounts) = accounts {
            protected = protected.file(accounts);
            if let Some(dir) = accounts.parent() {
                protected = protected.file(dir.join("hooks.toml"));
            }
        }
        protected
    }

    /// Also protect everything under `dir`.
    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dirs.push(dir.into());
        self
    }

    /// Also protect `file` and the files beside it named `<file>.<more>`.
    pub fn file(mut self, file: impl Into<PathBuf>) -> Self {
        self.files.push(file.into());
        self
    }

    fn covers(&self, target: &Path) -> bool {
        let target = key(target);
        let under_dir = self
            .dirs
            .iter()
            .filter_map(|dir| resolve(dir).ok())
            .any(|dir| {
                let dir = key(&dir);
                target.len() > dir.len() && target.starts_with(&dir)
            });
        let on_file = self
            .files
            .iter()
            .filter_map(|file| resolve(file).ok())
            .any(|file| {
                let file = key(&file);
                let (Some((name, dir)), Some((target_name, target_dir))) =
                    (file.split_last(), target.split_last())
                else {
                    return false;
                };
                dir == target_dir
                    && (target_name == name
                        || target_name
                            .strip_prefix(name.as_str())
                            .is_some_and(|rest| rest.starts_with('.')))
            });
        under_dir || on_file
    }
}

/// Why an output was not written.
#[derive(Debug, PartialEq, Eq)]
pub enum WriteError<E> {
    /// The target is protected, a folder or a special file.
    Refused,
    /// The temporary file or the rename failed; the target is untouched.
    Io,
    /// The content producer failed; the target is untouched.
    Content(E),
}

/// The absolute target `path` names, its parent folder resolved through
/// symbolic links, when Cuotai may write it; [`WriteError::Refused`]
/// otherwise.
pub fn check<E>(path: &Path, protected: &Protected) -> Result<PathBuf, WriteError<E>> {
    let absolute = std::path::absolute(path).map_err(|_| WriteError::Refused)?;
    let name = match absolute.components().next_back() {
        Some(Component::Normal(name)) => name.to_os_string(),
        _ => return Err(WriteError::Refused),
    };
    let parent = absolute.parent().ok_or(WriteError::Refused)?;
    let target = resolve(parent).map_err(|_| WriteError::Refused)?.join(name);
    match std::fs::symlink_metadata(&target) {
        // A link is replaced by the rename, never followed.
        Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => {}
        Ok(_) => return Err(WriteError::Refused),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(WriteError::Refused),
    }
    if protected.covers(&target) {
        return Err(WriteError::Refused);
    }
    Ok(target)
}

/// Write what `fill` produces to `path` (see the module notes); `path`'s
/// folder must exist. Returns what `fill` returned.
pub fn write<T, E>(
    path: &Path,
    protected: &Protected,
    fill: impl FnOnce(&mut dyn Write) -> Result<T, E>,
) -> Result<T, WriteError<E>> {
    let target = check(path, protected)?;
    let dir = target.parent().ok_or(WriteError::Io)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".cuotai-output-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(|_| WriteError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| WriteError::Io)?;
    }
    let value = {
        let mut out = io::BufWriter::new(tmp.as_file_mut());
        let value = fill(&mut out).map_err(WriteError::Content)?;
        out.flush().map_err(|_| WriteError::Io)?;
        value
    };
    tmp.as_file().sync_all().map_err(|_| WriteError::Io)?;
    // The target may have become a folder while the content was produced.
    check::<E>(&target, protected)?;
    crate::private_fs::persist(tmp, &target).map_err(|_| WriteError::Io)?;
    let _ = crate::private_fs::sync_dir(dir);
    Ok(value)
}

/// `path` made absolute, with its longest existing prefix resolved through
/// symbolic links (`canonicalize`) and the missing rest appended as is; a
/// `..` in the missing rest is refused.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    loop {
        match existing.canonicalize() {
            Ok(mut base) => {
                for part in missing.iter().rev() {
                    base.push(part);
                }
                return Ok(base);
            }
            Err(_) => {
                match existing.components().next_back() {
                    Some(Component::Normal(name)) => missing.push(name.to_os_string()),
                    _ => return Err(io::Error::other("unresolvable")),
                }
                existing = existing
                    .parent()
                    .ok_or_else(|| io::Error::other("unresolvable"))?;
            }
        }
    }
}

/// The components of `path` for comparison: case-folded where the usual
/// file systems ignore case (macOS, Windows).
fn key(path: &Path) -> Vec<String> {
    path.components()
        .map(|part| {
            let text = part.as_os_str().to_string_lossy();
            if cfg!(any(target_os = "macos", windows)) {
                text.to_lowercase()
            } else {
                text.into_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    type R = Result<(), WriteError<()>>;

    fn write_text(path: &Path, protected: &Protected, text: &str) -> R {
        write(path, protected, |out| {
            out.write_all(text.as_bytes()).map_err(|_| ())
        })
    }

    #[test]
    fn refuses_cuotai_files_folders_and_special_targets() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let history = data.join("history");
        std::fs::create_dir_all(&history).unwrap();
        let toml = root.path().join("cuotai.toml");
        std::fs::write(&toml, "accounts = []\n").unwrap();
        let protected = Protected::none()
            .dir(&data)
            .dir(root.path().join("config/cuotai"))
            .file(&toml);
        for name in [
            "history.sqlite3",
            "history.sqlite3-wal",
            "history.sqlite3-shm",
            "history.sqlite3.bak",
        ] {
            assert_eq!(
                write_text(&history.join(name), &protected, "x"),
                Err(WriteError::Refused),
                "{name}"
            );
        }
        for path in [
            toml.clone(),
            root.path().join("cuotai.toml.bak"),
            root.path().join("config/cuotai/desktop.json"),
            data.join("not-yet/out.csv"),
            // A folder is never replaced.
            root.path().join("data"),
            history.join("..").join("history.sqlite3"),
        ] {
            assert_eq!(
                write_text(&path, &protected, "x"),
                Err(WriteError::Refused),
                "{}",
                path.display()
            );
        }
        assert_eq!(std::fs::read_to_string(&toml).unwrap(), "accounts = []\n");
        // Beside the TOML, other names stay writable.
        write_text(
            &root.path().join("cuotai.toml-export.csv"),
            &protected,
            "ok",
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_parent_into_a_protected_folder_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let link = root.path().join("innocent");
        std::os::unix::fs::symlink(&data, &link).unwrap();
        let protected = Protected::none().dir(&data);
        assert_eq!(
            write_text(&link.join("history.sqlite3"), &protected, "x"),
            Err(WriteError::Refused)
        );
        assert!(!data.join("history.sqlite3").exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_wide_file_ends_private_and_a_link_is_replaced_not_followed() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out.csv");
        std::fs::write(&out, "old").unwrap();
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_text(&out, &Protected::none(), "new").unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "new");
        let mode = std::fs::metadata(&out).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        let precious = root.path().join("precious.txt");
        std::fs::write(&precious, "keep").unwrap();
        let link = root.path().join("link.csv");
        std::os::unix::fs::symlink(&precious, &link).unwrap();
        write_text(&link, &Protected::none(), "export").unwrap();
        assert_eq!(std::fs::read_to_string(&precious).unwrap(), "keep");
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(meta.is_file(), "the link itself was replaced");
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "export");
    }

    #[test]
    fn a_failed_producer_leaves_the_previous_file_and_no_temporary() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out.json");
        std::fs::write(&out, "previous").unwrap();
        let result: Result<(), WriteError<&str>> = write(&out, &Protected::none(), |out| {
            out.write_all(b"partial").unwrap();
            Err("boom")
        });
        assert_eq!(result, Err(WriteError::Content("boom")));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "previous");
        let names: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("out.json")]);
    }

    #[test]
    fn a_relative_path_resolves_against_the_working_directory() {
        let target: Result<PathBuf, WriteError<()>> =
            check(Path::new("out.csv"), &Protected::none());
        let target = target.unwrap();
        assert!(target.is_absolute());
        assert_eq!(target.file_name().unwrap(), "out.csv");
    }
}
