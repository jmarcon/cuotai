//! PRD 0006 RF4 — retention, resolution reduction, size ceiling, export and
//! erasure of the measurement history.
//!
//! Pruning is file-wide (the history is a per-device file shared by every
//! configuration on it) and runs in short `BEGIN IMMEDIATE` batches, so the
//! other instances keep reading (WAL) and writing (within the busy timeout)
//! while it works. Export and erasure are explicit user actions.

use super::{finite, instances, instant, millis, HistoryError, HistoryStore, WindowSample};
use crate::models::account_key;
use crate::text::{params, Localized};
use crate::{Account, Provider};
use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::collections::BTreeMap;
use std::io::Write;
use std::ops::RangeBounds;

/// Rows deleted per transaction, so no batch holds the write lock long.
const BATCH: i64 = 2_000;
/// Samples younger than this keep their full resolution (RF4.2).
const FULL_RESOLUTION: TimeDelta = TimeDelta::days(7);
const HOUR_MS: i64 = 3_600_000;
/// A series marked removed is pruned once the mark is this old, so an
/// account renamed back (or a TOML briefly edited by hand) keeps its data.
const REMOVED_GRACE: TimeDelta = TimeDelta::hours(24);
/// After a ceiling prune the file is brought this far below the ceiling, so
/// the next readings do not trigger another prune at once.
const CEILING_TARGET_PERCENT: u64 = 90;
/// `meta` key: samples older than this instant are already reduced.
const REDUCED_UNTIL: &str = "reduced_until";

/// How long and how large the history may grow (RF4.1, RF4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    days: u16,
    max_bytes: u64,
}

impl Retention {
    pub const DEFAULT_DAYS: u16 = 90;
    pub const MIN_DAYS: u16 = 7;
    pub const MAX_DAYS: u16 = 365;
    /// Default size ceiling: 64 MiB of used pages.
    pub const DEFAULT_MAX_BYTES: u64 = 64 << 20;

    /// Keep `days` (7 to 365) of history under the default ceiling.
    pub fn days(days: u32) -> Result<Self, RetentionError> {
        u16::try_from(days)
            .ok()
            .filter(|days| (Self::MIN_DAYS..=Self::MAX_DAYS).contains(days))
            .map(|days| Self {
                days,
                ..Self::default()
            })
            .ok_or(RetentionError::Bounds)
    }

    /// The same retention under another size ceiling (in bytes of used
    /// database pages).
    pub fn with_max_bytes(self, max_bytes: u64) -> Self {
        Self { max_bytes, ..self }
    }

    pub fn keep_days(&self) -> u16 {
        self.days
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            days: Self::DEFAULT_DAYS,
            max_bytes: Self::DEFAULT_MAX_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RetentionError {
    #[error("A retenção do histórico vai de 7 a 365 dias.")]
    Bounds,
}

impl RetentionError {
    pub fn l10n(&self) -> Localized {
        Localized::with_params(
            "history.error.retention_bounds",
            params([
                ("min", Retention::MIN_DAYS.into()),
                ("max", Retention::MAX_DAYS.into()),
            ]),
            self.to_string(),
        )
    }
}

/// What one [`HistoryStore::prune`] removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Samples of accounts gone from their configuration for over a day.
    pub removed: usize,
    /// Samples older than the retention.
    pub expired: usize,
    /// Samples folded by the hourly resolution of data older than 7 days.
    pub reduced: usize,
    /// Set when the file exceeded the size ceiling (a visible warning).
    pub ceiling: Option<CeilingPrune>,
}

/// The size ceiling forced pruning: `removed` samples, the oldest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CeilingPrune {
    pub removed: usize,
    pub max_bytes: u64,
}

/// Export file format (RF4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Json,
}

const CSV_HEADER: &str =
    "account,provider,metric,metric_key,unit,kind,window,taken_at,used,limit,remaining,resets_at";

/// One exported sample; missing values are `null` (JSON) or empty (CSV).
#[derive(serde::Serialize)]
struct Row<'a> {
    account: &'a str,
    provider: Provider,
    metric: String,
    metric_key: String,
    unit: String,
    kind: String,
    window: Option<String>,
    taken_at: DateTime<Utc>,
    used: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
}

