//! PRD 0042 S-06: session, request and log-file digests keyed to this
//! installation, and the migration that keys an existing index.

use super::*;
use crate::models::digest;

/// Every byte of the index on disk, its WAL folded in.
fn index_bytes(f: &Fixture) -> Vec<u8> {
    let index = f.scanner.index();
    rusqlite::Connection::open(index.path())
        .unwrap()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    std::fs::read(index.path()).unwrap()
}

fn holds(bytes: &[u8], text: &str) -> bool {
    bytes.windows(text.len()).any(|w| w == text.as_bytes())
}

/// The digest an index before S-06 stored for a log path.
fn plain_file(source: SourceId, path: &Path) -> String {
    digest(format!("{}\0{}", source.slug(), path.to_string_lossy()).as_bytes())
}

#[test]
fn keyed_digests_differ_by_key_and_a_sealed_session_opens_only_with_it() {
    let a = source::ProjectKey::new([1; 32]);
    let b = source::ProjectKey::new([2; 32]);
    let plain = source::anonymous(SourceId::CodexCli, CODEX_SESSION);
    let keyed = a.keyed(&plain);
    assert_eq!(keyed.len(), 64);
    assert_ne!(keyed, plain);
    assert_ne!(keyed, b.keyed(&plain));
    assert_eq!(a.keyed(""), "", "no session stays no session");
    let mut state = source::FileState {
        key: Some(a.clone()),
        ..source::FileState::default()
    };
    state.set_session(plain.clone());
    assert_eq!(state.session.as_deref(), Some(keyed.as_str()));
    let seal = state.seal.clone().unwrap();
    assert_ne!(seal, plain);
    assert_eq!(state.plain_session(), plain);
    // Stored and read back (the key is never serialized).
    let text = serde_json::to_string(&state).unwrap();
    assert!(!text.contains(&plain), "{text}");
    let mut back: source::FileState = serde_json::from_str(&text).unwrap();
    back.key = Some(a);
    assert_eq!(back.plain_session(), plain);
    back.key = Some(b);
    assert_eq!(back.plain_session(), keyed, "another key opens nothing");
}

/// The plain SHA-256 of a session id, of a request id and of a log path,
/// which anyone holding the id or the path can compute, appears nowhere in
/// the index or in its readings.
#[test]
fn the_plain_digest_of_a_session_id_appears_nowhere() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    f.enable(SourceId::CodexCli);
    let claude = f.claude_log("a");
    write(&claude, &claude_line(1, 10, 2));
    let codex = f.codex_log("sessions");
    write(&codex, &codex_rollout());
    f.scan();
    let session = source::anonymous(SourceId::ClaudeCode, &format!("session-{SECRET}"));
    let codex_session = source::anonymous(SourceId::CodexCli, CODEX_SESSION);
    let plain = [
        session.clone(),
        codex_session.clone(),
        source::anonymous(SourceId::ClaudeCode, "msg_1\0req_1"),
        source::anonymous(SourceId::CodexCli, &format!("{codex_session}\0150")),
        plain_file(SourceId::ClaudeCode, &claude),
        plain_file(SourceId::CodexCli, &codex),
    ];
    let bytes = index_bytes(&f);
    for digest in &plain {
        assert!(!holds(&bytes, digest), "the index holds {digest}");
        assert!(
            !holds(&bytes, &digest[..16]),
            "the index holds a prefix of {digest}"
        );
    }
    let index = f.scanner.index();
    let key = index.fingerprint_key().unwrap();
    let from = Utc.with_ymd_and_hms(2025, 10, 1, 0, 0, 0).unwrap();
    let requests = index.requests(None, from, now()).unwrap();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        assert!(!plain.contains(&request.session));
    }
    assert!(requests.iter().any(|r| r.session == key.keyed(&session)));
    assert!(requests
        .iter()
        .any(|r| r.session == key.keyed(&codex_session)));
}

