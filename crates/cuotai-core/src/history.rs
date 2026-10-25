//! PRD 0006 — local measurement history.
//!
//! [`HistoryStore`] keeps successful readings per account and metric in
//! `history.sqlite3`, a file of its own (never the model catalog, the TOML or
//! a credential store). Rows hold only digests, numbers and window metadata:
//! the SHA-256 of [`ConfigIdentity`] and of the account identity (the same
//! derivation as the model catalog), the metric label, unit, kind, window and
//! `resets_at`. Never a secret, an HTTP body, a URL, a note or the account
//! name. A missing value is `NULL`, never zero.
//!
//! The store is opened per operation (cheap, so building one never touches
//! disk) in WAL mode with a busy timeout, so the TUI, the desktop app and the
//! CLI pointing at the same TOML write to it concurrently; a write is
//! idempotent per `(account, metric, taken_at)` and skips a sample identical
//! to the previous one of its metric taken less than 15 minutes before
//! (RF2.3). Every open, read or write failure surfaces as
//! [`HistoryError::Storage`], never as success.
//!
//! The queries ([`HistoryStore::latest`], [`HistoryStore::series`],
//! [`HistoryStore::windows`], [`HistoryStore::bounds`],
//! [`HistoryStore::recent`], [`HistoryStore::account_history`]) read the
//! file only; nothing here touches the network. [`HistoryRecorder`] records
//! refresh results off the refresh's critical path (RF2) and prunes by
//! [`Retention`] (RF4), which also owns export and erasure.

mod recorder;
mod retention;
mod trend;

pub use recorder::{HistoryRecorder, HistoryStatus};
pub use retention::{CeilingPrune, ExportFormat, PruneReport, Retention, RetentionError};
pub use trend::{Chart, ChartPoint, ChartReset, HistoryRange, MetricHistory, Trend};

use crate::config_file::{ConfigFile, ConfigIdentity};
use crate::models::{account_key, create_private_file, digest};
use crate::pace::Point;
use crate::resets::DRIFT_TOLERANCE;
use crate::text::Localized;
use crate::{Account, Config, Metric, MetricKind, MetricWindow, Report, Status};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::ops::{Bound, RangeBounds};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HistoryError {
    #[error("Falha ao ler ou gravar o histórico local de medições.")]
    Storage,
    /// The export destination refused the data (RF4.4).
    #[error("Falha ao gravar a exportação do histórico.")]
    Export,
}

impl HistoryError {
    pub fn l10n(&self) -> Localized {
        let id = match self {
            Self::Storage => "history.error.storage",
            Self::Export => "history.error.export",
        };
        Localized::fixed(id, self.to_string())
    }
}

impl From<rusqlite::Error> for HistoryError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}

/// A drop in `used` above this fraction of `limit` ends the window (a reset).
const RESET_DROP: f64 = 0.2;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// RF2.3: a sample identical to the previous one of its metric taken less
/// than this after it is not stored.
const COMPACT_WITHIN_MS: i64 = 15 * 60 * 1000;
/// FIX-10: tolerate small wall-clock differences, but never let a later
/// timestamp poison the history. Times in the database have millisecond
/// precision, so the bound uses the same unit.
const FUTURE_SAMPLE_TOLERANCE_MS: i64 = 5 * 60 * 1000;
/// Ceiling of the WAL file kept after a checkpoint, so a large prune does
/// not leave a large side file behind.
const WAL_SIZE_LIMIT: i64 = 4 << 20;

/// Forward migrations: entry `n` takes `PRAGMA user_version` from `n` to
/// `n + 1`. Append new steps, never edit a shipped one.
///
/// `series` names each metric of an account (the label `latest` shows), its
/// position and the newest reading that carried it (`seen_at`); `removed_at`
/// marks the series of an account gone from the configuration for pruning.
/// Times are UTC unix milliseconds. Version 2 indexes samples by age (the
/// file-wide retention and size pruning, RF4) and adds `meta` (the
/// resolution reduction's progress).
const MIGRATIONS: [&str; 3] = [
    "CREATE TABLE series (
    config_key TEXT NOT NULL,
    account_digest TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    label TEXT NOT NULL,
    position INTEGER NOT NULL,
    seen_at INTEGER NOT NULL,
    removed_at INTEGER,
    PRIMARY KEY (config_key, account_digest, metric_key)
) WITHOUT ROWID;
CREATE TABLE samples (
    config_key TEXT NOT NULL,
    account_digest TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    taken_at INTEGER NOT NULL,
    used REAL,
    \"limit\" REAL,
    remaining REAL,
    unit TEXT NOT NULL,
    kind TEXT NOT NULL,
    \"window\" TEXT,
    resets_at INTEGER,
    PRIMARY KEY (config_key, account_digest, metric_key, taken_at)
) WITHOUT ROWID;
CREATE INDEX samples_by_time ON samples (config_key, taken_at);",
    "CREATE INDEX samples_by_age ON samples (taken_at);
CREATE TABLE meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL) WITHOUT ROWID;",
    // Version 3 (PRD 0026 RF1.5): whether a metric is part of the plan or
    // paid extra usage, so statistics tell plan limits from overage.
    "ALTER TABLE series ADD COLUMN billing TEXT NOT NULL DEFAULT 'included';",
];

/// PRD 0029 RF2.2: `legacy_digest` (the pre-0029 series identity,
/// [`account_key`]) → `account_id` of every series moved to its id. Created
/// on open *without* a `user_version` step: an older release opening the
/// file after a downgrade must not reject it as newer (it just starts new
/// series under its legacy digests, which the next open of this release
/// merges back into the id's series).
const ALIASES: &str = "CREATE TABLE IF NOT EXISTS aliases (
    config_key TEXT NOT NULL,
    legacy_digest TEXT NOT NULL,
    account_id TEXT NOT NULL,
    PRIMARY KEY (config_key, legacy_digest)
) WITHOUT ROWID;";

