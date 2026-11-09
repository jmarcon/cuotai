//! The advisory lock beside `notified.json` (PRD 0032 RF4.2): a
//! `notified.lock` file created exclusively, held for the milliseconds a
//! process needs to re-read the ledger, record what it claims and write it
//! back, and never across a network call or a delivery. A stale lock (older
//! than [`STALE_AFTER`], left by a process that died) is broken, and waiting
//! gives up after [`WAIT`] so a stuck lock never blocks the UI thread: the
//! caller then proceeds without it (a duplicate notice is the worst case).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A lock older than this belongs to a process that is gone.
pub const STALE_AFTER: Duration = Duration::from_secs(10);
/// How long to wait for another process to release the lock.
pub const WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);

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

    pub(crate) fn acquire_with(ledger: &Path, wait: Duration, stale: Duration) -> Option<Self> {
        let path = lock_path(ledger);
        let deadline = std::time::Instant::now() + wait;
        let mut created_dir = false;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let token = token();
                    let _ = write!(file, "{token}");
                    return Some(Self { path, token });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_stale(&path, stale) {
                        break_stale(&path, stale);
                        continue;
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
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(POLL);
        }
    }
}

/// Older than `stale`, or modified in the future (a clock set back, a file
/// copied from another machine): such a lock would otherwise never age.
fn is_stale(path: &Path, stale: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .is_some_and(
            |modified| match SystemTime::now().duration_since(modified) {
                Ok(age) => age > stale,
                Err(_) => true,
            },
        )
}

/// Break a stale lock without removing a fresh one another waiter created
/// in between: move it aside under a name of this holder's own, and put it
/// back if what was moved turns out not to be stale.
fn break_stale(path: &Path, stale: Duration) {
    let aside = path.with_extension(format!("lock.stale-{}", token().replace(':', "-")));
    if std::fs::rename(path, &aside).is_err() {
        return;
    }
    if !is_stale(&aside, stale) {
        // Someone else broke it first and took the lock: give theirs back
        // (a hard link fails, keeping the newer lock, when one exists).
        let _ = std::fs::hard_link(&aside, path);
    }
    let _ = std::fs::remove_file(&aside);
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
}
