//! PRD 0033 RF5.3: the last-query record of a metered account, shared by
//! every process of the user so one Cost Explorer request serves them all
//! within the account's interval.
//!
//! The file `<dir>/<account digest>.json` (`0600`, in a `0700` directory)
//! holds the time of the last successful query and its report: a report
//! already carries no credential, URL or response body. An advisory lock on
//! a sibling `.lock` file serializes the read-decide-claim and the write
//! steps; it is never held while the network is used.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::{Account, Report, Status};

/// Debug builds only: the folder of the metered records instead of the
/// fixed per-user one (tests and manual smokes never touch the real one).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) const DEBUG_ENV: &str = "CUOTAI_METERED_DIR";

/// How long another process's claim on the next query is respected.
const CLAIM_TTL: TimeDelta = TimeDelta::minutes(2);

/// The record format: 2 binds the record to the account's settings.
const SCHEMA: u32 = 2;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    schema: u32,
    /// [`crate::models::restore_fingerprint`] of the account that wrote the
    /// record: provider, credential reference and every non-display param
    /// (for AWS Bedrock the profile, region, CLI path and override). No
    /// secret and no name. A record of other settings counts as absent.
    #[serde(default)]
    settings: Option<String>,
    queried_at: Option<DateTime<Utc>>,
    claimed_at: Option<DateTime<Utc>>,
    report: Option<Report>,
}

/// What [`Store::begin`] decided.
pub(crate) enum Decision {
    /// Another process queried within the interval: use its report.
    Reuse(Box<Report>),
    /// Query now.
    Query,
}

/// The directory of the records (`<OS local data dir>/cuotai/metered`).
#[derive(Debug, Clone)]
pub(crate) struct Store {
    dir: PathBuf,
}

impl Store {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// PRD 0033 re-validation M-4: one fixed per-user place,
    /// `<OS local data dir>/cuotai/metered`, whatever `CUOTAI_DATA_DIR` or
    /// the desktop's data folder says, so every front end shares one record
    /// of billed queries. Debug builds honour [`DEBUG_ENV`] for manual
    /// smokes and tests; release builds ignore it.
    pub(crate) fn default_dir() -> Option<PathBuf> {
        #[cfg(debug_assertions)]
        if let Some(dir) = std::env::var_os(DEBUG_ENV).filter(|dir| !dir.is_empty()) {
            return Some(PathBuf::from(dir));
        }
        dirs::data_local_dir().map(|dir| dir.join("cuotai").join("metered"))
    }

    /// The record of `account`, named after a digest of its stable id
    /// (PRD 0029 RF2). A record written under the legacy name key by an
    /// earlier build moves to the id name the first time it is looked up.
    fn path(&self, account: &Account) -> PathBuf {
        let path = self.file(&account.state_key());
        if !path.exists() {
            let legacy = self.file(&account.key());
            if legacy.exists() {
                let _ = std::fs::rename(&legacy, &path);
            }
        }
        path
    }

    fn file(&self, key: &str) -> PathBuf {
        let digest = crate::models::digest(key.as_bytes());
        self.dir.join(format!("{digest}.json"))
    }

