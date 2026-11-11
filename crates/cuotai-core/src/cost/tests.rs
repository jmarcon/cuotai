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

/// The default limits with a time budget no scan can spend. The budget is
/// wall-clock: on a loaded CI runner (notably Windows) opening the index
/// alone can take the default five seconds, and the scan then stops before
/// its first file. Only the tests of the budget itself set a real one.
fn limits() -> Limits {
    Limits {
        max_duration: Duration::from_secs(24 * 3600),
        pace: false,
        ..Limits::default()
    }
}

fn fixture() -> Fixture {
    fixture_with(limits())
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
    // A first scan that stopped early would leave nothing to restart.
    assert!(f.scan().complete);
    assert_eq!((f.requests(), f.input()), (2, 30));
    // Rotation: the old content moves aside and a new file takes the name.
    // NTFS gives the new file the old one's creation time (tunneling), so on
    // Windows the identity matches and the shorter size tells them apart.
    let rotated = log.with_file_name("s1-old.jsonl");
    std::fs::rename(&log, &rotated).unwrap();
    write(&log, &claude_line(3, 5, 2));
    let report = f.scan();
    assert!(report.complete);
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
        ..limits()
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
        project: None,
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
            backfill: 0,
        },
        entries: vec![(now().date_naive(), entry)],
        replayed: 0,
        folders: Vec::new(),
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
    let f = fixture();
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

/// PRD 0042: one Claude Code assistant line of `session` in `cwd` at
/// `at` (RFC 3339) with `input` tokens.
fn claude_at(n: u32, session: &str, cwd: &str, at: &str, input: u64) -> String {
    format!(
        r#"{{"cwd":"{cwd}","sessionId":"{session}","message":{{"id":"m{n}","model":"claude-sonnet-4-5-20250929","content":"{SECRET}","usage":{{"input_tokens":{input},"output_tokens":10}}}},"requestId":"r{n}","type":"assistant","timestamp":"{at}"}}"#
    ) + "\n"
}

/// The project folder of the PRD 0042 fixtures: a personal name that
/// must never reach the index or any output.
const PERSONAL: &str = "/Users/ana-maria-example/clients/acme-secret";

fn three_sessions(f: &Fixture) {
    write(
        &f.claude_log("one"),
        &(claude_at(1, "s-one", PERSONAL, "2025-10-03T08:00:00Z", 100)
            + &claude_at(2, "s-one", PERSONAL, "2025-10-03T09:30:00Z", 100)),
    );
    write(
        &f.claude_log("two"),
        &claude_at(3, "s-two", PERSONAL, "2025-10-03T10:00:00Z", 50),
    );
    write(
        &f.claude_log("three"),
        &(claude_at(
            4,
            "s-three",
            "/srv/example/other",
            "2025-10-03T11:00:00Z",
            10,
        ) + &claude_at(
            5,
            "s-three",
            "/srv/example/other/",
            "2025-10-03T11:40:00Z",
            10,
        )),
    );
}

fn pricing(f: &Fixture) -> PricingFile {
    PricingFile::at(f.dir.path().join("data/cost/pricing.json"))
}

