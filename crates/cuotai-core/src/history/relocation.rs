//! PRD 0051 RF5.4/RF5.5: carry the history of a configuration file that
//! moved.
//!
//! The history is keyed by the digest of the TOML's canonical path, so a
//! moved or renamed file starts empty. [`HistoryStore::relocation_candidate`]
//! offers the old history only when the previous path no longer exists and
//! the accounts it holds are exactly the accounts of the current file, by
//! stable id (an account without an `id` line derives its id from the path,
//! so it never matches); names and providers are never compared. The front
//! end asks first; [`HistoryStore::adopt`] then moves the rows in one
//! transaction after a `.bak` of the database. Nothing is written when the
//! user declines, and no non-interactive mode adopts.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::TransactionBehavior;

use super::{digest, HistoryError, HistoryStore};
use crate::config_file::ConfigIdentity;
use crate::Account;

/// A history the current configuration can adopt.
///
/// Its texts (`history.relocation.*`) name the old path with the home as
/// `~`, the reading count and the account count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocation {
    /// The previous path of the TOML (gone).
    pub from: PathBuf,
    /// Samples stored under it.
    pub samples: u64,
    /// Accounts they belong to.
    pub accounts: usize,
    from_key: String,
}

/// The offer for `file` when `previous` is the path this front end opened
/// last ([`crate::last_config::previous`], read before the new one is
/// recorded). Best effort: any storage failure means no offer.
pub fn relocation_offer(
    file: &crate::config_file::ConfigFile,
    previous: Option<&Path>,
) -> Option<(HistoryStore, Relocation)> {
    let previous = previous?;
    let store = HistoryStore::in_data_dir(file).ok()?;
    let relocation = store
        .relocation_candidate(previous, &file.config.accounts)
        .ok()??;
    Some((store, relocation))
}

/// The series identity of `account` (see [`super::series_digest`]).
fn id_digest(account: &Account) -> String {
    digest(format!("cuotai.series.v2\u{1f}{}", account.state_key()).as_bytes())
}

impl Relocation {
    /// The question a front end asks.
    pub fn prompt(&self) -> crate::text::Localized {
        use crate::text::{params, ParamValue};
        crate::text::Localized::with_params(
            "history.relocation.prompt",
            params([
                ("path", ParamValue::from(crate::text::home_relative(&self.from))),
                ("count", ParamValue::from(self.samples)),
                ("accounts", ParamValue::from(self.accounts)),
            ]),
            "The configuration file moved: {path} no longer exists and its history (readings: {count}; accounts: {accounts}) has exactly the accounts of this file. Use that history here?",
        )
    }

    /// What adopting did.
    pub fn adopted(moved: u64) -> crate::text::Localized {
        crate::text::Localized::with_param(
            "history.relocation.adopted",
            "count",
            moved,
            "History carried over; readings: {count}.",
        )
    }
}

