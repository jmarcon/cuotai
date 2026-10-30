//! Scanner, index and worker tests over synthetic logs in a temporary home
//! (PRD 0015 RF1 and RF2 acceptance criteria).

use super::*;
use chrono::TimeZone;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Marks every content field of the synthetic logs; it must never reach
/// the index, a report, a debug rendering or a text.
const SECRET: &str = "SENSITIVE-CONTENT-7f3a";

struct Fixture {
    dir: tempfile::TempDir,
    env: Env,
    scanner: Scanner,
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 10, 3, 12, 0, 0).unwrap()
}

fn fixture() -> Fixture {
    fixture_with(Limits {
        pace: false,
        ..Limits::default()
    })
}

fn fixture_with(limits: Limits) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let env = Env::new(Some(dir.path().join("home")), []);
    let scanner = Scanner::new(
        CostIndex::new(dir.path().join("data/cost/cost.sqlite3")),
        SettingsStore::at(dir.path().join("data/cost/settings.json")),
        env.clone(),
    )
    .with_limits(limits)
    .with_zone(DayZone::Fixed(FixedOffset::east_opt(0).unwrap()));
    Fixture { dir, env, scanner }
}

impl Fixture {
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn enable(&self, source: SourceId) {
        let store = self.scanner.settings();
        let disclosure = store.load().unwrap().disclosure(source, &self.env);
        store
            .enable(source, Vec::new(), &disclosure.digest, &self.env, now())
            .unwrap();
    }

    /// A Claude Code transcript of a project whose path is secret.
    fn claude_log(&self, name: &str) -> PathBuf {
        let dir = self
            .home()
            .join(".claude/projects")
            .join(format!("-Users-ana-{SECRET}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{name}.jsonl"))
    }

    fn codex_log(&self, folder: &str) -> PathBuf {
        let dir = self.home().join(".codex").join(folder).join("2025/10/02");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("rollout-2025-10-02T09-00-00-0199a.jsonl")
    }

    fn scan(&self) -> ScanReport {
        self.scanner.scan(now()).unwrap()
    }

    fn usage(&self) -> Vec<DailyUsage> {
        self.scanner
            .index()
            .usage(None, None, now().date_naive())
            .unwrap()
    }

    fn requests(&self) -> u64 {
        self.usage().iter().map(|u| u.requests).sum()
    }

    fn input(&self) -> u64 {
        self.usage().iter().map(|u| u.tokens.input).sum()
    }