impl Row<'_> {
    fn csv(&self) -> String {
        let number = |value: Option<f64>| value.map(|v| v.to_string()).unwrap_or_default();
        let time = |at: Option<DateTime<Utc>>| at.map(|at| at.to_rfc3339()).unwrap_or_default();
        [
            csv_text(self.account),
            self.provider.slug().to_owned(),
            csv_text(&self.metric),
            self.metric_key.clone(),
            csv_text(&self.unit),
            self.kind.clone(),
            self.window.clone().unwrap_or_default(),
            self.taken_at.to_rfc3339(),
            number(self.used),
            number(self.limit),
            number(self.remaining),
            time(self.resets_at),
        ]
        .join(",")
    }
}

/// A CSV text field: quoted when it holds a separator, a quote or a line
/// break, and prefixed with `'` when a spreadsheet would read it as a
/// formula.
fn csv_text(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// Run `sql` (a `DELETE … LIMIT ?2` with the cutoff in `?1`) in batches of
/// [`BATCH`], one short transaction each, until it removes fewer.
fn delete_batches(db: &mut Connection, sql: &str, cutoff: i64) -> Result<usize, HistoryError> {
    let mut total = 0;
    loop {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = tx.execute(sql, rusqlite::params![cutoff, BATCH])?;
        tx.commit()?;
        total += removed;
        if (removed as i64) < BATCH {
            return Ok(total);
        }
    }
}

/// Bytes held by live database pages (free pages are reused, so this is
/// what the file grows by).
fn used_bytes(db: &Connection) -> rusqlite::Result<u64> {
    let pragma = |name: &str| db.query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0));
    let pages = pragma("page_count")? - pragma("freelist_count")?;
    Ok(u64::try_from(pages.max(0)).unwrap_or(0) * u64::try_from(pragma("page_size")?).unwrap_or(0))
}

fn drop_orphan_series(db: &Connection) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM series WHERE NOT EXISTS (SELECT 1 FROM samples x
            WHERE x.config_key = series.config_key AND x.account_digest = series.account_digest
              AND x.metric_key = series.metric_key)",
        [],
    )
}

/// RF4.2: which samples of one series survive the hourly resolution: per
/// window instance and hour, the one with the highest `used` (the first on
/// ties, so each window's peak, which [`super::WindowUsage`] dates by its
/// first occurrence, stays). An hour with no finite `used` keeps its last
/// sample.
fn hourly_keepers(samples: &[(i64, WindowSample)]) -> Vec<bool> {
    let windows: Vec<WindowSample> = samples.iter().map(|(_, s)| *s).collect();
    let mut best: BTreeMap<(i64, Option<usize>), usize> = BTreeMap::new();
    for (index, ((at, sample), window)) in samples.iter().zip(instances(&windows)).enumerate() {
        let key = (at.div_euclid(HOUR_MS), window);
        match best.get(&key) {
            Some(&kept) if window.is_some() => {
                if finite(sample.used) > finite(samples[kept].1.used) {
                    best.insert(key, index);
                }
            }
            _ => {
                best.insert(key, index);
            }
        }
    }
    let mut keep = vec![false; samples.len()];
    for (&(hour, window), &index) in &best {
        let measured_hour = best
            .range((hour, Some(0))..(hour + 1, None))
            .next()
            .is_some();
        if window.is_some() || !measured_hour {
            keep[index] = true;
        }
    }
    keep
}