/// PRD 0029 RF2.2: the series identity of `account`, a digest of its id
/// alone, so a rename or a parameter edit keeps the series. Samples an
/// older release stored under the legacy digest of the account as it is
/// configured now move to it here, once (recorded in `aliases`), so old
/// samples join the new series; rows already under the id win.
pub(crate) fn series_digest(
    db: &Connection,
    config_key: &str,
    account: &Account,
) -> rusqlite::Result<String> {
    let id = account.state_key();
    let current = digest(format!("cuotai.series.v2\u{1f}{id}").as_bytes());
    let legacy = account_key(account);
    let pending = db
        .query_row(
            "SELECT 1 FROM series WHERE config_key = ?1 AND account_digest = ?2
             UNION ALL SELECT 1 FROM samples WHERE config_key = ?1 AND account_digest = ?2
             LIMIT 1",
            rusqlite::params![config_key, legacy],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if pending {
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO series (config_key, account_digest, metric_key, label,
                position, seen_at, removed_at, billing)
             SELECT config_key, ?3, metric_key, label, position, seen_at, removed_at, billing
             FROM series WHERE config_key = ?1 AND account_digest = ?2",
            rusqlite::params![config_key, legacy, current],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO samples (config_key, account_digest, metric_key, taken_at,
                used, \"limit\", remaining, unit, kind, \"window\", resets_at)
             SELECT config_key, ?3, metric_key, taken_at, used, \"limit\", remaining, unit,
                kind, \"window\", resets_at
             FROM samples WHERE config_key = ?1 AND account_digest = ?2",
            rusqlite::params![config_key, legacy, current],
        )?;
        for table in ["samples", "series"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE config_key = ?1 AND account_digest = ?2"),
                rusqlite::params![config_key, legacy],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO aliases (config_key, legacy_digest, account_id)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![config_key, legacy, id],
        )?;
        tx.commit()?;
    }
    Ok(current)
}

/// Measurement history of one configuration. Holds only the database path
/// and the configuration digest; errors surface on use.
#[derive(Debug, Clone)]
pub struct HistoryStore {
    path: PathBuf,
    config_key: String,
    /// What [`Self::prune`] keeps (RF4); the default until
    /// [`Self::with_retention`].
    retention: Retention,
}

/// Stable identity of a metric's series: the digest of its kind, window,
/// label and unit. A label that changes between adapter versions starts a
/// new series.
pub fn metric_key(metric: &Metric) -> String {
    let identity = serde_json::to_vec(&(metric.kind, metric.window, &metric.label, &metric.unit))
        .expect("metric identity serializes");
    digest(&identity)
}

/// The last recorded reading of an account.
#[derive(Debug, Clone)]
pub struct StoredReport {
    /// When the last recorded reading was taken: its original time, never
    /// the time it was read back (age it with [`crate::freshness::age`]).
    pub checked_at: DateTime<Utc>,
    /// The metrics of that reading in their reported order.
    pub metrics: Vec<StoredMetric>,
}

impl StoredReport {
    /// The reading as a `Ready` report of `account` with its original
    /// `checked_at`, for the preserved last valid reading (PRD 0010 RF2.3).
    /// It carries no notes and no source: only numbers were stored.
    pub fn into_report(self, account: &Account) -> Report {
        Report {
            account_id: Some(account.id()),
            diagnostic: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: self.checked_at,
            status: Status::Ready,
            metrics: self.metrics.into_iter().map(|m| m.metric).collect(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }
}

/// The last sample of one metric. `metric.period` is not stored (`None`).
#[derive(Debug, Clone)]
pub struct StoredMetric {
    /// [`metric_key`] of the metric.
    pub key: String,
    /// When this value was sampled; never after [`StoredReport::checked_at`].
    pub taken_at: DateTime<Utc>,
    pub metric: Metric,
}

/// Use of a metric over one window instance: consecutive samples with the
/// same `resets_at` (within [`DRIFT_TOLERANCE`]) and no reset-sized drop.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WindowUsage {
    /// First and last sample of the window in the queried range.
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    /// Last `resets_at` reported in the window, if any.
    pub resets_at: Option<DateTime<Utc>>,
    /// Last finite `limit` reported in the window, if any.
    pub limit: Option<f64>,
    pub initial_used: f64,
    pub final_used: f64,
    pub peak_used: f64,
    pub peak_at: DateTime<Utc>,
    pub samples: usize,
}

/// First and last recorded sample times of the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
}

/// One stored sample as the window grouping reads it.
#[derive(Debug, Clone, Copy)]
struct WindowSample {
    taken_at: DateTime<Utc>,
    used: Option<f64>,
    limit: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
}

fn millis(at: DateTime<Utc>) -> i64 {
    at.timestamp_millis()
}
fn future_sample_cutoff(now: DateTime<Utc>) -> i64 {
    millis(now).saturating_add(FUTURE_SAMPLE_TOLERANCE_MS)
}

fn instant(ms: i64) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::from_timestamp_millis(ms).ok_or(rusqlite::Error::IntegralValueOutOfRange(0, ms))
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite())
}

/// The serde name of a `kind`/`window` value (`quota`, `window_5h`, …).
fn tag<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(tag)) => tag,
        _ => unreachable!("unit enums serialize as strings"),
    }
}

fn untag<T: serde::de::DeserializeOwned>(tag: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(tag)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

/// Inclusive millisecond span of `range`.
fn span(range: impl RangeBounds<DateTime<Utc>>) -> (i64, i64) {
    let start = match range.start_bound() {
        Bound::Included(at) => millis(*at),
        Bound::Excluded(at) => millis(*at).saturating_add(1),
        Bound::Unbounded => i64::MIN,
    };
    let end = match range.end_bound() {
        Bound::Included(at) => millis(*at),
        Bound::Excluded(at) => millis(*at).saturating_sub(1),
        Bound::Unbounded => i64::MAX,
    };
    (start, end)
}

/// Switch the file to WAL (persistent, so once per file). SQLite answers a
/// journal-mode change racing another first open with `BUSY` without
/// consulting the busy handler, so retry it within [`BUSY_TIMEOUT`].
pub(crate) fn wal(db: &Connection) -> rusqlite::Result<()> {
    let mode =
        |db: &Connection| db.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0));
    if mode(db)?.eq_ignore_ascii_case("wal") {
        return Ok(());
    }
    let deadline = std::time::Instant::now() + BUSY_TIMEOUT;
    loop {
        match db.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))
        {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::DatabaseBusy
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            other => return other.map(drop),
        }
    }
}

/// Apply the steps of `migrations` the file has not seen yet, inside one
/// `BEGIN IMMEDIATE` so concurrent first opens migrate once. A file from a
/// newer Cuotai (a higher `user_version`) is rejected, never guessed at.
pub(crate) fn migrate(db: &mut Connection, migrations: &[&str]) -> Result<(), HistoryError> {
    let latest = i64::try_from(migrations.len()).map_err(|_| HistoryError::Storage)?;
    let version =
        |db: &Connection| db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0));
    if version(db)? == latest {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = version(&tx)?;
    if !(0..=latest).contains(&current) {
        return Err(HistoryError::Storage);
    }
    for (done, step) in (current..).zip(&migrations[current as usize..]) {
        tx.execute_batch(step)?;
        tx.pragma_update(None, "user_version", done + 1)?;
    }
    tx.commit()?;
    Ok(())
}

/// The window instance (0, 1, …) of each sample, in time order: `None` for
/// a sample without a finite `used`, which is skipped. A window ends when
/// `used` drops by more than [`RESET_DROP`] of a finite positive `limit`
/// (the sample's, else the previous one's) or when `resets_at` moves beyond
/// [`DRIFT_TOLERANCE`] from the last one seen in the window.
fn instances(samples: &[WindowSample]) -> Vec<Option<usize>> {
    let mut ids = Vec::with_capacity(samples.len());
    let mut count = 0;
    let mut previous: Option<(f64, Option<f64>)> = None;
    let mut resets_at: Option<DateTime<Utc>> = None;
    for sample in samples {
        let Some(used) = finite(sample.used) else {
            ids.push(None);
            continue;
        };
        let limit = finite(sample.limit);
        let same = previous.is_some_and(|(last_used, last_limit)| {
            let dropped = limit
                .or(last_limit)
                .filter(|limit| *limit > 0.0)
                .is_some_and(|limit| last_used - used > RESET_DROP * limit);
            let moved = matches!(
                (resets_at, sample.resets_at),
                (Some(known), Some(next)) if (next - known).abs() > DRIFT_TOLERANCE
            );
            !dropped && !moved
        });
        if same {
            resets_at = sample.resets_at.or(resets_at);
        } else {
            count += 1;
            resets_at = sample.resets_at;
        }
        ids.push(Some(count - 1));
        previous = Some((used, limit));
    }
    ids
}

/// Group samples (in time order) into window instances (see [`instances`]).
/// A non-finite `limit` counts as missing.
fn group_windows(samples: impl IntoIterator<Item = WindowSample>) -> Vec<WindowUsage> {
    let samples: Vec<WindowSample> = samples.into_iter().collect();
    let mut windows: Vec<WindowUsage> = Vec::new();
    for (sample, id) in samples.iter().zip(instances(&samples)) {
        let (Some(id), Some(used)) = (id, finite(sample.used)) else {
            continue;
        };
        let limit = finite(sample.limit);
        match windows.get_mut(id) {
            Some(window) => {
                window.ended_at = sample.taken_at;
                window.final_used = used;
                window.samples += 1;
                window.resets_at = sample.resets_at.or(window.resets_at);
                window.limit = limit.or(window.limit);
                if used > window.peak_used {
                    window.peak_used = used;
                    window.peak_at = sample.taken_at;
                }
            }
            None => windows.push(WindowUsage {
                started_at: sample.taken_at,
                ended_at: sample.taken_at,
                resets_at: sample.resets_at,
                limit,
                initial_used: used,
                final_used: used,
                peak_used: used,
                peak_at: sample.taken_at,
                samples: 1,
            }),
        }
    }
    windows
}

/// The stored values of one sample, as compaction compares them.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Values {
    used: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    resets_at: Option<i64>,
}

impl Values {
    fn of(metric: &Metric) -> Self {
        Self {
            used: finite(metric.used),
            limit: finite(metric.limit),
            remaining: finite(metric.remaining),
            resets_at: metric.resets_at.map(millis),
        }
    }