    fn rows(&self, table: &str) -> i64 {
        let db = rusqlite::Connection::open(self.scanner.index().path()).unwrap();
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

fn source_scan(report: &ScanReport, source: SourceId) -> &SourceScan {
    report.sources.iter().find(|s| s.source == source).unwrap()
}

/// One assistant line with `input` tokens, request `n`, at `day` of October 2025.
fn claude_line(n: u32, input: u64, day: u32) -> String {
    format!(
        r#"{{"parentUuid":null,"cwd":"/Users/ana/{SECRET}","sessionId":"session-{SECRET}","message":{{"id":"msg_{n}","role":"assistant","model":"claude-sonnet-4-5-20250929","content":[{{"type":"text","text":"{SECRET} answer"}}],"usage":{{"input_tokens":{input},"output_tokens":10,"cache_read_input_tokens":100,"cache_creation_input_tokens":5}}}},"requestId":"req_{n}","type":"assistant","timestamp":"2025-10-{day:02}T08:00:00.000Z"}}"#
    ) + "\n"
}

fn user_line() -> String {
    format!(
        r#"{{"type":"user","cwd":"/Users/ana/{SECRET}","message":{{"role":"user","content":"{SECRET} prompt"}},"timestamp":"2025-10-02T07:59:00.000Z"}}"#
    ) + "\n"
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn append(path: &Path, text: &str) {
    OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

fn codex_rollout() -> String {
    let lines = [
        format!(r#"{{"timestamp":"2025-10-02T09:00:00.000Z","type":"session_meta","payload":{{"id":"0199a","cwd":"/Users/ana/{SECRET}","instructions":"{SECRET}"}}}}"#),
        r#"{"timestamp":"2025-10-02T09:00:01.000Z","type":"turn_context","payload":{"cwd":"/x","model":"gpt-5-codex"}}"#.to_owned(),
        format!(r#"{{"timestamp":"2025-10-02T09:00:02.000Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"{SECRET}"}}]}}}}"#),
        r#"{"timestamp":"2025-10-02T09:01:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":120,"cached_input_tokens":20,"output_tokens":30,"total_tokens":150},"last_token_usage":{"input_tokens":120,"cached_input_tokens":20,"output_tokens":30,"total_tokens":150}}}}"#.to_owned(),
        r#"{"timestamp":"2025-10-02T09:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":200,"cached_input_tokens":20,"output_tokens":50,"total_tokens":250},"last_token_usage":{"input_tokens":80,"cached_input_tokens":0,"output_tokens":20,"total_tokens":100}}}}"#.to_owned(),
    ];
    lines.join("\n") + "\n"
}

#[test]
fn nothing_is_read_or_created_while_every_source_is_off() {
    let f = fixture();
    write(&f.claude_log("a"), &claude_line(1, 10, 2));
    let report = f.scan();
    assert!(report
        .sources
        .iter()
        .all(|s| s.state == SourceState::Off && s.roots.is_empty() && s.files == 0));
    assert!(!f.scanner.index().path().exists());
    assert!(f.usage().is_empty());
}

#[test]
fn growth_is_read_incrementally_and_a_rescan_changes_nothing() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let log = f.claude_log("s1");
    write(
        &log,
        &(user_line() + &claude_line(1, 10, 2) + &claude_line(2, 20, 2)),
    );
    let first = f.scan();
    let claude = source_scan(&first, SourceId::ClaudeCode);
    assert_eq!(claude.state, SourceState::Ready);
    assert_eq!((claude.files, claude.read, claude.requests), (1, 1, 2));
    assert_eq!(f.input(), 30);

    let again = f.scan();
    let claude = source_scan(&again, SourceId::ClaudeCode);
    assert_eq!((claude.read, claude.requests), (0, 0));
    assert_eq!(f.input(), 30);

    append(&log, &claude_line(3, 5, 3));
    let grown = f.scan();
    assert_eq!(source_scan(&grown, SourceId::ClaudeCode).requests, 1);
    let usage = f.usage();
    assert_eq!(usage.len(), 2, "{usage:?}");
    assert_eq!(
        (usage[0].day.to_string(), usage[0].requests),
        ("2025-10-02".into(), 2)
    );
    assert_eq!(
        usage[0].tokens,
        Tokens {
            input: 30,
            output: 20,
            cache_read: 200,
            cache_write: 10,
            cache_write_1h: 0,
        }
    );
    assert_eq!(usage[0].sessions, 1);
    assert_eq!(usage[1].tokens.input, 5);
}

#[test]
fn a_line_being_written_waits_for_its_newline() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let log = f.claude_log("s1");
    let line = claude_line(1, 10, 2);
    let (head, tail) = line.split_at(40);
    write(&log, head);
    f.scan();
    assert_eq!(f.requests(), 0);
    append(&log, tail);
    f.scan();
    assert_eq!(f.requests(), 1);
}

#[test]
fn a_truncated_file_restarts_and_replaces_its_share() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let log = f.claude_log("s1");
    write(
        &log,
        &(claude_line(1, 10, 2) + &claude_line(2, 20, 2) + &claude_line(3, 30, 2)),
    );
    f.scan();
    assert_eq!(f.input(), 60);
    write(&log, &claude_line(9, 7, 2));
    let report = f.scan();
    assert_eq!(source_scan(&report, SourceId::ClaudeCode).restarted, 1);
    assert_eq!((f.requests(), f.input()), (1, 7));
}

#[test]
fn a_rotated_file_is_counted_once() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let log = f.claude_log("s1");
    write(&log, &(claude_line(1, 10, 2) + &claude_line(2, 20, 2)));
    f.scan();
    // Rotation: the old content moves aside and a new file takes the name.
    let rotated = log.with_file_name("s1-old.jsonl");
    std::fs::rename(&log, &rotated).unwrap();
    write(&log, &claude_line(3, 5, 2));
    let report = f.scan();
    assert_eq!(source_scan(&report, SourceId::ClaudeCode).restarted, 1);
    assert_eq!((f.requests(), f.input()), (3, 35));
    // A copy of requests already counted (a resumed session) adds nothing.
    write(
        &f.claude_log("s2"),
        &(claude_line(1, 10, 2) + &claude_line(3, 5, 2)),
    );
    let report = f.scan();
    assert_eq!(source_scan(&report, SourceId::ClaudeCode).duplicates, 2);
    assert_eq!((f.requests(), f.input()), (3, 35));
}

#[test]
fn an_archived_codex_rollout_is_counted_once() {
    let f = fixture();
    f.enable(SourceId::CodexCli);
    let active = f.codex_log("sessions");
    write(&active, &codex_rollout());
    f.scan();
    let usage = f.usage();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].model, "gpt-5-codex");
    assert_eq!(
        usage[0].tokens,
        Tokens {
            input: 180,
            output: 50,
            cache_read: 20,
            cache_write: 0,
            cache_write_1h: 0,
        }
    );
    let archived = f.codex_log("archived_sessions");
    std::fs::rename(&active, &archived).unwrap();
    let report = f.scan();
    assert_eq!(source_scan(&report, SourceId::CodexCli).duplicates, 2);
    assert_eq!(f.usage(), usage);
}

