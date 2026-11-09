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
//!
//! PRD 0042 RF1 (schema 2): `requests` keeps one row per request (time,
//! model, session digest, project fingerprint, token counts) for the
//! session, block and project views; `projects` holds per fingerprint the
//! person's label, the opt-in folder-name hint and whether it is hidden;
//! `session_labels` the person's session labels. The migration re-reads
//! every followed file once from its start: lines already counted only add
//! their request rows (`files.backfill`), so the day totals do not change.

use super::source::{Entry, FileState};
use super::{CostError, Period, SourceId, Tokens};
use crate::history::{migrate, wal};
use crate::models::create_private_file;
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Forward migrations: entry `n` takes `PRAGMA user_version` from `n` to
/// `n + 1`. Append new steps, never edit a shipped one. Days are local
/// `YYYY-MM-DD`; times are UTC unix milliseconds.
/// PRD 0046 RF4: the migration count this release knows, for `--doctor`.
pub(crate) const SCHEMA_REVISION: i64 = MIGRATIONS.len() as i64;

pub(crate) const MIGRATIONS: [&str; 3] = [
    "CREATE TABLE usage (
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
) WITHOUT ROWID;",
    // PRD 0030 RF5.2: 1-hour cache writes. Claude files are forgotten so the
    // next scan re-reads them once and corrects the history.
    "ALTER TABLE usage ADD COLUMN cache_write_1h INTEGER NOT NULL DEFAULT 0;
DELETE FROM usage WHERE source = 'claude_code';
DELETE FROM sessions WHERE source = 'claude_code';
DELETE FROM seen WHERE source = 'claude_code';
DELETE FROM files WHERE source = 'claude_code';
-- PRD 0030 RF7.2: a seen key keeps the counts of the line it accepted, so a
-- later streaming line with higher counts applies the difference.
ALTER TABLE seen ADD COLUMN model TEXT NOT NULL DEFAULT '';
ALTER TABLE seen ADD COLUMN input INTEGER NOT NULL DEFAULT 0;
ALTER TABLE seen ADD COLUMN output INTEGER NOT NULL DEFAULT 0;
ALTER TABLE seen ADD COLUMN cache_read INTEGER NOT NULL DEFAULT 0;
ALTER TABLE seen ADD COLUMN cache_write INTEGER NOT NULL DEFAULT 0;
ALTER TABLE seen ADD COLUMN cache_write_1h INTEGER NOT NULL DEFAULT 0;",
    // PRD 0042 RF1.1: per-request rows, projects and labels. Every followed
    // file is read again from its start; the lines before `backfill` were
    // counted already and only add their request rows.
    "CREATE TABLE requests (
    source TEXT NOT NULL,
    file TEXT NOT NULL,
    key TEXT,
    at INTEGER NOT NULL,
    day TEXT NOT NULL,
    model TEXT NOT NULL,
    session TEXT NOT NULL,
    project TEXT,
    input INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cache_read INTEGER NOT NULL,
    cache_write INTEGER NOT NULL,
    cache_write_1h INTEGER NOT NULL
);
CREATE INDEX requests_by_time ON requests (source, at);
CREATE INDEX requests_by_file ON requests (source, file);
CREATE INDEX requests_by_key ON requests (source, key);
CREATE INDEX requests_by_day ON requests (day);
CREATE TABLE projects (
    project TEXT NOT NULL PRIMARY KEY,
    label TEXT,
    folder_hint TEXT,
    hint_source TEXT,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    hidden INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
CREATE TABLE session_labels (
    session TEXT NOT NULL PRIMARY KEY,
    label TEXT NOT NULL
) WITHOUT ROWID;
ALTER TABLE files ADD COLUMN backfill INTEGER NOT NULL DEFAULT 0;
UPDATE files SET backfill = position, position = 0, examined = 0;",
];

/// Tables whose rows all derive from the logs, per source and file.
const TABLES: [&str; 5] = ["usage", "sessions", "seen", "files", "requests"];

/// PRD 0042: the person's own rows (labels, hidden projects, the opt-in
/// folder names); kept while a request still refers to them.
const LABEL_TABLES: [&str; 2] = ["projects", "session_labels"];

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