#[test]
fn sessions_and_projects_come_from_the_request_rows() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    three_sessions(&f);
    f.scan();
    let pricing = pricing(&f);
    let span = Span::Period(Period::Today);
    let page = f.scanner.sessions(&pricing, span, now(), None, 0).unwrap();
    assert_eq!(page.count, 3);
    assert_eq!(page.total.requests, 5);
    let newest = &page.sessions[0];
    assert_eq!(newest.estimate.requests, 2);
    assert_eq!(newest.duration_minutes, 40);
    assert_eq!(
        newest.name.resolve("en-US"),
        "Session · 2025-10-03 11:00–11:40 · 40m"
    );
    assert!(
        page.sessions
            .iter()
            .any(|s| s.estimate.requests == 2 && s.duration_minutes == 90),
        "{:?}",
        page.sessions
    );
    // Paging.
    let second = f
        .scanner
        .sessions(&pricing, span, now(), Some(1), 1)
        .unwrap();
    assert_eq!((second.count, second.sessions.len()), (3, 1));
    assert_eq!(second.sessions[0], page.sessions[1]);

    let projects = f.scanner.projects(&pricing, span, now()).unwrap();
    assert_eq!(
        projects.projects.len(),
        2,
        "a trailing slash is the same folder"
    );
    let personal = projects
        .projects
        .iter()
        .find(|p| p.estimate.requests == 3)
        .unwrap();
    assert_eq!(personal.sessions, 2);
    let prefix = personal.short.clone().unwrap();
    assert_eq!(personal.name.resolve("en-US"), format!("Project {prefix}"));

    // Labels round trip by prefix; the session label is stored apart.
    let index = f.scanner.index();
    index
        .set_label(LabelTarget::Project, &prefix[..6], Some("  Client work  "))
        .unwrap();
    index
        .set_label(LabelTarget::Session, &newest.short, Some("Refactor"))
        .unwrap();
    assert_eq!(
        index.set_label(LabelTarget::Project, "zz", Some("x")),
        Err(CostError::UnknownPrefix)
    );
    assert_eq!(
        index.set_label(LabelTarget::Project, &prefix, Some(&"x".repeat(49))),
        Err(CostError::InvalidLabel)
    );
    let projects = f.scanner.projects(&pricing, span, now()).unwrap();
    let labelled = projects
        .projects
        .iter()
        .find(|p| p.estimate.requests == 3)
        .unwrap();
    assert_eq!(labelled.label.as_deref(), Some("Client work"));
    assert_eq!(labelled.name.resolve("en-US"), "Client work");
    let page = f.scanner.sessions(&pricing, span, now(), None, 0).unwrap();
    assert_eq!(page.sessions[0].name.resolve("en-US"), "Refactor");
    index
        .set_label(LabelTarget::Project, &prefix, None)
        .unwrap();

    // A hidden project is listed but left out of the total.
    index.set_hidden(&prefix, true).unwrap();
    // PRD 0046: the labels a backup carries come back where none is set.
    let labels = index.labels().unwrap();
    assert!(labels
        .projects
        .iter()
        .any(|p| p.hidden && p.label.is_none()));
    assert_eq!(labels.sessions.len(), 1);
    let json = serde_json::to_string(&labels).unwrap();
    assert!(!json.contains("folder"), "{json}");
    index
        .set_label(LabelTarget::Session, &newest.short, None)
        .unwrap();
    assert_eq!(index.import_labels(&labels, false).unwrap(), 1);
    assert!(
        index.session_labels().unwrap().is_empty(),
        "a dry run writes nothing"
    );
    assert_eq!(index.import_labels(&labels, true).unwrap(), 1);
    assert_eq!(index.import_labels(&labels, true).unwrap(), 0);
    let mut foreign = labels.clone();
    foreign.sessions[0].session = "ffff0000".into();
    foreign.projects.clear();
    index
        .set_label(LabelTarget::Session, &newest.short, None)
        .unwrap();
    assert_eq!(
        index.import_labels(&foreign, true).unwrap(),
        0,
        "unknown sessions are skipped"
    );
    let projects = f.scanner.projects(&pricing, span, now()).unwrap();
    assert_eq!(projects.hidden, 1);
    assert_eq!(projects.projects.len(), 2);
    assert_eq!(projects.total.requests, 2);
    let summary = f
        .scanner
        .summary(&pricing, None, Some(span), now())
        .unwrap();
    assert_eq!(
        summary.projects.len(),
        1,
        "a hidden project is never exported"
    );

    // The personal path never reaches the index or an output.
    rusqlite::Connection::open(index.path())
        .unwrap()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    let bytes = std::fs::read(index.path()).unwrap();
    let outputs = [
        serde_json::to_string(&projects).unwrap(),
        serde_json::to_string(&page).unwrap(),
        serde_json::to_string(&summary).unwrap(),
    ];
    for leak in ["ana-maria", "acme-secret", "/srv/example", SECRET] {
        assert!(
            !bytes.windows(leak.len()).any(|w| w == leak.as_bytes()),
            "{leak}"
        );
        for output in &outputs {
            assert!(!output.contains(leak), "{leak} in {output}");
        }
    }
}