    fn lock(&self, path: &Path) -> std::io::Result<File> {
        crate::private_fs::ensure_dir(&self.dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.lock()?;
        Ok(lock)
    }

    fn read(path: &Path) -> Record {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(path: &Path, record: &Record) {
        if let Ok(bytes) = serde_json::to_vec(record) {
            let _ = crate::private_fs::write_file(path, &bytes);
        }
    }

    /// Decide whether `account` may be queried at `now`. A query claims the
    /// next slot so a concurrent process reuses the previous report
    /// instead of paying for a second request. Any I/O problem lets the
    /// query go ahead: the record is an optimization, never a gate.
    pub(crate) fn begin(
        &self,
        account: &Account,
        interval: TimeDelta,
        now: DateTime<Utc>,
    ) -> Decision {
        let path = self.path(account);
        let Ok(lock) = self.lock(&path) else {
            return Decision::Query;
        };
        let settings = crate::models::restore_fingerprint(account);
        let mut record = Self::read(&path);
        // PRD 0033 re-validation H-1: a record written for other settings
        // (an edited profile, another config sharing the id, a schema-1
        // record without a digest) is never reused nor its claim honoured;
        // the next query overwrites it.
        if record.schema != SCHEMA || record.settings.as_deref() != Some(settings.as_str()) {
            record = Record {
                schema: SCHEMA,
                settings: Some(settings),
                ..Record::default()
            };
        }
        let fresh = record
            .queried_at
            .is_some_and(|at| at <= now && now - at < interval);
        let claimed = record
            .claimed_at
            .is_some_and(|at| at <= now && now - at < CLAIM_TTL);
        let decision = match record.report.clone() {
            Some(report) if report.status == Status::Ready && (fresh || claimed) => {
                Decision::Reuse(Box::new(report))
            }
            _ => Decision::Query,
        };
        if matches!(decision, Decision::Query) {
            record.claimed_at = Some(now);
            Self::write(&path, &record);
        }
        drop(lock);
        decision
    }

    /// Record the outcome of the query `begin` allowed.
    pub(crate) fn finish(&self, account: &Account, report: &Report, now: DateTime<Utc>) {
        let path = self.path(account);
        let Ok(lock) = self.lock(&path) else {
            return;
        };
        let settings = crate::models::restore_fingerprint(account);
        let mut record = Self::read(&path);
        if record.schema != SCHEMA || record.settings.as_deref() != Some(settings.as_str()) {
            record = Record {
                schema: SCHEMA,
                settings: Some(settings),
                ..Record::default()
            };
        }
        record.claimed_at = None;
        if report.status == Status::Ready {
            record.queried_at = Some(now);
            record.report = Some(report.clone());
        }
        Self::write(&path, &record);
        drop(lock);
    }
}

/// PRD 0033 re-validation H-1 (R-2): a reused report belongs to the
/// account reading it, not to the process that wrote it: its name and id,
/// and its own interval note instead of the writer's.
pub(crate) fn rebind(report: &mut Report, account: &Account) {
    report.account = account.name.clone();
    report.account_id = Some(account.id());
    report.strip_disguising_text();
    let keep: Vec<bool> = report
        .notes_l10n
        .iter()
        .map(|note| !note.id.starts_with("note.metered."))
        .collect();
    if keep.len() == report.notes.len() {
        let mut flags = keep.iter();
        report.notes.retain(|_| *flags.next().unwrap_or(&true));
    }
    report
        .notes_l10n
        .retain(|note| !note.id.starts_with("note.metered."));
    if let Some(note) = account.metered_interval_note() {
        report.notes.push(note.text().to_owned());
        report.notes_l10n.push(note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Provider;

    fn account() -> Account {
        Account {
            name: "b".into(),
            provider: Provider::AwsBedrock,
            ..Account::default()
        }
    }

    fn report(status: Status) -> Report {
        Report {
            overage_hint: Default::default(),
            account_id: None,
            diagnostic: None,
            sub_reports: Vec::new(),
            account: "b".into(),
            provider: Provider::AwsBedrock,
            checked_at: Utc::now(),
            status,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            as_of: None,
            breakdown: Vec::new(),
        }
    }

    #[test]
    fn one_query_per_interval_across_stores() {
        let dir = tempfile::tempdir().unwrap();
        let first = Store::new(dir.path().to_path_buf());
        let second = Store::new(dir.path().to_path_buf());
        let account = account();
        let interval = TimeDelta::hours(1);
        let t0 = Utc::now();
        assert!(matches!(
            first.begin(&account, interval, t0),
            Decision::Query
        ));
        first.finish(&account, &report(Status::Ready), t0);
        let soon = t0 + TimeDelta::minutes(59);
        assert!(matches!(
            second.begin(&account, interval, soon),
            Decision::Reuse(_)
        ));
        let later = t0 + TimeDelta::minutes(61);
        assert!(matches!(
            second.begin(&account, interval, later),
            Decision::Query
        ));
    }

    #[test]
    fn failures_are_not_recorded_and_do_not_block_a_retry() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let account = account();
        let t0 = Utc::now();
        let hour = TimeDelta::hours(1);
        assert!(matches!(store.begin(&account, hour, t0), Decision::Query));
        store.finish(&account, &report(Status::Error), t0);
        let next = t0 + TimeDelta::seconds(1);
        assert!(matches!(store.begin(&account, hour, next), Decision::Query));
    }

    /// PRD 0029 RF2: the record follows the stable id across a rename, and
    /// a record an earlier build wrote under the name key is adopted. It
    /// carries no settings digest (schema 1), so it counts as absent once
    /// (PRD 0033 re-validation H-1): one query, then reuse.
    #[test]
    fn the_record_is_keyed_by_the_id_and_adopts_the_legacy_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let account = Account {
            id: Some(crate::AccountId::parse("acc_0123456789ab").unwrap()),
            ..account()
        };
        let t0 = Utc::now();
        let hour = TimeDelta::hours(1);
        // Written under the legacy name key, as before the stable id.
        let legacy = store.file(&account.key());
        Store::write(
            &legacy,
            &Record {
                queried_at: Some(t0),
                report: Some(report(Status::Ready)),
                ..Record::default()
            },
        );
        let soon = t0 + TimeDelta::minutes(5);
        assert!(matches!(store.begin(&account, hour, soon), Decision::Query));
        assert!(!legacy.exists());
        store.finish(&account, &report(Status::Ready), soon);
        let renamed = Account {
            name: "renamed".into(),
            ..account.clone()
        };
        let later = soon + TimeDelta::minutes(1);
        let Decision::Reuse(mut reused) = store.begin(&renamed, hour, later) else {
            panic!("same settings under a new name must reuse");
        };
        rebind(&mut reused, &renamed);
        assert_eq!(reused.account, "renamed");
        assert_eq!(reused.account_id, Some(renamed.id()));
    }

    /// PRD 0033 re-validation H-1: an edited profile never reuses the
    /// figures of the previous one; it queries once and is then reused.
    #[test]
    fn a_changed_profile_queries_once_and_never_shows_the_old_figures() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let mut account = account();
        account.params.insert("profile".into(), "first".into());
        let t0 = Utc::now();
        let hour = TimeDelta::hours(1);
        assert!(matches!(store.begin(&account, hour, t0), Decision::Query));
        store.finish(&account, &report(Status::Ready), t0);
        account.params.insert("profile".into(), "second".into());
        let soon = t0 + TimeDelta::minutes(1);
        assert!(matches!(store.begin(&account, hour, soon), Decision::Query));
        store.finish(&account, &report(Status::Ready), soon);
        let later = soon + TimeDelta::minutes(1);
        assert!(matches!(
            store.begin(&account, hour, later),
            Decision::Reuse(_)
        ));
        // The first profile's record was overwritten: going back queries.
        account.params.insert("profile".into(), "first".into());
        assert!(matches!(
            store.begin(&account, hour, later),
            Decision::Query
        ));
    }

    /// Two ids with the same settings keep separate records.
    #[test]
    fn two_ids_with_the_same_settings_have_separate_records() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let first = Account {
            id: Some(crate::AccountId::parse("acc_0123456789ab").unwrap()),
            ..account()
        };
        let second = Account {
            id: Some(crate::AccountId::parse("acc_ba9876543210").unwrap()),
            ..account()
        };
        let t0 = Utc::now();
        let hour = TimeDelta::hours(1);
        assert!(matches!(store.begin(&first, hour, t0), Decision::Query));
        store.finish(&first, &report(Status::Ready), t0);
        assert!(matches!(store.begin(&second, hour, t0), Decision::Query));
    }

    /// The reused report drops the writer's interval note for the reader's.
    #[test]
    fn rebind_replaces_the_writers_interval_note() {
        let mut writer = account();
        writer.refresh_seconds = Some(600);
        let mut report = report(Status::Ready);
        let note = writer.metered_interval_note().unwrap();
        report.notes.push(note.text().to_owned());
        report.notes_l10n.push(note);
        let reader = account();
        rebind(&mut report, &reader);
        assert!(report.notes_l10n.is_empty());
        assert!(report.notes.is_empty());
    }
}
