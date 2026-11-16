//! The advisory lock beside `notified.json` (PRD 0032 RF4.2): a
//! `notified.lock` file created exclusively, held for the milliseconds a
//! process needs to re-read the ledger, record what it claims and write it
//! back, and never across a network call or a delivery. A stale lock (older
//! than [`STALE_AFTER`], left by a process that died) is broken, and waiting
//! gives up after [`WAIT`] so a stuck lock never blocks the UI thread: the
//! caller then proceeds without it (a duplicate notice is the worst case).
//!
//! The lock path is never followed: its age is read with `lstat`, so a
//! symlink (or any other entry) planted there is aged by itself and broken
//! like any lock, and its target is never read, created or written. An
//! entry whose `lstat` fails is broken once [`STALE_AFTER`] has passed
//! since the first failure seen. After a claim waited out a stuck lock, the
//! claims of the next [`STALE_AFTER`] try once without waiting, so a lock
//! that cannot be taken costs at most one [`WAIT`] per [`STALE_AFTER`].

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// A lock older than this belongs to a process that is gone.
pub const STALE_AFTER: Duration = Duration::from_secs(10);
/// How long to wait for another process to release the lock.
pub const WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);

/// What this process remembers per lock path, across claims.
#[derive(Default)]
struct Memory {
    /// When `lstat` of the path first failed (cleared by a success).
    lstat_failed_at: HashMap<PathBuf, Instant>,
    /// When a claim last waited out the lock (cleared by a success).
    gave_up_at: HashMap<PathBuf, Instant>,
}

static MEMORY: LazyLock<Mutex<Memory>> = LazyLock::new(Mutex::default);

fn memory() -> MutexGuard<'static, Memory> {
    MEMORY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A held lock; dropping it releases the file, unless another holder
/// took it over meanwhile (PRD 0003 re-validation S-07).
#[derive(Debug)]
pub struct LedgerLock {
    path: PathBuf,
    /// What this holder wrote into the file: its PID and a per-process
    /// counter, so two holders of one process differ too.
    token: String,
}

/// A fresh token for a new holder.
fn token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}:{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// `notified.lock` next to `ledger` (`notified.json`).
pub fn lock_path(ledger: &Path) -> PathBuf {
    ledger.with_extension("lock")
}

impl LedgerLock {
    /// Take the lock for the ledger at `ledger`, waiting up to [`WAIT`].
    /// `None` when it could not be taken (waited out, or the directory is
    /// not writable): the caller continues without it.
    pub fn acquire(ledger: &Path) -> Option<Self> {
        Self::acquire_with(ledger, WAIT, STALE_AFTER)
    }

    /// `stale` is both the age that breaks a lock and the back-off after a
    /// claim gave up, during which a claim tries once and does not wait.
    pub(crate) fn acquire_with(ledger: &Path, wait: Duration, stale: Duration) -> Option<Self> {
        let path = lock_path(ledger);
        let start = Instant::now();
        let backing_off = memory()
            .gave_up_at
            .get(&path)
            .is_some_and(|at| start.saturating_duration_since(*at) < stale);
        let deadline = if backing_off { start } else { start + wait };
        let mut created_dir = false;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let token = token();
                    let _ = write!(file, "{token}");
                    let mut memory = memory();
                    memory.gave_up_at.remove(&path);
                    memory.lstat_failed_at.remove(&path);
                    return Some(Self { path, token });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(by_age) = staleness(&path, stale) {
                        if break_stale(&path, stale, by_age) {
                            continue;
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !created_dir => {
                    created_dir = true;
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    continue;
                }
                Err(_) => return None,
            }
            if Instant::now() >= deadline {
                if !backing_off {
                    memory().gave_up_at.insert(path, Instant::now());
                }
                return None;
            }
            std::thread::sleep(POLL);
        }
    }
}

/// `lstat` of `path`: never follows a symlink. Tests can make it fail.
fn lstat(path: &Path) -> std::io::Result<std::fs::Metadata> {
    #[cfg(test)]
    if tests::lstat_fails(path) {
        return Err(std::io::Error::other("lstat refused by the test"));
    }
    std::fs::symlink_metadata(path)
}

/// Older than `stale`, or modified in the future (a clock set back, a file
/// copied from another machine): such a lock would otherwise never age.
/// The entry's own modification time, never a link target's.
fn aged_out(meta: &std::fs::Metadata, stale: Duration) -> bool {
    meta.modified().is_ok_and(
        |modified| match SystemTime::now().duration_since(modified) {
            Ok(age) => age > stale,
            Err(_) => true,
        },
    )
}

/// Whether the lock at `path` may be broken: `Some(true)` when it aged
/// out, `Some(false)` when its `lstat` has failed for longer than `stale`
/// (no age to read), `None` while it must be respected.
fn staleness(path: &Path, stale: Duration) -> Option<bool> {
    match lstat(path) {
        Ok(meta) => {
            memory().lstat_failed_at.remove(path);
            aged_out(&meta, stale).then_some(true)
        }
        // Released meanwhile: the next attempt creates it.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            memory().lstat_failed_at.remove(path);
            None
        }
        Err(_) => {
            let now = Instant::now();
            let first = *memory()
                .lstat_failed_at
                .entry(path.to_path_buf())
                .or_insert(now);
            (now.saturating_duration_since(first) >= stale).then_some(false)
        }
    }
}