impl HistoryStore {
    /// PRD 0051 RF5.4: the history of `previous` (the path this front end
    /// last opened) that this store's configuration, with `accounts`, may
    /// adopt. `None` unless `previous` is a different file that no longer
    /// exists and its history holds exactly these accounts' ids.
    pub fn relocation_candidate(
        &self,
        previous: &Path,
        accounts: &[Account],
    ) -> Result<Option<Relocation>, HistoryError> {
        if previous.exists() || accounts.is_empty() {
            return Ok(None);
        }
        let from_key = ConfigIdentity::File(previous.to_path_buf()).digest();
        if from_key == self.config_key {
            return Ok(None);
        }
        let Some(db) = self.open_existing()? else {
            return Ok(None);
        };
        let mut statement = db.prepare(
            "SELECT DISTINCT account_digest FROM series s
             WHERE config_key = ?1 AND EXISTS (SELECT 1 FROM samples WHERE series_id = s.id)",
        )?;
        let stored: BTreeSet<String> = statement
            .query_map([&from_key], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let current: BTreeSet<String> = accounts.iter().map(id_digest).collect();
        if stored.is_empty() || stored != current {
            return Ok(None);
        }
        let samples: i64 = db.query_row(
            "SELECT COUNT(*) FROM samples x JOIN series s ON s.id = x.series_id
             WHERE s.config_key = ?1",
            [&from_key],
            |row| row.get(0),
        )?;
        Ok(Some(Relocation {
            from: previous.to_path_buf(),
            samples: u64::try_from(samples).unwrap_or(0),
            accounts: stored.len(),
            from_key,
        }))
    }

    /// PRD 0051 RF5.5: move the samples, series and journal entries of
    /// `relocation` to this configuration, after replacing
    /// `history.sqlite3.bak` with a consistent copy. Idempotent: a series
    /// this configuration already has keeps its rows and gains the moved
    /// samples it lacks (same time: the existing one wins). Returns the
    /// samples moved. No path or account name is written.
    pub fn adopt(&self, relocation: &Relocation) -> Result<u64, HistoryError> {
        let mut db = self.open()?;
        crate::private_fs::sqlite_backup(&db, &self.path).map_err(|_| HistoryError::Storage)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (from, to) = (relocation.from_key.as_str(), self.config_key.as_str());
        // Series of the destination that already exist take the samples of
        // their twin; the twin then goes.
        let merged = tx.execute(
            "INSERT OR IGNORE INTO samples (series_id, taken_at, used, \"limit\", remaining, resets_at)
             SELECT d.id, x.taken_at, x.used, x.\"limit\", x.remaining, x.resets_at
             FROM samples x
             JOIN series o ON o.id = x.series_id AND o.config_key = ?1
             JOIN series d ON d.config_key = ?2 AND d.account_digest = o.account_digest
                AND d.metric_key = o.metric_key",
            [from, to],
        )?;
        tx.execute(
            "DELETE FROM samples WHERE series_id IN (
                SELECT o.id FROM series o JOIN series d ON d.config_key = ?2
                    AND d.account_digest = o.account_digest AND d.metric_key = o.metric_key
                WHERE o.config_key = ?1)",
            [from, to],
        )?;
        tx.execute(
            "DELETE FROM series WHERE config_key = ?1 AND EXISTS (
                SELECT 1 FROM series d WHERE d.config_key = ?2
                    AND d.account_digest = series.account_digest
                    AND d.metric_key = series.metric_key)",
            [from, to],
        )?;
        // The rest simply changes configuration.
        let moved: i64 = tx.query_row(
            "SELECT COUNT(*) FROM samples x JOIN series s ON s.id = x.series_id
             WHERE s.config_key = ?1",
            [from],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE series SET config_key = ?2 WHERE config_key = ?1",
            [from, to],
        )?;
        tx.execute(
            "UPDATE OR IGNORE journal SET config_key = ?2 WHERE config_key = ?1",
            [from, to],
        )?;
        tx.execute("DELETE FROM journal WHERE config_key = ?1", [from])?;
        tx.execute(
            "UPDATE OR IGNORE aliases SET config_key = ?2 WHERE config_key = ?1",
            [from, to],
        )?;
        tx.execute("DELETE FROM aliases WHERE config_key = ?1", [from])?;
        tx.commit()?;
        Ok(u64::try_from(moved).unwrap_or(0) + merged as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    const CONFIG: &str = "[[accounts]]\nid = \"acc_00000000000a\"\nname = \"one\"\nprovider = \"groq\"\ncredential_env = \"K1\"\n\n[[accounts]]\nid = \"acc_00000000000b\"\nname = \"two\"\nprovider = \"mistral\"\ncredential_env = \"K2\"\n";

    fn accounts(text: &str) -> Vec<Account> {
        crate::Config::from_toml(text).unwrap().accounts
    }

    fn file_store(db: &Path, toml: &Path) -> HistoryStore {
        HistoryStore::new(db.to_path_buf(), &ConfigIdentity::File(toml.to_path_buf()))
    }

    fn samples(db: &Path, toml: &Path) -> i64 {
        rusqlite::Connection::open(db)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM samples x JOIN series s ON s.id = x.series_id
                 WHERE s.config_key = ?1",
                [ConfigIdentity::File(toml.to_path_buf()).digest()],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_moved_file_with_the_same_ids_can_adopt_its_history_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let old = dir.path().join("old.toml");
        let new = dir.path().join("new.toml");
        let list = accounts(CONFIG);
        let before = file_store(&db, &old);
        let t0 = super::super::tests::t0();
        for (index, account) in list.iter().enumerate() {
            for step in 0..3 {
                before
                    .record(
                        account,
                        t0 + TimeDelta::minutes(20 * step),
                        &[super::super::tests::quota(
                            "5h",
                            Some(10.0 + index as f64 + step as f64),
                            Some(100.0),
                        )],
                    )
                    .unwrap();
            }
        }
        assert_eq!(samples(&db, &old), 6);
        let after = file_store(&db, &new);

        // The old path still exists: no offer.
        std::fs::write(&old, CONFIG).unwrap();
        assert_eq!(after.relocation_candidate(&old, &list).unwrap(), None);
        std::fs::remove_file(&old).unwrap();

        // Different ids: no offer, whatever the names.
        let other = accounts(&CONFIG.replace("acc_00000000000b", "acc_00000000000c"));
        assert_eq!(after.relocation_candidate(&old, &other).unwrap(), None);
        // A subset is not the same set either.
        assert_eq!(after.relocation_candidate(&old, &list[..1]).unwrap(), None);

        let offer = after.relocation_candidate(&old, &list).unwrap().unwrap();
        assert_eq!((offer.samples, offer.accounts), (6, 2));
        // Declining writes nothing.
        assert_eq!(samples(&db, &old), 6);
        assert!(!crate::private_fs::backup_path(&db).exists());

        assert_eq!(after.adopt(&offer).unwrap(), 6);
        assert_eq!(samples(&db, &old), 0);
        assert_eq!(samples(&db, &new), 6);
        assert!(crate::private_fs::backup_path(&db).exists(), "a .bak first");
        assert!(after.latest(&list[1]).unwrap().is_some());
        // Nothing left to offer, and a second adoption moves nothing.
        assert_eq!(after.relocation_candidate(&old, &list).unwrap(), None);
        assert_eq!(after.adopt(&offer).unwrap(), 0);
        assert_eq!(samples(&db, &new), 6);
    }

    #[test]
    fn adopting_into_a_configuration_that_already_has_readings_merges_them() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let old = dir.path().join("old.toml");
        let new = dir.path().join("new.toml");
        let list = accounts(CONFIG);
        let t0 = super::super::tests::t0();
        let quota = |used| super::super::tests::quota("5h", Some(used), Some(100.0));
        let before = file_store(&db, &old);
        let after = file_store(&db, &new);
        for account in &list {
            before.record(account, t0, &[quota(10.0)]).unwrap();
            before
                .record(account, t0 + TimeDelta::minutes(30), &[quota(20.0)])
                .unwrap();
            // The new file already read the second time, and a third.
            after
                .record(account, t0 + TimeDelta::minutes(30), &[quota(21.0)])
                .unwrap();
            after
                .record(account, t0 + TimeDelta::minutes(60), &[quota(30.0)])
                .unwrap();
        }
        let offer = after.relocation_candidate(&old, &list).unwrap().unwrap();
        assert_eq!(offer.samples, 4);
        after.adopt(&offer).unwrap();
        assert_eq!(samples(&db, &old), 0);
        // Per account: t0 moved, t0+30 kept the existing one, t0+60 stays.
        assert_eq!(samples(&db, &new), 6);
        let used: f64 = rusqlite::Connection::open(&db)
            .unwrap()
            .query_row(
                "SELECT used FROM samples WHERE taken_at = ?1 LIMIT 1",
                [(t0 + TimeDelta::minutes(30)).timestamp_millis()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(used, 21.0, "the destination's reading wins");
    }
}
