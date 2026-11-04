//! Cross-process guard for refresh-token rotation.
//!
//! The TUI, `cuotai --serve` and the desktop app are separate processes that
//! can read the same vault entry. [`OAuthSession`](super::OAuthSession)
//! serializes rotation inside one process only; two processes refreshing the
//! same stale token at once both send the same refresh token, the provider
//! rotates it for the first and refuses (or revokes the grant for) the
//! second, and whichever write lands last can leave an invalidated token in
//! the vault: the account ends logged out.
//!
//! Every vault write of a session that has a lock directory therefore holds
//! an advisory, exclusive lock on `<dir>/<storage key>.lock`, taken after the
//! session's in-process lock. A refresh takes it, then reads the vault again
//! and refreshes only when the token it finds is still stale: a process that
//! waited for another one's rotation uses the stored result instead of
//! sending an invalidated refresh token.
//!
//! The lock is the operating system's whole-file lock ([`File::try_lock`]:
//! `flock` on Unix, `LockFileEx` on Windows). It belongs to the open file, so
//! two sessions of one process exclude each other as two processes do, and
//! the system releases it when the holder exits or crashes; a leftover lock
//! file is harmless. The files are empty: no token, key material or account
//! data is ever written to them. The lock is independent of the store, so the
//! OS vault and the debug SQLite store get the same guard.

use super::OAuthError;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::Instant;

/// How long a process waits for another one's rotation. A holder is bounded
/// by the 30-second HTTP timeout plus vault operations, so a longer wait
/// means a stuck holder: the caller fails as a transient error and retries on
/// its next refresh.
pub(super) const WAIT: Duration = Duration::from_secs(90);
const POLL: Duration = Duration::from_millis(20);

/// The lock directory of the OS vault, beside the vault index.
pub(super) fn default_dir() -> Option<PathBuf> {
    super::vault_index::default_path()
        .and_then(|index| index.parent().map(|dir| dir.join("oauth-locks")))
}

/// The lock directory of the debug-only SQLite store at `database`:
/// `<database>.oauth-locks`, beside it.
pub(super) fn development_dir(database: &Path) -> PathBuf {
    let mut name = database.file_name().unwrap_or_default().to_os_string();
    name.push(".oauth-locks");
    database.with_file_name(name)
}

/// An exclusive lock on one storage key across processes, released on drop.
pub(super) struct ProcessLock {
    _file: File,
}

/// Wait up to `wait` for the lock of `key` in `dir`.
///
/// `Ok(None)` means the lock file cannot be created (a read-only or missing
/// data directory): the session then keeps its in-process guarantee only,
/// as before this guard existed, rather than refusing every refresh.
pub(super) async fn acquire(
    dir: &Path,
    key: &str,
    wait: Duration,
) -> Result<Option<ProcessLock>, OAuthError> {
    let key = super::store::validate_id(key)?;
    let Some(file) = open(dir, key) else {
        return Ok(None);
    };
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(ProcessLock { _file: file })),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                tokio::time::sleep(POLL).await;
            }
            Err(TryLockError::WouldBlock) => return Err(OAuthError::Network),
            // A file system without lock support: same as no lock file.
            Err(TryLockError::Error(_)) => return Ok(None),
        }
    }
}

fn open(dir: &Path, key: &str) -> Option<File> {
    std::fs::create_dir_all(dir).ok()?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(dir.join(format!("{key}.lock"))).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0f8e5a3c-1b2d-4c6e-8a9b-112233445566";

    #[tokio::test]
    async fn a_held_lock_excludes_a_second_holder_until_it_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let held = acquire(dir.path(), KEY, WAIT).await.unwrap().expect("lock");
        let blocked = acquire(dir.path(), KEY, Duration::from_millis(100)).await;
        assert!(matches!(blocked, Err(OAuthError::Network)));
        // Another key is independent.
        let other = acquire(dir.path(), "other-key", Duration::ZERO).await;
        assert!(matches!(other, Ok(Some(_))));
        drop(held);
        let again = acquire(dir.path(), KEY, Duration::ZERO).await;
        assert!(matches!(again, Ok(Some(_))));
    }

    #[tokio::test]
    async fn a_waiter_gets_the_lock_once_the_holder_releases_it() {
        let dir = tempfile::tempdir().unwrap();
        let held = acquire(dir.path(), KEY, WAIT).await.unwrap().expect("lock");
        let path = dir.path().to_path_buf();
        let waiter = tokio::spawn(async move { acquire(&path, KEY, WAIT).await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!waiter.is_finished());
        drop(held);
        assert!(matches!(waiter.await.unwrap(), Ok(Some(_))));
    }

    #[tokio::test]
    async fn the_lock_file_stays_empty_and_rejects_unsafe_keys() {
        let dir = tempfile::tempdir().unwrap();
        let _held = acquire(dir.path(), KEY, WAIT).await.unwrap().expect("lock");
        let file = dir.path().join(format!("{KEY}.lock"));
        assert_eq!(std::fs::metadata(&file).unwrap().len(), 0);
        for key in ["", "../escape", "with/slash", "with.dot"] {
            assert!(matches!(
                acquire(dir.path(), key, Duration::ZERO).await,
                Err(OAuthError::Configuration(_))
            ));
        }
    }

    #[tokio::test]
    async fn an_unusable_directory_degrades_to_no_lock() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-directory");
        std::fs::write(&blocker, b"").unwrap();
        assert!(matches!(
            acquire(&blocker.join("locks"), KEY, Duration::ZERO).await,
            Ok(None)
        ));
    }

    #[test]
    fn development_locks_sit_beside_the_database() {
        assert_eq!(
            development_dir(Path::new("/tmp/x/cuotai-dev.sqlite3")),
            Path::new("/tmp/x/cuotai-dev.sqlite3.oauth-locks")
        );
    }

    /// Environment variable that turns [`child_holds_the_lock`] into the
    /// helper process of [`the_lock_excludes_another_process`].
    const CHILD_DIR: &str = "CUOTAI_TEST_PROCESS_LOCK_DIR";

    /// Helper run in a child process: holds the lock, announces it with a
    /// marker file, and releases it by exiting once the parent removes the
    /// marker. A no-op in a normal test run.
    #[tokio::test]
    async fn child_holds_the_lock() {
        let Some(dir) = std::env::var_os(CHILD_DIR).map(PathBuf::from) else {
            return;
        };
        let _held = acquire(&dir, KEY, WAIT).await.unwrap().expect("lock");
        let marker = dir.join("held");
        std::fs::write(&marker, b"").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while marker.exists() && Instant::now() < deadline {
            tokio::time::sleep(POLL).await;
        }
    }

    /// A real second process holding the lock keeps this one out until it
    /// exits: the guard works across processes, not only across sessions.
    #[tokio::test]
    async fn the_lock_excludes_another_process() {
        if std::env::var_os(CHILD_DIR).is_some() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "oauth::process_lock::tests::child_holds_the_lock",
                "--nocapture",
            ])
            .env(CHILD_DIR, dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let marker = dir.path().join("held");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !marker.exists() {
            assert!(Instant::now() < deadline, "the child never took the lock");
            tokio::time::sleep(POLL).await;
        }
        assert!(matches!(
            acquire(dir.path(), KEY, Duration::from_millis(200)).await,
            Err(OAuthError::Network)
        ));
        let path = dir.path().to_path_buf();
        let waiter = tokio::spawn(async move { acquire(&path, KEY, WAIT).await });
        std::fs::remove_file(&marker).unwrap();
        assert!(child.wait().await.unwrap().success());
        assert!(matches!(waiter.await.unwrap(), Ok(Some(_))));
    }
}
