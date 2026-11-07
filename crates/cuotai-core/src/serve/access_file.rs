//! PRD 0050 RF6.3: the desktop's private access log of the local server.
//!
//! Entries ([`super::AccessEntry`], already free of addresses, headers,
//! tokens, queries and account keys) are appended as one line each to
//! `<data root>/logs/serve-access.log` through [`crate::private_fs`]
//! (`0700` directory, `0600` file on Unix). Before a line would take the
//! file past [`ACCESS_LOG_ROTATE_BYTES`] it moves to `serve-access.log.1`,
//! replacing the previous one, so the two never hold more than 2 MiB.
//! Nothing is created until the first entry is written, so with
//! [`super::AccessLogMode::Off`] (which builds no entry) no file appears.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::private_fs::RotatingFile;

use super::{AccessEntry, AccessLog};

/// Each generation of the access log stays at or below 1 MiB.
pub const ACCESS_LOG_ROTATE_BYTES: u64 = 1024 * 1024;

/// `<root>/logs/serve-access.log`, where `root` is the data root
/// ([`crate::data_dir::root`]).
pub fn access_log_path(root: &Path) -> PathBuf {
    root.join("logs").join("serve-access.log")
}

/// The access log under the data root, `None` without one.
pub fn default_access_log_path() -> Option<PathBuf> {
    crate::data_dir::root().map(|root| access_log_path(&root))
}

/// The rotating private file behind a desktop access log.
#[derive(Debug)]
pub struct AccessLogFile {
    file: RotatingFile,
}

impl AccessLogFile {
    /// A log at `path` rotating at [`ACCESS_LOG_ROTATE_BYTES`]. Creates
    /// nothing yet.
    pub fn new(path: PathBuf) -> Self {
        Self::with_limit(path, ACCESS_LOG_ROTATE_BYTES)
    }

    /// The same with another limit (tests).
    pub fn with_limit(path: PathBuf, limit: u64) -> Self {
        Self {
            file: RotatingFile::new(path, limit),
        }
    }

    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// `<file>.1`, the rotated generation.
    pub fn older(&self) -> PathBuf {
        self.file.older()
    }

    /// Append one entry; a failure drops the line (logging never stops
    /// the server).
    pub fn record(&self, entry: &AccessEntry) {
        if let Some(dir) = self.path().parent().filter(|d| !d.as_os_str().is_empty()) {
            if crate::private_fs::ensure_dir(dir).is_err() {
                return;
            }
        }
        let _ = self.file.append(format!("{entry}\n").as_bytes());
    }

    /// The [`AccessLog`] callback for [`super::ServeConfig::access_log`].
    pub fn sink(self: &Arc<Self>) -> AccessLog {
        let file = Arc::clone(self);
        Arc::new(move |entry| file.record(entry))
    }

    /// Remove both generations; a missing file is not an error. Symbolic
    /// links are removed, never followed.
    pub fn erase(&self) -> io::Result<()> {
        for path in [self.path().to_path_buf(), self.older()] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Bytes of both generations (missing files count zero).
    pub fn size(&self) -> u64 {
        [self.path().to_path_buf(), self.older()]
            .iter()
            .filter_map(|path| std::fs::symlink_metadata(path).ok())
            .map(|meta| meta.len())
            .sum()
    }

    /// The text of the current file, then the rotated one (tests and
    /// diagnostics of the line format only).
    pub fn generations(&self) -> [String; 2] {
        self.file.generations()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve::PeerClass;
    use std::time::Duration;

    fn entry(status: u16) -> AccessEntry {
        AccessEntry {
            at: chrono::Utc::now(),
            peer: PeerClass::Loopback,
            method: "GET",
            route: "/v1/accounts/:key",
            status,
            elapsed: Duration::from_millis(3),
        }
    }

    #[test]
    fn nothing_is_created_until_an_entry_is_recorded_and_erase_removes_both() {
        let dir = tempfile::tempdir().unwrap();
        let path = access_log_path(dir.path());
        let log = Arc::new(AccessLogFile::with_limit(path.clone(), 200));
        assert!(
            !path.parent().unwrap().exists(),
            "no directory before a line"
        );
        let sink = log.sink();
        for _ in 0..8 {
            sink(&entry(404));
        }
        assert!(
            path.exists() && log.older().exists(),
            "rotated at the limit"
        );
        for file in [path.clone(), log.older()] {
            let len = std::fs::metadata(&file).unwrap().len();
            assert!(len <= 200, "{} holds {len} bytes", file.display());
        }
        let [current, _] = log.generations();
        assert!(current.contains("loopback GET /v1/accounts/:key 404 3ms"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
        log.erase().unwrap();
        assert!(!path.exists() && !log.older().exists());
        assert_eq!(log.size(), 0);
        log.erase().unwrap();
    }

    #[test]
    fn the_default_limit_is_one_mebibyte() {
        assert_eq!(ACCESS_LOG_ROTATE_BYTES, 1_048_576);
    }
}
