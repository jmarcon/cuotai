//! Config identity each frontend last opened (PRD 0002 RF1.2), so the other
//! frontend can warn when both use different files (and so separate accounts
//! and model caches). Stored as `<data dir>/cuotai/last-config-<kind>.json`
//! holding only the normalized path and a timestamp; never account data.
//! A missing or unreadable record means "unknown" (no notice).

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
}

impl LastConfig {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
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
    /// `current`; `None` when equal, unknown or never recorded.
    pub fn other_differs(&self, me: Frontend, current: &ConfigIdentity) -> Option<PathBuf> {
        let other = self.last(me.other())?;
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

    #[test]
    fn notice_only_when_other_frontend_used_another_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = LastConfig::at(dir.path().join("data"));
        let a = dir.path().join("a.toml");
        let b = dir.path().join("b.toml");
        std::fs::write(&a, "").unwrap();
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
        let store = LastConfig::at(dir.path());
        assert_eq!(store.other_differs(Frontend::Tui, &id(&a)), None);
    }
}
