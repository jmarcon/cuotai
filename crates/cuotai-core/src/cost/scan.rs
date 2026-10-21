//! RF1.4/RF2.1/RF2.2: one bounded, incremental scan of the sources that
//! are on.
//!
//! Only the directories of a source that is on are visited, without
//! following symbolic links, and only regular files matching its pattern,
//! owned by the user (Unix) and modified within the retention are opened.
//! Each file is read from where the last scan stopped, one complete line
//! at a time; a shrunk or replaced file restarts and replaces its earlier
//! share. A scan stops at [`Limits`] (files per scan, lines per file, time
//! per cycle) and the next one continues; it paces itself so it never
//! competes with the user's work. A directory that cannot be read becomes
//! a "no access" state; nothing tries to get around it.

use super::index::{Chunk, CostIndex, FileRow};
use super::source::{anonymous, Entry, FileState, Line};
use super::{CostError, DayZone, Env, SettingsStore, SourceId};
use crate::models::digest;
use crate::text::{params, Localized};
use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use rusqlite::Connection;
use serde::Serialize;
use std::fs::{File, Metadata};
use std::io::{self, BufRead, BufReader, ErrorKind, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// A line without its final newline counts once the file has not changed
/// for this long (the tool finished writing it).
const SETTLED: Duration = Duration::from_secs(10 * 60);
/// Lines read between two looks at the clock.
const CLOCK_EVERY: usize = 512;
/// The longest pause after a file (pacing).
const MAX_PAUSE: Duration = Duration::from_millis(50);

/// RF2.2: what one scan may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Log files read per scan (unchanged files are not counted).
    pub max_files: usize,
    /// Lines read per file per scan.
    pub max_lines: usize,
    /// Time per scan.
    pub max_duration: Duration,
    /// A longer line is skipped and counted as invalid.
    pub max_line_bytes: usize,
    /// Directory entries looked at per directory root.
    pub max_entries: usize,
    /// Directory levels below a root.
    pub max_depth: usize,
    /// After each file, pause as long as reading it took (up to 50 ms).
    pub pace: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: 64,
            max_lines: 50_000,
            max_duration: Duration::from_secs(5),
            max_line_bytes: 8 << 20,
            max_entries: 100_000,
            max_depth: 8,
            pace: true,
        }
    }
}

/// How a directory root answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RootState {
    Readable,
    /// A default directory that does not exist (the tool wrote no logs).
    Missing,
    /// Refused, not a directory, or a chosen directory that is gone (a
    /// missing volume).
    NoAccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootScan {
    pub path: PathBuf,
    pub state: RootState,
}

/// RF1.4: a source as the last scan found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    /// Not consented: nothing was read.
    Off,
    /// At least one directory was read.
    Ready,
    /// None of the default directories exists.
    NoLogs,
    /// No directory could be read.
    NoAccess,
}

/// What one scan did with one source. Counts only; never a file name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceScan {
    pub source: SourceId,
    pub state: SourceState,
    pub roots: Vec<RootScan>,
    /// Log files found in the retention window.
    pub files: usize,
    /// Files read by this scan.
    pub read: usize,
    /// Files that had shrunk or been replaced and were read again.
    pub restarted: usize,
    /// Files or folders the system refused.
    pub unreadable: usize,
    /// Files of another user, left alone.
    pub foreign: usize,
    pub lines: u64,
    /// New requests indexed.
    pub requests: u64,
    /// Requests already counted from another file.
    pub duplicates: u64,
    /// Lines skipped as invalid.
    pub invalid: u64,
    /// Usage older than the retention, not indexed.
    pub expired: u64,
    /// Everything that changed was read (no limit was reached).
    pub complete: bool,
}

impl SourceScan {
    fn new(source: SourceId, state: SourceState) -> Self {
        Self {
            source,
            state,
            roots: Vec::new(),
            files: 0,
            read: 0,
            restarted: 0,
            unreadable: 0,
            foreign: 0,
            lines: 0,
            requests: 0,
            duplicates: 0,
            invalid: 0,
            expired: 0,
            complete: true,
        }
    }

