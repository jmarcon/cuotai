//! PRD 0033 RF5.3: the last-query record of a metered account, shared by
//! every process of the user so one Cost Explorer request serves them all
//! within the account's interval.
//!
//! The record is the cross-process billing gate. A one-shot process never
//! queries a metered account it cannot record; a long-lived one queries at
//! most once per interval on its own and says the record is unavailable
//! (PRD 0033 re-validation M-1).
//!
//! The file `<dir>/<account digest>.json` (`0600`, in a `0700` directory)
//! holds the time of the last successful query, its report (a report
//! already carries no credential, URL or response body) and a digest of the
//! account's settings. An OS advisory lock beside it (bounded wait, never
//! broken by age, released when the process dies) serializes the
//! read-decide-claim and the write steps; it is never held while the
//! network is used. A link, a file in place of the folder or another
//! user's file is refused ([`crate::private_fs::decide_locked`]).

use std::path::PathBuf;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::private_fs::UpdateError;
use crate::{Account, Report, Status};

/// Debug builds only: the folder of the metered records instead of the
/// fixed per-user one (tests and manual smokes never touch the real one).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) const DEBUG_ENV: &str = "CUOTAI_METERED_DIR";

/// The record format: 2 binds the record to the account's settings.
const SCHEMA: u32 = 2;

/// How long a reader waits for another process's lock on a record.
pub(crate) const LOCK_WAIT: std::time::Duration = crate::private_fs::UPDATE_LOCK_WAIT;

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
    /// A process is querying until then: longer than its query can take
    /// (PRD 0033 re-validation L-1).
    #[serde(default)]
    claim_until: Option<DateTime<Utc>>,
    report: Option<Report>,
}

impl Record {
    /// The record of `settings` in `bytes`; absent (a default record of
    /// these settings) when missing, damaged, of another schema or of
    /// other settings, so the next query overwrites it.
    fn of(bytes: Option<&[u8]>, settings: &str) -> Self {
        bytes
            .and_then(|bytes| serde_json::from_slice::<Record>(bytes).ok())
            .filter(|record| {
                record.schema == SCHEMA && record.settings.as_deref() == Some(settings)
            })
            .unwrap_or_else(|| Record {
                schema: SCHEMA,
                settings: Some(settings.to_owned()),
                ..Record::default()
            })
    }

    fn ready_report(&self) -> Option<&Report> {
        self.report
            .as_ref()
            .filter(|report| report.status == Status::Ready)
    }

    fn fresh(&self, interval: TimeDelta, now: DateTime<Utc>) -> bool {
        self.ready_report().is_some()
            && self
                .queried_at
                .is_some_and(|at| at <= now && now - at < interval)
    }

    fn claimed(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.claim_until.filter(|until| *until > now)
    }

    fn bytes(&self) -> Result<Vec<u8>, Unavailable> {
        serde_json::to_vec(self).map_err(|_| Unavailable::Io)
    }
}

/// What [`Store::begin`] decided.
#[derive(Debug)]
pub(crate) enum Decision {
    /// A process queried within the interval, or is querying now and an
    /// earlier reading exists: use this report.
    Reuse(Box<Report>),
    /// Query now; the claim is taken.
    Query,
    /// Another process is querying and no reading exists yet: wait for
    /// its reading until `until`.
    Pending { until: DateTime<Utc> },
}

/// Why the record cannot be used (PRD 0033 re-validation M-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unavailable {
    /// Another process held the lock for longer than [`LOCK_WAIT`].
    Busy,
    /// A link, a file in place of the folder, or another user's file.
    NotPrivate,
    /// Reading or writing failed (read-only or full disk, no folder).
    Io,
}

impl<E> From<UpdateError<E>> for Unavailable {
    fn from(error: UpdateError<E>) -> Self {
        match error {
            UpdateError::Busy => Self::Busy,
            UpdateError::NotPrivate => Self::NotPrivate,
            UpdateError::Refused(_) | UpdateError::Io(_) => Self::Io,
        }
    }
}

/// The directory of the records (`<OS local data dir>/cuotai/metered`).
#[derive(Debug, Clone)]
pub(crate) struct Store {
    dir: PathBuf,
    wait: std::time::Duration,
}