#[test]
fn line_file_time_and_size_limits_stop_and_the_next_scan_continues() {
    let f = fixture_with(Limits {
        max_lines: 2,
        max_files: 1,
        max_line_bytes: 4096,
        pace: false,
        ..Limits::default()
    });
    f.enable(SourceId::ClaudeCode);
    let lines: String = (1..=3).map(|n| claude_line(n, 1, 2)).collect();
    write(&f.claude_log("a"), &lines);
    write(&f.claude_log("b"), &claude_line(10, 100, 2));
    let long = format!(r#"{{"message":{{"content":"{}"}}}}"#, SECRET.repeat(400)) + "\n";
    write(&f.claude_log("c"), &(long + &claude_line(20, 1000, 2)));

    let mut scans = 0;
    loop {
        let report = f.scan();
        scans += 1;
        let claude = source_scan(&report, SourceId::ClaudeCode);
        assert!(claude.read <= 1, "one file per scan");
        assert!(claude.lines <= 2, "two lines per file per scan");
        if report.complete {
            break;
        }
        assert!(scans < 10, "the scans never caught up");
    }
    assert_eq!(f.requests(), 5);
    assert_eq!(f.input(), 1103);
    let invalid: i64 = rusqlite::Connection::open(f.scanner.index().path())
        .unwrap()
        .query_row("SELECT SUM(invalid) FROM files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(invalid, 1, "the over-long line is skipped and counted");

    let stopped = fixture_with(Limits {
        max_duration: Duration::ZERO,
        pace: false,
        ..Limits::default()
    });
    stopped.enable(SourceId::ClaudeCode);
    write(&stopped.claude_log("a"), &claude_line(1, 1, 2));
    let report = stopped.scan();
    assert!(!report.complete);
    assert_eq!(source_scan(&report, SourceId::ClaudeCode).read, 0);
}

#[test]
fn invalid_lines_are_skipped_and_counted() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let broken = format!("{{\"truncated\": \"{SECRET}\n") + "plain text\n" + "\n";
    write(&f.claude_log("a"), &(broken + &claude_line(1, 10, 2)));
    let report = f.scan();
    let claude = source_scan(&report, SourceId::ClaudeCode);
    assert_eq!((claude.invalid, claude.requests), (2, 1));
    assert!(claude.summary().resolve("en-US").contains("2 invalid"));
    // A rescan reads nothing new; the index still tells the lines skipped.
    assert_eq!(source_scan(&f.scan(), SourceId::ClaudeCode).invalid, 0);
    assert_eq!(
        f.scanner.index().indexed(SourceId::ClaudeCode).unwrap(),
        Indexed {
            files: 1,
            invalid_lines: 2
        }
    );
}

#[test]
fn usage_older_than_the_retention_is_not_kept() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    f.scanner
        .settings()
        .set_retention(Retention::days(7).unwrap())
        .unwrap();
    // 2025-09-20 is 13 days before `now`; 2025-09-28 is within 7.
    let old = claude_line(1, 10, 2).replace("2025-10-02", "2025-09-20");
    let kept = claude_line(2, 20, 2).replace("2025-10-02", "2025-09-28");
    write(&f.claude_log("a"), &(old + &kept));
    let report = f.scan();
    assert_eq!(source_scan(&report, SourceId::ClaudeCode).expired, 1);
    assert_eq!(f.input(), 20);

    let today = now().date_naive();
    let index = f.scanner.index();
    assert!(index
        .usage_in(None, Period::Today, today)
        .unwrap()
        .is_empty());
    assert_eq!(
        index
            .usage_in(None, Period::Last7Days, today)
            .unwrap()
            .len(),
        1
    );
    assert!(index
        .usage_in(None, Period::CurrentMonth, today)
        .unwrap()
        .is_empty());
    assert_eq!(index.usage_in(None, Period::All, today).unwrap().len(), 1);
    // Pruning drops days before the first one kept.
    index
        .prune(NaiveDate::from_ymd_opt(2025, 9, 29).unwrap(), 0)
        .unwrap();
    assert!(f.usage().is_empty());
}