    /// One sentence about the source for front ends.
    pub fn summary(&self) -> Localized {
        let label = self.source.label();
        match self.state {
            SourceState::Off => off_text(label),
            SourceState::NoLogs => no_logs_text(label),
            SourceState::NoAccess => {
                let paths = self
                    .roots
                    .iter()
                    .filter(|root| root.state == RootState::NoAccess)
                    .map(|root| root.path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                Localized::with_params(
                    "cost.scan.no_access",
                    params([("source", label.into()), ("paths", paths.as_str().into())]),
                    format!("{label}: sem acesso aos diretórios ({paths}); nada foi contornado."),
                )
            }
            SourceState::Ready => Localized::with_params(
                if self.complete {
                    "cost.scan.ready"
                } else {
                    "cost.scan.partial"
                },
                params([
                    ("source", label.into()),
                    ("files", self.files.into()),
                    ("requests", self.requests.into()),
                    ("invalid", self.invalid.into()),
                ]),
                if self.complete {
                    format!(
                        "{label}: {} log(s); {} requisição(ões) nova(s); {} linha(s) inválida(s) \
                         ignorada(s).",
                        self.files, self.requests, self.invalid
                    )
                } else {
                    format!(
                        "{label}: {} log(s); {} requisição(ões) nova(s); {} linha(s) inválida(s) \
                         ignorada(s); a leitura continua no próximo ciclo.",
                        self.files, self.requests, self.invalid
                    )
                },
            ),
        }
    }
}

/// `label`'s source is off: nothing is read.
pub(crate) fn off_text(label: &str) -> Localized {
    Localized::with_param(
        "cost.scan.off",
        "source",
        label,
        format!("{label}: desligado; nada é lido."),
    )
}

/// None of `label`'s default directories exists.
pub(crate) fn no_logs_text(label: &str) -> Localized {
    Localized::with_param(
        "cost.scan.no_logs",
        "source",
        label,
        format!("{label}: nenhum log encontrado nos diretórios."),
    )
}

/// One scan of every source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanReport {
    pub scanned_at: DateTime<Utc>,
    pub sources: Vec<SourceScan>,
    /// Every source that is on was read to the end.
    pub complete: bool,
}

/// Reads the logs of the sources that are on into the index.
#[derive(Debug, Clone)]
pub struct Scanner {
    index: CostIndex,
    settings: SettingsStore,
    env: Env,
    limits: Limits,
    zone: DayZone,
}

/// A log file found under a root.
struct Candidate {
    path: PathBuf,
    size: u64,
    identity: String,
    modified: SystemTime,
}

/// What reading one log file found.
#[derive(Default)]
struct FileRead {
    lines: u64,
    invalid: u64,
    expired: u64,
    stopped: bool,
    /// The file was replaced between the listing and the opening.
    restarted: bool,
}

/// The bounds of one scan.
struct Budget {
    deadline: Instant,
    files: usize,
}

impl Budget {
    fn spent(&self) -> bool {
        self.files == 0 || Instant::now() >= self.deadline
    }
}

impl Scanner {
    pub fn new(index: CostIndex, settings: SettingsStore, env: Env) -> Self {
        Self {
            index,
            settings,
            env,
            limits: Limits::default(),
            zone: DayZone::default(),
        }
    }

    pub fn with_limits(self, limits: Limits) -> Self {
        Self { limits, ..self }
    }

    pub fn with_zone(self, zone: DayZone) -> Self {
        Self { zone, ..self }
    }

    pub fn index(&self) -> &CostIndex {
        &self.index
    }

    pub fn settings(&self) -> &SettingsStore {
        &self.settings
    }

    pub fn env(&self) -> &Env {
        &self.env
    }

    pub fn zone(&self) -> DayZone {
        self.zone
    }

