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
    #[cfg_attr(not(unix), allow(unused_mut))]
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

/// Why [`update_locked`] did not replace the file.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError<E> {
    /// Another writer held the lock for longer than the bounded wait.
    #[error("the file is busy")]
    Busy,
    /// The file or its directory is a symbolic link, not a regular file or
    /// directory, or belongs to another user.
    #[error("the file is not private to this user")]
    NotPrivate,
    /// The change refused the current content (corrupt, newer schema...).
    #[error("the change was refused")]
    Refused(E),
    /// Reading, writing, syncing or renaming failed; the previous file is
    /// unchanged.
    #[error("I/O failure: {0}")]
    Io(#[from] io::Error),
}

/// How long [`update_locked`] waits for another writer.
pub const UPDATE_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(3);
/// Temporary files of a crashed writer older than this are removed.
const ORPHAN_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// `<dir>/.<name>.lock`, the advisory lock beside `path`.
fn lock_path(path: &Path) -> std::path::PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.lock"))
}

/// The prefix and suffix of this writer's temporary files: `.<name>.` and
/// `.tmp`, so cleanup removes only those.
fn temp_affixes(path: &Path) -> (String, &'static str) {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    (format!(".{name}."), ".tmp")
}

/// Take the OS advisory lock on `lock`, polling up to `wait`. The lock is
/// released when the returned file closes, also when the process dies, so
/// a lock is never broken by its age.
fn lock_file(lock: &Path, wait: std::time::Duration) -> io::Result<Option<std::fs::File>> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock)?;
    let deadline = std::time::Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Flush the directory entry of a rename to stable storage where the
/// platform can (Unix); elsewhere the rename is already durable enough.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Remove this writer's temporary files left by a crash, once they are a
/// day old. Anything else in the directory is never touched.
fn clean_orphans(path: &Path) {
    let Some(dir) = path.parent() else { return };
    let (prefix, suffix) = temp_affixes(path);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(&prefix) && name.ends_with(suffix)) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > ORPHAN_AGE);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// PRD 0051 RF4: the one read-modify-write of a private state file
/// (`desktop.json`). Under an advisory lock beside it (bounded wait, never
/// broken by age) it re-reads the current bytes (`None` when missing),
/// lets `change` compute the new ones once, and replaces the file
/// atomically: a uniquely named temporary file in the same directory,
/// `0600`, written, flushed and synced, renamed over the file, then the
/// directory synced. The directory is created or narrowed to `0700`; a
/// symbolic link or another user's file is refused. A failure at any step
/// leaves the previous file as it was. No lock is held after it returns,
/// so callers must not do UI or network work inside `change`.
pub fn update_locked<E>(
    path: &Path,
    change: impl FnOnce(Option<Vec<u8>>) -> Result<Vec<u8>, E>,
) -> Result<(), UpdateError<E>> {
    update_locked_with(path, UPDATE_LOCK_WAIT, change)
}

pub(crate) fn update_locked_with<E>(
    path: &Path,
    wait: std::time::Duration,
    change: impl FnOnce(Option<Vec<u8>>) -> Result<Vec<u8>, E>,
) -> Result<(), UpdateError<E>> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    ensure_dir(dir).map_err(|error| match error.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::Other => UpdateError::NotPrivate,
        _ => UpdateError::Io(error),
    })?;
    let Some(_lock) = lock_file(&lock_path(path), wait)? else {
        return Err(UpdateError::Busy);
    };
    let current = match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || foreign_owner(&meta) => {
            return Err(UpdateError::NotPrivate);
        }
        Ok(_) => Some(std::fs::read(path)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let bytes = change(current).map_err(UpdateError::Refused)?;
    let (prefix, suffix) = temp_affixes(path);
    let mut tmp = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(suffix)
        .tempfile_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600))?;
    }
    io::Write::write_all(&mut tmp, &bytes)?;
    io::Write::flush(&mut tmp)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|error| error.error)?;
    sync_dir(dir)?;
    clean_orphans(path);
    Ok(())
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

/// On Unix, whether the group or other users may read, write or traverse
/// what `meta` describes. Always `false` elsewhere.
#[cfg_attr(not(feature = "serve"), allow(dead_code))]
pub(crate) fn open_to_others(meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o077 != 0
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
            assert_eq!(open_to_others(&meta), bits != 0o600, "{bits:o}");
            assert!(!foreign_owner(&meta));
        }
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use std::sync::Arc;

    fn add_key(path: &Path, key: String) -> Result<(), UpdateError<()>> {
        update_locked(path, |bytes| {
            let mut map: serde_json::Map<String, serde_json::Value> = match bytes {
                Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| ())?,
                None => serde_json::Map::new(),
            };
            map.insert(key, serde_json::Value::Bool(true));
            Ok(serde_json::to_vec(&map).unwrap())
        })
    }

    #[test]
    fn concurrent_mutations_are_serialized_and_none_is_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("cuotai").join("desktop.json"));
        let threads: Vec<_> = (0..32)
            .map(|n| {
                let path = Arc::clone(&path);
                std::thread::spawn(move || add_key(&path, format!("key{n}")).unwrap())
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(&std::fs::read(&*path).unwrap()).unwrap();
        assert_eq!(map.len(), 32);
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn a_refused_change_or_a_busy_lock_leaves_the_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(matches!(
            add_key(&path, "x".into()),
            Err(UpdateError::Refused(()))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"{not json");
        let held = lock_file(&lock_path(&path), UPDATE_LOCK_WAIT)
            .unwrap()
            .unwrap();
        let busy = update_locked_with(&path, std::time::Duration::from_millis(30), |_| {
            Ok::<_, ()>(b"{}".to_vec())
        });
        assert!(matches!(busy, Err(UpdateError::Busy)));
        drop(held);
        assert_eq!(std::fs::read(&path).unwrap(), b"{not json");
        assert!(
            lock_path(&path).exists(),
            "the lock file stays; it is never removed by age"
        );
    }

    #[test]
    fn old_orphan_temporaries_are_removed_and_other_files_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        let orphan = dir.path().join(".desktop.json.abc123.tmp");
        let fresh = dir.path().join(".desktop.json.def456.tmp");
        let other = dir.path().join(".other.json.abc.tmp");
        for file in [&orphan, &fresh, &other] {
            std::fs::write(file, b"x").unwrap();
        }
        let day_ago =
            std::time::SystemTime::now() - ORPHAN_AGE - std::time::Duration::from_secs(60);
        for file in [&orphan, &other] {
            std::fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(day_ago)
                .unwrap();
        }
        add_key(&path, "k".into()).unwrap();
        assert!(!orphan.exists());
        assert!(fresh.exists() && other.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_0600_in_a_0700_directory_and_a_link_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("desktop.json");
        add_key(&path, "k".into()).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        let target = dir.path().join("elsewhere.json");
        std::fs::write(&target, b"{}").unwrap();
        let link = dir.path().join("state").join("linked.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            add_key(&link, "k".into()),
            Err(UpdateError::NotPrivate)
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"{}");
    }
}