    /// RF2.3: `self`, taken at `at`, repeats `previous` (taken at
    /// `previous_at`): same values, less than 15 minutes later and the same
    /// window (`resets_at` within [`DRIFT_TOLERANCE`]).
    fn repeats(&self, at: i64, previous: &Values, previous_at: i64) -> bool {
        let same_window = match (self.resets_at, previous.resets_at) {
            (None, None) => true,
            (Some(next), Some(known)) => {
                next.abs_diff(known) <= DRIFT_TOLERANCE.num_milliseconds().unsigned_abs()
            }
            _ => false,
        };
        (0..COMPACT_WITHIN_MS).contains(&(at - previous_at))
            && same_window
            && (self.used, self.limit, self.remaining)
                == (previous.used, previous.limit, previous.remaining)
    }
}

impl HistoryStore {
    /// Store at `path`, isolated by `config` (only its digest is written).
    pub fn new(path: PathBuf, config: &ConfigIdentity) -> Self {
        Self {
            path,
            config_key: config.digest(),
            retention: Retention::default(),
        }
    }

    /// `<OS local data dir>/cuotai/history/history.sqlite3` keyed by
    /// [`ConfigFile::identity`]; front ends sharing a TOML share series.
    pub fn in_data_dir(file: &ConfigFile) -> Result<Self, HistoryError> {
        let identity = file.identity().map_err(|_| HistoryError::Storage)?;
        let dir = dirs::data_local_dir().ok_or(HistoryError::Storage)?;
        Ok(Self::new(
            dir.join("cuotai").join("history").join("history.sqlite3"),
            &identity,
        ))
    }

    /// The same store pruned by `retention` ([`Self::prune`]).
    pub fn with_retention(self, retention: Retention) -> Self {
        Self { retention, ..self }
    }

    pub fn retention(&self) -> Retention {
        self.retention
    }

    /// Open (creating privately: directory 0700, file 0600 on Unix) and
    /// migrate the store.
    fn open(&self) -> Result<Connection, HistoryError> {
        create_private_file(&self.path).map_err(|_| HistoryError::Storage)?;
        let mut db = Connection::open(&self.path)?;
        db.busy_timeout(BUSY_TIMEOUT)?;
        wal(&db)?;
        db.pragma_update(None, "journal_size_limit", WAL_SIZE_LIMIT)?;
        migrate(&mut db, &MIGRATIONS)?;
        db.execute_batch(ALIASES)?;
        Ok(db)
    }

    /// Open the store only when the file exists: reads that find nothing
    /// recorded never create it.
    fn open_existing(&self) -> Result<Option<Connection>, HistoryError> {
        if !self.path.try_exists().map_err(|_| HistoryError::Storage)? {
            return Ok(None);
        }
        self.open().map(Some)
    }

    /// Record the reading of `account` taken at `taken_at`: one sample per
    /// metric, in one transaction. Non-finite values are stored as `NULL`.
    /// FIX-10 rejects the whole reading when it is more than five minutes
    /// ahead of the current UTC wall clock, before opening or changing the
    /// store. Older readings have no lower bound. A sample already stored
    /// for the same account, metric and time (to the millisecond) is kept as
    /// is, and a sample identical to the previous one of its metric taken
    /// less than 15 minutes before it is not stored unless `resets_at` moved
    /// beyond [`DRIFT_TOLERANCE`] (a new window; RF2.3). The series still
    /// notes the reading, so [`Self::latest`] dates it. Returns how many
    /// samples were new.
    pub fn record(
        &self,
        account: &Account,
        taken_at: DateTime<Utc>,
        metrics: &[Metric],
    ) -> Result<usize, HistoryError> {
        self.record_at(account, taken_at, metrics, Utc::now())
    }