/// Break a stale lock without removing a fresh one another waiter created
/// in between: move it aside under a name of this holder's own, and put it
/// back if what was moved turns out not to be stale. Only a regular file
/// is ever put back (a hard link may follow a symlink on some platforms),
/// and a lock broken for a failing `lstat` (`by_age` false) has no age to
/// compare, so it is only removed. `true` when the path was moved aside.
fn break_stale(path: &Path, stale: Duration, by_age: bool) -> bool {
    let aside = path.with_extension(format!("lock.stale-{}", token().replace(':', "-")));
    if std::fs::rename(path, &aside).is_err() {
        return false;
    }
    if by_age {
        if let Ok(meta) = lstat(&aside) {
            if meta.file_type().is_file() && !aged_out(&meta, stale) {
                // Someone else broke it first and took the lock: give theirs
                // back (a hard link fails, keeping the newer lock, when one
                // exists).
                let _ = std::fs::hard_link(&aside, path);
            }
        }
    }
    let _ = std::fs::remove_file(&aside);
    memory().lstat_failed_at.remove(path);
    true
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        // Only this holder's own file: a lock taken over as stale belongs
        // to its new holder.
        if std::fs::read_to_string(&self.path).is_ok_and(|held| held == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        /// The path whose `lstat` fails on this thread (test seam).
        static LSTAT_FAILS: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    pub(super) fn lstat_fails(path: &Path) -> bool {
        LSTAT_FAILS.with(|fails| fails.borrow().as_deref() == Some(path))
    }

    #[test]
    fn the_second_holder_waits_for_the_first_and_then_takes_it() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("notified.json");
        let first = LedgerLock::acquire(&ledger).expect("free");
        assert!(lock_path(&ledger).exists());
        assert!(
            LedgerLock::acquire_with(&ledger, Duration::from_millis(60), STALE_AFTER).is_none(),
            "held: the waiter gives up"
        );
        drop(first);
        assert!(!lock_path(&ledger).exists());
        assert!(LedgerLock::acquire(&ledger).is_some());
    }

    #[test]
    fn a_stale_lock_is_broken() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("notified.json");
        std::fs::write(lock_path(&ledger), "999999").unwrap();
        // Not stale yet for a long threshold: waiting fails.
        assert!(LedgerLock::acquire_with(
            &ledger,
            Duration::from_millis(40),
            Duration::from_secs(60)
        )
        .is_none());
        // Stale for a zero threshold once any time passed.
        std::thread::sleep(Duration::from_millis(20));
        let lock = LedgerLock::acquire_with(&ledger, Duration::from_millis(500), Duration::ZERO);
        assert!(lock.is_some(), "the dead process's lock is taken over");
    }

    /// PRD 0003 re-validation S-07: a lock dated in the future is stale at
    /// once, and dropping a lock leaves another holder's file in place.
    #[test]
    fn a_future_lock_is_broken_and_drop_keeps_another_holders_lock() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("notified.json");
        let path = lock_path(&ledger);
        std::fs::write(&path, "999999:0").unwrap();
        let future = SystemTime::now() + Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(future)
            .unwrap();
        let held = LedgerLock::acquire_with(&ledger, Duration::from_millis(100), STALE_AFTER)
            .expect("a future lock is broken at once");
        // Another process takes it over (as if this one were stale).
        std::fs::write(&path, "424242:7").unwrap();
        drop(held);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "424242:7");
        let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(leftovers, 1, "nothing but the other holder's lock");
    }

    #[test]
    fn a_missing_directory_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("nested").join("notified.json");
        assert!(LedgerLock::acquire(&ledger).is_some());
    }

    /// C-11: a lock whose `lstat` fails is respected for `stale` after the
    /// first failure, then broken (and not put back) like any stale lock.
    #[test]
    fn a_lock_whose_lstat_fails_is_broken_after_the_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("notified.json");
        let path = lock_path(&ledger);
        std::fs::write(&path, "999999:0").unwrap();
        LSTAT_FAILS.with(|fails| *fails.borrow_mut() = Some(path.clone()));
        let stale = Duration::from_millis(150);
        // Within the threshold: respected, the claim gives up.
        assert!(LedgerLock::acquire_with(&ledger, Duration::from_millis(30), stale).is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "999999:0");
        std::thread::sleep(stale);
        let held = LedgerLock::acquire_with(&ledger, Duration::from_millis(500), stale)
            .expect("broken once the threshold passed");
        LSTAT_FAILS.with(|fails| *fails.borrow_mut() = None);
        assert_ne!(std::fs::read_to_string(&path).unwrap(), "999999:0");
        drop(held);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// C-11: after a claim waited out a held lock, the next claims of the
    /// back-off try once and return at once.
    #[test]
    fn a_claim_within_the_back_off_does_not_wait() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("notified.json");
        let held = LedgerLock::acquire(&ledger).expect("free");
        assert!(
            LedgerLock::acquire_with(&ledger, Duration::from_millis(30), STALE_AFTER).is_none()
        );
        let started = Instant::now();
        assert!(LedgerLock::acquire_with(&ledger, WAIT, STALE_AFTER).is_none());
        assert!(started.elapsed() < Duration::from_millis(100));
        // A free lock is still taken during the back-off, which it ends.
        drop(held);
        let again = LedgerLock::acquire(&ledger).expect("released");
        drop(again);
        assert!(!memory().gave_up_at.contains_key(&lock_path(&ledger)));
    }

    /// C-11: a dangling symlink at the lock path is aged by itself, broken
    /// after the threshold, and its target is never created.
    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_is_broken_and_its_target_never_created() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("notified.json");
        let path = lock_path(&ledger);
        let target = dir.path().join("nowhere");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(LedgerLock::acquire_with(
            &ledger,
            Duration::from_millis(30),
            Duration::from_secs(60)
        )
        .is_none());
        std::thread::sleep(Duration::from_millis(20));
        let held = LedgerLock::acquire_with(&ledger, Duration::from_millis(500), Duration::ZERO)
            .expect("the symlink is broken");
        assert!(!target.exists(), "the link target is never created");
        assert!(lstat(&path).unwrap().file_type().is_file());
        drop(held);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// C-11: a symlink to another file is judged by its own age (not the
    /// target's), and breaking it leaves the target intact.
    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_victim_leaves_the_victim_intact() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("notified.json");
        let path = lock_path(&ledger);
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, "precious").unwrap();
        // An old target: following the link would call the lock stale.
        let old = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&victim)
            .unwrap()
            .set_modified(old)
            .unwrap();
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        assert!(
            LedgerLock::acquire_with(&ledger, Duration::from_millis(30), Duration::from_secs(60))
                .is_none(),
            "a fresh link is respected whatever its target's age"
        );
        std::thread::sleep(Duration::from_millis(20));
        let held = LedgerLock::acquire_with(&ledger, Duration::from_millis(500), Duration::ZERO)
            .expect("the symlink is broken");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious");
        assert!(lstat(&path).unwrap().file_type().is_file());
        drop(held);
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from("victim.txt")]);
    }
}