#[test]
fn the_folder_hint_is_opt_in_and_deleted_when_turned_off() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let store = f.scanner.settings();
    let index = f.scanner.index();
    write(
        &f.claude_log("one"),
        &claude_at(1, "s", PERSONAL, "2025-10-03T08:00:00Z", 1),
    );
    f.scan();
    let hints = |index: &CostIndex| -> Vec<Option<String>> {
        index
            .projects()
            .unwrap()
            .into_iter()
            .map(|p| p.folder_hint)
            .collect()
    };
    assert_eq!(hints(index), [None], "off by default");
    set_folder_hint(store, index, SourceId::ClaudeCode, true).unwrap();
    append(
        &f.claude_log("one"),
        &claude_at(2, "s", PERSONAL, "2025-10-03T08:10:00Z", 1),
    );
    f.scan();
    assert_eq!(hints(index), [Some("acme-secret".to_owned())]);
    assert_eq!(
        set_folder_hint(store, index, SourceId::ClaudeCode, false).unwrap(),
        1
    );
    assert_eq!(hints(index), [None]);
    assert!(!store.load().unwrap().folder_hint(SourceId::ClaudeCode));
}

#[test]
fn blocks_and_the_live_summary_answer_from_the_index() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    // A closed block in the morning, then an active one from 10:00 with
    // three requests in the last ten minutes before noon.
    let lines = [
        claude_at(1, "s", "/p", "2025-10-03T03:10:00Z", 100),
        claude_at(2, "s", "/p", "2025-10-03T10:05:00Z", 1000),
        claude_at(3, "s", "/p", "2025-10-03T11:51:00Z", 1000),
        claude_at(4, "s", "/p", "2025-10-03T11:55:00Z", 1000),
        claude_at(5, "s", "/p", "2025-10-03T11:58:00Z", 1000),
    ]
    .concat();
    write(&f.claude_log("s"), &lines);
    f.scan();
    let pricing = pricing(&f);
    let today = Span::Period(Period::Today);
    let page = f.scanner.blocks(&pricing, None, today, now(), &[]).unwrap();
    assert_eq!(page.blocks.len(), 2);
    let active = &page.blocks[0];
    assert!(active.active);
    assert_eq!(
        active.start,
        Utc.with_ymd_and_hms(2025, 10, 3, 10, 0, 0).unwrap()
    );
    assert_eq!(
        active.end,
        Utc.with_ymd_and_hms(2025, 10, 3, 15, 0, 0).unwrap()
    );
    assert!(!page.blocks[1].active);
    let live = f.scanner.live(&pricing, None, now(), &[]).unwrap();
    assert_eq!(live.len(), 1);
    let rate = live[0].burn_rate_tokens_per_min.unwrap();
    // Three requests of 1010 tokens over the last ten minutes.
    assert!((rate - 303.0).abs() < 0.5, "{rate}");
    assert_eq!(live[0].left_minutes, 180);
    assert!(live[0].projected_block_cost.is_some(), "40% elapsed");
    assert!(live[0].line.is_some());
    // The block length follows the setting.
    f.scanner
        .settings()
        .set_block_hours(SourceId::ClaudeCode, 1)
        .unwrap();
    let page = f.scanner.blocks(&pricing, None, today, now(), &[]).unwrap();
    assert_eq!(page.blocks.len(), 3);
    assert_eq!(
        f.scanner
            .settings()
            .set_block_hours(SourceId::ClaudeCode, 25),
        Err(CostError::BlockHours)
    );
}