#[test]
fn turning_a_source_off_and_erasing_remove_everything_derived() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    f.enable(SourceId::CodexCli);
    write(&f.claude_log("a"), &claude_line(1, 10, 2));
    write(&f.codex_log("sessions"), &codex_rollout());
    f.scan();
    let index = f.scanner.index();
    assert_eq!(
        index
            .usage(Some(SourceId::ClaudeCode), None, now().date_naive())
            .unwrap()
            .len(),
        1
    );

    let removed = turn_off(f.scanner.settings(), index, SourceId::ClaudeCode).unwrap();
    assert!(
        removed >= 4,
        "usage, session, seen and file rows: {removed}"
    );
    let db = rusqlite::Connection::open(index.path()).unwrap();
    for table in ["usage", "sessions", "seen", "files"] {
        let left: i64 = db
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE source = 'claude_code'"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "{table}");
    }
    assert!(f.usage().iter().all(|u| u.source == SourceId::CodexCli));
    assert_eq!(
        index.indexed(SourceId::ClaudeCode).unwrap(),
        Indexed::default()
    );
    // Off stops any read: new lines are not indexed.
    append(&f.claude_log("a"), &claude_line(2, 10, 2));
    let report = f.scan();
    assert_eq!(
        source_scan(&report, SourceId::ClaudeCode).state,
        SourceState::Off
    );
    assert!(f.usage().iter().all(|u| u.source == SourceId::CodexCli));

    assert!(index.erase().unwrap() > 0);
    for table in ["usage", "sessions", "seen", "files"] {
        assert_eq!(f.rows(table), 0, "{table}");
    }
}

#[test]
fn a_commit_after_the_source_was_turned_off_writes_nothing() {
    let f = fixture();
    let index = f.scanner.index();
    let mut db = index.open().unwrap();
    let entry = source::Entry {
        at: now(),
        model: "m".into(),
        tokens: Tokens {
            input: 1,
            ..Tokens::default()
        },
        session: String::new(),
        dedup: None,
    };
    let chunk = index::Chunk {
        source: SourceId::ClaudeCode,
        file: "f",
        restart: false,
        row: index::FileRow {
            identity: String::new(),
            examined: 1,
            position: 1,
            state: source::FileState::default(),
            invalid: 0,
        },
        entries: vec![(now().date_naive(), entry)],
        read_at: 0,
    };
    assert_eq!(CostIndex::commit(&mut db, chunk, || false).unwrap(), None);
    assert_eq!(f.rows("usage") + f.rows("files"), 0);
}

