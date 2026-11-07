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
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
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

/// Waits between attempts of [`persist`] when the rename meets a transient
/// lock: about 1.4 s in all, the same budget SQLite's Windows VFS gives
/// itself for the same errors (`winIoerrRetry`).
const RENAME_RETRY_DELAYS_MS: [u64; 10] = [25, 50, 75, 100, 125, 150, 175, 200, 225, 250];

/// Whether a failed rename may succeed if tried again shortly. Windows
/// refuses to replace (or move) a file another process holds open without
/// `FILE_SHARE_DELETE`, which antivirus scanners, the search indexer and
/// backup agents do for a moment after each write; it reports that as
/// `ERROR_ACCESS_DENIED` (also a target that is still delete-pending),
/// `ERROR_SHARING_VIOLATION` or `ERROR_LOCK_VIOLATION`. Elsewhere a rename
/// within one directory has no such transient failure.
fn transient_rename_error(error: &io::Error) -> bool {
    #[cfg(windows)]
    {
        const ERROR_ACCESS_DENIED: i32 = 5;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        matches!(
            error.raw_os_error(),
            Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
        )
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        false
    }
}

/// Run `rename` until it succeeds, fails for good, or the delays run out.
fn rename_with_retry(
    mut rename: impl FnMut() -> io::Result<()>,
    transient: impl Fn(&io::Error) -> bool,
    delays: &[u64],
) -> io::Result<()> {
    let mut delays = delays.iter();
    loop {
        match rename() {
            Err(error) if transient(&error) => match delays.next() {
                Some(ms) => std::thread::sleep(std::time::Duration::from_millis(*ms)),
                None => return Err(error),
            },
            other => return other,
        }
    }
}

/// Move the written and synced `tmp` over `path`, replacing it atomically.
///
/// `tempfile`'s own `persist` calls `MoveFileExW` on Windows, which fails
/// with `ERROR_ACCESS_DENIED` whenever any handle is open on `path`: another
/// Cuotai process (the TUI beside the desktop app) merely reading the file
/// makes the write fail. `std::fs::rename` falls back to a POSIX-semantics
/// rename there, which replaces a file other handles hold open with
/// `FILE_SHARE_DELETE` (the default of every `std::fs` open), and the
/// short bounded retry covers scanners that open it without that share.
/// Our own handle on the temporary file is closed before the rename. On
/// failure the temporary file is removed and `path` is left as it was.
pub(crate) fn persist(tmp: tempfile::NamedTempFile, path: &Path) -> io::Result<()> {
    let (file, tmp) = tmp.keep().map_err(|error| error.error)?;
    drop(file);
    replace(&tmp, path)
}

/// [`persist`] for a temporary file that is no longer open.
fn persist_path(tmp: tempfile::TempPath, path: &Path) -> io::Result<()> {
    let tmp = tmp.keep().map_err(|error| error.error)?;
    replace(&tmp, path)
}

fn replace(tmp: &Path, path: &Path) -> io::Result<()> {
    rename_with_retry(
        || std::fs::rename(tmp, path),
        transient_rename_error,
        &RENAME_RETRY_DELAYS_MS,
    )
    .inspect_err(|_| {
        let _ = std::fs::remove_file(tmp);
    })
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
    fault::at(fault::Step::Write)?;
    io::Write::write_all(&mut tmp, &bytes)?;
    io::Write::flush(&mut tmp)?;
    fault::at(fault::Step::Sync)?;
    tmp.as_file().sync_all()?;
    fault::at(fault::Step::Rename)?;
    persist(tmp, path)?;
    sync_dir(dir)?;
    clean_orphans(path);
    Ok(())
}

/// PRD 0051 RF4 acceptance: test-only failures injected at the write, sync
/// and rename steps of [`update_locked`]; a no-op outside tests.
mod fault {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Step {
        Write,
        Sync,
        Rename,
    }

    #[cfg(test)]
    thread_local! {
        pub(super) static FAIL_AT: std::cell::Cell<Option<Step>> = const { std::cell::Cell::new(None) };
    }

    #[cfg(test)]
    pub(super) fn at(step: Step) -> std::io::Result<()> {
        if FAIL_AT.with(|fail| fail.get()) == Some(step) {
            return Err(std::io::Error::other("injected fault"));
        }
        Ok(())
    }