/// PRD 0042 RF1.1: one request, as the session, block and project views
/// read it. Digests and numbers only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub source: SourceId,
    pub at: DateTime<Utc>,
    pub day: NaiveDate,
    pub model: String,
    /// The anonymous session digest.
    pub session: String,
    /// The project fingerprint; `None` counts under "No project".
    pub project: Option<String>,
    pub tokens: Tokens,
}

/// PRD 0042 RF4: what the index holds about one project fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub project: String,
    pub label: Option<String>,
    /// RF4.3: the last folder name, only while the person keeps them.
    pub folder_hint: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub hidden: bool,
}

/// PRD 0046 RF1.1: the labels a backup carries (`prefs/cost-labels.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Labels {
    pub schema: u32,
    #[serde(default)]
    pub projects: Vec<ProjectLabel>,
    #[serde(default)]
    pub sessions: Vec<SessionLabel>,
}

/// One project's label and hidden flag, by fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ProjectLabel {
    pub project: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

/// One session's label, by digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SessionLabel {
    pub session: String,
    pub label: String,
}

/// PRD 0042 RF2.2/RF4.2: what a label is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelTarget {
    Session,
    Project,
}

impl LabelTarget {
    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "session" => Some(Self::Session),
            "project" => Some(Self::Project),
            _ => None,
        }
    }
}

/// The longest label (RF4.2), in characters.
pub const MAX_LABEL_CHARS: usize = 48;
/// The shortest digest prefix accepted on the command line.
pub const MIN_PREFIX: usize = 4;

/// A label as stored: trimmed, 1 to [`MAX_LABEL_CHARS`] characters, no
/// control characters.
fn clean_label(label: &str) -> Result<String, CostError> {
    let label = label.trim();
    if label.is_empty()
        || label.chars().count() > MAX_LABEL_CHARS
        || label.chars().any(char::is_control)
    {
        return Err(CostError::InvalidLabel);
    }
    Ok(label.to_owned())
}

