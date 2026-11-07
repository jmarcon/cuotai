//! PRD 0051 RF6: the compatibility matrix over the immutable fixtures in
//! `tests/fixtures/compat/<contract>/major-<M>-revision-<R>/`.
//!
//! Each fixture directory has a `manifest.json` (writer release, the
//! compatibility this release must find, the SHA-256 of every data file and
//! the frozen reader that checks it). [`compat_matrix`] enumerates them,
//! verifies the hashes and runs the reader:
//!
//! - `current`: this release reads it as its own;
//! - `newer_revision`: a later additive revision of the same major (what
//!   the next release may write): every value this release knows reads
//!   back unchanged, and a write keeps the unknown material;
//! - `newer_major`: refused with the typed error, the file byte-identical;
//! - `older`: migrated by this release, with the `.bak` of the old state.
//!
//! [`n_minus_1`] freezes what this release (vNEXT) reads from each
//! contract, so the next release's fixtures are checked against it. Never
//! edit a shipped fixture: add a revision (`cargo xtask compat-check`
//! refuses a changed one).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The fixtures' root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compat")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub contract: String,
    pub major: u32,
    pub revision: u32,
    pub writer: String,
    pub expected: String,
    pub reader: String,
    #[serde(default)]
    pub backup: bool,
    pub files: BTreeMap<String, String>,
}