    /// Scan every source that is on, then prune by the retention. A source
    /// that is off is not touched at all (no directory is listed).
    pub fn scan(&self, now: DateTime<Utc>) -> Result<ScanReport, CostError> {
        let settings = self.settings.load()?;
        let on = settings.enabled_sources(&self.env);
        let retention = settings.retention();
        let kept = TimeDelta::days(i64::from(retention.keep_days()));
        let window = Window {
            now,
            first_day: retention.first_day(self.zone.day(now)),
            oldest: SystemTime::from(now - kept),
        };
        let mut budget = Budget {
            deadline: Instant::now() + self.limits.max_duration,
            files: self.limits.max_files,
        };
        let mut db = None;
        let mut sources = Vec::new();
        for source in SourceId::ALL {
            if !on.contains(&source) {
                sources.push(SourceScan::new(source, SourceState::Off));
                continue;
            }
            let db = match &mut db {
                Some(db) => db,
                None => db.insert(self.index.open()?),
            };
            let roots = settings.roots(source, &self.env);
            let custom = settings
                .sources
                .get(&source)
                .is_some_and(|s| !s.paths.is_empty());
            sources.push(self.scan_source(db, source, &roots, custom, &window, &mut budget)?);
        }
        if !on.is_empty() {
            self.index
                .prune(window.first_day, (now - kept).timestamp_millis())?;
        }
        Ok(ScanReport {
            scanned_at: now,
            complete: sources.iter().all(|s| s.complete),
            sources,
        })
    }

    fn scan_source(
        &self,
        db: &mut Connection,
        source: SourceId,
        roots: &[PathBuf],
        custom: bool,
        window: &Window,
        budget: &mut Budget,
    ) -> Result<SourceScan, CostError> {
        let mut scan = SourceScan::new(source, SourceState::NoLogs);
        let mut candidates = Vec::new();
        for root in roots {
            let state = self.walk(source, root, custom, window, &mut candidates, &mut scan);
            scan.roots.push(RootScan {
                path: root.clone(),
                state,
            });
        }
        scan.state = if scan.roots.iter().any(|r| r.state == RootState::Readable) {
            SourceState::Ready
        } else if scan.roots.iter().any(|r| r.state == RootState::NoAccess) {
            SourceState::NoAccess
        } else {
            SourceState::NoLogs
        };
        scan.files = candidates.len();
        // The newest logs first, so a limited scan shows today's usage.
        candidates.sort_by_key(|c| std::cmp::Reverse(c.modified));
        // A shrunk or replaced file loses its share before anything is
        // read, so a rotated copy elsewhere counts its requests again
        // whatever order the files are read in.
        let mut pending = Vec::new();
        for candidate in candidates {
            let key = format!("{}\0{}", source.slug(), candidate.path.to_string_lossy());
            let file = digest(key.as_bytes());
            let known = match CostIndex::file(db, source, &file)? {
                Some(row)
                    if row.identity != candidate.identity || candidate.size < row.examined =>
                {
                    CostIndex::forget(db, source, &file)?;
                    scan.restarted += 1;
                    None
                }
                Some(row) if row.examined == candidate.size => continue,
                known => known,
            };
            pending.push((candidate, file, known));
        }
        for (candidate, file, known) in pending {
            if budget.spent() {
                scan.complete = false;
                break;
            }
            budget.files -= 1;
            let started = Instant::now();
            let row =
                known.unwrap_or_else(|| FileRow::fresh(source, &file, candidate.identity.clone()));
            let mut entries = Vec::new();
            let (row, found) = match self.read_file(
                source,
                &file,
                &candidate.path,
                row,
                window,
                budget,
                &mut entries,
            ) {
                Ok(read) => read,
                // Removed since the listing: nothing to read.
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(_) => {
                    scan.unreadable += 1;
                    continue;
                }
            };
            let committed = CostIndex::commit(
                db,
                Chunk {
                    source,
                    file: &file,
                    restart: found.restarted,
                    row,
                    entries,
                    read_at: window.now.timestamp_millis(),
                },
                || self.still_on(source),
            )?;
            let Some(committed) = committed else {
                // Turned off meanwhile: nothing more is read or reported.
                return Ok(SourceScan::new(source, SourceState::Off));
            };
            scan.read += 1;
            scan.restarted += usize::from(found.restarted);
            scan.lines += found.lines;
            scan.invalid += found.invalid;
            scan.expired += found.expired;
            scan.requests += committed.requests;
            scan.duplicates += committed.duplicates;
            scan.complete &= !found.stopped;
            if self.limits.pace {
                std::thread::sleep(started.elapsed().min(MAX_PAUSE));
            }
        }
        Ok(scan)
    }