impl Store {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            wait: LOCK_WAIT,
        }
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
    /// earlier build moves to the id name the first time it is looked up
    /// (it has no settings digest, so it then counts as absent once).
    fn path(&self, account: &Account) -> PathBuf {
        let path = self.file(&account.state_key());
        if std::fs::symlink_metadata(&path).is_err() {
            let legacy = self.file(&account.key());
            if std::fs::symlink_metadata(&legacy).is_ok_and(|meta| meta.is_file()) {
                let _ = std::fs::rename(&legacy, &path);
            }
        }
        path
    }

    fn file(&self, key: &str) -> PathBuf {
        let digest = crate::models::digest(key.as_bytes());
        self.dir.join(format!("{digest}.json"))
    }

    /// Run `decide` on the record of `account` under its lock; it returns
    /// its answer and, when the record changes, the new record.
    fn decide<T>(
        &self,
        account: &Account,
        decide: impl FnOnce(Record) -> (T, Option<Record>),
    ) -> Result<T, Unavailable> {
        let settings = crate::models::restore_fingerprint(account);
        let path = self.path(account);
        crate::private_fs::decide_locked(&path, self.wait, |current| {
            let record = Record::of(current.as_deref(), &settings);
            let (answer, next) = decide(record);
            let bytes = next.as_ref().map(Record::bytes).transpose()?;
            Ok::<_, Unavailable>((answer, bytes))
        })
        .map_err(|error| match error {
            UpdateError::Refused(reason) => reason,
            other => other.into(),
        })
    }

    /// Decide whether `account` may be queried at `now`. A query claims the
    /// next slot until `now + claim_for`, so a concurrent process waits for
    /// or reuses its reading instead of paying for a second request.
    pub(crate) fn begin(
        &self,
        account: &Account,
        interval: TimeDelta,
        claim_for: TimeDelta,
        now: DateTime<Utc>,
    ) -> Result<Decision, Unavailable> {
        self.decide(account, |mut record| {
            if record.fresh(interval, now) {
                let report = record.report.take().expect("a fresh record has a report");
                return (Decision::Reuse(Box::new(report)), None);
            }
            if let Some(until) = record.claimed(now) {
                return match record.report.take() {
                    Some(report) if report.status == Status::Ready => {
                        (Decision::Reuse(Box::new(report)), None)
                    }
                    _ => (Decision::Pending { until }, None),
                };
            }
            record.claim_until = Some(now + claim_for);
            (Decision::Query, Some(record))
        })
    }

    /// Record the outcome of the query `begin` allowed, and drop the claim.
    pub(crate) fn finish(
        &self,
        account: &Account,
        report: &Report,
        now: DateTime<Utc>,
    ) -> Result<(), Unavailable> {
        self.decide(account, |mut record| {
            record.claim_until = None;
            if report.status == Status::Ready {
                record.queried_at = Some(now);
                record.report = Some(report.clone());
            }
            ((), Some(record))
        })
    }

    /// Test-only: a store whose lock wait is `wait`.
    #[cfg(test)]
    pub(crate) fn with_wait(mut self, wait: std::time::Duration) -> Self {
        self.wait = wait;
        self
    }
}

/// `note.metered.record_unavailable` / `record_busy`: why a one-shot
/// command did not query a metered account (PRD 0033 re-validation M-1).
pub(crate) fn note_unavailable(reason: Unavailable) -> crate::text::Localized {
    match reason {
        Unavailable::Busy => crate::text::Localized::fixed(
            "note.metered.record_busy",
            "Not queried: another Cuotai program held the shared record of billed AWS Cost Explorer queries for too long. Try again in a moment.",
        ),
        Unavailable::NotPrivate | Unavailable::Io => crate::text::Localized::fixed(
            "note.metered.record_unavailable",
            "Not queried: Cuotai could not use its shared record of billed AWS Cost Explorer queries. Run `cuotai --doctor` to see the folder and how to fix it.",
        ),
    }
}

/// `note.metered.pending`: another process kept its claim past this
/// process's timeout.
pub(crate) fn note_pending() -> crate::text::Localized {
    crate::text::Localized::fixed(
        "note.metered.pending",
        "Not queried: another Cuotai program is querying this account now. Try again in a moment to see its reading.",
    )
}

