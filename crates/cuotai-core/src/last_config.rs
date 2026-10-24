//! Config identity each frontend last opened (PRD 0002 RF1.2), so the other
//! frontend can warn when both use different files (and so separate accounts
//! and model caches). Stored as `<data dir>/cuotai/last-config-<kind>.json`
//! holding only the normalized path and a timestamp; never account data.
//! A missing or unreadable record means "unknown" (no notice).
//!
//! Throwaway configs never count: a file under the OS temp dir (a test or
//! development run) is not recorded, and a recorded path that is under it or
//! no longer exists is ignored, so it cannot keep a stale notice alive.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config_file::ConfigIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frontend {
    Tui,
    Desktop,
}

impl Frontend {
    fn other(self) -> Self {
        match self {
            Self::Tui => Self::Desktop,
            Self::Desktop => Self::Tui,
        }
    }
    fn file(self) -> &'static str {
        match self {
            Self::Tui => "last-config-tui.json",
            Self::Desktop => "last-config-desktop.json",
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Record {
    path: PathBuf,
    at: chrono::DateTime<chrono::Utc>,
}

/// Directory holding the records; injectable so tests never touch OS dirs.
#[derive(Debug, Clone)]
pub struct LastConfig {
    dir: PathBuf,
    temp_roots: Vec<PathBuf>,
}

/// Canonical roots of the OS temp dirs: `std::env::temp_dir()` (on macOS
/// `/var/folders/…` → `/private/var/folders/…`) and, on Unix, `/tmp` (on
/// macOS `/private/tmp`).
pub fn system_temp_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir()];
    if cfg!(unix) {
        roots.push(PathBuf::from("/tmp"));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        let real = std::fs::canonicalize(&root).unwrap_or(root);
        if !out.contains(&real) {
            out.push(real);
        }
    }
    out
}

impl LastConfig {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            temp_roots: system_temp_roots(),
        }
    }

    /// Replace the temp roots (tests keep their fixtures in the OS temp dir).
    pub fn with_temp_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.temp_roots = roots;
        self
    }

    /// `path` lies under a temp root, compared canonically (a missing file
    /// resolves through its parent, as in [`ConfigIdentity::of_path`]).
    fn is_throwaway(&self, path: &Path) -> bool {
        let real = match ConfigIdentity::of_path(path) {
            Ok(ConfigIdentity::File(real)) => real,
            _ => path.to_path_buf(),
        };
        self.temp_roots
            .iter()
            .any(|root| real.starts_with(root) || path.starts_with(root))
    }

    /// `<dirs::data_local_dir()>/cuotai`, next to the model cache.
    pub fn in_data_dir() -> Option<Self> {
        dirs::data_local_dir().map(|d| Self::at(d.join("cuotai")))
    }

    /// Atomically record that `me` opened `identity` (file identities only).
    pub fn record(&self, me: Frontend, identity: &ConfigIdentity) -> std::io::Result<()> {
        let ConfigIdentity::File(path) = identity else {
            return Ok(());
        };
        if self.is_throwaway(path) {
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir)?;
        let body = serde_json::to_vec(&Record {
            path: path.clone(),
            at: chrono::Utc::now(),
        })?;
        // tempfile creates 0600 files on Unix.
        let mut tmp = tempfile::NamedTempFile::new_in(&self.dir)?;
        tmp.write_all(&body)?;
        tmp.as_file().sync_all()?;
        tmp.persist(self.dir.join(me.file())).map_err(|e| e.error)?;
        Ok(())
    }

    fn last(&self, kind: Frontend) -> Option<PathBuf> {
        let bytes = std::fs::read(self.dir.join(kind.file())).ok()?;
        serde_json::from_slice::<Record>(&bytes)
            .ok()
            .map(|r| r.path)
    }

    /// Path last opened by the other frontend when it differs from
    /// `current`; `None` when equal, unknown, never recorded, gone or under
    /// the temp dir.
    pub fn other_differs(&self, me: Frontend, current: &ConfigIdentity) -> Option<PathBuf> {
        let other = self.last(me.other())?;
        if !other.exists() || self.is_throwaway(&other) {
            return None;
        }
        let same = matches!(current, ConfigIdentity::File(p) if Path::new(p) == other)
            || ConfigIdentity::of_path(&other).ok().as_ref() == Some(current);
        (!same).then_some(other)
    }
}