#[test]
fn schema_two_files_replay_only_their_request_rows() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let streamed = include_str!("../../tests/fixtures/cost/claude_streamed_numbers.jsonl");
    write(&f.claude_log("s1"), streamed);
    write(&f.claude_log("copy"), streamed);
    f.scan();
    let before = f.usage();
    let path = f.scanner.index().path().to_path_buf();
    // What the schema 2 migration leaves: no request rows, and every file
    // to be read again up to where it was.
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "DELETE FROM requests;
             UPDATE files SET backfill = position, position = 0, examined = 0;",
        )
        .unwrap();
    }
    f.scan();
    assert_eq!(f.usage(), before, "day totals do not change");
    let requests = f
        .scanner
        .index()
        .requests(None, DateTime::<Utc>::default(), now())
        .unwrap();
    assert_eq!(requests.len() as u64, before[0].requests);
    let output: u64 = requests.iter().map(|r| r.tokens.output).sum();
    assert_eq!(
        output, before[0].tokens.output,
        "the highest counts are kept"
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    let pending: i64 = db
        .query_row("SELECT COUNT(*) FROM files WHERE backfill > 0", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(pending, 0);
}

#[test]
fn schema_two_is_migrated_in_place() {
    let f = fixture();
    let path = f.scanner.index().path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(super::index::MIGRATIONS[0]).unwrap();
        db.execute_batch(super::index::MIGRATIONS[1]).unwrap();
        db.pragma_update(None, "user_version", 2).unwrap();
        db.execute(
            "INSERT INTO files VALUES ('codex_cli','f','id',90,90,'{}',0,0)",
            [],
        )
        .unwrap();
    }
    let db = f.scanner.index().open().unwrap();
    let row: (i64, i64, i64) = db
        .query_row("SELECT backfill, position, examined FROM files", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(row, (90, 0, 0));
    drop(db);
    for table in ["requests", "projects", "session_labels"] {
        assert_eq!(f.rows(table), 0, "{table}");
    }
    assert!(f.scanner.index().erase().is_ok());
}

#[test]
fn a_source_on_under_the_first_disclosure_is_asked_again() {
    let f = fixture();
    let store = f.scanner.settings();
    let settings = store.load().unwrap();
    let disclosure = settings.disclosure(SourceId::CodexCli, &f.env);
    // The digest the first wording recorded.
    let old = {
        let identity = serde_json::to_vec(&(
            1u32,
            SourceId::CodexCli,
            &disclosure.directories,
            disclosure.pattern,
        ))
        .unwrap();
        crate::models::digest(&identity)
    };
    std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    std::fs::write(
        store.path(),
        serde_json::json!({ "sources": { "codex_cli": { "consent": old } } }).to_string(),
    )
    .unwrap();
    let saved = store.load().unwrap();
    assert!(
        !saved.enabled(SourceId::CodexCli, &f.env),
        "nothing is read"
    );
    assert!(saved.needs_reconsent(SourceId::CodexCli, &f.env));
    assert!(!saved.needs_reconsent(SourceId::ClaudeCode, &f.env));
    let summary = f.scanner.summary(&pricing(&f), None, None, now()).unwrap();
    assert!(summary.source(SourceId::CodexCli).unwrap().reconsent);
    f.enable(SourceId::CodexCli);
    assert!(!store
        .load()
        .unwrap()
        .needs_reconsent(SourceId::CodexCli, &f.env));
}

/// A schema-2 index with data, opened once so the migration writes its
/// pre-upgrade copy. Returns the path of that `.bak`.
fn migrated_with_backup(f: &Fixture) -> PathBuf {
    let path = f.scanner.index().path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(super::index::MIGRATIONS[0]).unwrap();
        db.execute_batch(super::index::MIGRATIONS[1]).unwrap();
        db.pragma_update(None, "user_version", 2).unwrap();
        db.execute(
            "INSERT INTO files VALUES ('codex_cli','f','id',90,90,'{}',0,0)",
            [],
        )
        .unwrap();
    }
    drop(f.scanner.index().open().unwrap());
    let bak = crate::private_fs::backup_path(&path);
    assert!(bak.exists(), "the migration leaves a pre-upgrade copy");
    bak
}

#[test]
fn erasing_the_index_removes_the_pre_upgrade_copy() {
    let f = fixture();
    let bak = migrated_with_backup(&f);
    f.scanner.index().erase().unwrap();
    assert!(!bak.exists(), "S-01: the .bak survived Erase the index");
}

#[test]
fn turning_a_source_off_removes_the_pre_upgrade_copy() {
    let f = fixture();
    let bak = migrated_with_backup(&f);
    turn_off(f.scanner.settings(), f.scanner.index(), SourceId::CodexCli).unwrap();
    assert!(
        !bak.exists(),
        "S-01: the .bak survived turning a source off"
    );
}

#[test]
fn choosing_other_folders_removes_the_pre_upgrade_copy() {
    let f = fixture();
    f.enable(SourceId::CodexCli);
    let bak = migrated_with_backup(&f);
    let other = f.dir.path().join("elsewhere");
    std::fs::create_dir_all(&other).unwrap();
    choose_paths(
        f.scanner.settings(),
        f.scanner.index(),
        SourceId::CodexCli,
        vec![other],
        &f.env,
    )
    .unwrap();
    assert!(!bak.exists(), "S-01: the .bak survived a folder change");
}