    fn record_at(
        &self,
        account: &Account,
        taken_at: DateTime<Utc>,
        metrics: &[Metric],
        now: DateTime<Utc>,
    ) -> Result<usize, HistoryError> {
        let at = millis(taken_at);
        if at > future_sample_cutoff(now) {
            return Ok(0);
        }
        let mut db = self.open()?;
        let account_digest = series_digest(&db, &self.config_key, account)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut added = 0;
        {
            let mut series = tx.prepare(
                "INSERT INTO series (config_key, account_digest, metric_key, label, position, seen_at,
                    billing)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (config_key, account_digest, metric_key) DO UPDATE SET
                    label = excluded.label,
                    billing = excluded.billing,
                    position = CASE WHEN excluded.seen_at >= seen_at
                        THEN excluded.position ELSE position END,
                    seen_at = MAX(seen_at, excluded.seen_at),
                    removed_at = NULL",
            )?;
            let mut sample = tx.prepare(
                "INSERT OR IGNORE INTO samples (config_key, account_digest, metric_key, taken_at,
                    used, \"limit\", remaining, unit, kind, \"window\", resets_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            let mut previous = tx.prepare(
                "SELECT taken_at, used, \"limit\", remaining, resets_at FROM samples
                 WHERE config_key = ?1 AND account_digest = ?2 AND metric_key = ?3
                   AND taken_at <= ?4
                 ORDER BY taken_at DESC LIMIT 1",
            )?;
            for (position, metric) in metrics.iter().enumerate() {
                let key = metric_key(metric);
                let position = i64::try_from(position).map_err(|_| HistoryError::Storage)?;
                series.execute(rusqlite::params![
                    self.config_key,
                    account_digest,
                    key,
                    metric.label,
                    position,
                    at,
                    tag(metric.billing)
                ])?;
                let values = Values::of(metric);
                let last = previous
                    .query_row(
                        rusqlite::params![self.config_key, account_digest, key, at],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                Values {
                                    used: row.get(1)?,
                                    limit: row.get(2)?,
                                    remaining: row.get(3)?,
                                    resets_at: row.get(4)?,
                                },
                            ))
                        },
                    )
                    .optional()?;
                if last.is_some_and(|(last_at, last)| values.repeats(at, &last, last_at)) {
                    continue;
                }
                added += sample.execute(rusqlite::params![
                    self.config_key,
                    account_digest,
                    key,
                    at,
                    values.used,
                    values.limit,
                    values.remaining,
                    metric.unit,
                    tag(metric.kind),
                    metric.window.map(tag),
                    values.resets_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// Mark for pruning the series of this configuration whose account
    /// identity is not in `saved` (removed, renamed or with another
    /// credential, product or region), and clear the mark of those that are
    /// (an identity that came back). Data stays until it is pruned. Pass
    /// only the configuration just loaded from or saved to the TOML.
    pub fn mark_removed(&self, saved: &Config, now: DateTime<Utc>) -> Result<(), HistoryError> {
        // Nothing stored yet: do not create the store just to mark it.
        if !self.path.try_exists().map_err(|_| HistoryError::Storage)? {
            return Ok(());
        }
        let mut db = self.open()?;
        // The id series of each account, plus legacy digests not moved yet.
        let mut kept = Vec::new();
        for account in &saved.accounts {
            kept.push(series_digest(&db, &self.config_key, account)?);
            kept.push(account_key(account));
        }
        let keep = serde_json::to_string(&kept).map_err(|_| HistoryError::Storage)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE series SET removed_at = ?3 WHERE config_key = ?1 AND removed_at IS NULL
             AND account_digest NOT IN (SELECT value FROM json_each(?2))",
            rusqlite::params![self.config_key, keep, millis(now)],
        )?;
        tx.execute(
            "UPDATE series SET removed_at = NULL WHERE config_key = ?1 AND removed_at IS NOT NULL
             AND account_digest IN (SELECT value FROM json_each(?2))",
            rusqlite::params![self.config_key, keep],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The metrics of the last recorded reading of `account`, each with its
    /// last stored sample; `None` when nothing was recorded. FIX-10 ignores
    /// samples more than five minutes ahead of the current UTC wall clock,
    /// including rows written by an older Cuotai version.
    pub fn latest(&self, account: &Account) -> Result<Option<StoredReport>, HistoryError> {
        self.latest_at(account, Utc::now())
    }

    fn latest_at(
        &self,
        account: &Account,
        now: DateTime<Utc>,
    ) -> Result<Option<StoredReport>, HistoryError> {
        let db = self.open()?;
        Ok(self.latest_in(&db, account, now)?)
    }

    /// PRD 0010 RF2.3: the last recorded reading of every account of
    /// `config`, in its order, as a `Ready` [`Report`] with its original
    /// `checked_at` (no notes, no source) for
    /// [`crate::refresh::RefreshCycle::restore_last_good`]. All `None` when
    /// nothing was recorded yet (the file is not created).
    pub fn latest_reports(&self, config: &Config) -> Result<Vec<Option<Report>>, HistoryError> {
        let Some(db) = self.open_existing()? else {
            return Ok(vec![None; config.accounts.len()]);
        };
        let now = Utc::now();
        config
            .accounts
            .iter()
            .map(|account| {
                Ok(self
                    .latest_in(&db, account, now)?
                    .map(|stored| stored.into_report(account)))
            })
            .collect()
    }

    fn latest_in(
        &self,
        db: &Connection,
        account: &Account,
        now: DateTime<Utc>,
    ) -> rusqlite::Result<Option<StoredReport>> {
        let account_digest = series_digest(db, &self.config_key, account)?;
        let cutoff = future_sample_cutoff(now);
        let current_seen_at = db.query_row(
            "SELECT MAX(seen_at) FROM series
             WHERE config_key = ?1 AND account_digest = ?2 AND seen_at <= ?3",
            rusqlite::params![self.config_key, account_digest, cutoff],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        // Older builds may have advanced every series cursor to a future row.
        // If no bounded cursor survives, recover the last readable report
        // from the bounded samples instead of hiding all valid history.
        let (seen_at, recover_poisoned_cursor) = if let Some(seen_at) = current_seen_at {
            (seen_at, false)
        } else {
            let Some(seen_at) = db.query_row(
                "SELECT MAX(taken_at) FROM samples
                 WHERE config_key = ?1 AND account_digest = ?2 AND taken_at <= ?3",
                rusqlite::params![self.config_key, account_digest, cutoff],
                |row| row.get::<_, Option<i64>>(0),
            )?
            else {
                return Ok(None);
            };
            (seen_at, true)
        };
        let mut query = db.prepare_cached(
            "SELECT s.metric_key, s.label, x.taken_at, x.used, x.\"limit\", x.remaining,
                    x.unit, x.kind, x.\"window\", x.resets_at
             FROM series s JOIN samples x
               ON x.config_key = s.config_key AND x.account_digest = s.account_digest
              AND x.metric_key = s.metric_key
             WHERE s.config_key = ?1 AND s.account_digest = ?2
               AND (?3 OR s.seen_at = ?4)
               AND x.taken_at = (SELECT MAX(taken_at) FROM samples
                    WHERE config_key = s.config_key AND account_digest = s.account_digest
                      AND metric_key = s.metric_key AND taken_at <= ?5)
             ORDER BY s.position",
        )?;
        let metrics = query
            .query_map(
                rusqlite::params![
                    self.config_key,
                    account_digest,
                    recover_poisoned_cursor,
                    seen_at,
                    cutoff
                ],
                |row| {
                    Ok(StoredMetric {
                        key: row.get(0)?,
                        taken_at: instant(row.get(2)?)?,
                        metric: Metric {
                            billing: Default::default(),
                            scope: None,
                            label: row.get(1)?,
                            used: finite(row.get(3)?),
                            limit: finite(row.get(4)?),
                            remaining: finite(row.get(5)?),
                            unit: row.get(6)?,
                            kind: untag::<MetricKind>(row.get(7)?)?,
                            window: row
                                .get::<_, Option<String>>(8)?
                                .map(untag::<MetricWindow>)
                                .transpose()?,
                            period: None,
                            resets_at: row.get::<_, Option<i64>>(9)?.map(instant).transpose()?,
                        },
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Some(StoredReport {
            checked_at: instant(seen_at)?,
            metrics,
        }))
    }

    /// PRD 0007 RF1.5 input: for each of `metrics` (a reading of `account`),
    /// the recorded points of its series in the span [`crate::pace::assess`]
    /// reads at `now` ([`crate::pace::recent_since`]); empty for a metric
    /// with no pace. Pass each list as `recent` to `assess`. Nothing is read
    /// (nor created) when no metric has a pace or nothing was recorded.
    pub fn recent(
        &self,
        account: &Account,
        metrics: &[Metric],
        now: DateTime<Utc>,
    ) -> Result<Vec<Vec<Point>>, HistoryError> {
        let spans: Vec<Option<DateTime<Utc>>> = metrics
            .iter()
            .map(|metric| crate::pace::recent_since(metric, now))
            .collect();
        let mut points = vec![Vec::new(); metrics.len()];
        if spans.iter().all(Option::is_none) {
            return Ok(points);
        }
        let Some(db) = self.open_existing()? else {
            return Ok(points);
        };
        let account_digest = series_digest(&db, &self.config_key, account)?;
        let mut query = db.prepare(
            "SELECT taken_at, used FROM samples
             WHERE config_key = ?1 AND account_digest = ?2 AND metric_key = ?3
               AND taken_at BETWEEN ?4 AND ?5 AND used IS NOT NULL
             ORDER BY taken_at",
        )?;
        for ((metric, since), points) in metrics.iter().zip(spans).zip(&mut points) {
            let Some(since) = since else {
                continue;
            };
            *points = query
                .query_map(
                    rusqlite::params![
                        self.config_key,
                        account_digest,
                        metric_key(metric),
                        millis(since),
                        millis(now)
                    ],
                    |row| {
                        Ok(Point {
                            taken_at: instant(row.get(0)?)?,
                            used: row.get(1)?,
                        })
                    },
                )?
                .filter(|point| point.as_ref().map_or(true, |p| p.used.is_finite()))
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        Ok(points)
    }

    /// Samples of one metric within `range` (any `RangeBounds`, compared to
    /// the millisecond) in time order. Samples without a finite `used` are
    /// left out.
    pub fn series(
        &self,
        account: &Account,
        metric_key: &str,
        range: impl RangeBounds<DateTime<Utc>>,
    ) -> Result<Vec<Point>, HistoryError> {
        Ok(self
            .samples(account, metric_key, range)?
            .into_iter()
            .filter_map(|sample| {
                finite(sample.used).map(|used| Point {
                    taken_at: sample.taken_at,
                    used,
                })
            })
            .collect())
    }

    /// Window instances of one metric within `range`, in time order. A
    /// window ends when `used` drops by more than 20% of `limit` or when
    /// `resets_at` moves beyond [`DRIFT_TOLERANCE`]; a metric without a
    /// finite positive `limit` only splits on `resets_at`. Non-finite or
    /// missing values are ignored.
    pub fn windows(
        &self,
        account: &Account,
        metric_key: &str,
        range: impl RangeBounds<DateTime<Utc>>,
    ) -> Result<Vec<WindowUsage>, HistoryError> {
        Ok(group_windows(self.samples(account, metric_key, range)?))
    }

    fn samples(
        &self,
        account: &Account,
        metric_key: &str,
        range: impl RangeBounds<DateTime<Utc>>,
    ) -> Result<Vec<WindowSample>, HistoryError> {
        let (start, end) = span(range);
        let db = self.open()?;
        let account_digest = series_digest(&db, &self.config_key, account)?;
        let mut query = db.prepare(
            "SELECT taken_at, used, \"limit\", resets_at FROM samples
             WHERE config_key = ?1 AND account_digest = ?2 AND metric_key = ?3
               AND taken_at BETWEEN ?4 AND ?5
             ORDER BY taken_at",
        )?;
        let samples = query
            .query_map(
                rusqlite::params![self.config_key, account_digest, metric_key, start, end],
                |row| {
                    Ok(WindowSample {
                        taken_at: instant(row.get(0)?)?,
                        used: row.get(1)?,
                        limit: row.get(2)?,
                        resets_at: row.get::<_, Option<i64>>(3)?.map(instant).transpose()?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(samples)
    }

    /// [`Self::bounds`] for a read that must not create the store: `None`
    /// when the file does not exist yet.
    pub(crate) fn recorded_bounds(&self) -> Result<Option<Bounds>, HistoryError> {
        if self.open_existing()?.is_none() {
            return Ok(None);
        }
        self.bounds()
    }

    /// First and last sample times of this configuration; `None` when empty.
    pub fn bounds(&self) -> Result<Option<Bounds>, HistoryError> {
        let db = self.open()?;
        let (first, last) = db.query_row(
            "SELECT MIN(taken_at), MAX(taken_at) FROM samples WHERE config_key = ?1",
            [&self.config_key],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
        )?;
        match (first, last) {
            (Some(first), Some(last)) => Ok(Some(Bounds {
                first: instant(first)?,
                last: instant(last)?,
            })),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeDelta, TimeZone};
    use std::path::Path;

    pub(super) const CONFIG: &str = r#"
[[accounts]]
name = "alpha"
provider = "groq"
credential_env = "GROQ_KEY_ALPHA"
[[accounts]]
name = "beta"
provider = "groq"
credential_env = "GROQ_KEY_BETA"
"#;

    pub(super) fn config(toml: &str) -> Config {
        Config::from_toml(toml).unwrap()
    }

    /// Takes the first account out of a parsed test config without `Vec::remove`.
    pub(super) fn first_account(config: Config) -> Account {
        config
            .accounts
            .into_iter()
            .next()
            .expect("the config has an account")
    }

    fn groq(name: &str, env: &str, extra: &str) -> String {
        format!("[[accounts]]\nname = \"{name}\"\nprovider = \"groq\"\ncredential_env = \"{env}\"\n{extra}\n")
    }

    pub(super) fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap()
    }

    fn mins(n: i64) -> TimeDelta {
        TimeDelta::minutes(n)
    }

    pub(super) fn store(path: &Path, config: &str) -> HistoryStore {
        HistoryStore::new(path.to_path_buf(), &ConfigIdentity::Explicit(config.into()))
    }

    pub(super) fn quota(label: &str, used: Option<f64>, limit: Option<f64>) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: label.into(),
            used,
            limit,
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: Some("5h".into()),
            resets_at: Some(t0() + TimeDelta::hours(5)),
        }
    }

    fn raw(path: &Path) -> Connection {
        Connection::open(path).unwrap()
    }

    fn count(path: &Path, filter: &str) -> i64 {
        raw(path)
            .query_row(
                &format!("SELECT COUNT(*) FROM samples WHERE {filter}"),
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// Number of series rows of `config` marked for pruning.
    fn marked(path: &Path, config: &str) -> i64 {
        raw(path)
            .query_row(
                "SELECT COUNT(*) FROM series WHERE config_key = ?1 AND removed_at IS NOT NULL",
                [ConfigIdentity::Explicit(config.into()).digest()],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn creates_the_schema_migrates_forward_and_rejects_a_newer_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        assert_eq!(history.bounds(), Ok(None), "a fresh store is empty");
        let version = |path: &Path| {
            raw(path)
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        assert_eq!(version(&db), MIGRATIONS.len() as i64);
        let mode: String = raw(&db)
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");

        // A version-1 file (0.37.0) is migrated on first use and gains the
        // age index.
        let empty = dir.path().join("empty.sqlite3");
        migrate(&mut raw(&empty), &MIGRATIONS[..1]).unwrap();
        assert_eq!(version(&empty), 1);
        let config = config(CONFIG);
        let alpha = &config.accounts[0];
        store(&empty, "one")
            .record(alpha, t0(), &[quota("5h", Some(10.0), Some(100.0))])
            .unwrap();
        assert_eq!(version(&empty), MIGRATIONS.len() as i64);
        let billing: i64 = raw(&empty)
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('series') WHERE name = 'billing'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(billing, 1, "v1 files gain the billing column");
        let indexed: i64 = raw(&empty)
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'samples_by_age'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 1);

        // A later step runs alone on a current file and keeps its data.
        let mut current = raw(&empty);
        let mut next = MIGRATIONS.to_vec();
        next.push("ALTER TABLE samples ADD COLUMN note_free INTEGER");
        migrate(&mut current, &next).unwrap();
        assert_eq!(version(&empty), MIGRATIONS.len() as i64 + 1);
        assert_eq!(count(&empty, "used = 10.0"), 1);

        // Written by a newer Cuotai: every operation fails, nothing is guessed.
        for op in [
            store(&empty, "one").bounds().map(drop),
            store(&empty, "one").latest(alpha).map(drop),
            store(&empty, "one")
                .record(alpha, t0() + mins(5), &[quota("5h", Some(11.0), None)])
                .map(drop),
        ] {
            assert_eq!(op, Err(HistoryError::Storage));
        }
        assert_eq!(count(&empty, "1"), 1, "the newer file is untouched");
    }

    #[cfg(unix)]
    #[test]
    fn store_dir_and_files_are_private_even_when_preexisting() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let alpha = first_account(config(CONFIG));
        let fresh = dir.path().join("history").join("history.sqlite3");
        store(&fresh, "one")
            .record(&alpha, t0(), &[quota("5h", Some(1.0), None)])
            .unwrap();
        assert_eq!(
            (mode(fresh.parent().unwrap()), mode(&fresh)),
            (0o700, 0o600)
        );
        // While another instance has the file open, its WAL and shared
        // memory files exist and carry the file's mode.
        let reader = raw(&fresh);
        reader
            .query_row("SELECT COUNT(*) FROM samples", [], |r| r.get::<_, i64>(0))
            .unwrap();
        store(&fresh, "one")
            .record(&alpha, t0() + mins(1), &[quota("5h", Some(2.0), None)])
            .unwrap();
        for suffix in ["-wal", "-shm"] {
            let side = fresh.with_file_name(format!("history.sqlite3{suffix}"));
            assert_eq!(mode(&side), 0o600, "{suffix} follows the file");
        }
        drop(reader);

        let open = dir.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        let db = open.join("history.sqlite3");
        std::fs::write(&db, b"").unwrap();
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
        store(&db, "one").bounds().unwrap();
        assert_eq!((mode(&open), mode(&db)), (0o700, 0o600));
    }

    #[test]
    fn two_instances_write_concurrently_and_writes_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let alpha = first_account(config(CONFIG));
        // TUI and desktop: independent handles on one file, overlapping in
        // half of their readings.
        let barrier = std::sync::Barrier::new(2);
        let added: Vec<usize> = std::thread::scope(|scope| {
            let writers: Vec<_> = [0, 20]
                .into_iter()
                .map(|offset| {
                    let (history, alpha, barrier) = (store(&db, "one"), &alpha, &barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        (offset..offset + 40)
                            .map(|n| {
                                let metrics = [
                                    quota("5h", Some(n as f64), Some(100.0)),
                                    quota("7d", Some(n as f64 / 2.0), Some(100.0)),
                                ];
                                history.record(alpha, t0() + mins(n), &metrics).unwrap()
                            })
                            .sum()
                    })
                })
                .collect();
            writers.into_iter().map(|w| w.join().unwrap()).collect()
        });
        assert_eq!(
            added.iter().sum::<usize>(),
            2 * 60,
            "each sample stored once"
        );
        let history = store(&db, "one");
        let five_h = metric_key(&quota("5h", None, None));
        let points = history.series(&alpha, &five_h, ..).unwrap();
        assert_eq!(points.len(), 60);
        assert!(points.windows(2).all(|p| p[0].taken_at < p[1].taken_at));
        assert_eq!(
            raw(&db)
                .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );

        // Replaying a reading adds nothing and keeps the first values.
        let replay = [quota("5h", Some(99.0), Some(100.0))];
        assert_eq!(history.record(&alpha, t0(), &replay), Ok(0));
        assert_eq!(
            history.series(&alpha, &five_h, ..=t0()).unwrap()[0].used,
            0.0
        );
    }

    #[test]
    fn missing_and_non_finite_values_are_null_never_zero() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let alpha = first_account(config(CONFIG));
        let unknown = quota("5h", None, Some(100.0));
        let mut odd = quota("7d", Some(f64::NAN), Some(f64::INFINITY));
        odd.remaining = Some(f64::NEG_INFINITY);
        odd.window = None;
        odd.resets_at = None;
        history
            .record(&alpha, t0(), &[unknown.clone(), odd])
            .unwrap();

        assert_eq!(count(&db, "used IS NULL AND remaining IS NULL"), 2);
        assert_eq!(
            count(&db, "\"limit\" IS NULL"),
            1,
            "an infinite limit is unknown"
        );
        assert_eq!(count(&db, "\"window\" IS NULL AND resets_at IS NULL"), 1);
        let latest = history.latest(&alpha).unwrap().unwrap();
        let values: Vec<_> = latest
            .metrics
            .iter()
            .map(|m| (m.metric.used, m.metric.limit, m.metric.remaining))
            .collect();
        assert_eq!(values, [(None, Some(100.0), None), (None, None, None)]);
        assert_eq!(
            history.series(&alpha, &metric_key(&unknown), ..),
            Ok(vec![]),
            "a reading without `used` is not a point"
        );
    }

    /// PRD 0029 RF2.2: samples an older release stored under the legacy
    /// digest join the account id's series on the first read, survive a
    /// rename and a credential edit afterwards, and leave the schema version
    /// an older release accepts.
    #[test]
    fn legacy_series_move_to_the_account_id_and_survive_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let mut account = config(CONFIG).accounts[0].clone();
        account.id = crate::AccountId::parse("acc_0123456789ab");
        history
            .record(&account, t0(), &[quota("5h", Some(10.0), Some(100.0))])
            .unwrap();
        // What a pre-0029 release left behind: the same rows under the
        // legacy digest of the account.
        let conn = Connection::open(&db).unwrap();
        for table in ["samples", "series"] {
            conn.execute(
                &format!("UPDATE {table} SET account_digest = ?1"),
                [account_key(&account)],
            )
            .unwrap();
        }
        assert!(
            history.latest(&account).unwrap().is_some(),
            "adopted on read"
        );
        let legacy_left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM samples WHERE account_digest = ?1",
                [account_key(&account)],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_left, 0);
        let aliases: i64 = conn
            .query_row("SELECT COUNT(*) FROM aliases", [], |row| row.get(0))
            .unwrap();
        assert_eq!(aliases, 1);

        let mut renamed = account.clone();
        renamed.name = "renamed".into();
        renamed.credential_env = Some("GROQ_KEY_OTHER".into());
        let stored = history.latest(&renamed).unwrap().expect("same series");
        assert_eq!(stored.metrics.len(), 1);
        assert!(history.latest(&renamed).unwrap().is_some(), "idempotent");
        // No schema step an older release would reject as newer.
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn identity_follows_moves_and_starts_over_on_rename_or_new_credential() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let original = config(CONFIG);
        for account in &original.accounts {
            history
                .record(account, t0(), &[quota("5h", Some(10.0), Some(100.0))])
                .unwrap();
        }
        store(&db, "two")
            .record(
                &original.accounts[0],
                t0(),
                &[quota("5h", Some(70.0), None)],
            )
            .unwrap();
        let found = |toml: &str, index: usize| {
            history
                .latest(&config(toml).accounts[index])
                .unwrap()
                .is_some()
        };
        let alpha = groq("alpha", "GROQ_KEY_ALPHA", "");
        let beta = groq("beta", "GROQ_KEY_BETA", "");
        assert!(found(&(beta.clone() + &alpha), 1), "moved: same series");
        assert!(
            found(
                &groq("alpha", "GROQ_KEY_ALPHA", "params = { models = \"llama\" }"),
                0
            ),
            "display params do not change the identity"
        );
        // PRD 0029 RF2.2: the series is the account id's. These accounts
        // have no id line, so theirs derives from the legacy key: a rename
        // without an id is another account, while a credential or region
        // edit keeps the series.
        assert!(
            !found(&groq("alpha2", "GROQ_KEY_ALPHA", ""), 0),
            "renamed: new series"
        );
        for (case, edited) in [
            ("new credential", groq("alpha", "GROQ_KEY_OTHER", "")),
            (
                "new region",
                groq("alpha", "GROQ_KEY_ALPHA", "params = { region = \"eu\" }"),
            ),
        ] {
            assert!(found(&edited, 0), "{case}: same series");
        }

        // Saved without alpha: alpha's series of this configuration are
        // marked for pruning, kept until then; other configurations untouched.
        history.mark_removed(&config(&beta), t0()).unwrap();
        assert_eq!((marked(&db, "one"), marked(&db, "two")), (1, 0));
        assert!(found(&alpha, 0), "marked data stays until pruned");
        history
            .mark_removed(&config(&(alpha + &beta)), t0())
            .unwrap();
        assert_eq!(
            marked(&db, "one"),
            0,
            "an identity that comes back is unmarked"
        );
        history.mark_removed(&config(&beta), t0()).unwrap();
        history
            .record(
                &original.accounts[0],
                t0() + mins(5),
                &[quota("5h", Some(11.0), None)],
            )
            .unwrap();
        assert_eq!(marked(&db, "one"), 0, "recording unmarks too");

        // Marking never creates a store.
        let absent = dir.path().join("absent.sqlite3");
        store(&absent, "one").mark_removed(&original, t0()).unwrap();
        assert!(!absent.exists());
    }

    #[test]
    fn storage_failures_are_errors_never_success() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = first_account(config(CONFIG));
        let metrics = [quota("5h", Some(10.0), Some(100.0))];
        let file = dir.path().join("file");
        std::fs::write(&file, b"").unwrap();
        let corrupt = dir.path().join("corrupt.sqlite3");
        std::fs::write(&corrupt, vec![0x5a; 8192]).unwrap();
        let as_dir = dir.path().join("dir.sqlite3");
        std::fs::create_dir(&as_dir).unwrap();
        for (case, path) in [
            ("path is a directory", as_dir),
            ("parent is a file", file.join("history.sqlite3")),
            ("not a database", corrupt),
        ] {
            let broken = store(&path, "one");
            assert_eq!(
                broken.record(&alpha, t0(), &metrics),
                Err(HistoryError::Storage),
                "{case}"
            );
            assert_eq!(
                broken.latest(&alpha).map(drop),
                Err(HistoryError::Storage),
                "{case}"
            );
            assert_eq!(broken.bounds(), Err(HistoryError::Storage), "{case}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_is_reported_and_keeps_the_stored_data() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let alpha = first_account(config(CONFIG));
        history
            .record(&alpha, t0(), &[quota("5h", Some(10.0), Some(100.0))])
            .unwrap();
        // The disk refuses writes (read-only file, as on a full or remounted volume).
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o400)).unwrap();
        let later = [quota("5h", Some(20.0), Some(100.0))];
        assert_eq!(
            history.record(&alpha, t0() + mins(5), &later),
            Err(HistoryError::Storage)
        );
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o600)).unwrap();
        let five_h = metric_key(&later[0]);
        assert_eq!(
            history.series(&alpha, &five_h, ..).unwrap(),
            [Point {
                taken_at: t0(),
                used: 10.0
            }]
        );
    }

    #[test]
    fn the_file_holds_no_secret_name_or_note() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let mut alpha = first_account(config(CONFIG));
        alpha
            .params
            .insert("region".into(), "us-east-sekrit".into());
        store(&db, "secret-config-id")
            .record(&alpha, t0(), &[quota("5h", Some(10.0), Some(100.0))])
            .unwrap();
        let mut bytes = std::fs::read(&db).unwrap();
        bytes.extend(std::fs::read(db.with_file_name("history.sqlite3-wal")).unwrap_or_default());
        let bytes = String::from_utf8_lossy(&bytes);
        for raw in [
            "alpha",
            "GROQ_KEY",
            "sekrit",
            "secret-config-id",
            "groq",
            "5h\u{0}",
        ] {
            assert!(!bytes.contains(raw), "{raw} stored in the history");
        }
    }

    #[test]
    fn latest_returns_the_last_reading_per_metric_with_its_original_time() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir.path().join("history.sqlite3"), "one");
        let config = config(CONFIG);
        let (alpha, beta) = (&config.accounts[0], &config.accounts[1]);
        assert!(history.latest(alpha).unwrap().is_none());
        let mut week = quota("7d", Some(30.0), Some(100.0));
        week.window = Some(MetricWindow::Weekly);
        history
            .record(
                alpha,
                t0(),
                &[
                    quota("5h", Some(10.0), Some(100.0)),
                    week.clone(),
                    quota("old", Some(1.0), None),
                ],
            )
            .unwrap();
        // The next reading drops "old" and reorders the rest.
        week.used = Some(35.0);
        let later = t0() + mins(10);
        history
            .record(alpha, later, &[week, quota("5h", Some(12.0), Some(100.0))])
            .unwrap();
        history
            .record(beta, later + mins(1), &[quota("5h", Some(90.0), None)])
            .unwrap();

        let stored = history.latest(alpha).unwrap().unwrap();
        assert_eq!(
            stored.checked_at, later,
            "original time, not when it was read back"
        );
        let summary: Vec<_> = stored
            .metrics
            .iter()
            .map(|m| {
                (
                    m.metric.label.as_str(),
                    m.metric.used,
                    m.taken_at,
                    m.metric.window,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("7d", Some(35.0), later, Some(MetricWindow::Weekly)),
                ("5h", Some(12.0), later, Some(MetricWindow::Window5h)),
            ]
        );
        let five_h = &stored.metrics[1];
        assert_eq!(five_h.key, metric_key(&five_h.metric));
        assert_eq!(five_h.metric.resets_at, Some(t0() + TimeDelta::hours(5)));
        assert_eq!(
            (five_h.metric.kind, five_h.metric.period.as_deref()),
            (MetricKind::Quota, None)
        );
        assert_eq!(five_h.metric.unit, "%");
        assert_eq!(
            crate::freshness::age(stored.checked_at, later + mins(30)),
            Duration::from_secs(30 * 60)
        );
    }

    #[test]
    fn future_sample_is_rejected_and_last_good_stays_latest() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let alpha = first_account(config(CONFIG));
        let now = t0();
        let good_at = now - mins(1);
        let good = quota("5h", Some(10.0), Some(100.0));

        assert_eq!(history.record_at(&alpha, good_at, &[good], now), Ok(1));
        assert_eq!(
            history.record_at(
                &alpha,
                now + mins(5) + TimeDelta::milliseconds(1),
                &[quota("5h", Some(99.0), Some(100.0))],
                now,
            ),
            Ok(0)
        );

        assert_eq!(count(&db, "1 = 1"), 1);
        let latest = history.latest_at(&alpha, now).unwrap().unwrap();
        assert_eq!(latest.checked_at, good_at);
        assert_eq!(latest.metrics[0].metric.used, Some(10.0));
    }

    #[test]
    fn current_samples_and_the_future_tolerance_boundary_are_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let alpha = first_account(config(CONFIG));
        let now = t0();
        let accepted = [
            (now - TimeDelta::milliseconds(1), 10.0),
            (now, 11.0),
            (now + mins(5), 12.0),
        ];

        for (taken_at, used) in accepted {
            assert_eq!(
                history.record_at(
                    &alpha,
                    taken_at,
                    &[quota("5h", Some(used), Some(100.0))],
                    now,
                ),
                Ok(1),
                "{taken_at} should be inside the future bound"
            );
        }

        assert_eq!(count(&db, "1 = 1"), 3);
        assert_eq!(
            history.latest_at(&alpha, now).unwrap().unwrap().checked_at,
            now + mins(5)
        );
    }

    #[test]
    fn backdated_sample_is_accepted_without_replacing_the_newer_reading() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let alpha = first_account(config(CONFIG));
        let now = t0();
        let old = now - TimeDelta::days(30);

        assert_eq!(
            history.record_at(&alpha, now, &[quota("5h", Some(20.0), Some(100.0))], now,),
            Ok(1)
        );
        assert_eq!(
            history.record_at(&alpha, old, &[quota("5h", Some(5.0), Some(100.0))], now,),
            Ok(1)
        );

        assert_eq!(count(&db, "1 = 1"), 2, "the old import is retained");
        let latest = history.latest_at(&alpha, now).unwrap().unwrap();
        assert_eq!(latest.checked_at, now);
        assert_eq!(latest.metrics[0].metric.used, Some(20.0));
    }

    #[test]
    fn latest_ignores_a_future_row_already_present_in_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let alpha = first_account(config(CONFIG));
        let now = t0();
        let metric = quota("5h", Some(10.0), Some(100.0));
        history.record_at(&alpha, now, &[metric], now).unwrap();
        let db = raw(&db);
        db.execute(
            "INSERT INTO samples
                (config_key, account_digest, metric_key, taken_at, used, \"limit\",
                 remaining, unit, kind, \"window\", resets_at)
             SELECT config_key, account_digest, metric_key, ?1, 99.0, \"limit\",
                    remaining, unit, kind, \"window\", resets_at
             FROM samples WHERE taken_at = ?2",
            rusqlite::params![millis(now + mins(10)), millis(now)],
        )
        .unwrap();
        db.execute(
            "UPDATE series SET seen_at = ?1 WHERE seen_at = ?2",
            rusqlite::params![millis(now + mins(10)), millis(now)],
        )
        .unwrap();

        let latest = history.latest_at(&alpha, now).unwrap().unwrap();
        assert_eq!(latest.checked_at, now);
        assert_eq!(latest.metrics[0].metric.used, Some(10.0));
    }

    #[test]
    fn series_and_bounds_honour_the_range_and_the_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let config = config(CONFIG);
        let (alpha, beta) = (&config.accounts[0], &config.accounts[1]);
        for n in 0..5 {
            let used = if n == 2 { f64::INFINITY } else { n as f64 };
            history
                .record(
                    alpha,
                    t0() + mins(10 * n),
                    &[quota("5h", Some(used), Some(100.0))],
                )
                .unwrap();
        }
        history
            .record(beta, t0() - mins(30), &[quota("5h", Some(1.0), None)])
            .unwrap();
        store(&db, "two")
            .record(
                alpha,
                t0() - TimeDelta::days(9),
                &[quota("5h", Some(1.0), None)],
            )
            .unwrap();

        let key = metric_key(&quota("5h", None, None));
        let used = |range: (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>)| -> Vec<f64> {
            history
                .series(alpha, &key, range)
                .unwrap()
                .iter()
                .map(|p| p.used)
                .collect()
        };
        use Bound::{Excluded, Included, Unbounded};
        assert_eq!(
            used((Unbounded, Unbounded)),
            [0.0, 1.0, 3.0, 4.0],
            "non-finite skipped"
        );
        assert_eq!(
            used((Included(t0() + mins(10)), Included(t0() + mins(30)))),
            [1.0, 3.0]
        );
        assert_eq!(
            used((Excluded(t0() + mins(10)), Excluded(t0() + mins(40)))),
            [3.0]
        );
        assert_eq!(
            used((Included(t0() + mins(41)), Unbounded)),
            Vec::<f64>::new()
        );
        assert!(history.series(alpha, "unknown", ..).unwrap().is_empty());
        assert_eq!(
            history.windows(alpha, &key, t0()..).unwrap().len(),
            1,
            "one window, the non-finite sample ignored"
        );

        assert_eq!(
            history.bounds(),
            Ok(Some(Bounds {
                first: t0() - mins(30),
                last: t0() + mins(40)
            })),
            "every account of this configuration, no other"
        );
    }

    #[test]
    fn windows_group_samples_by_instance() {
        let reset = t0() + TimeDelta::hours(5);
        let sample = |at: i64, used: f64, limit: Option<f64>, resets: Option<i64>| WindowSample {
            taken_at: t0() + mins(at),
            used: Some(used),
            limit,
            resets_at: resets.map(|r| reset + mins(r)),
        };
        let cap = Some(100.0);
        let day = 24 * 60;
        /// (initial, final, peak, samples) of one window.
        type Summary = (f64, f64, f64, usize);
        let cases: Vec<(&str, Vec<WindowSample>, Vec<Summary>)> = vec![
            (
                "increasing",
                vec![
                    sample(0, 10.0, cap, Some(0)),
                    sample(60, 20.0, cap, Some(0)),
                    sample(120, 35.0, cap, Some(0)),
                ],
                vec![(10.0, 35.0, 35.0, 3)],
            ),
            (
                "reset: drop above 20% of the limit",
                vec![
                    sample(0, 80.0, cap, Some(0)),
                    sample(60, 90.0, cap, Some(0)),
                    sample(120, 5.0, cap, Some(0)),
                ],
                vec![(80.0, 90.0, 90.0, 2), (5.0, 5.0, 5.0, 1)],
            ),
            (
                "a drop up to 20% stays in the window",
                vec![
                    sample(0, 50.0, cap, None),
                    sample(60, 30.0, cap, None),
                    sample(90, 40.0, cap, None),
                ],
                vec![(50.0, 40.0, 50.0, 3)],
            ),
            (
                "resets_at drift within the tolerance",
                vec![
                    sample(0, 10.0, cap, Some(0)),
                    sample(10, 12.0, cap, Some(2)),
                    sample(20, 14.0, cap, Some(4)),
                    sample(30, 16.0, cap, Some(6)),
                ],
                vec![(10.0, 16.0, 16.0, 4)],
            ),
            (
                "resets_at advances beyond the tolerance",
                vec![
                    sample(0, 10.0, cap, Some(0)),
                    sample(10, 12.0, cap, Some(3)),
                    sample(20, 14.0, cap, Some(3)),
                ],
                vec![(10.0, 10.0, 10.0, 1), (12.0, 14.0, 14.0, 2)],
            ),
            (
                "gaps of days",
                vec![
                    sample(0, 10.0, cap, Some(20 * day)),
                    sample(3 * day, 30.0, cap, Some(20 * day)),
                    sample(15 * day, 60.0, cap, Some(20 * day)),
                    sample(22 * day, 70.0, cap, Some(50 * day)),
                    sample(25 * day, 72.0, cap, None),
                ],
                vec![(10.0, 60.0, 60.0, 3), (70.0, 72.0, 72.0, 2)],
            ),
            (
                "no limit: a drop alone never resets",
                vec![
                    sample(0, 100.0, None, None),
                    sample(60, 150.0, None, None),
                    sample(120, 20.0, None, None),
                ],
                vec![(100.0, 20.0, 150.0, 3)],
            ),
            (
                "no limit: resets_at still splits",
                vec![
                    sample(0, 100.0, None, Some(0)),
                    sample(60, 150.0, None, Some(0)),
                    sample(120, 20.0, None, Some(day)),
                ],
                vec![(100.0, 150.0, 150.0, 2), (20.0, 20.0, 20.0, 1)],
            ),
            (
                "non-finite values are ignored",
                vec![
                    sample(0, 80.0, Some(f64::INFINITY), None),
                    sample(30, f64::NAN, cap, None),
                    sample(40, f64::INFINITY, cap, None),
                    sample(60, 5.0, Some(f64::NAN), None),
                ],
                vec![(80.0, 5.0, 80.0, 2)],
            ),
        ];
        for (case, samples, expected) in cases {
            let got: Vec<_> = group_windows(samples)
                .iter()
                .map(|w| (w.initial_used, w.final_used, w.peak_used, w.samples))
                .collect();
            assert_eq!(got, expected, "{case}");
        }

        let windows = group_windows([
            sample(0, 10.0, None, Some(0)),
            sample(30, 60.0, cap, Some(1)),
            sample(60, 50.0, None, Some(2)),
        ]);
        assert_eq!(windows.len(), 1);
        let window = &windows[0];
        assert_eq!(
            (window.started_at, window.ended_at, window.peak_at),
            (t0(), t0() + mins(60), t0() + mins(30))
        );
        assert_eq!(
            (window.limit, window.resets_at),
            (cap, Some(reset + mins(2)))
        );
        assert!(group_windows([]).is_empty());
    }

    #[test]
    fn a_repeated_sample_within_15_minutes_is_compacted_unless_the_window_changes() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let alpha = first_account(config(CONFIG));
        let reset = t0() + TimeDelta::hours(5);
        let reading = |used: f64, resets: TimeDelta| Metric {
            resets_at: Some(reset + resets),
            ..quota("5h", Some(used), Some(100.0))
        };
        let secs = TimeDelta::seconds;
        // (case, offset from t0, metric, stored)
        let steps = [
            ("first", secs(0), reading(10.0, secs(0)), 1),
            ("same, 14:59 later", secs(899), reading(10.0, secs(0)), 0),
            (
                "same window, drifted 2 min",
                mins(10),
                reading(10.0, mins(2)),
                0,
            ),
            (
                "same, 15 min after the stored one",
                mins(15),
                reading(10.0, secs(0)),
                1,
            ),
            ("changed value", mins(16), reading(11.0, secs(0)), 1),
            (
                "new window, same value",
                mins(17),
                reading(11.0, secs(121)),
                1,
            ),
            ("replayed instant", mins(17), reading(11.0, secs(121)), 0),
        ];
        for (case, offset, metric, stored) in steps {
            assert_eq!(
                history.record(&alpha, t0() + offset, &[metric]),
                Ok(stored),
                "{case}"
            );
        }
        let missing = quota("other", None, None);
        history
            .record(&alpha, t0() + mins(18), std::slice::from_ref(&missing))
            .unwrap();
        assert_eq!(
            history.record(&alpha, t0() + mins(19), &[missing]),
            Ok(0),
            "missing values repeat too"
        );
        let key = metric_key(&reading(0.0, secs(0)));
        assert_eq!(history.series(&alpha, &key, ..).unwrap().len(), 4);

        // A compacted reading still dates the account's last reading.
        history
            .record(&alpha, t0() + mins(20), &[reading(11.0, secs(121))])
            .unwrap();
        let latest = history.latest(&alpha).unwrap().unwrap();
        assert_eq!(latest.checked_at, t0() + mins(20));
        assert_eq!(latest.metrics[0].taken_at, t0() + mins(17));
    }

    #[test]
    fn latest_reports_restore_ready_readings_in_configuration_order() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let config = config(CONFIG);
        assert_eq!(history.latest_reports(&config).unwrap().len(), 2);
        assert!(!db.exists(), "reading never creates the file");
        history
            .record(
                &config.accounts[1],
                t0(),
                &[quota("5h", Some(42.0), Some(100.0))],
            )
            .unwrap();
        let restored = history.latest_reports(&config).unwrap();
        assert!(restored[0].is_none());
        let beta = restored[1].as_ref().unwrap();
        assert_eq!(
            (beta.account.as_str(), beta.status, beta.checked_at),
            ("beta", Status::Ready, t0())
        );
        assert_eq!(beta.metrics[0].used, Some(42.0));
        assert!(beta.notes.is_empty() && beta.source.is_empty());
    }

    #[test]
    fn recent_reads_the_pace_lookback_of_each_eligible_metric() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let alpha = first_account(config(CONFIG));
        // 5-hour window ending at t0 + 5h, so `now` = t0 + 3h is 3/5 in.
        let now = t0() + TimeDelta::hours(3);
        let five_h = |used: f64| Metric {
            resets_at: Some(t0() + TimeDelta::hours(5)),
            ..quota("5h", Some(used), Some(100.0))
        };
        let rate = Metric {
            kind: MetricKind::RateLimit,
            ..five_h(1.0)
        };
        assert_eq!(
            history.recent(&alpha, &[five_h(50.0), rate.clone()], now),
            Ok(vec![vec![], vec![]])
        );
        assert!(!db.exists(), "nothing recorded: nothing read or created");
        for minute in [0, 50, 70, 120, 150, 180, 190] {
            let at = t0() + mins(minute);
            history
                .record(&alpha, at, &[five_h(minute as f64 / 4.0), rate.clone()])
                .unwrap();
        }
        let recent = history.recent(&alpha, &[five_h(50.0), rate], now).unwrap();
        let minutes: Vec<i64> = recent[0]
            .iter()
            .map(|p| (p.taken_at - t0()).num_minutes())
            .collect();
        assert_eq!(minutes, [70, 120, 150, 180], "the last 2 hours up to now");
        assert!(recent[1].is_empty(), "a rate limit has no pace");
        let pace = crate::pace::assess(&five_h(45.0), now, Some(&recent[0])).unwrap();
        assert_eq!(pace.source, crate::pace::PaceSource::Recent);
    }
}