    #[cfg(not(test))]
    #[inline(always)]
    pub(super) fn at(_: Step) -> std::io::Result<()> {
        Ok(())
    }
}

/// PRD 0051 RF3.1: the version of a local-state contract. A `major` change
/// alters what an older reader would understand; a `revision` only adds
/// (columns, tables, members) and an older reader of the same major keeps
/// reading and writing what it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StateVersion {
    pub major: u32,
    pub revision: u32,
}

/// How a file's [`StateVersion`] relates to the one this release writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateCompatibility {
    Current,
    /// Written by an older release; this one migrates it (with a `.bak`).
    Older,
    /// Same major, later revision: readable and writable as is.
    NewerRevision,
    /// A later major: refused read-only, never rewritten.
    NewerMajor,
}

impl StateVersion {
    pub fn compatibility(self, ours: StateVersion) -> StateCompatibility {
        use std::cmp::Ordering::*;
        match (
            self.major.cmp(&ours.major),
            self.revision.cmp(&ours.revision),
        ) {
            (Greater, _) => StateCompatibility::NewerMajor,
            (Less, _) | (Equal, Less) => StateCompatibility::Older,
            (Equal, Equal) => StateCompatibility::Current,
            (Equal, Greater) => StateCompatibility::NewerRevision,
        }
    }
}

/// PRD 0051 RF3.1: the `schema` (major) and `schema_revision` (absent
/// means 0) of a JSON state file, reading only those two members. `None`
/// when the bytes are not a JSON object; a file without `schema` is major
/// `unversioned_major` (the files written before the envelope).
pub(crate) fn json_state_version(bytes: &[u8], unversioned_major: u32) -> Option<StateVersion> {
    #[derive(serde::Deserialize)]
    struct Envelope {
        schema: Option<u32>,
        schema_revision: Option<u32>,
    }
    let envelope: Envelope = serde_json::from_slice(bytes).ok()?;
    Some(StateVersion {
        major: envelope.schema.unwrap_or(unversioned_major),
        revision: envelope.schema_revision.unwrap_or(0),
    })
}

/// Durably replace `path` with `bytes`: a uniquely named `0600` temporary
/// file in the same directory, written, synced and renamed, then the
/// directory synced (PRD 0051 RF3.3/RF4.3).
pub(crate) fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    io::Write::write_all(&mut tmp, bytes)?;
    io::Write::flush(&mut tmp)?;
    tmp.as_file().sync_all()?;
    persist(tmp, path)?;
    sync_dir(dir)
}