    /// The preference as it is on disk now (another front end may have
    /// turned the source off); unreadable counts as off.
    fn still_on(&self, source: SourceId) -> bool {
        self.settings
            .load()
            .is_ok_and(|settings| settings.enabled(source, &self.env))
    }

    /// Collect the logs of `source` under `root` into `candidates`, depth
    /// first, without following symbolic links.
    fn walk(
        &self,
        source: SourceId,
        root: &Path,
        custom: bool,
        window: &Window,
        candidates: &mut Vec<Candidate>,
        scan: &mut SourceScan,
    ) -> RootState {
        match std::fs::metadata(root) {
            Ok(meta) if meta.is_dir() => {}
            Err(error) if error.kind() == ErrorKind::NotFound && !custom => {
                return RootState::Missing
            }
            _ => return RootState::NoAccess,
        }
        let adapter = source.adapter();
        let mut pending = vec![(root.to_path_buf(), 0)];
        let mut looked = 0;
        while let Some((dir, depth)) = pending.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(_) if dir == root => return RootState::NoAccess,
                Err(_) => {
                    scan.unreadable += 1;
                    continue;
                }
            };
            for entry in entries {
                looked += 1;
                if looked > self.limits.max_entries {
                    scan.complete = false;
                    return RootState::Readable;
                }
                let Ok((entry, kind)) = entry.and_then(|e| e.file_type().map(|kind| (e, kind)))
                else {
                    scan.unreadable += 1;
                    continue;
                };
                if kind.is_dir() {
                    if depth < self.limits.max_depth {
                        pending.push((entry.path(), depth + 1));
                    }
                    continue;
                }
                if !kind.is_file() || !adapter.is_log(&entry.file_name().to_string_lossy()) {
                    continue;
                }
                let Ok(meta) = entry.metadata() else {
                    scan.unreadable += 1;
                    continue;
                };
                if !own(&meta) {
                    scan.foreign += 1;
                    continue;
                }
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if modified < window.oldest {
                    continue;
                }
                candidates.push(Candidate {
                    path: entry.path(),
                    size: meta.len(),
                    identity: identity(&meta),
                    modified,
                });
            }
        }
        RootState::Readable
    }

    /// Read `path` from `row.position`: complete lines only, up to the line
    /// and time limits, parsed into `entries` (usage of kept days).
    #[allow(clippy::too_many_arguments)]
    fn read_file(
        &self,
        source: SourceId,
        file: &str,
        path: &Path,
        mut row: FileRow,
        window: &Window,
        budget: &Budget,
        entries: &mut Vec<(NaiveDate, Entry)>,
    ) -> io::Result<(FileRow, FileRead)> {
        let mut handle = File::open(path)?;
        let meta = handle.metadata()?;
        // Swapped for another user's file (or a link to one) since the
        // listing: left alone, like any file the user does not own.
        if !own(&meta) {
            return Err(ErrorKind::PermissionDenied.into());
        }
        let size = meta.len();
        let mut found = FileRead::default();
        // Replaced between the listing and the opening: start over.
        if identity(&meta) != row.identity || size < row.position {
            row = FileRow::fresh(source, file, identity(&meta));
            found.restarted = true;
        }
        handle.seek(SeekFrom::Start(row.position))?;
        let settled = SystemTime::from(window.now)
            .duration_since(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH))
            .is_ok_and(|age| age >= SETTLED);
        let mut reader = BufReader::with_capacity(64 << 10, handle);
        let adapter = source.adapter();
        let mut buf = Vec::new();
        loop {
            let lines = usize::try_from(found.lines).unwrap_or(usize::MAX);
            if lines >= self.limits.max_lines
                || (lines > 0 && lines % CLOCK_EVERY == 0 && Instant::now() >= budget.deadline)
            {
                found.stopped = true;
                break;
            }
            let (consumed, too_long) =
                match next_line(&mut reader, &mut buf, self.limits.max_line_bytes)? {
                    Next::Line { consumed, too_long } => (consumed, too_long),
                    // The tool finished the file without a final newline.
                    Next::Partial { consumed, too_long } if settled => (consumed, too_long),
                    Next::Partial { .. } | Next::End => break,
                };
            row.position += consumed;
            found.lines += 1;
            if too_long {
                found.invalid += 1;
                continue;
            }
            let text = trim(&buf);
            if text.is_empty() {
                continue;
            }
            match adapter.parse(text, &mut row.state) {
                Line::Usage(entry) => {
                    let day = self.zone.day(entry.at);
                    if day < window.first_day {
                        found.expired += 1;
                    } else {
                        entries.push((day, entry));
                    }
                }
                Line::Skip => {}
                Line::Invalid => found.invalid += 1,
            }
        }
        row.examined = if found.stopped {
            row.position
        } else {
            size.max(row.position)
        };
        row.invalid += found.invalid;
        Ok((row, found))
    }
}

