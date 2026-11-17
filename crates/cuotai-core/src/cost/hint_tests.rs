//! PRD 0042 re-validation K3 (S-01, S-01b, S-02, S-03): folder names are
//! kept per source only while that source's hint is on, never leave the
//! index, and follow a turn-off and the retention.

use super::*;

/// The folder of the hint fixtures: its last part is what a hint keeps.
const HINTED: &str = "/fake/HINTMARK-folder";
/// A second project of the fixtures, hidden in the every-output test.
const HIDDEN: &str = "/fake/HINTMARK-hidden";
/// What no output may carry.
const MARK: &str = "HINTMARK";

fn still(on: bool, keep_folders: bool, same_key: bool) -> index::Still {
    index::Still {
        on,
        keep_folders,
        same_key,
    }
}

fn hints(f: &Fixture) -> Vec<(String, String, String)> {
    let db = rusqlite::Connection::open(f.scanner.index().path()).unwrap();
    let mut query = db
        .prepare("SELECT project, source, folder_hint FROM project_hints ORDER BY source")
        .unwrap();
    let rows = query
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    rows.collect::<Result<_, _>>().unwrap()
}

fn hint_on(f: &Fixture, source: SourceId) {
    set_folder_hint(f.scanner.settings(), f.scanner.index(), source, true).unwrap();
}

/// A Claude Code line in `cwd` on 2025-10-`day`.
fn claude_in(n: u32, cwd: &str, day: u32) -> String {
    claude_at(
        n,
        "s",
        cwd,
        &format!("2025-10-{day:02}T08:00:00Z"),
        u64::from(n) * 10,
    )
}

/// A Codex CLI rollout whose every line runs in `cwd`.
fn codex_in(cwd: &str) -> String {
    codex_rollout()
        .replace(&format!("/Users/ana/{SECRET}"), cwd)
        .replace(r#""cwd":"/x""#, &format!(r#""cwd":"{cwd}""#))
}

/// S-01: a commit whose source turned its hint off after the read began
/// (the folders were collected) stores no name; with the hint on it does.
#[test]
fn a_commit_after_the_hint_was_turned_off_stores_no_name() {
    let f = fixture();
    let index = f.scanner.index();
    let mut db = index.open().unwrap();
    let chunk = |file: &'static str| index::Chunk {
        source: SourceId::ClaudeCode,
        file,
        restart: false,
        row: index::FileRow {
            identity: String::new(),
            examined: 1,
            position: 1,
            state: source::FileState::default(),
            invalid: 0,
            backfill: 0,
        },
        entries: vec![(
            now().date_naive(),
            source::Entry {
                at: now(),
                model: "m".into(),
                tokens: Tokens {
                    input: 1,
                    ..Tokens::default()
                },
                session: "s".into(),
                dedup: None,
                project: Some("ab12cd34ab12cd34".into()),
            },
        )],
        replayed: 0,
        folders: vec![("ab12cd34ab12cd34".into(), "HINTMARK-folder".into())],
        read_at: 0,
        started: None,
    };
    CostIndex::commit(&mut db, chunk("a"), || still(true, false, true))
        .unwrap()
        .unwrap();
    assert!(hints(&f).is_empty(), "off at commit: no name");
    assert_eq!(f.rows("requests"), 1, "the usage is still counted");
    CostIndex::commit(&mut db, chunk("b"), || still(true, true, true))
        .unwrap()
        .unwrap();
    assert_eq!(hints(&f).len(), 1, "on at commit: the name is kept");
    // A key replaced meanwhile (a restore): nothing is written, and the
    // next scan reads the file again with the new key.
    let dropped = CostIndex::commit(&mut db, chunk("c"), || still(true, true, false))
        .unwrap()
        .unwrap();
    assert!(dropped.dropped);
    assert_eq!(f.rows("requests"), 2);
}

/// S-01 through the scanner: the closure reloads the settings, so turning
/// the hint off before a commit keeps the names out; turning it off
/// deletes the names kept and the pre-upgrade copy.
#[test]
fn turning_the_hint_off_deletes_the_names_and_the_pre_upgrade_copy() {
    let f = fixture();
    let bak = migrated_with_backup(&f);
    f.enable(SourceId::ClaudeCode);
    hint_on(&f, SourceId::ClaudeCode);
    write(&f.claude_log("a"), &claude_in(1, HINTED, 2));
    f.scan();
    assert_eq!(hints(&f).len(), 1);
    assert_eq!(
        f.scanner.index().projects().unwrap()[0]
            .folder_hint
            .as_deref(),
        Some("HINTMARK-folder")
    );
    assert!(bak.exists());
    let removed = set_folder_hint(
        f.scanner.settings(),
        f.scanner.index(),
        SourceId::ClaudeCode,
        false,
    )
    .unwrap();
    assert_eq!(removed, 1);
    assert!(hints(&f).is_empty());
    assert!(!bak.exists(), "the pre-upgrade copy holds every name");
    // Nothing on disk keeps the name (the WAL was truncated).
    for path in [
        f.scanner.index().path().to_path_buf(),
        PathBuf::from(format!("{}-wal", f.scanner.index().path().display())),
    ] {
        let bytes = std::fs::read(&path).unwrap_or_default();
        assert!(
            !bytes.windows(MARK.len()).any(|w| w == MARK.as_bytes()),
            "{}",
            path.display()
        );
    }
}