#[test]
fn sensitive_content_never_reaches_the_index_reports_or_texts() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    f.enable(SourceId::CodexCli);
    write(
        &f.claude_log(&format!("{SECRET}-session")),
        &(user_line() + &claude_line(1, 10, 2) + &format!("{{\"bad\": \"{SECRET}\n")),
    );
    write(&f.codex_log("sessions"), &codex_rollout());
    let report = f.scan();
    assert_eq!(f.requests(), 3);
    let index = f.scanner.index();
    // Fold the WAL into the database, then look at every byte on disk.
    rusqlite::Connection::open(index.path())
        .unwrap()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    let leaks = [SECRET, "session-", "msg_1", "req_1", "0199a", "/Users/ana"];
    for path in [
        index.path().to_path_buf(),
        f.scanner.settings().path().to_path_buf(),
    ] {
        let bytes = std::fs::read(&path).unwrap();
        for leak in leaks {
            let found = bytes.windows(leak.len()).any(|w| w == leak.as_bytes());
            assert!(!found, "{} holds {leak}", path.display());
        }
    }
    let settings = f.scanner.settings().load().unwrap();
    let mut outputs = vec![
        serde_json::to_string(&report).unwrap(),
        format!("{report:?}"),
        serde_json::to_string(&f.usage()).unwrap(),
        format!("{:?}", f.usage()),
    ];
    for scan in &report.sources {
        let disclosure = settings.disclosure(scan.source, &f.env);
        outputs.push(serde_json::to_string(&disclosure).unwrap());
        outputs.push(format!("{:?}", index.indexed(scan.source).unwrap()));
        for locale in ["pt-BR", "en-US", "es", "it"] {
            outputs.push(scan.summary().resolve(locale));
        }
    }
    for output in outputs {
        for leak in leaks {
            assert!(!output.contains(leak), "{leak} in {output}");
        }
    }
}

#[cfg(unix)]
#[test]
fn unreadable_or_missing_directories_are_a_state_never_worked_around() {
    use std::os::unix::fs::PermissionsExt;
    if system::effective_uid() == 0 {
        return; // root reads through permissions; nothing to observe
    }
    let f = fixture();
    f.enable(SourceId::CodexCli);
    let report = f.scan();
    let codex = source_scan(&report, SourceId::CodexCli);
    assert_eq!(codex.state, SourceState::NoLogs);
    assert!(codex.roots.iter().all(|r| r.state == RootState::Missing));

    let sessions = f.home().join(".codex/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::set_permissions(&sessions, std::fs::Permissions::from_mode(0o000)).unwrap();
    let report = f.scan();
    std::fs::set_permissions(&sessions, std::fs::Permissions::from_mode(0o700)).unwrap();
    let codex = source_scan(&report, SourceId::CodexCli);
    assert_eq!(codex.state, SourceState::NoAccess);
    assert!(codex.summary().resolve("en-US").contains("no access"));

    // A chosen directory that is gone (a missing volume) is no access too.
    let store = f.scanner.settings();
    let chosen = vec![PathBuf::from("/Volumes/unplugged-disk/claude")];
    let disclosure = Disclosure::new(SourceId::ClaudeCode, &chosen, &f.env);
    store
        .enable(
            SourceId::ClaudeCode,
            chosen,
            &disclosure.digest,
            &f.env,
            now(),
        )
        .unwrap();
    let report = f.scan();
    assert_eq!(
        source_scan(&report, SourceId::ClaudeCode).state,
        SourceState::NoAccess
    );
}

#[cfg(unix)]
#[test]
fn the_index_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    f.scan();
    let mode = std::fs::metadata(f.scanner.index().path())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_not_followed() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let outside = f.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    write(&outside.join("x.jsonl"), &claude_line(1, 10, 2));
    let projects = f.home().join(".claude/projects");
    std::fs::create_dir_all(&projects).unwrap();
    std::os::unix::fs::symlink(&outside, projects.join("linked")).unwrap();
    std::os::unix::fs::symlink(outside.join("x.jsonl"), projects.join("y.jsonl")).unwrap();
    let report = f.scan();
    assert_eq!(source_scan(&report, SourceId::ClaudeCode).files, 0);
    assert_eq!(f.requests(), 0);
}

