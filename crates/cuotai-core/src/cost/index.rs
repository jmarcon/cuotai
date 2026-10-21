//! RF2.1/RF2.4: `cost.sqlite3`, the aggregated usage index.
//!
//! `usage` holds token sums per `(source, local day, model)`, split by the
//! digest of the log file they came from so a restarted file replaces
//! exactly its own share; `sessions` counts distinct anonymous sessions per
//! day and model; `seen` holds the digests of requests already counted (a
//! tool that copies requests between files is counted once); `files` is the
//! per-file control: the digest of the path, its identity, the bytes
//! examined, the position read and the parser's carried numbers. Nothing
//! else enters the file: no path, no file name, no content. Deleted rows
//! are overwritten (`secure_delete`) and erasure truncates the WAL.

use super::source::{Entry, FileState};
use super::{CostError, Period, SourceId, Tokens};
use crate::history::{migrate, wal};
use crate::models::create_private_file;
use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Forward migrations: entry `n` takes `PRAGMA user_version` from `n` to
/// `n + 1`. Append new steps, never edit a shipped one. Days are local
/// `YYYY-MM-DD`; times are UTC unix milliseconds.
const MIGRATIONS: [&str; 1] = ["CREATE TABLE usage (
    source TEXT NOT NULL,
    day TEXT NOT NULL,
    model TEXT NOT NULL,
    file TEXT NOT NULL,
    input INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cache_read INTEGER NOT NULL,
    cache_write INTEGER NOT NULL,
    requests INTEGER NOT NULL,
    PRIMARY KEY (source, day, model, file)
) WITHOUT ROWID;
CREATE INDEX usage_by_day ON usage (day);
CREATE INDEX usage_by_file ON usage (source, file);
CREATE TABLE sessions (
    source TEXT NOT NULL,
    day TEXT NOT NULL,
    model TEXT NOT NULL,
    session TEXT NOT NULL,
    file TEXT NOT NULL,
    PRIMARY KEY (source, day, model, session)
) WITHOUT ROWID;
CREATE INDEX sessions_by_file ON sessions (source, file);
CREATE TABLE seen (
    source TEXT NOT NULL,
    key TEXT NOT NULL,
    file TEXT NOT NULL,
    day TEXT NOT NULL,
    PRIMARY KEY (source, key)
) WITHOUT ROWID;
CREATE INDEX seen_by_file ON seen (source, file);
CREATE INDEX seen_by_day ON seen (day);
CREATE TABLE files (
    source TEXT NOT NULL,
    file TEXT NOT NULL,
    identity TEXT NOT NULL,
    examined INTEGER NOT NULL,
    position INTEGER NOT NULL,
    state TEXT NOT NULL,
    invalid INTEGER NOT NULL,
    read_at INTEGER NOT NULL,
    PRIMARY KEY (source, file)
) WITHOUT ROWID;"];

/// Tables whose rows all derive from the logs.
const TABLES: [&str; 4] = ["usage", "sessions", "seen", "files"];

/// The cost index of this computer. Holds only its path; errors surface on
/// use.
#[derive(Debug, Clone)]
pub struct CostIndex {
    path: PathBuf,
}

/// What the index follows of one source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Indexed {
    pub files: u64,
    pub invalid_lines: u64,
}

/// Usage of one source, local day and model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DailyUsage {
    pub source: SourceId,
    pub day: NaiveDate,
    pub model: String,
    pub tokens: Tokens,
    /// Requests counted (one per usage line).
    pub requests: u64,
    /// Distinct anonymous sessions.
    pub sessions: u64,
}

/// What the index knows about one log file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FileRow {
    pub identity: String,
    /// Bytes looked at: the file size when the read reached its end, the
    /// position when a limit stopped it (so the next scan continues).
    pub examined: u64,
    pub position: u64,
    pub state: FileState,
    pub invalid: u64,
}

/// One read of one file, written in one transaction.
#[derive(Debug)]
pub(crate) struct Chunk<'a> {
    pub source: SourceId,
    pub file: &'a str,
    /// The file was shrunk or replaced: its earlier share goes first.
    pub restart: bool,
    pub row: FileRow,
    pub entries: Vec<(NaiveDate, Entry)>,
    pub read_at: i64,
}

/// What a committed chunk added.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Committed {
    pub requests: u64,
    pub duplicates: u64,
}