/// S-02: turning one source off removes the names it kept and its switch,
/// even when another source still uses the project; the other source's
/// name stays.
#[test]
fn turning_a_source_off_removes_its_names_and_its_switch() {
    let f = fixture();
    for source in [SourceId::ClaudeCode, SourceId::CodexCli] {
        f.enable(source);
        hint_on(&f, source);
    }
    write(&f.claude_log("a"), &claude_in(1, HINTED, 2));
    write(&f.codex_log("sessions"), &codex_in(HINTED));
    f.scan();
    let kept = hints(&f);
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[0].0, kept[1].0, "one project, two sources");
    turn_off(
        f.scanner.settings(),
        f.scanner.index(),
        SourceId::CodexCli,
        None,
    )
    .unwrap();
    let left = hints(&f);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].1, "claude_code");
    let saved = f.scanner.settings().load().unwrap();
    assert!(!saved.folder_hint(SourceId::CodexCli), "the switch is off");
    assert!(saved.folder_hint(SourceId::ClaudeCode));
    // Its first and last request now come from Claude Code's only.
    let project = &f.scanner.index().projects().unwrap()[0];
    assert_eq!(
        project.first_seen,
        Some(Utc.with_ymd_and_hms(2025, 10, 2, 8, 0, 0).unwrap())
    );
    assert_eq!(project.first_seen, project.last_seen);
}

/// S-03: past the retention, the folder names and the projects left
/// without a request go; a project with the person's label stays.
#[test]
fn the_retention_takes_the_names_and_the_unlabelled_projects() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    hint_on(&f, SourceId::ClaudeCode);
    write(
        &f.claude_log("a"),
        &(claude_in(1, HINTED, 1) + &claude_in(2, HIDDEN, 1) + &claude_in(3, "/fake/new", 3)),
    );
    f.scan();
    let index = f.scanner.index();
    assert_eq!(index.projects().unwrap().len(), 3);
    assert_eq!(hints(&f).len(), 3);
    let labelled = index
        .projects()
        .unwrap()
        .into_iter()
        .find(|p| p.folder_hint.as_deref() == Some("HINTMARK-hidden"))
        .unwrap();
    index
        .set_label(LabelTarget::Project, &labelled.project, Some("Mine"))
        .unwrap();
    index
        .prune(NaiveDate::from_ymd_opt(2025, 10, 2).unwrap(), 0)
        .unwrap();
    let left = index.projects().unwrap();
    assert_eq!(left.len(), 2, "the unlabelled old project went");
    let mine = left.iter().find(|p| p.project == labelled.project).unwrap();
    assert_eq!(mine.label.as_deref(), Some("Mine"));
    assert_eq!((mine.folder_hint.as_deref(), mine.last_seen), (None, None));
    assert_eq!(hints(&f).len(), 1, "only the kept request's name");
    assert_eq!(left[0].folder_hint.as_deref(), Some("new"));
}

/// Schema 6 moves each folder name under the source that kept it and
/// keeps labels and hidden flags; times are computed from the requests.
#[test]
fn schema_five_is_migrated_with_its_names_per_source() {
    let f = fixture();
    let path = f.scanner.index().path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // The fingerprint key exists, so migration 5 has a key to use.
    drop(f.scanner.index().fingerprint_key().unwrap());
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        for step in &super::index::MIGRATIONS[..4] {
            db.execute_batch(step).unwrap();
        }
        db.pragma_update(None, "user_version", 4).unwrap();
        db.execute_batch(
            "INSERT INTO projects (project, label, folder_hint, hint_source, first_seen, last_seen, hidden)
                 VALUES ('aaaa', 'Work', 'HINTMARK-a', 'codex_cli', 1, 2, 1),
                        ('bbbb', NULL, NULL, NULL, 3, 4, 0);
             INSERT INTO requests VALUES
                 ('claude_code','f','k',5000,'1970-01-01','m','s','aaaa',1,0,0,0,0);",
        )
        .unwrap();
    }
    let index = f.scanner.index();
    let key = index.fingerprint_key().unwrap();
    let projects = index.projects().unwrap();
    assert_eq!(projects.len(), 2);
    let work = &projects[0];
    assert_eq!(work.project, key.fingerprint("aaaa"));
    assert_eq!((work.label.as_deref(), work.hidden), (Some("Work"), true));
    assert_eq!(work.folder_hint.as_deref(), Some("HINTMARK-a"));
    assert_eq!(work.last_seen.map(|t| t.timestamp_millis()), Some(5000));
    assert_eq!(projects[1].last_seen, None, "no request: no time");
    assert_eq!(
        hints(&f),
        [(
            work.project.clone(),
            "codex_cli".into(),
            "HINTMARK-a".into()
        )]
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    let columns: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('projects')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(columns, ["project", "label", "hidden"]);
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, super::index::SCHEMA_REVISION);
}