fn instant_of(millis: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(millis).unwrap_or_default()
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
    /// PRD 0042 RF1.1: the lines before this byte were counted before
    /// schema 2 and are read again only for their request rows; 0 when
    /// nothing is replayed.
    pub backfill: u64,
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
    /// PRD 0042 RF1.1: how many leading `entries` are replays of lines
    /// counted before schema 2 (only their request rows are added).
    pub replayed: usize,
    /// PRD 0042 RF4.3: `(project, folder name)` pairs seen, only when the
    /// source keeps folder names.
    pub folders: Vec<(String, String)>,
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
        let dir = crate::data_dir::root().ok_or(CostError::Storage)?;
        Ok(Self::new(dir.join("cost").join("cost.sqlite3")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Open (creating privately: directory 0700, file 0600 on Unix) and
    /// migrate the index.
    pub(crate) fn open(&self) -> Result<Connection, CostError> {
        create_private_file(&self.path).map_err(|_| CostError::Storage)?;
        let mut db = Connection::open(&self.path)?;
        crate::private_fs::sqlite_busy_wait(&mut db)?;
        wal(&db)?;
        db.pragma_update(None, "secure_delete", true)?;
        migrate(&mut db, &MIGRATIONS).map_err(|error| match error {
            crate::history::HistoryError::NewerMajor => CostError::NewerMajor,
            _ => CostError::Storage,
        })?;
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
                "SELECT identity, examined, position, state, invalid, backfill FROM files
                 WHERE source = ?1 AND file = ?2",
                params![source.slug(), file],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(
            |(identity, examined, position, state, invalid, backfill)| FileRow {
                identity,
                examined: count(examined),
                position: count(position),
                // A carried state that no longer parses restarts empty.
                state: serde_json::from_str(&state).unwrap_or_default(),
                invalid: count(invalid),
                backfill: count(backfill),
            },
        ))
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
            for table in ["usage", "sessions", "seen", "requests"] {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE source = ?1 AND file = ?2"),
                    params![source, chunk.file],
                )?;
            }
        }
        let mut done = Committed::default();
        {
            let mut seen = tx.prepare_cached(
                "INSERT OR IGNORE INTO seen (source, key, file, day, model, input, output,
                     cache_read, cache_write, cache_write_1h)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut known = tx.prepare_cached(
                "SELECT file, day, model, input, output, cache_read, cache_write, cache_write_1h
                 FROM seen WHERE source = ?1 AND key = ?2",
            )?;
            let mut grow = tx.prepare_cached(
                "UPDATE usage SET input = input + ?5, output = output + ?6,
                     cache_read = cache_read + ?7, cache_write = cache_write + ?8,
                     cache_write_1h = cache_write_1h + ?9
                 WHERE source = ?1 AND day = ?2 AND model = ?3 AND file = ?4",
            )?;
            let mut remember = tx.prepare_cached(
                "UPDATE seen SET input = ?3, output = ?4, cache_read = ?5, cache_write = ?6,
                     cache_write_1h = ?7 WHERE source = ?1 AND key = ?2",
            )?;
            let mut usage = tx.prepare_cached(
                "INSERT INTO usage (source, day, model, file, input, output, cache_read,
                     cache_write, cache_write_1h, requests)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)
                 ON CONFLICT (source, day, model, file) DO UPDATE SET
                     input = input + excluded.input,
                     output = output + excluded.output,
                     cache_read = cache_read + excluded.cache_read,
                     cache_write = cache_write + excluded.cache_write,
                     cache_write_1h = cache_write_1h + excluded.cache_write_1h,
                     requests = requests + 1",
            )?;
            let mut sessions = tx.prepare_cached(
                "INSERT OR IGNORE INTO sessions (source, day, model, session, file)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut request = tx.prepare_cached(
                "INSERT INTO requests (source, file, key, at, day, model, session, project,
                     input, output, cache_read, cache_write, cache_write_1h)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )?;
            let mut grow_request = tx.prepare_cached(
                "UPDATE requests SET input = input + ?3, output = output + ?4,
                     cache_read = cache_read + ?5, cache_write = cache_write + ?6,
                     cache_write_1h = cache_write_1h + ?7
                 WHERE source = ?1 AND key = ?2",
            )?;
            let mut has_request =
                tx.prepare_cached("SELECT 1 FROM requests WHERE source = ?1 AND key = ?2 LIMIT 1")?;
            let mut project = tx.prepare_cached(
                "INSERT INTO projects (project, first_seen, last_seen) VALUES (?1, ?2, ?2)
                 ON CONFLICT (project) DO UPDATE SET
                     first_seen = MIN(first_seen, excluded.first_seen),
                     last_seen = MAX(last_seen, excluded.last_seen)",
            )?;
            let add_request = |request: &mut rusqlite::CachedStatement<'_>,
                               day: &str,
                               entry: &Entry,
                               key: Option<&str>,
                               t: Tokens|
             -> rusqlite::Result<usize> {
                request.execute(params![
                    source,
                    chunk.file,
                    key,
                    entry.at.timestamp_millis(),
                    day,
                    entry.model,
                    entry.session,
                    entry.project,
                    stored(t.input),
                    stored(t.output),
                    stored(t.cache_read),
                    stored(t.cache_write),
                    stored(t.cache_write_1h),
                ])
            };
            for (position, (day, entry)) in chunk.entries.iter().enumerate() {
                let day = day_text(*day);
                if let Some(digest) = &entry.project {
                    project.execute(params![digest, entry.at.timestamp_millis()])?;
                }
                if position < chunk.replayed {
                    // Counted before schema 2: only its request row, once,
                    // from the file whose copy was accepted, with the
                    // highest counts kept.
                    match &entry.dedup {
                        None => {
                            add_request(&mut request, &day, entry, None, entry.tokens)?;
                        }
                        Some(key) => {
                            let accepted = known
                                .query_row(params![source, key], |row| {
                                    Ok((
                                        row.get::<_, String>(0)?,
                                        row.get::<_, String>(2)?,
                                        Tokens {
                                            input: count(row.get(3)?),
                                            output: count(row.get(4)?),
                                            cache_read: count(row.get(5)?),
                                            cache_write: count(row.get(6)?),
                                            cache_write_1h: count(row.get(7)?),
                                        },
                                    ))
                                })
                                .optional()?;
                            if let Some((file, model, kept)) = accepted {
                                let present = has_request.exists(params![source, key])?;
                                if file == chunk.file && !present {
                                    let tokens = if model.is_empty() { entry.tokens } else { kept };
                                    add_request(&mut request, &day, entry, Some(key), tokens)?;
                                }
                            }
                        }
                    }
                    continue;
                }
                if let Some(key) = &entry.dedup {
                    let t = entry.tokens;
                    let fields = [
                        t.input,
                        t.output,
                        t.cache_read,
                        t.cache_write,
                        t.cache_write_1h,
                    ];
                    let inserted = seen.execute(params![
                        source,
                        key,
                        chunk.file,
                        day,
                        entry.model,
                        stored(fields[0]),
                        stored(fields[1]),
                        stored(fields[2]),
                        stored(fields[3]),
                        stored(fields[4]),
                    ])?;
                    if inserted == 0 {
                        done.duplicates += 1;
                        // PRD 0030 RF7.2: a streamed message repeats with
                        // growing counts; the first line holds a partial
                        // output. Apply only what grew, to the row of the
                        // line that was accepted; rows without stored
                        // counts (empty model) stay plain duplicates.
                        let old = known
                            .query_row(params![source, key], |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, String>(2)?,
                                    [
                                        count(row.get(3)?),
                                        count(row.get(4)?),
                                        count(row.get(5)?),
                                        count(row.get(6)?),
                                        count(row.get(7)?),
                                    ],
                                ))
                            })
                            .optional()?;
                        if let Some((file, old_day, model, kept)) = old {
                            if !model.is_empty() {
                                let gain = std::array::from_fn::<u64, 5, _>(|i| {
                                    fields[i].saturating_sub(kept[i])
                                });
                                if gain.iter().any(|g| *g > 0) {
                                    grow_request.execute(params![
                                        source,
                                        key,
                                        stored(gain[0]),
                                        stored(gain[1]),
                                        stored(gain[2]),
                                        stored(gain[3]),
                                        stored(gain[4]),
                                    ])?;
                                    grow.execute(params![
                                        source,
                                        old_day,
                                        model,
                                        file,
                                        stored(gain[0]),
                                        stored(gain[1]),
                                        stored(gain[2]),
                                        stored(gain[3]),
                                        stored(gain[4]),
                                    ])?;
                                    let top = std::array::from_fn::<u64, 5, _>(|i| {
                                        fields[i].max(kept[i])
                                    });
                                    remember.execute(params![
                                        source,
                                        key,
                                        stored(top[0]),
                                        stored(top[1]),
                                        stored(top[2]),
                                        stored(top[3]),
                                        stored(top[4]),
                                    ])?;
                                }
                            }
                        }
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
                    stored(t.cache_write_1h),
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
                add_request(&mut request, &day, entry, entry.dedup.as_deref(), t)?;
                done.requests += 1;
            }
            let mut hint = tx.prepare_cached(
                "UPDATE projects SET folder_hint = ?2, hint_source = ?3 WHERE project = ?1",
            )?;
            for (digest, folder) in &chunk.folders {
                hint.execute(params![digest, folder, source])?;
            }
        }
        let row = &chunk.row;
        let state = serde_json::to_string(&row.state).map_err(|_| CostError::Storage)?;
        tx.execute(
            "INSERT OR REPLACE INTO files (source, file, identity, examined, position, state,
                 invalid, read_at, backfill)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                source,
                chunk.file,
                row.identity,
                stored(row.examined),
                stored(row.position),
                state,
                stored(row.invalid),
                chunk.read_at,
                stored(row.backfill),
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
                 SUM(u.cache_write_1h),
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
                        cache_write_1h: count(row.get(8)?),
                    },
                    count(row.get(7)?),
                    count(row.get(9)?),
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
        for table in ["usage", "sessions", "seen", "requests"] {
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
        // PRD 0042 RF1.1: labels, hidden flags and folder names go with
        // the last request that refers to them (all of them on Erase the
        // index).
        removed += match source {
            None => {
                let mut all = 0;
                for table in LABEL_TABLES {
                    all += tx.execute(&format!("DELETE FROM {table}"), [])?;
                }
                all
            }
            Some(_) => {
                tx.execute(
                    "DELETE FROM projects WHERE project NOT IN
                         (SELECT project FROM requests WHERE project IS NOT NULL)",
                    [],
                )? + tx.execute(
                    "DELETE FROM session_labels WHERE session NOT IN
                         (SELECT session FROM requests)",
                    [],
                )?
            }
        };
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

    /// PRD 0042 RF1.1: every request from `from` (inclusive) to `to`
    /// (exclusive), oldest first; only `source` when given. Never creates
    /// the file.
    pub fn requests(
        &self,
        source: Option<SourceId>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Request>, CostError> {
        let Some(db) = self.open_existing()? else {
            return Ok(Vec::new());
        };
        let mut query = db.prepare(
            "SELECT source, at, day, model, session, project, input, output, cache_read,
                 cache_write, cache_write_1h
             FROM requests
             WHERE at >= ?1 AND at < ?2 AND (?3 IS NULL OR source = ?3)
             ORDER BY at, source",
        )?;
        let rows = query.query_map(
            params![
                from.timestamp_millis(),
                to.timestamp_millis(),
                source.map(SourceId::slug)
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    Tokens {
                        input: count(row.get(6)?),
                        output: count(row.get(7)?),
                        cache_read: count(row.get(8)?),
                        cache_write: count(row.get(9)?),
                        cache_write_1h: count(row.get(10)?),
                    },
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (slug, at, day, model, session, project, tokens) = row?;
            if let Some(source) = SourceId::from_slug(&slug) {
                out.push(Request {
                    source,
                    at: instant_of(at),
                    day: day_of(&day)?,
                    model,
                    session,
                    project,
                    tokens,
                });
            }
        }
        Ok(out)
    }

    /// PRD 0042 RF4: every project the index knows. Never creates the file.
    pub fn projects(&self) -> Result<Vec<ProjectRow>, CostError> {
        let Some(db) = self.open_existing()? else {
            return Ok(Vec::new());
        };
        let mut query = db.prepare(
            "SELECT project, label, folder_hint, first_seen, last_seen, hidden
             FROM projects ORDER BY last_seen DESC, project",
        )?;
        let rows = query.query_map([], |row| {
            Ok(ProjectRow {
                project: row.get(0)?,
                label: row.get(1)?,
                folder_hint: row.get(2)?,
                first_seen: instant_of(row.get(3)?),
                last_seen: instant_of(row.get(4)?),
                hidden: row.get::<_, i64>(5)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// PRD 0042 RF2.2: the person's session labels, by session digest.
    pub fn session_labels(&self) -> Result<std::collections::BTreeMap<String, String>, CostError> {
        let Some(db) = self.open_existing()? else {
            return Ok(Default::default());
        };
        let mut query = db.prepare("SELECT session, label FROM session_labels")?;
        let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The one digest of `target` that starts with `prefix` (at least
    /// [`MIN_PREFIX`] hex characters).
    fn resolve(db: &Connection, target: LabelTarget, prefix: &str) -> Result<String, CostError> {
        let prefix = prefix.trim().to_ascii_lowercase();
        if prefix.len() < MIN_PREFIX || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CostError::UnknownPrefix);
        }
        let sql = match target {
            LabelTarget::Session => {
                "SELECT DISTINCT session FROM requests
                 WHERE substr(session, 1, length(?1)) = ?1 LIMIT 2"
            }
            LabelTarget::Project => {
                "SELECT project FROM projects WHERE substr(project, 1, length(?1)) = ?1 LIMIT 2"
            }
        };
        let mut query = db.prepare(sql)?;
        let found: Vec<String> = query
            .query_map([&prefix], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        match found.as_slice() {
            [one] => Ok(one.clone()),
            [] => Err(CostError::UnknownPrefix),
            _ => Err(CostError::AmbiguousPrefix),
        }
    }

    /// PRD 0042 RF2.2/RF4.2: label (or, with `None`, unlabel) the session
    /// or project whose digest starts with `prefix`. Returns the digest.
    pub fn set_label(
        &self,
        target: LabelTarget,
        prefix: &str,
        label: Option<&str>,
    ) -> Result<String, CostError> {
        let label = label.map(clean_label).transpose()?;
        let Some(db) = self.open_existing()? else {
            return Err(CostError::UnknownPrefix);
        };
        let digest = Self::resolve(&db, target, prefix)?;
        match (target, label) {
            (LabelTarget::Session, Some(label)) => db.execute(
                "INSERT INTO session_labels (session, label) VALUES (?1, ?2)
                 ON CONFLICT (session) DO UPDATE SET label = excluded.label",
                params![digest, label],
            )?,
            (LabelTarget::Session, None) => {
                db.execute("DELETE FROM session_labels WHERE session = ?1", [&digest])?
            }
            (LabelTarget::Project, label) => db.execute(
                "UPDATE projects SET label = ?2 WHERE project = ?1",
                params![digest, label],
            )?,
        };
        Ok(digest)
    }

    /// PRD 0046 RF1.1: the person's labels and hidden projects, as a backup
    /// carries them (never a folder name). Never creates the file.
    pub fn labels(&self) -> Result<Labels, CostError> {
        let projects = self
            .projects()?
            .into_iter()
            .filter(|row| row.label.is_some() || row.hidden)
            .map(|row| ProjectLabel {
                project: row.project,
                label: row.label,
                hidden: row.hidden,
            })
            .collect();
        let sessions = self
            .session_labels()?
            .into_iter()
            .map(|(session, label)| SessionLabel { session, label })
            .collect();
        Ok(Labels {
            schema: 1,
            projects,
            sessions,
        })
    }

    /// PRD 0046 RF3.2: merge `labels` from a backup; with `apply` false,
    /// only count. Only projects and sessions this index already knows
    /// take them, and only where this computer has no label (or hidden
    /// flag) of its own: the local choice wins. Returns the rows taken.
    pub fn import_labels(&self, labels: &Labels, apply: bool) -> Result<usize, CostError> {
        let Some(mut db) = self.open_existing()? else {
            return Ok(0);
        };
        let digest_ok = |digest: &str| {
            !digest.is_empty()
                && digest.len() <= 64
                && digest.bytes().all(|b| b.is_ascii_hexdigit())
        };
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut taken = 0;
        for project in &labels.projects {
            let label = project.label.as_deref().map(clean_label).transpose();
            let (true, Ok(label)) = (digest_ok(&project.project), label) else {
                continue;
            };
            taken += tx.execute(
                "UPDATE projects SET label = ?2, hidden = ?3
                 WHERE project = ?1 AND label IS NULL AND hidden = 0",
                params![project.project, label, project.hidden],
            )?;
        }
        for session in &labels.sessions {
            let (true, Ok(label)) = (digest_ok(&session.session), clean_label(&session.label))
            else {
                continue;
            };
            taken += tx.execute(
                "INSERT INTO session_labels (session, label)
                 SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM requests WHERE session = ?1)
                 ON CONFLICT (session) DO NOTHING",
                params![session.session, label],
            )?;
        }
        if apply {
            tx.commit()?;
        }
        Ok(taken)
    }

    /// PRD 0042 RF4.1: hide (or show again) the project whose digest starts
    /// with `prefix`. Returns the digest.
    pub fn set_hidden(&self, prefix: &str, hidden: bool) -> Result<String, CostError> {
        let Some(db) = self.open_existing()? else {
            return Err(CostError::UnknownPrefix);
        };
        let digest = Self::resolve(&db, LabelTarget::Project, prefix)?;
        db.execute(
            "UPDATE projects SET hidden = ?2 WHERE project = ?1",
            params![digest, hidden],
        )?;
        Ok(digest)
    }

    /// PRD 0042 RF4.3: delete the folder names `source` stored (the hint was
    /// turned off). Returns the rows changed.
    pub fn clear_folder_hints(&self, source: SourceId) -> Result<usize, CostError> {
        let Some(db) = self.open_existing()? else {
            return Ok(0);
        };
        let changed = db.execute(
            "UPDATE projects SET folder_hint = NULL, hint_source = NULL WHERE hint_source = ?1",
            [source.slug()],
        )?;
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(changed)
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