fn manifests() -> Vec<(PathBuf, Manifest)> {
    let mut out = Vec::new();
    for contract in std::fs::read_dir(root()).unwrap() {
        let contract = contract.unwrap().path();
        if !contract.is_dir() {
            continue;
        }
        for fixture in std::fs::read_dir(&contract).unwrap() {
            let dir = fixture.unwrap().path();
            let text = std::fs::read_to_string(dir.join("manifest.json"))
                .unwrap_or_else(|_| panic!("{} has no manifest.json", dir.display()));
            let manifest: Manifest =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            out.push((dir, manifest));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A copy of `file` of `dir` in a fresh temporary folder (fixtures are
/// never opened in place).
fn scratch(dir: &Path, file: &str) -> (tempfile::TempDir, PathBuf, Vec<u8>) {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = std::fs::read(dir.join(file)).unwrap();
    let path = tmp.path().join(file);
    std::fs::write(&path, &bytes).unwrap();
    (tmp, path, bytes)
}

/// An SQLite file built from the fixture's `.sql` dump, named `name`.
fn sqlite_from(dir: &Path, sql: &str, name: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(name);
    let text = std::fs::read_to_string(dir.join(sql)).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(&text).unwrap();
    // Every Cuotai store is in WAL mode; a dump does not keep it.
    let _: String = db
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .unwrap();
    drop(db);
    (tmp, path)
}

/// The account every fixture refers to (a fixed id, so digests match).
fn account() -> crate::Account {
    crate::Config::from_toml(
        "[[accounts]]\nid = \"acc_0000000000c0\"\nname = \"compat\"\nprovider = \"groq\"\ncredential_env = \"GROQ_API_KEY\"\n",
    )
    .unwrap()
    .accounts
    .remove(0)
}

fn identity() -> crate::config_file::ConfigIdentity {
    crate::config_file::ConfigIdentity::Explicit("compat".into())
}

fn t0() -> chrono::DateTime<chrono::Utc> {
    use chrono::TimeZone;
    chrono::Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap()
}

/// An export of no account, for the widget publisher.
fn empty_export() -> crate::export::v1::Export {
    let config = crate::Config {
        accounts: Vec::new(),
        ..crate::Config::default()
    };
    let cycle = crate::refresh::RefreshCycle::new(&config, std::time::Instant::now());
    crate::export::v1::Export::from_cycle(
        &config,
        &identity(),
        &cycle,
        &[],
        t0(),
        crate::export::v1::Options::default(),
    )
}

fn metric(used: f64) -> crate::Metric {
    crate::Metric {
        billing: Default::default(),
        scope: None,
        label: "5h".into(),
        used: Some(used),
        limit: Some(100.0),
        remaining: None,
        unit: "%".into(),
        kind: crate::MetricKind::Quota,
        window: Some(crate::MetricWindow::Window5h),
        period: Some("5h".into()),
        resets_at: Some(t0() + chrono::TimeDelta::hours(5)),
        derived_from: None,
    }
}

/// PRD 0051 RF6.2: what this release (vNEXT) reads from each contract,
/// frozen. The next release keeps these functions as they are and adds its
/// own beside them.
pub(crate) mod n_minus_1 {
    use super::*;

    /// `cuotai.toml`: the tolerant reader keeps the known accounts ready.
    pub fn config(dir: &Path, m: &Manifest) {
        let text = std::fs::read_to_string(dir.join(m.files.keys().next().unwrap())).unwrap();
        let parsed =
            crate::config_file::parse(&text, crate::config_file::ConfigParseMode::Tolerant)
                .unwrap();
        parsed.config.validate().unwrap();
        assert!(!parsed.config.accounts.is_empty(), "{}", dir.display());
        assert!(
            crate::Config::from_toml(&text).is_err(),
            "strict names the new syntax"
        );
    }

    /// `notified.json`: the members this release reads.
    #[derive(Debug, Deserialize, PartialEq)]
    struct Ledger {
        #[serde(default)]
        notified: Vec<Notified>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Notified {
        digest: String,
        resets_at: chrono::DateTime<chrono::Utc>,
        at: chrono::DateTime<chrono::Utc>,
        #[serde(default)]
        skipped: bool,
    }

    pub fn notified(dir: &Path, m: &Manifest) {
        let (_tmp, path, bytes) = scratch(dir, "notified.json");
        let ledger = crate::resets::Ledger::load(&path);
        match m.expected.as_str() {
            "newer_major" => {
                assert!(ledger.is_frozen());
                let mut ledger = ledger;
                ledger.claim(&path, |_| ());
                assert_eq!(std::fs::read(&path).unwrap(), bytes, "never rewritten");
            }
            _ => {
                assert!(!ledger.is_frozen());
                let frozen: Ledger = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(frozen.notified.len(), 1, "the known entry reads back");
                let mut ledger = ledger;
                ledger.claim(&path, |_| ());
                let after: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                let before: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let again: Ledger = serde_json::from_value(after.clone()).unwrap();
                assert_eq!(again, frozen, "known values unchanged");
                for (key, value) in before.as_object().unwrap() {
                    if !matches!(key.as_str(), "notified" | "alerts") {
                        assert_eq!(after.get(key), Some(value), "kept {key}");
                    }
                }
            }
        }
    }

    pub fn hooks(dir: &Path, m: &Manifest) {
        let (_tmp, path, bytes) = scratch(dir, "hooks.toml");
        let document = crate::hooks::Document::read(&path);
        match m.expected.as_str() {
            "newer_major" => {
                assert_eq!(
                    document.hooks().as_ref().unwrap_err().id,
                    "state.error.newer_major"
                );
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            }
            _ => {
                let hooks = document.hooks();
                assert_eq!(hooks.as_ref().unwrap().rules()[0].name, "notify");
                let drafts = document.drafts().unwrap().to_vec();
                crate::hooks::Document::save(&path, document.revision(), drafts).unwrap();
                let saved = std::fs::read_to_string(&path).unwrap();
                let before = String::from_utf8(bytes).unwrap();
                assert_eq!(saved, before, "an unchanged save keeps the file");
            }
        }
    }

    /// `widget.json`: the members a widget of this release reads.
    #[derive(Debug, Deserialize)]
    struct Widget {
        schema: u32,
        accounts: Vec<WidgetAccount>,
    }

    #[derive(Debug, Deserialize)]
    struct WidgetAccount {
        key: String,
        state: String,
    }

    pub fn widget(dir: &Path, m: &Manifest) {
        let (_tmp, path, bytes) = scratch(dir, "widget.json");
        if m.expected != "newer_major" {
            let widget: Widget = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(widget.schema, 1);
            assert_eq!(widget.accounts[0].key, "groq:compat");
            assert_eq!(widget.accounts[0].state, "ready");
        }
        // Whatever it is, the publisher keeps another version as a backup.
        let export = empty_export();
        crate::widget_snapshot::write(&export, &path, 6).unwrap();
        let bak = crate::private_fs::backup_path(&path);
        if m.expected == "current" {
            assert!(!bak.exists());
        } else {
            assert_eq!(std::fs::read(&bak).unwrap(), bytes, "kept as .bak");
        }
    }

    pub fn history(dir: &Path, m: &Manifest) {
        let (_tmp, path) = sqlite_from(dir, "history.sql", "history.sqlite3");
        let store = crate::history::HistoryStore::new(path.clone(), &identity());
        let account = account();
        match m.expected.as_str() {
            "newer_major" => {
                let before = std::fs::read(&path).unwrap();
                assert_eq!(
                    store.latest(&account).map(drop),
                    Err(crate::history::HistoryError::NewerMajor)
                );
                assert_eq!(std::fs::read(&path).unwrap(), before);
            }
            expected => {
                let latest = store.latest(&account).unwrap().expect("a stored reading");
                assert_eq!(latest.metrics[0].metric.used, Some(42.0));
                store
                    .record(
                        &account,
                        t0() + chrono::TimeDelta::hours(1),
                        &[metric(43.0)],
                    )
                    .unwrap();
                let samples: i64 = rusqlite::Connection::open(&path)
                    .unwrap()
                    .query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(samples, 2);
                let bak = crate::private_fs::backup_path(&path);
                assert_eq!(bak.exists(), expected == "older", "{}", dir.display());
                if expected == "older" {
                    let version: i64 = rusqlite::Connection::open(&bak)
                        .unwrap()
                        .query_row("PRAGMA user_version", [], |r| r.get(0))
                        .unwrap();
                    assert_eq!(version, i64::from(m.revision), "the .bak is the old state");
                }
            }
        }
    }

    pub fn models(dir: &Path, m: &Manifest) {
        let (_tmp, path) = sqlite_from(dir, "models.sql", "models.sqlite3");
        let cache = crate::models::ModelCache::new(path.clone(), &identity());
        let account = account();
        match m.expected.as_str() {
            "newer_major" => {
                let before = std::fs::read(&path).unwrap();
                assert_eq!(
                    cache.state(&account).map(drop),
                    Err(crate::models::ModelError::NewerMajor)
                );
                assert_eq!(std::fs::read(&path).unwrap(), before);
            }
            _ => {
                let state = cache.state(&account).unwrap().expect("a stored catalog");
                let catalog = state.catalog().expect("ready");
                assert_eq!(catalog.models[0].id, "compat-model");
            }
        }
    }

    pub fn cost(dir: &Path, m: &Manifest) {
        let (_tmp, path) = sqlite_from(dir, "cost.sql", "cost.sqlite3");
        let index = crate::cost::index::CostIndex::new(path.clone());
        match m.expected.as_str() {
            "newer_major" => {
                let before = std::fs::read(&path).unwrap();
                assert_eq!(
                    index.indexed(crate::cost::SourceId::CodexCli).map(drop),
                    Err(crate::cost::CostError::NewerMajor)
                );
                assert_eq!(std::fs::read(&path).unwrap(), before);
            }
            expected => {
                let indexed = index.indexed(crate::cost::SourceId::CodexCli).unwrap();
                assert_eq!(indexed.files, 1);
                let bak = crate::private_fs::backup_path(&path);
                assert_eq!(bak.exists(), expected == "older", "{}", dir.display());
            }
        }
    }
}

/// The frozen reader a manifest names.
fn run_reader(dir: &Path, m: &Manifest) {
    match m.reader.as_str() {
        "n_minus_1::config" => n_minus_1::config(dir, m),
        "n_minus_1::notified" => n_minus_1::notified(dir, m),
        "n_minus_1::hooks" => n_minus_1::hooks(dir, m),
        "n_minus_1::widget" => n_minus_1::widget(dir, m),
        "n_minus_1::history" => n_minus_1::history(dir, m),
        "n_minus_1::models" => n_minus_1::models(dir, m),
        "n_minus_1::cost" => n_minus_1::cost(dir, m),
        "desktop" => {} // Checked by the desktop crate's compat test.
        other => panic!("{}: unknown reader {other}", dir.display()),
    }
}

/// PRD 0051 RF6.2/RF6.4: every fixture verifies and passes its reader.
#[test]
fn compat_matrix() {
    let all = manifests();
    assert!(!all.is_empty());
    for (dir, m) in &all {
        let name = dir.file_name().unwrap().to_string_lossy();
        assert_eq!(
            name,
            format!("major-{}-revision-{}", m.major, m.revision),
            "{}",
            dir.display()
        );
        assert!(!m.writer.is_empty(), "{}: writer recorded", dir.display());
        for (file, hash) in &m.files {
            let bytes = std::fs::read(dir.join(file))
                .unwrap_or_else(|_| panic!("{}: missing {file}", dir.display()));
            assert_eq!(&sha256(&bytes), hash, "{}: {file} changed", dir.display());
        }
        assert!(
            ["current", "newer_revision", "newer_major", "older"].contains(&m.expected.as_str()),
            "{}",
            dir.display()
        );
        assert_eq!(
            m.backup,
            m.expected == "older",
            "{}: .bak expectation",
            dir.display()
        );
        run_reader(dir, m);
    }
    // Every versioned contract has the three compatibility cases.
    for contract in crate::state_contracts::VERSIONED {
        for expected in ["current", "newer_revision", "newer_major"] {
            assert!(
                all.iter()
                    .any(|(_, m)| m.contract == contract && m.expected == expected),
                "{contract} has no {expected} fixture"
            );
        }
    }
}

/// Builds the SQLite fixtures' starting files (run by hand with
/// `CUOTAI_COMPAT_OUT=<dir> cargo test -p cuotai-core generate_compat_fixtures
/// -- --ignored`, then dump each with `sqlite3 <file> .dump`). Kept so a new
/// revision is made the same way; shipped fixtures are never regenerated.
#[test]
#[ignore]
fn generate_compat_fixtures() {
    let out = PathBuf::from(std::env::var("CUOTAI_COMPAT_OUT").expect("CUOTAI_COMPAT_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let account = account();
    // History at this release's revision, with one reading.
    let history =
        crate::history::HistoryStore::new(out.join("history-current.sqlite3"), &identity());
    history.record(&account, t0(), &[metric(42.0)]).unwrap();
    // History at the previous revision (6), one series and one sample.
    let older = out.join("history-older.sqlite3");
    let mut db = rusqlite::Connection::open(&older).unwrap();
    crate::history::migrate(&mut db, &crate::history::MIGRATIONS[..6]).unwrap();
    // Model cache at this revision, one ready catalog.
    let models = crate::models::ModelCache::new(out.join("models-current.sqlite3"), &identity());
    let _ = models.state(&account).unwrap();
    let catalog = crate::models::ModelCatalog {
        models: vec![crate::models::ModelInfo {
            id: "compat-model".into(),
            name: None,
            description: None,
        }],
        source: crate::text::Localized::fixed("models.source.compat", "compat"),
        fetched_at: t0(),
    };
    rusqlite::Connection::open(out.join("models-current.sqlite3"))
        .unwrap()
        .execute(
            "INSERT INTO model_catalogs (config_key, account_key, status, catalog, attempted_at)
             VALUES (?1, ?2, 'ready', ?3, ?4)",
            [
                identity().digest(),
                crate::models::account_key(&account),
                serde_json::to_string(&catalog).unwrap(),
                t0().to_rfc3339(),
            ],
        )
        .unwrap();
    // Cost index at this revision and the previous one, one followed file.
    for (name, steps) in [
        ("cost-current.sqlite3", crate::cost::index::MIGRATIONS.len()),
        (
            "cost-older.sqlite3",
            crate::cost::index::MIGRATIONS.len() - 1,
        ),
    ] {
        let mut db = rusqlite::Connection::open(out.join(name)).unwrap();
        crate::history::migrate(&mut db, &crate::cost::index::MIGRATIONS[..steps]).unwrap();
        db.execute(
            "INSERT INTO files (source, file, identity, examined, position, state, invalid, read_at)
             VALUES ('codex_cli', 'f', 'i', 0, 0, '{}', 0, 0)",
            [],
        )
        .unwrap();
    }
}