/// The span of one scan.
struct Window {
    now: DateTime<Utc>,
    /// The oldest local day kept.
    first_day: NaiveDate,
    /// Files last modified before this hold no kept usage.
    oldest: SystemTime,
}

impl FileRow {
    /// A file read from its start; its usage without a session identifier
    /// counts as one anonymous session per file.
    fn fresh(source: SourceId, file: &str, identity: String) -> Self {
        Self {
            identity,
            examined: 0,
            position: 0,
            state: FileState {
                session: Some(anonymous(source, file)),
                ..FileState::default()
            },
            invalid: 0,
        }
    }
}

/// One step of [`next_line`].
#[derive(Debug, PartialEq, Eq)]
enum Next {
    /// A line ended by a newline; `consumed` includes it. A line over the
    /// limit is consumed but not kept (`too_long`).
    Line {
        consumed: u64,
        too_long: bool,
    },
    /// Bytes at the end of the file without a newline yet.
    Partial {
        consumed: u64,
        too_long: bool,
    },
    End,
}

/// The next line of `reader` into `buf` (without its newline), never
/// holding more than `max` bytes of it.
fn next_line(reader: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> io::Result<Next> {
    buf.clear();
    let mut consumed = 0u64;
    let mut too_long = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok(if consumed == 0 {
                Next::End
            } else {
                Next::Partial { consumed, too_long }
            });
        }
        let (part, ended) = match available.iter().position(|&b| b == b'\n') {
            Some(at) => (&available[..at], true),
            None => (available, false),
        };
        if !too_long {
            if buf.len() + part.len() > max {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(part);
            }
        }
        let used = part.len() + usize::from(ended);
        reader.consume(used);
        consumed += used as u64;
        if ended {
            return Ok(Next::Line { consumed, too_long });
        }
    }
}

/// `line` without surrounding ASCII whitespace (`\r` included).
fn trim(line: &[u8]) -> &[u8] {
    line.trim_ascii()
}

/// What tells a replaced file apart from the same file grown: the device
/// and inode on Unix, the creation time elsewhere.
fn identity(meta: &Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        meta.created()
            .ok()
            .and_then(|at| at.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|at| at.as_nanos().to_string())
            .unwrap_or_default()
    }
}

/// RF1.4: only the user's own files are read (Unix); elsewhere the system
/// decides on opening.
fn own(meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.uid() == super::system::effective_uid()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}