#[test]
fn the_pre_upgrade_copy_expires_after_seven_days() {
    let f = fixture();
    let bak = migrated_with_backup(&f);
    let written = DateTime::<Utc>::from(std::fs::metadata(&bak).unwrap().modified().unwrap());
    let index = f.scanner.index();
    assert!(!index
        .prune_backup(written + chrono::TimeDelta::days(6))
        .unwrap());
    assert!(bak.exists(), "kept for 7 days");
    assert!(index
        .prune_backup(written + chrono::TimeDelta::days(8))
        .unwrap());
    assert!(!bak.exists(), "removed after 7 days");
    assert!(!index
        .prune_backup(written + chrono::TimeDelta::days(9))
        .unwrap());
}

/// S-02: a sum past `i64::MAX` saturates in SQL instead of turning REAL.
#[test]
fn sql_sums_saturate_instead_of_overflowing() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    let sum = |a: i64, b: i64| -> rusqlite::types::Value {
        db.query_row(
            &format!("SELECT {}", super::index::add("?1", "?2")),
            [a, b],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        sum(i64::MAX - 1, 10),
        rusqlite::types::Value::Integer(i64::MAX)
    );
    assert_eq!(
        sum(i64::MAX, i64::MAX),
        rusqlite::types::Value::Integer(i64::MAX)
    );
    assert_eq!(sum(5, 10), rusqlite::types::Value::Integer(15));
}

/// S-02: rows already at the ceiling add up without failing the read.
#[test]
fn usage_rows_at_the_ceiling_read_back_saturated() {
    let f = fixture();
    let index = f.scanner.index();
    let db = index.open().unwrap();
    for file in ["a", "b"] {
        db.execute(
            "INSERT INTO usage VALUES ('claude_code','2025-10-02','m',?1,
                 9223372036854775807,1,0,0,1,0)",
            [file],
        )
        .unwrap();
    }
    drop(db);
    let usage = index
        .usage(None, None, NaiveDate::from_ymd_opt(2025, 10, 3).unwrap())
        .unwrap();
    assert_eq!(usage.len(), 1, "one day and model");
    assert_eq!(usage[0].tokens.input, 2 * i64::MAX as u64);
    assert_eq!(usage[0].tokens.output, 2);
}

/// S-02: an index poisoned before the fix (a REAL count) is clamped by
/// the migration and read, not left unreadable until erased.
#[test]
fn a_count_that_overflowed_into_real_is_clamped_on_upgrade() {
    let f = fixture();
    let path = f.scanner.index().path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        for step in &super::index::MIGRATIONS[..3] {
            db.execute_batch(step).unwrap();
        }
        db.pragma_update(None, "user_version", 3).unwrap();
        db.execute(
            "INSERT INTO usage VALUES ('claude_code','2025-10-02','m','f',1.0e19,1,0,0,1,0)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO usage VALUES ('claude_code','2025-10-02','m','g',5,1,0,0,1,0)",
            [],
        )
        .unwrap();
    }
    let usage = f
        .scanner
        .index()
        .usage(None, None, NaiveDate::from_ymd_opt(2025, 10, 3).unwrap())
        .unwrap();
    assert_eq!(usage[0].tokens.input, i64::MAX as u64 + 5);
    let kind: String = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT typeof(input) FROM usage WHERE file = 'f'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kind, "integer");
}

/// S-02: an index that cannot be read is an `unreadable` source with a
/// note, never a summary that looks like every source is off.
#[test]
fn an_unreadable_index_is_reported_per_source_not_as_off() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    write(&f.claude_log("a"), &claude_line(1, 10, 2));
    f.scan();
    let path = f.scanner.index().path().to_path_buf();
    for companion in ["-wal", "-shm"] {
        let mut name = path.clone().into_os_string();
        name.push(companion);
        let _ = std::fs::remove_file(name);
    }
    write(&path, &"not a database ".repeat(400));
    let summary = f.scanner.summary(&pricing(&f), None, None, now()).unwrap();
    assert!(summary.any_on(), "never shown as off");
    let claude = summary.source(SourceId::ClaudeCode).unwrap();
    assert_eq!(claude.state, CostState::Unreadable);
    assert!(claude.usage.is_none());
    assert_eq!(claude.note.as_ref().unwrap().id, "cost.view.unreadable");
    assert_eq!(summary.unreadable(), claude.note.as_ref());
    let codex = summary.source(SourceId::CodexCli).unwrap();
    assert_eq!(
        codex.state,
        CostState::Off,
        "a source that is off stays off"
    );
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(json["sources"][0]["state"], "unreadable");
}