fn day_text(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

fn day_of(text: &str) -> rusqlite::Result<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

fn count(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn stored(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl CostIndex {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// `<OS local data dir>/cuotai/cost/cost.sqlite3`.
    pub fn in_data_dir() -> Result<Self, CostError> {
        let dir = dirs::data_local_dir().ok_or(CostError::Storage)?;
        Ok(Self::new(
            dir.join("cuotai").join("cost").join("cost.sqlite3"),
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Open (creating privately: directory 0700, file 0600 on Unix) and
    /// migrate the index.
    pub(crate) fn open(&self) -> Result<Connection, CostError> {
        create_private_file(&self.path).map_err(|_| CostError::Storage)?;
        let mut db = Connection::open(&self.path)?;
        db.busy_timeout(BUSY_TIMEOUT)?;
        wal(&db)?;
        db.pragma_update(None, "secure_delete", true)?;
        migrate(&mut db, &MIGRATIONS).map_err(|_| CostError::Storage)?;
        Ok(db)
    }

    /// Open only when the file exists: a query or an erasure that finds
    /// nothing never creates it.
    fn open_existing(&self) -> Result<Option<Connection>, CostError> {
        if !self.path.try_exists().map_err(|_| CostError::Storage)? {
            return Ok(None);
        }
        self.open().map(Some)
    }

    pub(crate) fn file(
        db: &Connection,
        source: SourceId,
        file: &str,
    ) -> Result<Option<FileRow>, CostError> {
        let row = db
            .query_row(
                "SELECT identity, examined, position, state, invalid FROM files
                 WHERE source = ?1 AND file = ?2",
                params![source.slug(), file],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?;
        Ok(
            row.map(|(identity, examined, position, state, invalid)| FileRow {
                identity,
                examined: count(examined),
                position: count(position),
                // A carried state that no longer parses restarts empty.
                state: serde_json::from_str(&state).unwrap_or_default(),
                invalid: count(invalid),
            }),
        )
    }

    /// Drop the share and the control row of one file (shrunk or
    /// replaced), so the next read starts from its beginning.
    pub(crate) fn forget(
        db: &mut Connection,
        source: SourceId,
        file: &str,
    ) -> Result<(), CostError> {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for table in TABLES {
            tx.execute(
                &format!("DELETE FROM {table} WHERE source = ?1 AND file = ?2"),
                params![source.slug(), file],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Write `chunk` in one `BEGIN IMMEDIATE` transaction, only while
    /// `still_on` says the source is on (checked inside the transaction,
    /// so an erasure that follows a turn-off is never undone). `None` when
    /// the source was off.
    pub(crate) fn commit(
        db: &mut Connection,
        chunk: Chunk<'_>,
        still_on: impl FnOnce() -> bool,
    ) -> Result<Option<Committed>, CostError> {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !still_on() {
            return Ok(None);
        }
        let source = chunk.source.slug();
        if chunk.restart {
            for table in ["usage", "sessions", "seen"] {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE source = ?1 AND file = ?2"),
                    params![source, chunk.file],
                )?;
            }
        }
        let mut done = Committed::default();
        {
            let mut seen = tx.prepare_cached(
                "INSERT OR IGNORE INTO seen (source, key, file, day) VALUES (?1, ?2, ?3, ?4)",
            )?;
            let mut usage = tx.prepare_cached(
                "INSERT INTO usage (source, day, model, file, input, output, cache_read,
                     cache_write, requests)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
                 ON CONFLICT (source, day, model, file) DO UPDATE SET
                     input = input + excluded.input,
                     output = output + excluded.output,
                     cache_read = cache_read + excluded.cache_read,
                     cache_write = cache_write + excluded.cache_write,
                     requests = requests + 1",
            )?;
            let mut sessions = tx.prepare_cached(
                "INSERT OR IGNORE INTO sessions (source, day, model, session, file)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (day, entry) in &chunk.entries {
                let day = day_text(*day);
                if let Some(key) = &entry.dedup {
                    if seen.execute(params![source, key, chunk.file, day])? == 0 {
                        done.duplicates += 1;
                        continue;
                    }
                }
                let t = entry.tokens;
                usage.execute(params![
                    source,
                    day,
                    entry.model,
                    chunk.file,
                    stored(t.input),
                    stored(t.output),
                    stored(t.cache_read),
                    stored(t.cache_write),
                ])?;
                if !entry.session.is_empty() {
                    sessions.execute(params![
                        source,
                        day,
                        entry.model,
                        entry.session,
                        chunk.file
                    ])?;
                }
                done.requests += 1;
            }
        }
        let row = &chunk.row;
        let state = serde_json::to_string(&row.state).map_err(|_| CostError::Storage)?;
        tx.execute(
            "INSERT OR REPLACE INTO files (source, file, identity, examined, position, state,
                 invalid, read_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                source,
                chunk.file,
                row.identity,
                stored(row.examined),
                stored(row.position),
                state,
                stored(row.invalid),
                chunk.read_at,
            ],
        )?;
        tx.commit()?;
        Ok(Some(done))
    }

    /// Usage per source, local day and model from `first` (`None`: the
    /// oldest kept) to `last`, inclusive, oldest first; only `source` when
    /// given. Never creates the file.
    pub fn usage(
        &self,
        source: Option<SourceId>,
        first: Option<NaiveDate>,
        last: NaiveDate,
    ) -> Result<Vec<DailyUsage>, CostError> {
        let Some(db) = self.open_existing()? else {
            return Ok(Vec::new());
        };
        let first = first.map(day_text).unwrap_or_default();
        let mut query = db.prepare(
            "SELECT u.source, u.day, u.model, SUM(u.input), SUM(u.output), SUM(u.cache_read),
                 SUM(u.cache_write), SUM(u.requests),
                 (SELECT COUNT(*) FROM sessions s
                  WHERE s.source = u.source AND s.day = u.day AND s.model = u.model)
             FROM usage u
             WHERE u.day >= ?1 AND u.day <= ?2 AND (?3 IS NULL OR u.source = ?3)
             GROUP BY u.source, u.day, u.model
             ORDER BY u.day, u.source, u.model",
        )?;
        let rows = query.query_map(
            params![first, day_text(last), source.map(SourceId::slug)],
            |row| {
                let slug: String = row.get(0)?;
                let day: String = row.get(1)?;
                Ok((
                    slug,
                    day_of(&day)?,
                    row.get::<_, String>(2)?,
                    Tokens {
                        input: count(row.get(3)?),
                        output: count(row.get(4)?),
                        cache_read: count(row.get(5)?),
                        cache_write: count(row.get(6)?),
                    },
                    count(row.get(7)?),
                    count(row.get(8)?),
                ))
            },
        )?;
        let mut usage = Vec::new();
        for row in rows {
            let (slug, day, model, tokens, requests, sessions) = row?;
            // Rows of a source this version does not know are left alone.
            if let Some(source) = SourceId::from_slug(&slug) {
                usage.push(DailyUsage {
                    source,
                    day,
                    model,
                    tokens,
                    requests,
                    sessions,
                });
            }
        }
        Ok(usage)
    }

    /// RF2.3: the usage of `period` ending `today`.
    pub fn usage_in(
        &self,
        source: Option<SourceId>,
        period: Period,
        today: NaiveDate,
    ) -> Result<Vec<DailyUsage>, CostError> {
        self.usage(source, period.first_day(today), today)
    }

    /// RF2.3: drop usage of days before `first` and the control rows of
    /// files not read since `read_before` (unix ms). Returns rows removed.
    pub fn prune(&self, first: NaiveDate, read_before: i64) -> Result<usize, CostError> {
        let Some(mut db) = self.open_existing()? else {
            return Ok(0);
        };
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let first = day_text(first);
        let mut removed = 0;
        for table in ["usage", "sessions", "seen"] {
            removed += tx.execute(&format!("DELETE FROM {table} WHERE day < ?1"), [&first])?;
        }
        removed += tx.execute("DELETE FROM files WHERE read_at < ?1", [read_before])?;
        tx.commit()?;
        Ok(removed)
    }

    /// RF2.4: remove every row derived from `source`. Returns rows removed.
    pub fn erase_source(&self, source: SourceId) -> Result<usize, CostError> {
        self.erase_where(Some(source))
    }

    /// RF2.4: remove every row of the index. Returns rows removed.
    pub fn erase(&self) -> Result<usize, CostError> {
        self.erase_where(None)
    }

    fn erase_where(&self, source: Option<SourceId>) -> Result<usize, CostError> {
        let Some(mut db) = self.open_existing()? else {
            return Ok(0);
        };
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut removed = 0;
        for table in TABLES {
            removed += tx.execute(
                &format!("DELETE FROM {table} WHERE ?1 IS NULL OR source = ?1"),
                [source.map(SourceId::slug)],
            )?;
        }
        tx.commit()?;
        // The deleted pages were zeroed; drop the WAL copies too.
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(removed)
    }

    /// How many log files of `source` the index follows and the invalid
    /// lines skipped in them so far. Never creates the file.
    pub fn indexed(&self, source: SourceId) -> Result<Indexed, CostError> {
        let Some(db) = self.open_existing()? else {
            return Ok(Indexed::default());
        };
        let (files, invalid) = db.query_row(
            "SELECT COUNT(*), COALESCE(SUM(invalid), 0) FROM files WHERE source = ?1",
            [source.slug()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        Ok(Indexed {
            files: count(files),
            invalid_lines: count(invalid),
        })
    }

    /// Bytes the index takes on disk (the database and its WAL).
    pub fn size_on_disk(&self) -> Result<u64, CostError> {
        let size = |path: &Path| match std::fs::metadata(path) {
            Ok(meta) => Ok(meta.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(_) => Err(CostError::Storage),
        };
        let mut wal = self.path.clone().into_os_string();
        wal.push("-wal");
        Ok(size(&self.path)? + size(Path::new(&wal))?)
    }
}
