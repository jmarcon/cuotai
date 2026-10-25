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

/// A held lock; dropping it releases the file.
#[derive(Debug)]
pub struct LedgerLock {
    path: PathBuf,
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
                    let _ = write!(file, "{}", std::process::id());
                    return Some(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_stale(&path, stale) {
                        let _ = std::fs::remove_file(&path);
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

fn is_stale(path: &Path, stale: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age > stale)
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
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

    #[test]
    fn a_missing_directory_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("nested").join("notified.json");
        assert!(LedgerLock::acquire(&ledger).is_some());
    }
}