/// Record `me`'s identity and return the other frontend's differing path.
/// Best effort: any storage failure just means no notice.
pub fn note(me: Frontend, file: &crate::config_file::ConfigFile) -> Option<PathBuf> {
    let store = LastConfig::in_data_dir()?;
    let id = file.identity().ok()?;
    let _ = store.record(me, &id);
    store.other_differs(me, &id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(path: &Path) -> ConfigIdentity {
        ConfigIdentity::of_path(path).unwrap()
    }

    /// Fixtures live in the OS temp dir, so tests point the temp roots at
    /// a directory of their own.
    fn store(data: PathBuf) -> (LastConfig, tempfile::TempDir) {
        let fake_tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(fake_tmp.path()).unwrap();
        (LastConfig::at(data).with_temp_roots(vec![root]), fake_tmp)
    }

    #[test]
    fn notice_only_when_other_frontend_used_another_file() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _tmp) = store(dir.path().join("data"));
        let a = dir.path().join("a.toml");
        let b = dir.path().join("b.toml");
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        // Other frontend never ran: unknown.
        assert_eq!(store.other_differs(Frontend::Tui, &id(&a)), None);
        // Own record never counts as the other's.
        store.record(Frontend::Tui, &id(&b)).unwrap();
        assert_eq!(store.other_differs(Frontend::Tui, &id(&a)), None);
        store.record(Frontend::Desktop, &id(&a)).unwrap();
        assert_eq!(store.other_differs(Frontend::Tui, &id(&a)), None);
        let ConfigIdentity::File(b_real) = id(&b) else {
            unreachable!()
        };
        assert_eq!(
            store.other_differs(Frontend::Desktop, &id(&a)),
            Some(b_real)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(dir.path().join("data").join(Frontend::Tui.file()));
            assert_eq!(meta.unwrap().permissions().mode() & 0o077, 0);
            let link = dir.path().join("link.toml");
            std::os::unix::fs::symlink(&a, &link).unwrap();
            assert_eq!(store.other_differs(Frontend::Tui, &id(&link)), None);
        }
    }

    #[test]
    fn corrupt_record_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(Frontend::Desktop.file()), b"{not json").unwrap();
        let a = dir.path().join("a.toml");
        let (store, _tmp) = store(dir.path().to_path_buf());
        assert_eq!(store.other_differs(Frontend::Tui, &id(&a)), None);
    }

    #[test]
    fn temp_dir_config_is_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let (store, tmp) = store(dir.path().join("data"));
        let throwaway = tmp.path().join("cuotai-test.toml");
        std::fs::write(&throwaway, "").unwrap();
        store.record(Frontend::Tui, &id(&throwaway)).unwrap();
        assert!(!dir.path().join("data").join(Frontend::Tui.file()).exists());
        // An earlier real record survives a later throwaway run.
        let real = dir.path().join("real.toml");
        std::fs::write(&real, "").unwrap();
        store.record(Frontend::Tui, &id(&real)).unwrap();
        store.record(Frontend::Tui, &id(&throwaway)).unwrap();
        let ConfigIdentity::File(real_path) = id(&real) else {
            unreachable!()
        };
        assert_eq!(store.last(Frontend::Tui), Some(real_path));
    }

    /// The owner's case: a record written before this rule (or by an older
    /// build) that points at the temp dir, and one whose file was deleted.
    #[test]
    fn temp_or_missing_recorded_path_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let (store, tmp) = store(data.clone());
        let app = dir.path().join("app.toml");
        std::fs::write(&app, "").unwrap();
        let write = |path: &Path| {
            std::fs::create_dir_all(&data).unwrap();
            let body = serde_json::to_vec(&Record {
                path: path.to_path_buf(),
                at: chrono::Utc::now(),
            })
            .unwrap();
            std::fs::write(data.join(Frontend::Tui.file()), body).unwrap();
        };
        let throwaway = std::fs::canonicalize(tmp.path()).unwrap().join("t.toml");
        std::fs::write(&throwaway, "").unwrap();
        write(&throwaway);
        assert_eq!(store.other_differs(Frontend::Desktop, &id(&app)), None);
        let gone = dir.path().join("gone.toml");
        write(&gone);
        assert_eq!(store.other_differs(Frontend::Desktop, &id(&app)), None);
        let other = std::fs::canonicalize(dir.path())
            .unwrap()
            .join("other.toml");
        std::fs::write(&other, "").unwrap();
        write(&other);
        assert_eq!(
            store.other_differs(Frontend::Desktop, &id(&app)),
            Some(other)
        );
    }

    #[test]
    fn system_temp_roots_cover_the_os_temp_dir() {
        let roots = system_temp_roots();
        let tmp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        assert!(roots.contains(&tmp));
        let store = LastConfig::at("unused");
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(store.is_throwaway(file.path()));
        #[cfg(unix)]
        {
            assert!(store.is_throwaway(Path::new("/tmp/cuotai-metered-final.toml")));
            assert!(!store.is_throwaway(Path::new("/opt/cuotai/cuotai.toml")));
        }
    }
}
