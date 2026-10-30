//! Finds the provider CLIs (`bl`, `aws`) the adapters run.
//!
//! An app opened from the Finder or a desktop launcher does not inherit the
//! shell's `PATH` (on macOS it gets `/usr/bin:/bin:/usr/sbin:/sbin`), so a CLI
//! a terminal finds would read as not installed. [`resolve`] looks through
//! `PATH` first and then through the usual install directories; a name found
//! nowhere comes back as it is, so the spawn reports its own `NotFound`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The program to spawn for `name`: `name` itself when `PATH` has it or when
/// it already is a path, otherwise the first match in the install directories.
pub(crate) fn resolve(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    resolve_in(name, &path, &install_dirs())
}

/// Whether [`resolve`] finds `name`.
pub(crate) fn found(name: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    found_in(name, &path, &install_dirs())
}

fn resolve_in(name: &str, path: &OsStr, extra: &[PathBuf]) -> PathBuf {
    if Path::new(name).components().count() > 1 || in_path(name, path) {
        return PathBuf::from(name);
    }
    extra
        .iter()
        .map(|dir| dir.join(name))
        .find(|candidate| is_program(candidate))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn found_in(name: &str, path: &OsStr, extra: &[PathBuf]) -> bool {
    in_path(name, path) || extra.iter().any(|dir| is_program(&dir.join(name)))
}

fn in_path(name: &str, path: &OsStr) -> bool {
    std::env::split_paths(path).any(|dir| {
        program_names(name)
            .iter()
            .any(|file| is_program(&dir.join(file)))
    })
}

/// The file names `name` can have on this system.
fn program_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        [".exe", ".cmd", ".bat", ".ps1", ""]
            .iter()
            .map(|suffix| format!("{name}{suffix}"))
            .collect()
    } else {
        vec![name.to_string()]
    }
}

/// Windows hands desktop apps the full `PATH`, so it has no extra directory.
fn install_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        return Vec::new();
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("HOME")
        .map(|home| Path::new(&home).join(".local/bin"))
        .into_iter()
        .collect();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    dirs
}

#[cfg(unix)]
fn is_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_program(path: &Path) -> bool {
    path.is_file()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn program(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let file = dir.join(name);
        std::fs::write(&file, b"").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
        file
    }

    #[test]
    fn a_program_on_path_keeps_its_bare_name() {
        let path_dir = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        program(path_dir.path(), "bl", 0o755);
        program(install.path(), "bl", 0o755);
        let resolved = resolve_in(
            "bl",
            path_dir.path().as_os_str(),
            &[install.path().to_path_buf()],
        );
        assert_eq!(resolved, PathBuf::from("bl"));
    }

    #[test]
    fn a_desktop_path_still_finds_a_program_in_an_install_directory() {
        let empty = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let file = program(install.path(), "bl", 0o755);
        let dirs = [PathBuf::from("/nonexistent"), install.path().to_path_buf()];
        assert_eq!(resolve_in("bl", empty.path().as_os_str(), &dirs), file);
        assert!(found_in("bl", empty.path().as_os_str(), &dirs));
    }

    #[test]
    fn a_missing_or_non_executable_file_is_not_a_program() {
        let empty = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        program(install.path(), "bl", 0o644);
        let dirs = [install.path().to_path_buf()];
        assert_eq!(
            resolve_in("bl", empty.path().as_os_str(), &dirs),
            PathBuf::from("bl")
        );
        assert!(!found_in("bl", empty.path().as_os_str(), &dirs));
        assert!(!found_in("aws", empty.path().as_os_str(), &dirs));
    }

    #[test]
    fn a_path_is_never_searched() {
        let install = tempfile::tempdir().unwrap();
        let file = program(install.path(), "bl", 0o755);
        assert_eq!(
            resolve_in(
                "/opt/tool/bl",
                OsStr::new(""),
                &[install.path().to_path_buf()]
            ),
            PathBuf::from("/opt/tool/bl")
        );
        assert_eq!(
            resolve_in(file.to_str().unwrap(), OsStr::new(""), &[]),
            file
        );
    }
}
