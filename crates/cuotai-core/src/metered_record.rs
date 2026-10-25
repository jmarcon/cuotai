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

/// How long another process's claim on the next query is respected.
const CLAIM_TTL: TimeDelta = TimeDelta::minutes(2);

#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
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

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn default_dir() -> Option<PathBuf> {
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
        let mut record = Self::read(&path);
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
        let mut record = Self::read(&path);
        record.claimed_at = None;
        if report.status == Status::Ready {
            record.queried_at = Some(now);
            record.report = Some(report.clone());
        }
        Self::write(&path, &record);
        drop(lock);
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
    /// a record an earlier build wrote under the name key is adopted.
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
                claimed_at: None,
                report: Some(report(Status::Ready)),
            },
        );
        let soon = t0 + TimeDelta::minutes(5);
        assert!(matches!(
            store.begin(&account, hour, soon),
            Decision::Reuse(_)
        ));
        assert!(!legacy.exists());
        let renamed = Account {
            name: "renamed".into(),
            ..account.clone()
        };
        assert!(matches!(
            store.begin(&renamed, hour, soon),
            Decision::Reuse(_)
        ));
    }
}
