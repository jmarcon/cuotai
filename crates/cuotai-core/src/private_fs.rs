//! Private files and directories Cuotai keeps for itself: directories
//! `0700` and files `0600` on Unix, never through a symbolic link and never
//! owned by another user. Errors are plain `io::Error`s; callers turn them
//! into their own localized messages, which never carry the path.

use std::fs::Metadata;
use std::io;
use std::path::Path;

/// Create `dir` (and its missing parents) as a private directory, or check
/// an existing one: it must be a real directory (not a symbolic link), owned
/// by the current user on Unix, and is narrowed to `0700` there when it was
/// wider. Parents are created with the default permissions.
pub(crate) fn ensure_dir(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    match builder.create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(io::Error::other("not a private directory"));
    }
    if foreign_owner(&meta) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owned by another user",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o777 != 0o700 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// Replace `path` atomically with `bytes`, as a `0600` file inside a private
/// directory ([`ensure_dir`]).
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        ensure_dir(dir)?;
    }
    crate::hooks::write_atomically(path, bytes)
}

/// On Unix, whether the group or other users may write what `meta`
/// describes. Always `false` elsewhere (ACLs are out of scope).
pub(crate) fn writable_by_others(meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o022 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// On Unix, whether what `meta` describes belongs to neither the current
/// user nor root (another user could change it at will). Always `false`
/// elsewhere.
pub(crate) fn foreign_owner(meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: `geteuid` cannot fail and has no side effects.
        let me = unsafe { libc::geteuid() };
        meta.uid() != me && meta.uid() != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_private_directory_is_created_or_narrowed_to_0700() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a").join("b");
        ensure_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        ensure_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700, "an existing wide directory is narrowed");
    }

    #[test]
    fn a_symbolic_link_or_a_file_is_never_a_private_directory() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(ensure_dir(&link).is_err());
        let file = root.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(ensure_dir(&file).is_err());
    }

    #[test]
    fn a_private_file_is_0600_in_a_0700_directory() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dir").join("file.json");
        write_file(&path, b"{}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn permission_bits_are_classified() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("f");
        std::fs::write(&path, b"x").unwrap();
        for (bits, writable) in [
            (0o600, false),
            (0o640, false),
            (0o620, true),
            (0o602, true),
            (0o604, false),
        ] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(bits)).unwrap();
            let meta = std::fs::metadata(&path).unwrap();
            assert_eq!(writable_by_others(&meta), writable, "{bits:o}");
            assert!(!foreign_owner(&meta));
        }
    }
}