/// `note.metered.record_unavailable_shared`: a long-lived process queried
/// on its own record.
pub(crate) fn note_unavailable_shared() -> crate::text::Localized {
    crate::text::Localized::fixed(
        "note.metered.record_unavailable_shared",
        "The shared record of billed queries is unavailable, so other Cuotai programs may also query this account.",
    )
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

    const CLAIM: TimeDelta = TimeDelta::minutes(6);

    fn begin(
        store: &Store,
        account: &Account,
        interval: TimeDelta,
        now: DateTime<Utc>,
    ) -> Decision {
        store.begin(account, interval, CLAIM, now).unwrap()
    }

    fn finish(store: &Store, account: &Account, report: &Report, now: DateTime<Utc>) {
        store.finish(account, report, now).unwrap();
    }

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
            begin(&first, &account, interval, t0),
            Decision::Query
        ));
        finish(&first, &account, &report(Status::Ready), t0);
        let soon = t0 + TimeDelta::minutes(59);
        assert!(matches!(
            begin(&second, &account, interval, soon),
            Decision::Reuse(_)
        ));
        let later = t0 + TimeDelta::minutes(61);
        assert!(matches!(
            begin(&second, &account, interval, later),
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
        assert!(matches!(begin(&store, &account, hour, t0), Decision::Query));
        finish(&store, &account, &report(Status::Error), t0);
        let next = t0 + TimeDelta::seconds(1);
        assert!(matches!(
            begin(&store, &account, hour, next),
            Decision::Query
        ));
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
        let old = Record {
            queried_at: Some(t0),
            report: Some(report(Status::Ready)),
            ..Record::default()
        };
        crate::private_fs::write_file(&legacy, &old.bytes().unwrap()).unwrap();
        let soon = t0 + TimeDelta::minutes(5);
        assert!(matches!(
            begin(&store, &account, hour, soon),
            Decision::Query
        ));
        assert!(!legacy.exists());
        finish(&store, &account, &report(Status::Ready), soon);
        let renamed = Account {
            name: "renamed".into(),
            ..account.clone()
        };
        let later = soon + TimeDelta::minutes(1);
        let Decision::Reuse(mut reused) = begin(&store, &renamed, hour, later) else {
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
        assert!(matches!(begin(&store, &account, hour, t0), Decision::Query));
        finish(&store, &account, &report(Status::Ready), t0);
        account.params.insert("profile".into(), "second".into());
        let soon = t0 + TimeDelta::minutes(1);
        assert!(matches!(
            begin(&store, &account, hour, soon),
            Decision::Query
        ));
        finish(&store, &account, &report(Status::Ready), soon);
        let later = soon + TimeDelta::minutes(1);
        assert!(matches!(
            begin(&store, &account, hour, later),
            Decision::Reuse(_)
        ));
        // The first profile's record was overwritten: going back queries.
        account.params.insert("profile".into(), "first".into());
        assert!(matches!(
            begin(&store, &account, hour, later),
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
        assert!(matches!(begin(&store, &first, hour, t0), Decision::Query));
        finish(&store, &first, &report(Status::Ready), t0);
        assert!(matches!(begin(&store, &second, hour, t0), Decision::Query));
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

    /// PRD 0033 re-validation L-1: a claim with no reading yet makes the
    /// other process wait for it, never query beside it.
    #[test]
    fn a_claim_before_the_first_reading_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let account = account();
        let t0 = Utc::now();
        let hour = TimeDelta::hours(1);
        assert!(matches!(begin(&store, &account, hour, t0), Decision::Query));
        let soon = t0 + TimeDelta::seconds(30);
        assert!(matches!(
            begin(&store, &account, hour, soon),
            Decision::Pending { until } if until == t0 + CLAIM
        ));
        // Past the claim, a new query is allowed.
        let late = t0 + CLAIM + TimeDelta::seconds(1);
        assert!(matches!(
            begin(&store, &account, hour, late),
            Decision::Query
        ));
    }

    /// A damaged record counts as absent and is overwritten.
    #[test]
    fn a_damaged_record_counts_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let account = account();
        let path = store.file(&account.state_key());
        crate::private_fs::write_file(&path, b"{not json").unwrap();
        let t0 = Utc::now();
        assert!(matches!(
            begin(&store, &account, TimeDelta::hours(1), t0),
            Decision::Query
        ));
    }

    /// PRD 0033 re-validation M-1/M-2: a file in place of the folder and a
    /// lock held past the bounded wait are reported, never a query.
    #[test]
    fn an_unusable_folder_or_a_held_lock_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let account = account();
        let t0 = Utc::now();
        let hour = TimeDelta::hours(1);
        let file = dir.path().join("metered");
        std::fs::write(&file, b"x").unwrap();
        let blocked = Store::new(file);
        assert!(blocked.begin(&account, hour, CLAIM, t0).is_err());

        let store =
            Store::new(dir.path().join("ok")).with_wait(std::time::Duration::from_millis(200));
        finish(&store, &account, &report(Status::Ready), t0);
        let path = store.file(&account.state_key());
        let lock = path.with_file_name(format!(
            ".{}.lock",
            path.file_name().unwrap().to_string_lossy()
        ));
        let held = std::fs::OpenOptions::new().write(true).open(&lock).unwrap();
        held.lock().unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            store.begin(&account, hour, CLAIM, t0).unwrap_err(),
            Unavailable::Busy
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(4));
        drop(held);
        assert!(store.begin(&account, hour, CLAIM, t0).is_ok());
    }

    /// A link in place of the record is refused.
    #[cfg(unix)]
    #[test]
    fn a_linked_record_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("records"));
        let account = account();
        let path = store.file(&account.state_key());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let target = dir.path().join("elsewhere.json");
        std::fs::write(&target, b"{}").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert_eq!(
            store
                .begin(&account, TimeDelta::hours(1), CLAIM, Utc::now())
                .unwrap_err(),
            Unavailable::NotPrivate
        );
    }
}