/// PRD 0051 RF3.3: replace `<file>.bak` with the exact current bytes of
/// `path` (private, synced), before a schema-changing write. Nothing to do
/// when `path` does not exist.
pub(crate) fn backup_file(path: &Path) -> io::Result<()> {
    match std::fs::read(path) {
        Ok(bytes) => write_private_atomic(&backup_path(path), &bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// PRD 0051 RF3.2: the refusal of a state file written by a later schema
/// major. `file` is the file's name, never its path or content.
pub fn newer_major_note(file: &'static str) -> crate::text::Localized {
    crate::text::Localized::with_param(
        "state.error.newer_major",
        "file",
        file,
        "{file} was written by a newer version of Cuotai that changed its format; it is kept untouched. Update Cuotai to use it.",
    )
}

/// The schema major of every SQLite store this release writes.
pub(crate) const SQLITE_MAJOR: i64 = 1;

/// How long a connection to a Cuotai SQLite store waits for a lock another
/// connection (often another Cuotai process) holds before it reports
/// `SQLITE_BUSY`.
pub(crate) const SQLITE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The sleep between two attempts of [`sqlite_busy_wait`]'s handler.
const SQLITE_BUSY_POLL: std::time::Duration = std::time::Duration::from_millis(1);

thread_local! {
    /// When the current busy episode of this thread began. SQLite calls a
    /// busy handler on the thread running the statement, with `count` 0
    /// at the first retry of each lock.
    static BUSY_SINCE: std::cell::Cell<Option<std::time::Instant>> =
        const { std::cell::Cell::new(None) };
}

/// Make `db` wait up to [`SQLITE_BUSY_TIMEOUT`] (wall-clock time) for a
/// lock, retrying every millisecond.
///
/// SQLite's own `busy_timeout` handler backs off to one attempt every
/// 100 ms. Two connections that write back to back (the TUI and the
/// desktop app recording the same refresh, each opening the store per
/// operation and writing one transaction per account) leave gaps of a
/// millisecond or two between transactions, so a waiter polling that
/// rarely keeps missing them while the other writer takes the lock again,
/// and gives up with `SQLITE_BUSY` although the lock was free many times.
/// Measured on Windows runners, where flushes make every transaction slow,
/// with four copies of the two-writer test in parallel: 12 of 1,800 runs
/// starved past the 5 s timeout with SQLite's handler or the same schedule
/// in Rust, 3 of 600 when polling every 1 to 5 ms, none of 1,200 when
/// polling every millisecond (which was also faster overall).
pub(crate) fn sqlite_busy_wait(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.busy_handler(Some(busy_handler))
}

fn busy_handler(count: i32) -> bool {
    let now = std::time::Instant::now();
    let delay = BUSY_SINCE.with(|since| {
        let start = match since.get() {
            Some(start) if count > 0 => start,
            _ => {
                since.set(Some(now));
                now
            }
        };
        busy_delay(now.saturating_duration_since(start), SQLITE_BUSY_TIMEOUT)
    });
    match delay {
        Some(delay) => {
            std::thread::sleep(delay);
            true
        }
        None => false,
    }
}

/// The sleep before the next attempt at a lock waited for `waited` so far,
/// or `None` once `timeout` has passed.
fn busy_delay(
    waited: std::time::Duration,
    timeout: std::time::Duration,
) -> Option<std::time::Duration> {
    (waited < timeout).then(|| SQLITE_BUSY_POLL.min(timeout - waited))
}

/// PRD 0051 RF3.1: the one-row-per-key `state_meta` table beside
/// `PRAGMA user_version` (which stays the append-only migration count).
/// It is created without a `user_version` step, so a release that predates
/// it ignores it.
const STATE_META: &str = "CREATE TABLE IF NOT EXISTS state_meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
) WITHOUT ROWID;
INSERT OR IGNORE INTO state_meta (key, value) VALUES ('major', 1);";

/// The `major` recorded in `state_meta`, `None` when the table or the row
/// is missing (a file written before this release).
pub(crate) fn sqlite_major(db: &rusqlite::Connection) -> rusqlite::Result<Option<i64>> {
    let exists: bool = db.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'state_meta')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    match db.query_row(
        "SELECT value FROM state_meta WHERE key = 'major'",
        [],
        |row| row.get(0),
    ) {
        Ok(major) => Ok(Some(major)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Record this release's major in `state_meta` when the file has none.
pub(crate) fn ensure_sqlite_meta(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    if sqlite_major(db)?.is_none() {
        db.execute_batch(STATE_META)?;
    }
    Ok(())
}

/// `<file>.bak` beside the database `path`.
pub(crate) fn backup_path(path: &Path) -> std::path::PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// PRD 0051 RF3.3: before a migration, replace `<file>.bak` with a
/// consistent copy of the database as it is now. `VACUUM INTO` reads one
/// snapshot through SQLite (WAL included), never the raw main file; the
/// copy is written to a temporary name, made `0600`, synced, renamed over
/// the previous backup and the directory synced. A failure leaves the
/// previous backup and the database untouched, and the caller does not
/// migrate.
pub(crate) fn sqlite_backup(db: &rusqlite::Connection, path: &Path) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let target = backup_path(path);
    let (prefix, suffix) = temp_affixes(&target);
    let tmp = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(suffix)
        .tempfile_in(dir)?
        .into_temp_path();
    // `VACUUM INTO` refuses an existing file.
    std::fs::remove_file(&tmp)?;
    let destination = tmp.to_str().ok_or_else(|| io::Error::other("path"))?;
    db.execute("VACUUM INTO ?1", [destination])
        .map_err(io::Error::other)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    // Windows flushes only through a handle opened for writing.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&tmp)?
        .sync_all()?;
    persist_path(tmp, &target)?;
    sync_dir(dir)
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

/// A private append-only log with one rotated generation (PRD 0013 RF3's
/// `runs.log` policy, shared with the app log of PRD 0052 RF1.2): lines are
/// appended to a `0600` file (on Unix); when the next line would take the
/// file past the limit it moves to `<file>.1`, replacing the previous
/// backup, so the two generations never hold more than twice the limit.
#[derive(Debug)]
pub(crate) struct RotatingFile {
    path: std::path::PathBuf,
    limit: u64,
    lock: std::sync::Mutex<()>,
}

impl RotatingFile {
    pub(crate) fn new(path: std::path::PathBuf, limit: u64) -> Self {
        Self {
            path,
            limit,
            lock: std::sync::Mutex::new(()),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// `<file>.1`, where the log moves when it rotates.
    pub(crate) fn older(&self) -> std::path::PathBuf {
        let mut older = self.path.clone().into_os_string();
        older.push(".1");
        older.into()
    }

    /// Append `line` (which should end in `\n`). A line longer than the
    /// limit is cut to it, so neither generation ever exceeds the limit.
    pub(crate) fn append(&self, line: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let line = &line[..line
            .len()
            .min(usize::try_from(self.limit).unwrap_or(usize::MAX))];
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(dir) = self.path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let len = std::fs::metadata(&self.path).map_or(0, |meta| meta.len());
        if len > 0 && len + line.len() as u64 > self.limit {
            std::fs::rename(&self.path, self.older())?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&self.path)?.write_all(line)
    }

    /// The text of the current file, then the rotated one (missing files
    /// are empty), read under the writer's lock.
    pub(crate) fn generations(&self) -> [String; 2] {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        [self.path.clone(), self.older()]
            .map(|path| std::fs::read_to_string(path).unwrap_or_default())
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

    // security-invariant: SEC-PRIVATE-FS (PRD 0050 RF7.1)
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
        add_key_waiting(path, key, UPDATE_LOCK_WAIT)
    }

    fn add_key_waiting(
        path: &Path,
        key: String,
        wait: std::time::Duration,
    ) -> Result<(), UpdateError<()>> {
        update_locked_with(path, wait, |bytes| {
            let mut map: serde_json::Map<String, serde_json::Value> = match bytes {
                Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| ())?,
                None => serde_json::Map::new(),
            };
            map.insert(key, serde_json::Value::Bool(true));
            Ok(serde_json::to_vec(&map).unwrap())
        })
    }

    /// PRD 0051 RF4 acceptance: a failure at the write, the sync or the
    /// rename leaves the previous file byte for byte, and the next write
    /// still works (no lock or temporary file is left in the way).
    #[test]
    fn a_fault_at_any_step_keeps_the_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        add_key(&path, "first".into()).unwrap();
        let before = std::fs::read(&path).unwrap();
        for step in [fault::Step::Write, fault::Step::Sync, fault::Step::Rename] {
            fault::FAIL_AT.with(|fail| fail.set(Some(step)));
            let result = add_key(&path, format!("{step:?}"));
            fault::FAIL_AT.with(|fail| fail.set(None));
            assert!(matches!(result, Err(UpdateError::Io(_))), "{step:?}");
            assert_eq!(std::fs::read(&path).unwrap(), before, "{step:?}");
        }
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name != "desktop.json" && name != ".desktop.json.lock")
            .collect();
        assert!(names.is_empty(), "no temporary file is left: {names:?}");
        add_key(&path, "after".into()).unwrap();
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(map.contains_key("first") && map.contains_key("after"));
    }

    #[test]
    fn state_versions_compare_by_major_then_revision() {
        let ours = StateVersion {
            major: 1,
            revision: 2,
        };
        let at = |major, revision| StateVersion { major, revision }.compatibility(ours);
        assert_eq!(at(1, 2), StateCompatibility::Current);
        assert_eq!(at(1, 1), StateCompatibility::Older);
        assert_eq!(at(0, 9), StateCompatibility::Older);
        assert_eq!(at(1, 7), StateCompatibility::NewerRevision);
        assert_eq!(at(2, 0), StateCompatibility::NewerMajor);
    }

    #[test]
    fn concurrent_mutations_are_serialized_and_none_is_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("cuotai").join("desktop.json"));
        // 32 writers queue behind one lock, so the last waits for 31 synced
        // writes. A write usually takes ~11 ms on a Windows runner, but
        // flushes there spike to 300 ms and 32 queued writes were measured
        // at 4.1 s: past the production UPDATE_LOCK_WAIT (3 s, sized for
        // the two or three Cuotai processes that share the file), so the
        // last writers reported `Busy`. This test checks serialization and
        // that no change is lost, not the bound, which
        // `a_refused_change_or_a_busy_lock_leaves_the_file_untouched` covers.
        let wait = std::time::Duration::from_secs(120);
        let threads: Vec<_> = (0..32)
            .map(|n| {
                let path = Arc::clone(&path);
                std::thread::spawn(move || add_key_waiting(&path, format!("key{n}"), wait).unwrap())
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
    fn a_file_another_reader_holds_open_is_still_replaced() {
        // Another Cuotai process reading the file (std opens share delete)
        // must not make the write fail: `MoveFileExW` alone refuses this on
        // Windows with `ERROR_ACCESS_DENIED`.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        std::fs::write(&path, b"{}").unwrap();
        let reader = std::fs::File::open(&path).unwrap();
        add_key(&path, "k".into()).unwrap();
        drop(reader);
        assert_eq!(std::fs::read(&path).unwrap(), br#"{"k":true}"#);
        let tmp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let reader = std::fs::File::open(&path).unwrap();
        persist(tmp, &path).unwrap();
        drop(reader);
        assert_eq!(std::fs::read(&path).unwrap(), b"");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(names.len(), 2, "only the file and its lock: {names:?}");
    }

    #[test]
    fn a_transient_rename_failure_is_retried_within_a_bound() {
        let transient = |error: &io::Error| error.kind() == io::ErrorKind::ResourceBusy;
        let busy = || io::Error::from(io::ErrorKind::ResourceBusy);
        let mut calls = 0;
        let result = rename_with_retry(
            || {
                calls += 1;
                if calls < 3 {
                    Err(busy())
                } else {
                    Ok(())
                }
            },
            transient,
            &[0; 5],
        );
        assert!(result.is_ok());
        assert_eq!(calls, 3, "retried until it succeeded");
        let mut calls = 0;
        let result = rename_with_retry(
            || {
                calls += 1;
                Err(busy())
            },
            transient,
            &[0; 5],
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::ResourceBusy);
        assert_eq!(calls, 6, "one attempt plus one per delay, then the error");
        let mut calls = 0;
        let result = rename_with_retry(
            || {
                calls += 1;
                Err(io::Error::from(io::ErrorKind::NotFound))
            },
            transient,
            &[0; 5],
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(calls, 1, "a lasting failure is not retried");
    }

    #[test]
    fn the_busy_handler_polls_often_until_a_wall_clock_deadline() {
        let ms = std::time::Duration::from_millis;
        let timeout = ms(5000);
        assert_eq!(busy_delay(ms(0), timeout), Some(ms(1)));
        assert_eq!(busy_delay(ms(4000), timeout), Some(ms(1)));
        assert_eq!(
            busy_delay(std::time::Duration::from_micros(4_999_600), timeout),
            Some(std::time::Duration::from_micros(400)),
            "never past the deadline"
        );
        assert_eq!(busy_delay(ms(5000), timeout), None);
        assert_eq!(busy_delay(ms(6000), timeout), None);
    }

    #[test]
    fn a_connection_waits_for_a_lock_another_one_releases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.sqlite3");
        let holder = rusqlite::Connection::open(&path).unwrap();
        holder
            .execute_batch("PRAGMA journal_mode = WAL; CREATE TABLE t (x);")
            .unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let waiter = std::thread::spawn({
            let path = path.clone();
            move || {
                let db = rusqlite::Connection::open(path).unwrap();
                sqlite_busy_wait(&db).unwrap();
                let start = std::time::Instant::now();
                db.execute("INSERT INTO t VALUES (1)", []).unwrap();
                start.elapsed()
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        holder.execute_batch("COMMIT").unwrap();
        let waited = waiter.join().unwrap();
        assert!(
            waited >= std::time::Duration::from_millis(100),
            "{waited:?}"
        );
        let rows: i64 = holder
            .query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn a_failed_persist_leaves_the_target_and_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let missing = dir.path().join("missing").join("file.json");
        assert!(persist(tmp, &missing).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
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