/// The migration keys every stored session, request and file digest in
/// place: session labels keep their sessions, a file's state still yields
/// the request keys stored before, and a copy read afterwards (a resumed
/// Claude Code session, an archived Codex rollout) adds no duplicate.
#[test]
fn stored_digests_are_keyed_on_upgrade_without_duplicates_or_lost_labels() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    f.enable(SourceId::CodexCli);
    let path = f.scanner.index().path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let session = source::anonymous(SourceId::ClaudeCode, &format!("session-{SECRET}"));
    let request = source::anonymous(SourceId::ClaudeCode, "msg_1\0req_1");
    let codex_session = source::anonymous(SourceId::CodexCli, CODEX_SESSION);
    let codex_keys = [150, 250]
        .map(|total| source::anonymous(SourceId::CodexCli, &format!("{codex_session}\0{total}")));
    let claude_file = plain_file(SourceId::ClaudeCode, Path::new("/gone/a.jsonl"));
    let codex_file = plain_file(SourceId::CodexCli, Path::new("/gone/rollout.jsonl"));
    let at = Utc
        .with_ymd_and_hms(2025, 10, 2, 8, 0, 0)
        .unwrap()
        .timestamp_millis();
    {
        // The index as the release before S-06 left it.
        let db = rusqlite::Connection::open(&path).unwrap();
        db.create_scalar_function("cuotai_fingerprint", 1, Default::default(), |ctx| {
            ctx.get::<Option<String>>(0)
        })
        .unwrap();
        let before = super::index::MIGRATIONS.len() - 1;
        for step in &super::index::MIGRATIONS[..before] {
            db.execute_batch(step).unwrap();
        }
        db.pragma_update(None, "user_version", before as i64)
            .unwrap();
        let usage = "INSERT INTO usage (source, day, model, file, input, output, cache_read,
                 cache_write, requests) VALUES (?1, '2025-10-02', ?2, ?3, ?4, 0, 0, 0, 1)";
        let seen = "INSERT INTO seen (source, key, file, day) VALUES (?1, ?2, ?3, '2025-10-02')";
        let requests = "INSERT INTO requests (source, file, key, at, day, model, session,
                 input, output, cache_read, cache_write, cache_write_1h)
             VALUES (?1, ?2, ?3, ?4, '2025-10-02', ?5, ?6, ?7, 0, 0, 0, 0)";
        let claude_model = "claude-sonnet-4-5-20250929";
        db.execute(
            usage,
            rusqlite::params!["claude_code", claude_model, claude_file, 10],
        )
        .unwrap();
        db.execute(seen, ["claude_code", &request, &claude_file])
            .unwrap();
        db.execute(
            requests,
            rusqlite::params![
                "claude_code",
                claude_file,
                request,
                at,
                claude_model,
                session,
                10
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO sessions VALUES ('claude_code', '2025-10-02', ?1, ?2, ?3)",
            [claude_model, &session, &claude_file],
        )
        .unwrap();
        db.execute(
            "INSERT INTO session_labels (session, label) VALUES (?1, 'Refactor')",
            [&session],
        )
        .unwrap();
        db.execute(
            usage,
            rusqlite::params!["codex_cli", "gpt-5-codex", codex_file, 200],
        )
        .unwrap();
        for (key, input) in codex_keys.iter().zip([120, 80]) {
            db.execute(seen, ["codex_cli", key, &codex_file]).unwrap();
            db.execute(
                requests,
                rusqlite::params![
                    "codex_cli",
                    codex_file,
                    key,
                    at,
                    "gpt-5-codex",
                    codex_session,
                    input
                ],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO files (source, file, identity, examined, position, state, invalid,
                 read_at) VALUES ('codex_cli', ?1, 'gone', 9, 9, ?2, 0, 0)",
            [
                codex_file.clone(),
                format!(r#"{{"model":"gpt-5-codex","session":"{codex_session}","total":250}}"#),
            ],
        )
        .unwrap();
    }
    let index = f.scanner.index();
    let key = index.fingerprint_key().unwrap();
    // Labels survive, under the keyed session.
    let labels = index.session_labels().unwrap();
    assert_eq!(
        labels.get(&key.keyed(&session)).map(String::as_str),
        Some("Refactor")
    );
    assert_eq!(labels.len(), 1);
    // A followed file's state: the session keyed, its sealed original
    // still giving the request keys stored before.
    let db = rusqlite::Connection::open(&path).unwrap();
    let (file, state): (String, String) = db
        .query_row("SELECT file, state FROM files", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(file, key.keyed(&codex_file));
    let mut state: source::FileState = serde_json::from_str(&state).unwrap();
    assert_eq!(state.session, Some(key.keyed(&codex_session)));
    state.key = Some(key.clone());
    assert_eq!(state.plain_session(), codex_session);
    assert_eq!(state.total, Some(250));
    drop(db);
    let bytes = index_bytes(&f);
    for plain in [
        &session,
        &request,
        &codex_session,
        &claude_file,
        &codex_file,
    ]
    .into_iter()
    .chain(&codex_keys)
    {
        assert!(!holds(&bytes, plain), "the index still holds {plain}");
    }

    // Copies read after the upgrade: the request already counted is a
    // duplicate, the new one joins the labelled session.
    let resumed = f.claude_log("resumed");
    write(&resumed, &(claude_line(1, 10, 2) + &claude_line(2, 20, 2)));
    write(&f.codex_log("archived_sessions"), &codex_rollout());
    let report = f.scan();
    let claude = source_scan(&report, SourceId::ClaudeCode);
    assert_eq!((claude.requests, claude.duplicates), (1, 1));
    let codex = source_scan(&report, SourceId::CodexCli);
    assert_eq!((codex.requests, codex.duplicates), (0, 2));
    assert_eq!(f.input(), 10 + 20 + 200);
    let from = Utc.with_ymd_and_hms(2025, 10, 1, 0, 0, 0).unwrap();
    let requests = index.requests(None, from, now()).unwrap();
    assert_eq!(requests.len(), 4);
    let claude_sessions: std::collections::BTreeSet<_> = requests
        .iter()
        .filter(|r| r.source == SourceId::ClaudeCode)
        .map(|r| r.session.clone())
        .collect();
    assert_eq!(
        claude_sessions.into_iter().collect::<Vec<_>>(),
        [key.keyed(&session)]
    );
}

/// A backup written before S-06 (labels schema 1) holds unkeyed session
/// digests: a restore keys them, so the labels meet their sessions.
#[test]
fn a_schema_1_backup_labels_the_keyed_sessions() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    write(&f.claude_log("a"), &claude_line(1, 10, 2));
    f.scan();
    let index = f.scanner.index();
    let plain = source::anonymous(SourceId::ClaudeCode, &format!("session-{SECRET}"));
    let old = Labels {
        schema: 1,
        key: None,
        projects: Vec::new(),
        sessions: vec![SessionLabel {
            session: plain.clone(),
            label: "Refactor".into(),
        }],
    };
    assert_eq!(index.import_labels(&old, true).unwrap(), 1);
    let keyed = index.fingerprint_key().unwrap().keyed(&plain);
    assert_eq!(
        index
            .session_labels()
            .unwrap()
            .get(&keyed)
            .map(String::as_str),
        Some("Refactor")
    );
    // A backup written now carries keyed digests, and says so.
    let labels = index.labels().unwrap();
    assert_eq!(labels.schema, 2);
    assert_eq!(labels.sessions[0].session, keyed);
}
