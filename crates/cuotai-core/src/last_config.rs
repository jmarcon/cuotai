//! Config identity each frontend last opened (PRD 0002 RF1.2), so the other
//! frontend can warn when both use different files (and so separate accounts
//! and model caches). Stored as `<data dir>/cuotai/last-config-<kind>.json`
//! holding only the normalized path and a timestamp; never account data.
//! A missing or unreadable record means "unknown" (no notice).
//!
//! Throwaway configs never count: a file under the OS temp dir (a test or
//! development run) is not recorded, and a recorded path that is under it or
//! no longer exists is ignored, so it cannot keep a stale notice alive.
//!
//! PRD 0051 RF5.4: a frontend's record may also carry the moved file whose
//! history it offered to reuse and that was not answered yet, so the offer
//! comes back at the next start until the person adopts or declines it.

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
    /// PRD 0051 RF5.4: the old path of an unanswered moved-file offer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending: Option<PathBuf>,
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

    /// The data directory ([`crate::data_dir::root`]), next to the model cache.
    pub fn in_data_dir() -> Option<Self> {
        crate::data_dir::root().map(Self::at)
    }

    /// Atomically record that `me` opened `identity` (file identities only).
    pub fn record(&self, me: Frontend, identity: &ConfigIdentity) -> std::io::Result<()> {
        let ConfigIdentity::File(path) = identity else {
            return Ok(());
        };
        if self.is_throwaway(path) {
            return Ok(());
        }
        // An unanswered moved-file offer survives the new record.
        let pending = self.read(me).and_then(|r| r.pending);
        self.write(
            me,
            &Record {
                path: path.clone(),
                at: chrono::Utc::now(),
                pending,
            },
        )
    }

    fn write(&self, me: Frontend, record: &Record) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let body = serde_json::to_vec(record)?;
        // tempfile creates 0600 files on Unix.
        let mut tmp = tempfile::NamedTempFile::new_in(&self.dir)?;
        tmp.write_all(&body)?;
        tmp.as_file().sync_all()?;
        crate::private_fs::persist(tmp, &self.dir.join(me.file()))
    }

    fn read(&self, kind: Frontend) -> Option<Record> {
        let bytes = std::fs::read(self.dir.join(kind.file())).ok()?;
        serde_json::from_slice::<Record>(&bytes).ok()
    }

    fn last(&self, kind: Frontend) -> Option<PathBuf> {
        self.read(kind).map(|r| r.path)
    }

    /// PRD 0051 RF5.4: the path whose history `me` may offer to reuse: the
    /// old path of an offer not answered yet, else the path `me` opened last
    /// (read before [`Self::record`] replaces it), even when it no longer
    /// exists; `None` when unknown or under the temp dir.
    pub fn previous(&self, me: Frontend) -> Option<PathBuf> {
        let record = self.read(me)?;
        record
            .pending
            .filter(|path| !self.is_throwaway(path))
            .or_else(|| Some(record.path).filter(|path| !self.is_throwaway(path)))
    }

    /// PRD 0051 RF5.4: keep `from` as the unanswered offer of `me` (shown
    /// again at the next start), or drop it with `None` once the person
    /// answered or no offer applies. Needs a record from [`Self::record`].
    pub fn set_pending(&self, me: Frontend, from: Option<&Path>) -> std::io::Result<()> {
        let Some(mut record) = self.read(me) else {
            return Ok(());
        };
        let from = from.map(Path::to_path_buf);
        if record.pending == from {
            return Ok(());
        }
        record.pending = from;
        self.write(me, &record)
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

/// PRD 0051 RF5.4: the path `me` opened last, before [`note`] records the
/// current one. Best effort.
pub fn previous(me: Frontend) -> Option<PathBuf> {
    LastConfig::in_data_dir()?.previous(me)
}

/// PRD 0051 RF5.4: keep (`Some`) or drop (`None`) the unanswered moved-file
/// offer of `me`. Best effort.
pub fn set_pending(me: Frontend, from: Option<&Path>) {
    if let Some(store) = LastConfig::in_data_dir() {
        let _ = store.set_pending(me, from);
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
                pending: None,
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

    /// PRD 0051 RF5.4: an unanswered offer outlives new records and is the
    /// path offered next time; answering drops it.
    #[test]
    fn an_unanswered_offer_comes_back_until_answered() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _tmp) = store(dir.path().join("data"));
        let old = dir.path().join("old.toml");
        let new = dir.path().join("new.toml");
        std::fs::write(&new, "").unwrap();
        // No record yet: nothing to keep.
        store.set_pending(Frontend::Desktop, Some(&old)).unwrap();
        assert_eq!(store.previous(Frontend::Desktop), None);
        store.record(Frontend::Desktop, &id(&new)).unwrap();
        let ConfigIdentity::File(new_real) = id(&new) else {
            unreachable!()
        };
        assert_eq!(store.previous(Frontend::Desktop), Some(new_real.clone()));
        // The offer is kept across the next start's record.
        store.set_pending(Frontend::Desktop, Some(&old)).unwrap();
        store.record(Frontend::Desktop, &id(&new)).unwrap();
        assert_eq!(store.previous(Frontend::Desktop), Some(old.clone()));
        // The other frontend's notice still compares the opened path.
        assert_eq!(store.last(Frontend::Desktop), Some(new_real.clone()));
        // An answer drops it.
        store.set_pending(Frontend::Desktop, None).unwrap();
        assert_eq!(store.previous(Frontend::Desktop), Some(new_real));
        // A record written by an older build (no `pending`) still reads.
        let body = std::fs::read(dir.path().join("data").join(Frontend::Desktop.file())).unwrap();
        assert!(!String::from_utf8(body).unwrap().contains("pending"));
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