/// Only bounds a worker that never gets there: the worker thread runs in the
/// background band, which a busy machine can stall for seconds.
fn wait_for(worker: &CostWorker, done: impl Fn(&CostStatus) -> bool) -> CostStatus {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let status = worker.status();
        if done(&status) {
            return status;
        }
        assert!(Instant::now() < deadline, "worker stuck at {status:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

const WORKER_SCHEDULE: Schedule = Schedule {
    interval: Duration::from_secs(3600),
    catch_up: Duration::from_secs(3600),
};

#[test]
fn spawning_the_worker_never_waits_for_its_first_cycle() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let (release, released) = std::sync::mpsc::channel::<()>();
    // The first cycle stops here, before it sets any state, until the
    // spawning thread lets it go: a spawn that waited would never return.
    let worker = CostWorker::spawn_with(
        f.scanner.clone(),
        WORKER_SCHEDULE,
        Box::new(move || released.recv().is_ok()),
    );
    assert_eq!(worker.status().state, WorkerState::Starting);
    release.send(()).unwrap();
    wait_for(&worker, |s| s.state == WorkerState::Paused);
}

#[test]
fn the_worker_scans_on_its_own_thread_and_pauses_while_saving_energy() {
    // The scan's time budget is wall-clock, and on a busy machine the
    // background band can spend it before the first file. A stopped scan
    // has its own test; this one must not depend on the machine's speed.
    let f = fixture_with(Limits {
        max_duration: Duration::from_secs(24 * 3600),
        pace: false,
        ..Limits::default()
    });
    // The worker uses the real clock: a line from an hour ago.
    let recent = (Utc::now() - chrono::TimeDelta::hours(1)).to_rfc3339();
    write(
        &f.claude_log("a"),
        &claude_line(1, 10, 2).replace("2025-10-02T08:00:00.000Z", &recent),
    );
    let saving = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let probe = std::sync::Arc::clone(&saving);
    let worker = CostWorker::spawn_with(
        f.scanner.clone(),
        WORKER_SCHEDULE,
        Box::new(move || probe.load(std::sync::atomic::Ordering::Relaxed)),
    );
    wait_for(&worker, |s| s.state == WorkerState::Off);

    f.enable(SourceId::ClaudeCode);
    worker.wake();
    wait_for(&worker, |s| s.state == WorkerState::Paused);
    assert_eq!(
        f.scanner
            .index()
            .usage(None, None, now().date_naive())
            .unwrap_or_default()
            .len(),
        0
    );

    saving.store(false, std::sync::atomic::Ordering::Relaxed);
    worker.wake();
    let status = wait_for(&worker, |s| s.state == WorkerState::Waiting);
    let last = status.last.unwrap();
    assert!(last.complete);
    assert_eq!(source_scan(&last, SourceId::ClaudeCode).requests, 1);
}

#[test]
fn indexed_usage_is_estimated_at_the_embedded_prices_without_any_content() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    f.enable(SourceId::CodexCli);
    write(
        &f.claude_log("s1"),
        &(user_line() + &claude_line(1, 1_000, 2)),
    );
    write(&f.codex_log("sessions"), &codex_rollout());
    f.scan();
    let estimate = estimate(&f.usage(), &Prices::embedded());
    // claude-sonnet-4-5-20250929 at the undated entry: 1 000 input × $3,
    // 10 output × $15, 100 cache reads × $0.30, 5 cache writes × $3.75 per
    // million = $0.00319875; gpt-5-codex has no price.
    assert_eq!(estimate.total.unwrap().micros(), 3_199);
    assert_eq!(
        estimate.unknown_models().collect::<Vec<_>>(),
        ["gpt-5-codex"]
    );
    assert_eq!(estimate.unknown_tokens.total(), 250);
    assert_eq!(estimate.tokens.total(), 1_365);
    let json = serde_json::to_string(&estimate).unwrap();
    assert!(!json.contains(SECRET) && !format!("{estimate:?}").contains(SECRET));
}