impl HistoryStore {
    /// RF4: prune the whole file by this store's [`Retention`] at `now`:
    /// series of accounts marked removed over a day ago, samples older than
    /// the retention, samples older than 7 days down to one per hour and
    /// window (the highest `used`, so each window's peak stays), then, when
    /// the live pages exceed the size ceiling, the oldest samples until the
    /// file is 10% below it ([`PruneReport::ceiling`], to be shown, never
    /// silent). Each step deletes in short batches. Never creates the file.
    pub fn prune(&self, now: DateTime<Utc>) -> Result<PruneReport, HistoryError> {
        let Some(mut db) = self.open_existing()? else {
            return Ok(PruneReport::default());
        };
        let mut report = PruneReport {
            removed: delete_batches(
                &mut db,
                "DELETE FROM samples WHERE (config_key, account_digest, metric_key, taken_at) IN (
                    SELECT x.config_key, x.account_digest, x.metric_key, x.taken_at
                    FROM series s JOIN samples x ON x.config_key = s.config_key
                     AND x.account_digest = s.account_digest AND x.metric_key = s.metric_key
                    WHERE s.removed_at <= ?1 LIMIT ?2)",
                millis(now - REMOVED_GRACE),
            )?,
            expired: delete_batches(
                &mut db,
                "DELETE FROM samples WHERE (config_key, account_digest, metric_key, taken_at) IN (
                    SELECT config_key, account_digest, metric_key, taken_at FROM samples
                    WHERE taken_at < ?1 LIMIT ?2)",
                millis(now - TimeDelta::days(i64::from(self.retention.keep_days()))),
            )?,
            ..PruneReport::default()
        };
        report.reduced = self.reduce(&mut db, now)?;
        report.ceiling = self.enforce_ceiling(&mut db)?;
        drop_orphan_series(&db)?;
        Ok(report)
    }

    /// RF4.2 over the samples not reduced yet (from the `meta` mark to the
    /// hour that began 7 days ago), one series per transaction.
    fn reduce(&self, db: &mut Connection, now: DateTime<Utc>) -> Result<usize, HistoryError> {
        let until = millis(now - FULL_RESOLUTION).div_euclid(HOUR_MS) * HOUR_MS;
        let from = db
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                [REDUCED_UNTIL],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(i64::MIN);
        if from >= until {
            return Ok(0);
        }
        let series: Vec<(String, String, String)> = db
            .prepare(
                "SELECT DISTINCT config_key, account_digest, metric_key FROM samples
                 WHERE taken_at >= ?1 AND taken_at < ?2",
            )?
            .query_map([from, until], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut reduced = 0;
        for (config, account, metric) in series {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let samples: Vec<(i64, WindowSample)> = tx
                .prepare_cached(
                    "SELECT taken_at, used, \"limit\", resets_at FROM samples
                     WHERE config_key = ?1 AND account_digest = ?2 AND metric_key = ?3
                       AND taken_at >= ?4 AND taken_at < ?5
                     ORDER BY taken_at",
                )?
                .query_map(
                    rusqlite::params![config, account, metric, from, until],
                    |row| {
                        let at: i64 = row.get(0)?;
                        Ok((
                            at,
                            WindowSample {
                                taken_at: instant(at)?,
                                used: row.get(1)?,
                                limit: row.get(2)?,
                                resets_at: row
                                    .get::<_, Option<i64>>(3)?
                                    .map(instant)
                                    .transpose()?,
                            },
                        ))
                    },
                )?
                .collect::<rusqlite::Result<_>>()?;
            {
                let mut delete = tx.prepare_cached(
                    "DELETE FROM samples WHERE config_key = ?1 AND account_digest = ?2
                       AND metric_key = ?3 AND taken_at = ?4",
                )?;
                for ((at, _), keep) in samples.iter().zip(hourly_keepers(&samples)) {
                    if !keep {
                        reduced +=
                            delete.execute(rusqlite::params![config, account, metric, at])?;
                    }
                }
            }
            tx.commit()?;
        }
        db.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = MAX(value, excluded.value)",
            rusqlite::params![REDUCED_UNTIL, until],
        )?;
        Ok(reduced)
    }

    /// RF4.3: delete the oldest samples of the file while its live pages
    /// exceed the ceiling.
    fn enforce_ceiling(&self, db: &mut Connection) -> Result<Option<CeilingPrune>, HistoryError> {
        let max_bytes = self.retention.max_bytes();
        if used_bytes(db)? <= max_bytes {
            return Ok(None);
        }
        let target = max_bytes / 100 * CEILING_TARGET_PERCENT;
        let mut removed = 0;
        loop {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let deleted = tx.execute(
                "DELETE FROM samples WHERE (config_key, account_digest, metric_key, taken_at) IN (
                    SELECT config_key, account_digest, metric_key, taken_at FROM samples
                    ORDER BY taken_at LIMIT ?1)",
                [BATCH],
            )?;
            tx.commit()?;
            removed += deleted;
            if deleted == 0 || used_bytes(db)? <= target {
                break;
            }
        }
        Ok(Some(CeilingPrune { removed, max_bytes }))
    }

    /// RF4.4: write the samples of `accounts` (of this configuration), of
    /// one metric (`metric_key`) or all, taken within `range`, to `out` in
    /// `format`, ordered by account, metric position and time. Rows carry
    /// the account name and provider given here, the metric label, unit,
    /// kind and window, and the stored values; a missing value is `null`
    /// (JSON) or an empty field (CSV), never zero. Returns the rows written.
    pub fn export(
        &self,
        accounts: &[Account],
        metric_key: Option<&str>,
        range: impl RangeBounds<DateTime<Utc>>,
        format: ExportFormat,
        out: &mut dyn Write,
    ) -> Result<usize, HistoryError> {
        let (start, end) = super::span(range);
        let write = |out: &mut dyn Write, text: &str| {
            out.write_all(text.as_bytes())
                .map_err(|_| HistoryError::Export)
        };
        let header = match format {
            ExportFormat::Csv => format!("{CSV_HEADER}\n"),
            ExportFormat::Json => "{\"version\":1,\"samples\":[".to_owned(),
        };
        write(out, &header)?;
        let mut rows = 0;
        if let Some(db) = self.open_existing()? {
            let mut query = db.prepare(
                "SELECT s.label, x.metric_key, x.unit, x.kind, x.\"window\", x.taken_at,
                        x.used, x.\"limit\", x.remaining, x.resets_at
                 FROM samples x JOIN series s ON s.config_key = x.config_key
                   AND s.account_digest = x.account_digest AND s.metric_key = x.metric_key
                 WHERE x.config_key = ?1 AND x.account_digest = ?2
                   AND (?3 IS NULL OR x.metric_key = ?3) AND x.taken_at BETWEEN ?4 AND ?5
                 ORDER BY s.position, x.metric_key, x.taken_at",
            )?;
            for account in accounts {
                let mut found = query.query(rusqlite::params![
                    self.config_key,
                    account_key(account),
                    metric_key,
                    start,
                    end
                ])?;
                while let Some(sample) = found.next()? {
                    let row = Row {
                        account: &account.name,
                        provider: account.provider,
                        metric: sample.get(0)?,
                        metric_key: sample.get(1)?,
                        unit: sample.get(2)?,
                        kind: sample.get(3)?,
                        window: sample.get(4)?,
                        taken_at: instant(sample.get(5)?)?,
                        used: finite(sample.get(6)?),
                        limit: finite(sample.get(7)?),
                        remaining: finite(sample.get(8)?),
                        resets_at: sample.get::<_, Option<i64>>(9)?.map(instant).transpose()?,
                    };
                    let line = match format {
                        ExportFormat::Csv => row.csv() + "\n",
                        ExportFormat::Json => {
                            let json =
                                serde_json::to_string(&row).map_err(|_| HistoryError::Export)?;
                            format!("{}\n{json}", if rows == 0 { "" } else { "," })
                        }
                    };
                    write(out, &line)?;
                    rows += 1;
                }
            }
        }
        if format == ExportFormat::Json {
            write(out, "\n]}\n")?;
        }
        out.flush().map_err(|_| HistoryError::Export)?;
        Ok(rows)
    }

    /// RF4.4: erase the history of `account` in this configuration, plus the
    /// series of this configuration already marked removed (orphans).
    /// Deleted content is overwritten (`secure_delete`) and the WAL is
    /// truncated. Returns the samples removed; never creates the file.
    pub fn erase_account(&self, account: &Account) -> Result<usize, HistoryError> {
        let Some(mut db) = self.open_existing()? else {
            return Ok(0);
        };
        db.pragma_update(None, "secure_delete", true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let digest = account_key(account);
        let removed = tx.execute(
            "DELETE FROM samples WHERE config_key = ?1 AND (account_digest = ?2
                OR (account_digest, metric_key) IN (SELECT account_digest, metric_key FROM series
                    WHERE config_key = ?1 AND removed_at IS NOT NULL))",
            rusqlite::params![self.config_key, digest],
        )?;
        tx.execute(
            "DELETE FROM series WHERE config_key = ?1
               AND (account_digest = ?2 OR removed_at IS NOT NULL)",
            rusqlite::params![self.config_key, digest],
        )?;
        drop_orphan_series(&tx)?;
        tx.commit()?;
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(removed)
    }

    /// RF4.4: erase the whole history file (every configuration on this
    /// device), then rewrite it (`VACUUM`) and truncate the WAL so nothing
    /// of it stays on disk. Returns the samples removed; never creates the
    /// file.
    pub fn erase_all(&self) -> Result<usize, HistoryError> {
        let Some(mut db) = self.open_existing()? else {
            return Ok(0);
        };
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = tx.execute("DELETE FROM samples", [])?;
        tx.execute_batch("DELETE FROM series; DELETE FROM meta;")?;
        tx.commit()?;
        db.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(removed)
    }

    /// RF5.3: bytes the history file takes on disk with its WAL and shared
    /// memory side files (the whole per-device file, every configuration);
    /// 0 before anything was recorded. Never creates the file.
    pub fn size_on_disk(&self) -> Result<u64, HistoryError> {
        let mut total = 0;
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.clone().into_os_string();
            path.push(suffix);
            match std::fs::metadata(&path) {
                Ok(meta) => total += meta.len(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(HistoryError::Storage),
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{config, first_account, quota, store, t0, CONFIG};
    use super::super::{metric_key, Bounds};
    use super::*;
    use crate::{Metric, MetricKind, MetricWindow};
    use std::path::Path;

    fn hours(n: i64) -> TimeDelta {
        TimeDelta::hours(n)
    }

    fn days(n: i64) -> TimeDelta {
        TimeDelta::days(n)
    }

    fn total(path: &Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn retention_days_are_bounded() {
        assert_eq!(Retention::default().keep_days(), 90);
        assert_eq!(Retention::default().max_bytes(), 64 << 20);
        for days in [7, 90, 365] {
            assert_eq!(
                Retention::days(days).map(|r| r.keep_days()),
                Ok(days as u16)
            );
        }
        for days in [0, 6, 366, 70_000] {
            assert_eq!(Retention::days(days), Err(RetentionError::Bounds), "{days}");
        }
        let text = RetentionError::Bounds.l10n();
        assert_eq!(
            text.resolve("pt-BR"),
            "A retenção do histórico vai de 7 a 365 dias."
        );
        assert_eq!(
            text.resolve("en-US"),
            "History retention ranges from 7 to 365 days."
        );
    }

    #[test]
    fn prune_by_age_keeps_the_retention_and_drops_removed_accounts_after_a_day() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let config = config(CONFIG);
        let (alpha, beta) = (&config.accounts[0], &config.accounts[1]);
        let now = t0() + days(40);
        let history = store(&db, "one").with_retention(Retention::days(30).unwrap());
        assert_eq!(history.prune(now), Ok(PruneReport::default()));
        assert!(!db.exists(), "pruning never creates the file");
        // Alpha: one reading per day for 40 days; a different value each
        // time so compaction keeps all of them.
        for day in 0..=40 {
            let used = Some(day as f64);
            history
                .record(alpha, t0() + days(day), &[quota("5h", used, Some(100.0))])
                .unwrap();
        }
        history
            .record(beta, now - hours(1), &[quota("5h", Some(1.0), None)])
            .unwrap();
        // Another configuration on the same device shares the retention.
        store(&db, "two")
            .record(alpha, t0(), &[quota("5h", Some(5.0), None)])
            .unwrap();

        let report = history.prune(now).unwrap();
        assert_eq!(
            report.expired, 11,
            "days 0 to 9 of alpha and the other file's"
        );
        assert_eq!((report.removed, report.ceiling), (0, None));
        let key = metric_key(&quota("5h", None, None));
        let kept = history.series(alpha, &key, ..).unwrap();
        assert_eq!(kept.first().map(|p| p.taken_at), Some(t0() + days(10)));
        assert_eq!(kept.last().map(|p| p.taken_at), Some(now));
        assert!(
            store(&db, "two").latest(alpha).unwrap().is_none(),
            "orphan series dropped"
        );

        // Beta leaves the TOML: kept for a day, then pruned with its series.
        history
            .mark_removed(&config.without_account(1).unwrap(), now)
            .unwrap();
        assert_eq!(history.prune(now + hours(23)).unwrap().removed, 0);
        assert!(history.latest(beta).unwrap().is_some());
        assert_eq!(history.prune(now + hours(24)).unwrap().removed, 1);
        assert!(history.latest(beta).unwrap().is_none());
        assert!(history.latest(alpha).unwrap().is_some());
    }

    #[test]
    fn old_samples_fold_to_one_per_hour_keeping_each_window_peak() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let alpha = first_account(config(CONFIG));
        let history = store(&db, "one");
        let base = t0() - days(10);
        let at = |minutes: i64| base + TimeDelta::minutes(minutes);
        let sample = |used: f64, resets: TimeDelta| Metric {
            resets_at: Some(base + resets),
            ..quota("5h", Some(used), Some(100.0))
        };
        // Hour 0: one window rising to 30 at minute 40, then easing.
        for (minute, used) in [(0, 10.0), (20, 20.0), (40, 30.0), (50, 28.0)] {
            history
                .record(&alpha, at(minute), &[sample(used, hours(1))])
                .unwrap();
        }
        // Hour 1: the window peaks at 90 (minute 70), resets at minute 80
        // and the new window climbs to 15: both peaks stay.
        for (minute, used, resets) in [
            (65, 60.0, hours(1)),
            (70, 90.0, hours(1)),
            (80, 2.0, hours(6)),
            (100, 15.0, hours(6)),
            (110, 12.0, hours(6)),
        ] {
            history
                .record(&alpha, at(minute), &[sample(used, resets)])
                .unwrap();
        }
        // Hour 2: no `used` at all keeps one sample.
        for minute in [125, 140] {
            history
                .record(&alpha, at(minute), &[quota("5h", None, Some(100.0))])
                .unwrap();
        }
        // Six days old: full resolution.
        let recent = t0() - days(6);
        for (minute, used) in [(0, 1.0), (10, 2.0), (20, 3.0)] {
            history
                .record(
                    &alpha,
                    recent + TimeDelta::minutes(minute),
                    &[sample(used, days(5))],
                )
                .unwrap();
        }
        let key = metric_key(&sample(0.0, hours(1)));
        let peaks = |from, to| -> Vec<f64> {
            history
                .windows(&alpha, &key, from..to)
                .unwrap()
                .iter()
                .map(|w| w.peak_used)
                .collect()
        };
        assert_eq!(peaks(base, base + hours(3)), [90.0, 15.0]);

        let report = history.prune(t0()).unwrap();
        assert_eq!(report.reduced, 7);
        let used: Vec<(i64, f64)> = history
            .series(&alpha, &key, ..)
            .unwrap()
            .iter()
            .map(|p| ((p.taken_at - base).num_minutes(), p.used))
            .collect();
        let six_days = (recent - base).num_minutes();
        assert_eq!(
            used,
            [
                (40, 30.0),
                (70, 90.0),
                (100, 15.0),
                (six_days, 1.0),
                (six_days + 10, 2.0),
                (six_days + 20, 3.0),
            ]
        );
        assert_eq!(
            peaks(base, base + hours(3)),
            [90.0, 15.0],
            "peaks preserved"
        );
        assert_eq!(total(&db), 7, "plus the hour without `used`");
        // Idempotent: the reduced span is not folded again.
        assert_eq!(history.prune(t0()).unwrap().reduced, 0);
        assert_eq!(history.prune(t0() + days(1)).unwrap().reduced, 0);
    }

    #[test]
    fn the_size_ceiling_prunes_the_oldest_first_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let config = config(CONFIG);
        let roomy = store(&db, "one");
        let mut metrics: Vec<Metric> = (0..8)
            .map(|n| quota(&format!("metric {n}"), Some(0.0), Some(100.0)))
            .collect();
        for minute in 0..1_500 {
            for metric in &mut metrics {
                metric.used = Some(minute as f64);
            }
            let at = t0() - TimeDelta::minutes(1_500 - minute);
            roomy
                .record(&config.accounts[minute as usize % 2], at, &metrics)
                .unwrap();
        }
        let before = total(&db);
        let size = used_bytes(&Connection::open(&db).unwrap()).unwrap();
        assert_eq!(roomy.prune(t0()).unwrap().ceiling, None, "under 64 MiB");

        let max_bytes = size / 2;
        let tight = roomy.with_retention(Retention::default().with_max_bytes(max_bytes));
        let report = tight.prune(t0()).unwrap();
        let ceiling = report.ceiling.expect("the ceiling is reported");
        assert_eq!(ceiling.max_bytes, max_bytes);
        assert_eq!(i64::try_from(ceiling.removed).unwrap(), before - total(&db));
        assert!(used_bytes(&Connection::open(&db).unwrap()).unwrap() <= max_bytes);
        let Some(Bounds { first, last }) = tight.bounds().unwrap() else {
            panic!("the newest samples stay");
        };
        assert_eq!(last, t0() - TimeDelta::minutes(1));
        assert!(
            first > t0() - TimeDelta::minutes(1_500),
            "the oldest went first"
        );
        assert_eq!(
            tight.prune(t0()).unwrap().ceiling,
            None,
            "below the ceiling now"
        );
    }

    #[test]
    fn export_writes_csv_and_json_with_missing_values_empty_or_null() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let mut config = config(CONFIG);
        config.accounts[0].name = "=alpha, \"main\"".into();
        let (alpha, beta) = (&config.accounts[0], &config.accounts[1]);
        let mut week = quota("7d", None, Some(100.0));
        week.window = Some(MetricWindow::Weekly);
        week.resets_at = None;
        week.kind = MetricKind::Quota;
        history
            .record(
                alpha,
                t0(),
                &[quota("5h", Some(10.0), Some(100.0)), week.clone()],
            )
            .unwrap();
        history
            .record(
                alpha,
                t0() + hours(1),
                &[quota("5h", Some(12.5), Some(100.0))],
            )
            .unwrap();
        history
            .record(beta, t0(), &[quota("5h", Some(1.0), None)])
            .unwrap();

        let export = |accounts: &[Account], metric: Option<&str>, format| {
            let mut out = Vec::new();
            let rows = history
                .export(accounts, metric, t0()..=t0() + hours(1), format, &mut out)
                .unwrap();
            (rows, String::from_utf8(out).unwrap())
        };
        let (rows, csv) = export(&config.accounts, None, ExportFormat::Csv);
        assert_eq!(rows, 4);
        let five_h = metric_key(&quota("5h", None, None));
        let seven_d = metric_key(&week);
        let resets = (t0() + hours(5)).to_rfc3339();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(
            lines[1..],
            [
                format!("\"'=alpha, \"\"main\"\"\",groq,5h,{five_h},%,quota,window_5h,{},10,100,,{resets}", t0().to_rfc3339()),
                format!("\"'=alpha, \"\"main\"\"\",groq,5h,{five_h},%,quota,window_5h,{},12.5,100,,{resets}", (t0() + hours(1)).to_rfc3339()),
                format!("\"'=alpha, \"\"main\"\"\",groq,7d,{seven_d},%,quota,weekly,{},,100,,", t0().to_rfc3339()),
                format!("beta,groq,5h,{five_h},%,quota,window_5h,{},1,,,{resets}", t0().to_rfc3339()),
            ]
        );

        let (rows, json) = export(&config.accounts[..1], Some(&seven_d), ExportFormat::Json);
        assert_eq!(rows, 1);
        let json: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(json["version"], 1);
        let sample = &json["samples"][0];
        assert_eq!(sample["account"], "=alpha, \"main\"");
        assert_eq!(sample["metric"], "7d");
        assert!(sample["used"].is_null() && sample["resets_at"].is_null());
        assert_eq!(sample["limit"], 100.0);

        let (rows, empty) = export(&[], None, ExportFormat::Json);
        assert_eq!(rows, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&empty).unwrap()["samples"],
            serde_json::json!([])
        );

        struct Refuses;
        impl Write for Refuses {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(
            history.export(&config.accounts, None, .., ExportFormat::Csv, &mut Refuses),
            Err(HistoryError::Export)
        );
    }

    #[test]
    fn erase_one_account_with_orphans_or_everything() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        let config = config(CONFIG);
        let (alpha, beta) = (&config.accounts[0], &config.accounts[1]);
        let renamed = {
            let mut renamed = beta.clone();
            renamed.name = "gamma".into();
            renamed
        };
        for (n, account) in [alpha, beta, &renamed].into_iter().enumerate() {
            history
                .record(account, t0(), &[quota("5h", Some(n as f64 + 1.0), None)])
                .unwrap();
        }
        store(&db, "two")
            .record(alpha, t0(), &[quota("5h", Some(9.0), None)])
            .unwrap();
        // `gamma` is not in the saved configuration: an orphan.
        history.mark_removed(&config, t0()).unwrap();

        assert_eq!(history.erase_account(alpha), Ok(2), "alpha and the orphan");
        assert!(history.latest(alpha).unwrap().is_none());
        assert!(history.latest(&renamed).unwrap().is_none());
        assert!(history.latest(beta).unwrap().is_some());
        assert!(
            store(&db, "two").latest(alpha).unwrap().is_some(),
            "other configurations stay"
        );

        assert_eq!(history.erase_all(), Ok(2));
        assert_eq!(total(&db), 0);
        assert_eq!(store(&db, "two").bounds(), Ok(None));
        let bytes = std::fs::read(&db).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("5h"),
            "rewritten, not just unlinked"
        );
        assert_eq!(
            store(&dir.path().join("absent.sqlite3"), "one").erase_all(),
            Ok(0)
        );
        assert!(!dir.path().join("absent.sqlite3").exists());
    }

    #[test]
    fn size_on_disk_counts_the_file_and_its_wal_and_shrinks_after_erasing() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite3");
        let history = store(&db, "one");
        assert_eq!(history.size_on_disk(), Ok(0), "nothing recorded yet");
        assert!(!db.exists(), "never created by asking");

        let alpha = first_account(config(CONFIG));
        let reading = |n: i64| {
            let at = t0() + TimeDelta::minutes(n * 20);
            history
                .record(&alpha, at, &[quota("5h", Some(n as f64), Some(1e6))])
                .unwrap();
        };
        reading(0);
        // Another instance keeps the file open, so the WAL stays beside it.
        let other = Connection::open(&db).unwrap();
        other
            .query_row("SELECT COUNT(*) FROM samples", [], |r| r.get::<_, i64>(0))
            .unwrap();
        (1..500).for_each(reading);
        let size = history.size_on_disk().unwrap();
        let wal = std::fs::metadata(format!("{}-wal", db.display())).unwrap();
        assert!(wal.len() > 0);
        assert!(
            size >= std::fs::metadata(&db).unwrap().len() + wal.len(),
            "the WAL counts"
        );
        drop(other);

        history.erase_all().unwrap();
        assert!(history.size_on_disk().unwrap() < size);

        // A path that cannot be inspected is an error, never "empty".
        #[cfg(unix)]
        {
            let blocked = dir.path().join("file");
            std::fs::write(&blocked, b"").unwrap();
            assert_eq!(
                store(&blocked.join("history.sqlite3"), "one").size_on_disk(),
                Err(HistoryError::Storage)
            );
        }
    }

    #[test]
    fn hourly_keepers_tie_breaks_on_the_first_peak() {
        let base = t0().timestamp_millis();
        let sample = |minute: i64, used: Option<f64>| {
            (
                base + minute * 60_000,
                WindowSample {
                    taken_at: t0() + TimeDelta::minutes(minute),
                    used,
                    limit: Some(100.0),
                    resets_at: None,
                },
            )
        };
        let samples = [
            sample(0, Some(5.0)),
            sample(10, None),
            sample(20, Some(5.0)),
            sample(70, None),
            sample(80, None),
        ];
        assert_eq!(hourly_keepers(&samples), [true, false, false, false, true]);
    }
}