/// S-01b: with the hint on and a hidden project, no output carries a
/// folder name: the summary (`--cost --json`), every page, the live view,
/// the backup's labels and `export::v1`. The Settings page's `hints` is
/// the one place (desktop test).
#[test]
fn no_output_carries_a_folder_name() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    hint_on(&f, SourceId::ClaudeCode);
    write(
        &f.claude_log("a"),
        &(claude_at(1, "s1", HINTED, "2025-10-03T10:00:00Z", 100)
            + &claude_at(2, "s1", HINTED, "2025-10-03T11:55:00Z", 100)
            + &claude_at(3, "s2", HIDDEN, "2025-10-03T11:56:00Z", 100)
            + &claude_at(4, "s2", HIDDEN, "2025-10-03T11:57:00Z", 100)),
    );
    f.scan();
    let index = f.scanner.index();
    let hidden = index
        .projects()
        .unwrap()
        .into_iter()
        .find(|p| p.folder_hint.as_deref() == Some("HINTMARK-hidden"))
        .expect("the hint is kept");
    index.set_hidden(&hidden.project, true).unwrap();
    let pricing = pricing(&f);
    let span = Span::Period(Period::Today);
    let summary = f
        .scanner
        .summary(&pricing, None, Some(span), now())
        .unwrap();
    let projects = f.scanner.projects(&pricing, span, now()).unwrap();
    assert!(
        projects.projects.iter().any(|p| p.folder_hint.is_some()),
        "the core still reads the names"
    );
    let live = f.scanner.live(&pricing, None, now(), &[]).unwrap();
    assert!(!live.is_empty(), "an active block");
    let config = crate::Config::from_toml("accounts = []").unwrap();
    let cycle = crate::refresh::RefreshCycle::new(&config, std::time::Instant::now());
    let export = crate::export::v1::Export::from_cycle(
        &config,
        &crate::config_file::ConfigIdentity::Explicit("t".into()),
        &cycle,
        &[],
        now(),
        crate::export::v1::Options::default(),
    )
    .with_local_cost(Some(summary.clone()));
    let outputs = [
        serde_json::to_string(&summary).unwrap(),
        serde_json::to_string(&projects).unwrap(),
        serde_json::to_string(&f.scanner.sessions(&pricing, span, now(), None, 0).unwrap())
            .unwrap(),
        serde_json::to_string(&f.scanner.blocks(&pricing, None, span, now(), &[]).unwrap())
            .unwrap(),
        serde_json::to_string(&live).unwrap(),
        serde_json::to_string(&index.labels().unwrap()).unwrap(),
        serde_json::to_string(&export).unwrap(),
    ];
    for output in outputs {
        assert!(!output.contains(MARK), "{output}");
    }
}

/// S-01b: a new field of [`ProjectSummary`] fails to compile here until
/// someone decides whether it may leave the index (`folder_hint` may not:
/// it is never serialized).
#[test]
fn every_project_summary_field_is_classified() {
    let summary = ProjectSummary {
        project: Some("ab12cd34ab12cd34".into()),
        short: Some("ab12cd34".into()),
        label: None,
        folder_hint: Some(MARK.into()),
        name: project_name(Some("ab12cd34ab12cd34"), None),
        hidden: false,
        first: now(),
        last: now(),
        sessions: 1,
        estimate: estimate(&[], &Prices::embedded()),
    };
    let ProjectSummary {
        project: _,
        short: _,
        label: _,
        folder_hint: _,
        name: _,
        hidden: _,
        first: _,
        last: _,
        sessions: _,
        estimate: _,
    } = summary.clone();
    let json = serde_json::to_value(&summary).unwrap();
    assert!(json.get("folder_hint").is_none(), "{json}");
    assert!(!json.to_string().contains(MARK));
}