/// RF4: the summary every front end shows, from the index of synthetic
/// logs: yesterday's priced usage, today's absence, no content and no
/// directory in any of its renderings, and nothing once the source is off.
#[test]
fn the_summary_shows_indexed_days_without_content_and_nothing_once_off() {
    let f = fixture();
    let pricing = PricingFile::at(f.dir.path().join("data/cost/pricing.json"));
    let off = f.scanner.summary(&pricing, None, None, now()).unwrap();
    assert!(!off.any_on());
    assert!(
        !f.scanner.index().path().exists(),
        "a summary while off never creates the index"
    );

    f.enable(SourceId::ClaudeCode);
    write(
        &f.claude_log("s1"),
        &(user_line() + &claude_line(1, 1_000, 2)),
    );
    let report = f.scan();
    let summary = f
        .scanner
        .summary(&pricing, Some(&report), Some(Span::days(7).unwrap()), now())
        .unwrap();
    let claude = summary.source(SourceId::ClaudeCode).unwrap();
    assert_eq!(claude.state, CostState::Ready);
    let usage = claude.usage.as_ref().unwrap();
    assert_eq!(usage.yesterday.total.map(Usd::micros), Some(3_199));
    assert_eq!(usage.today.tokens.total(), 0);
    assert_eq!(usage.days.len(), 7);
    assert_eq!(summary.span.label.resolve("en-US"), "Last 7 days");

    let mut outputs = vec![
        serde_json::to_string(&summary).unwrap(),
        format!("{summary:?}"),
    ];
    for locale in ["pt-BR", "en-US", "es", "it"] {
        outputs.push(usage.yesterday.line(yesterday_label()).resolve(locale));
        outputs.extend(usage.days.iter().map(|day| day.line().resolve(locale)));
        for source in &summary.sources {
            outputs.extend(source.note.iter().map(|note| note.resolve(locale)));
        }
    }
    let home = f.home().display().to_string();
    for output in outputs {
        for leak in [SECRET, "session-", "msg_1", "req_1", home.as_str()] {
            assert!(!output.contains(leak), "{leak} in {output}");
        }
    }

    turn_off(
        f.scanner.settings(),
        f.scanner.index(),
        SourceId::ClaudeCode,
    )
    .unwrap();
    let after = f
        .scanner
        .summary(&pricing, Some(&report), None, now())
        .unwrap();
    assert!(!after.any_on());
    assert!(after.sources.iter().all(|s| s.usage.is_none()));
    assert_eq!(after.total.tokens.total(), 0);
}

/// RF4.1: choosing other directories needs a new consent, so a source
/// that is on turns off and loses what was derived; the same list changes
/// nothing and a relative directory is refused.
#[test]
fn choosing_other_directories_turns_the_source_off_and_erases_it() {
    let f = fixture();
    let (settings, index) = (f.scanner.settings(), f.scanner.index());
    f.enable(SourceId::ClaudeCode);
    write(&f.claude_log("s1"), &claude_line(1, 10, 2));
    f.scan();
    assert_eq!(f.requests(), 1);

    let defaults = choose_paths(settings, index, SourceId::ClaudeCode, Vec::new(), &f.env);
    assert_eq!(defaults, Ok(0), "the saved list: nothing changes");
    assert!(settings
        .load()
        .unwrap()
        .enabled(SourceId::ClaudeCode, &f.env));

    assert_eq!(
        choose_paths(
            settings,
            index,
            SourceId::ClaudeCode,
            vec![PathBuf::from("logs/claude")],
            &f.env
        ),
        Err(CostError::RelativePath)
    );
    assert_eq!(f.requests(), 1, "a refused list changes nothing");

    let chosen = vec![PathBuf::from("~/elsewhere"), PathBuf::from("~/elsewhere")];
    let removed = choose_paths(settings, index, SourceId::ClaudeCode, chosen, &f.env).unwrap();
    assert!(removed > 0);
    assert_eq!(f.requests(), 0);
    let saved = settings.load().unwrap();
    assert!(!saved.enabled(SourceId::ClaudeCode, &f.env));
    assert_eq!(
        saved.roots(SourceId::ClaudeCode, &f.env),
        [f.home().join("elsewhere")],
        "the duplicate is dropped and ~/ is expanded"
    );
    assert!(saved.disclosure(SourceId::ClaudeCode, &f.env).custom);
}

/// PRD 0030 RF5.1: a line with the split counts prices 1-hour writes apart;
/// RF7.2: a streamed message counts its highest output once.
fn claude_split_line(n: u32, output: u64, five: u64, hour: u64) -> String {
    format!(
        r#"{{"sessionId":"s","message":{{"id":"msg_{n}","model":"claude-sonnet-4-5-20250929","usage":{{"input_tokens":1,"output_tokens":{output},"cache_read_input_tokens":0,"cache_creation_input_tokens":{},"cache_creation":{{"ephemeral_5m_input_tokens":{five},"ephemeral_1h_input_tokens":{hour}}}}}}},"requestId":"req_{n}","timestamp":"2025-10-02T08:00:00.000Z"}}"#,
        five + hour
    ) + "\n"
}

#[test]
fn one_hour_cache_writes_are_kept_apart() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    write(
        &f.claude_log("s1"),
        &(claude_split_line(1, 5, 100, 40) + &claude_line(2, 1, 2)),
    );
    f.scan();
    let tokens = f.usage()[0].tokens;
    assert_eq!((tokens.cache_write, tokens.cache_write_1h), (105, 40));
}

#[test]
fn streamed_repeats_apply_only_the_growth() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let log = f.claude_log("s1");
    write(
        &log,
        &(claude_split_line(7, 3, 0, 0) + &claude_split_line(7, 3, 0, 0)),
    );
    f.scan();
    append(
        &log,
        &(claude_split_line(7, 1880, 0, 0) + &claude_split_line(7, 40, 0, 0)),
    );
    f.scan();
    let usage = f.usage();
    assert_eq!(usage[0].tokens.output, 1880);
    assert_eq!(usage[0].requests, 1);
}

/// PRD 0030 RF7.1: a transcript reduced to ids and counts (no content), after
/// the evidence that streamed messages repeat their key with growing
/// `output_tokens` (3, 3, 1880). The rule kept: the highest count per key.
#[test]
fn the_streamed_numbers_fixture_keeps_the_highest_output_per_message() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let fixture = include_str!("../../tests/fixtures/cost/claude_streamed_numbers.jsonl");
    write(&f.claude_log("s1"), fixture);
    f.scan();
    let once = f.usage();
    assert_eq!(once.len(), 1, "{once:?}");
    assert_eq!(once[0].requests, 4);
    assert_eq!(once[0].tokens.output, 1880 + 812 + 1 + 220);
    assert_eq!(once[0].tokens.cache_write_1h, 4 * 100);
    f.scan();
    assert_eq!(f.usage(), once, "a re-scan is idempotent");
}

#[test]
fn schema_one_is_migrated_and_claude_is_read_again() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let log = f.claude_log("s1");
    write(&log, &claude_split_line(1, 5, 100, 40));
    let path = f.scanner.index().path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(super::index::MIGRATIONS[0]).unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
        db.execute(
            "INSERT INTO usage VALUES ('claude_code','2025-10-02','m','x',9,9,9,9,1)",
            [],
        )
        .unwrap();
    }
    f.scan();
    let usage = f.usage();
    assert_eq!(usage.len(), 1, "{usage:?}");
    assert_eq!(usage[0].tokens.cache_write_1h, 40);
    assert_eq!(usage[0].requests, 1);
}
