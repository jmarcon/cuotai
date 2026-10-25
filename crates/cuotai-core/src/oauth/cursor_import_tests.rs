use super::*;
use crate::oauth::OAuthError;
use chrono::{Duration, Utc};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Build a fresh SQLite file shaped like Cursor's `state.vscdb` with the
/// given key/value pairs. Returns the path; the file is closed so the
/// production code opens it from scratch via the read-only flag.
fn build_state_db(dir: &TempDir, name: &str, rows: &[(&str, &str)]) -> PathBuf {
    let path = dir.path().join(name);
    let conn = Connection::open(&path).expect("open sqlite fixture");
    conn.execute_batch("CREATE TABLE IF NOT EXISTS itemTable (key TEXT PRIMARY KEY, value TEXT);")
        .expect("create itemTable");
    for (k, v) in rows {
        conn.execute(
            "INSERT OR REPLACE INTO itemTable (key, value) VALUES (?1, ?2)",
            params![k, v],
        )
        .expect("insert row");
    }
    drop(conn);
    path
}

// Smoke fixture values: the JWT matches the one the parent's manual
// smoke run will use (`auth0|cursor-test`, exp=2100), and the machine
// id is the short ASCII identifier also used by the smoke run.
const TOKEN_VALUE: &str =
    "smoke-token-very-long-enough-to-pass-the-fifty-byte-floor-padding-padding";
const MACHINE_VALUE: &str = "cursor-smoke-machine";
const TOKEN_ALIAS: &str = "cursorAuth/token";
const TOKEN_PRIMARY: &str = "cursorAuth/accessToken";
const MACHINE_PRIMARY: &str = "storage.serviceMachineId";
const MACHINE_ALIAS_2: &str = "storage.machineId";
const MACHINE_ALIAS_3: &str = "telemetry.machineId";

#[tokio::test]
async fn primary_keys_extract_cleanly() {
    let dir = TempDir::new().unwrap();
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, TOKEN_VALUE),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let token = import(Some(path)).await.expect("import");
    assert_eq!(token.access_token, TOKEN_VALUE);
    assert!(token.refresh_token.is_none());
    assert_eq!(
        token.metadata.get("auth_method").map(String::as_str),
        Some("imported")
    );
    assert_eq!(
        token.metadata.get("device_id").map(String::as_str),
        Some(MACHINE_VALUE)
    );
}

#[tokio::test]
async fn alias_token_key_used_when_primary_missing() {
    let dir = TempDir::new().unwrap();
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[(TOKEN_ALIAS, TOKEN_VALUE), (MACHINE_PRIMARY, MACHINE_VALUE)],
    );
    let token = import(Some(path)).await.expect("import");
    assert_eq!(token.access_token, TOKEN_VALUE);
}

#[tokio::test]
async fn primary_token_wins_over_alias() {
    let dir = TempDir::new().unwrap();
    let alias_padding = "alias-should-not-be-used-token-padded-to-fifty-chars-xx";
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, TOKEN_VALUE),
            (TOKEN_ALIAS, alias_padding),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let token = import(Some(path)).await.expect("import");
    assert_eq!(token.access_token, TOKEN_VALUE);
}

#[tokio::test]
async fn machine_id_aliases_in_priority_order() {
    // Alias 2 only.
    let dir = TempDir::new().unwrap();
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, TOKEN_VALUE),
            (MACHINE_ALIAS_2, MACHINE_VALUE),
        ],
    );
    let token = import(Some(path)).await.expect("import alias2");
    assert_eq!(
        token.metadata.get("device_id").map(String::as_str),
        Some(MACHINE_VALUE)
    );

    // Alias 3 only.
    let dir2 = TempDir::new().unwrap();
    let path2 = build_state_db(
        &dir2,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, TOKEN_VALUE),
            (MACHINE_ALIAS_3, MACHINE_VALUE),
        ],
    );
    let token2 = import(Some(path2)).await.expect("import alias3");
    assert_eq!(
        token2.metadata.get("device_id").map(String::as_str),
        Some(MACHINE_VALUE)
    );
}

#[tokio::test]
async fn json_quoted_strings_are_unwrapped() {
    let dir = TempDir::new().unwrap();
    let token_quoted = format!("\"{}\"", TOKEN_VALUE);
    let machine_quoted = format!("\"{}\"", MACHINE_VALUE);
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, token_quoted.as_str()),
            (MACHINE_PRIMARY, machine_quoted.as_str()),
        ],
    );
    let token = import(Some(path)).await.expect("import");
    assert_eq!(token.access_token, TOKEN_VALUE);
    assert_eq!(
        token.metadata.get("device_id").map(String::as_str),
        Some(MACHINE_VALUE)
    );
}

#[tokio::test]
async fn values_are_trimmed() {
    let dir = TempDir::new().unwrap();
    let token_padded = format!("   {}   ", TOKEN_VALUE);
    let machine_padded = format!(" {} ", MACHINE_VALUE);
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, token_padded.as_str()),
            (MACHINE_PRIMARY, machine_padded.as_str()),
        ],
    );
    let token = import(Some(path)).await.expect("import");
    assert_eq!(token.access_token, TOKEN_VALUE);
    assert_eq!(
        token.metadata.get("device_id").map(String::as_str),
        Some(MACHINE_VALUE)
    );
}

#[tokio::test]
async fn missing_token_is_actionable_error() {
    let dir = TempDir::new().unwrap();
    let path = build_state_db(&dir, "state.vscdb", &[(MACHINE_PRIMARY, MACHINE_VALUE)]);
    let err = import(Some(path)).await.err().expect("expected error");
    match err {
        OAuthError::Configuration(msg) => {
            let fb = msg.fallback.to_lowercase();
            assert!(
                fb.contains("token") || fb.contains("login") || fb.contains("caminho"),
                "expected actionable message, got {:?}",
                msg.fallback
            );
        }
        other => panic!("expected Configuration error, got {:?}", other),
    }
}

#[tokio::test]
async fn missing_machine_id_is_actionable_error() {
    let dir = TempDir::new().unwrap();
    let path = build_state_db(&dir, "state.vscdb", &[(TOKEN_PRIMARY, TOKEN_VALUE)]);
    let err = import(Some(path)).await.err().expect("expected error");
    match err {
        OAuthError::Configuration(msg) => {
            let fb = msg.fallback.to_lowercase();
            assert!(fb.contains("machine") || fb.contains("sign in"));
        }
        other => panic!("expected Configuration error, got {:?}", other),
    }
}

#[tokio::test]
async fn short_token_is_rejected() {
    let dir = TempDir::new().unwrap();
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, "short-token-under-50-bytes-xxxxxxxxxx"),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let err = import(Some(path)).await.err().expect("expected error");
    match err {
        OAuthError::Configuration(msg) => assert!(msg.fallback.to_lowercase().contains("token")),
        other => panic!("expected Configuration error, got {:?}", other),
    }
}

#[tokio::test]
async fn short_ascii_machine_id_is_accepted() {
    // The reference service names the machine id "UUID-like" but
    // several builds (and the parent's smoke fixture) store a short
    // ASCII identifier. We only enforce bounded length and absence of
    // control characters.
    let dir = TempDir::new().unwrap();
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[(TOKEN_PRIMARY, TOKEN_VALUE), (MACHINE_PRIMARY, "abc")],
    );
    let token = import(Some(path)).await.expect("import");
    assert_eq!(
        token.metadata.get("device_id").map(String::as_str),
        Some("abc")
    );
}

#[tokio::test]
async fn control_chars_are_rejected() {
    let dir = TempDir::new().unwrap();
    let bad_token = format!("{}\u{0001}", TOKEN_VALUE);
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, bad_token.as_str()),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let err = import(Some(path)).await.err().expect("expected error");
    match err {
        OAuthError::Configuration(_) => {}
        other => panic!("expected Configuration error, got {:?}", other),
    }
}

#[tokio::test]
async fn missing_file_is_actionable() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("does-not-exist.vscdb");
    let err = import(Some(missing)).await.err().expect("expected error");
    match err {
        OAuthError::Configuration(_) => {}
        other => panic!("expected Configuration error, got {:?}", other),
    }
}

#[tokio::test]
async fn source_bytes_unchanged_after_import() {
    let dir = TempDir::new().unwrap();
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, TOKEN_VALUE),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let before = std::fs::read(&path).expect("read fixture before");
    import(Some(path.clone())).await.expect("import");
    let after = std::fs::read(&path).expect("read fixture after");
    assert_eq!(before, after, "source db bytes must not be mutated");
}

#[tokio::test]
async fn noncreating_path_does_not_create_db() {
    let dir = TempDir::new().unwrap();
    let missing_dir = dir.path().join("no-such-dir");
    let missing = missing_dir.join("state.vscdb");
    let err = import(Some(missing.clone()))
        .await
        .err()
        .expect("expected error");
    assert!(matches!(err, OAuthError::Configuration(_)));
    assert!(!missing.exists(), "fixture must not have been created");
    assert!(
        !missing_dir.exists(),
        "parent dir must not have been created"
    );
}

#[tokio::test]
async fn jwt_exp_in_the_past_is_rejected() {
    let dir = TempDir::new().unwrap();
    let past = Utc::now() - Duration::hours(1);
    let exp = past.timestamp();
    let token = jwt_fixture(exp, Some("auth0|cursor-test"));
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, token.as_str()),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let err = import(Some(path)).await.err().expect("expected error");
    match err {
        OAuthError::Expired => {}
        other => panic!("expected Expired, got {:?}", other),
    }
}

#[tokio::test]
async fn jwt_exp_inside_session_refresh_skew_is_rejected() {
    let dir = TempDir::new().unwrap();
    let exp = (Utc::now() + Duration::seconds(MIN_TOKEN_VALIDITY_SECS - 5)).timestamp();
    let token = jwt_fixture(exp, Some("user_FreshSoonButUnusable"));
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, token.as_str()),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );

    let err = import(Some(path)).await.err().expect("expected error");
    assert!(matches!(err, OAuthError::Expired));
}

#[tokio::test]
async fn jwt_exp_in_the_future_is_accepted_and_recorded() {
    let dir = TempDir::new().unwrap();
    let future = Utc::now() + Duration::hours(6);
    let exp = future.timestamp();
    let sub = "auth0|user_MixedCase_42";
    let expected_user_id = "user_MixedCase_42";
    let token = jwt_fixture(exp, Some(sub));
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, token.as_str()),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let parsed = import(Some(path)).await.expect("import");
    assert_eq!(parsed.access_token, token);
    assert_eq!(
        parsed.metadata.get("user_id").map(String::as_str),
        Some(expected_user_id)
    );
    assert!(parsed.expires_at.is_some());
    assert!(parsed.expires_at.unwrap() <= future + Duration::seconds(5));
}

#[tokio::test]
async fn smoke_jwt_year_2100_is_accepted() {
    // Parent's smoke fixture: a legacy Auth0 subject whose Cursor session
    // user ID is the final `cursor-test` component, exp=2100.
    let dir = TempDir::new().unwrap();
    let exp = 4_102_444_800_i64; // 2100-01-01T00:00:00Z
    let sub = "auth0|cursor-test";
    let token = jwt_fixture(exp, Some(sub));
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, token.as_str()),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let parsed = import(Some(path)).await.expect("import smoke");
    assert_eq!(parsed.access_token, token);
    assert_eq!(
        parsed.metadata.get("device_id").map(String::as_str),
        Some(MACHINE_VALUE)
    );
    assert_eq!(
        parsed.metadata.get("user_id").map(String::as_str),
        Some("cursor-test")
    );
    assert_eq!(
        parsed.metadata.get("auth_method").map(String::as_str),
        Some("imported")
    );
    assert!(parsed.expires_at.is_some());
    assert!(parsed.refresh_token.is_none());
    assert!(parsed.metadata.contains_key("token_expires_at"));
}

#[tokio::test]
async fn malformed_jwt_does_not_block_import() {
    let dir = TempDir::new().unwrap();
    // Two parts only, not three: not a JWT.
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (
                TOKEN_PRIMARY,
                "opaque-cursor-access-token-longer-than-fifty-characters.payload",
            ),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let parsed = import(Some(path)).await.expect("import");
    assert!(parsed.expires_at.is_none());
    assert!(!parsed.metadata.contains_key("user_id"));
    assert!(!parsed.metadata.contains_key("token_expires_at"));
}

#[tokio::test]
async fn candidates_returns_metadata_paths_only() {
    let list = candidates();
    assert!(
        !list.is_empty(),
        "candidates() should return at least one path"
    );
    for p in &list {
        assert!(
            p.is_absolute() || p.starts_with("~"),
            "expected absolute path, got {:?}",
            p
        );
        assert!(
            p.ends_with("state.vscdb"),
            "expected vscdb file, got {:?}",
            p
        );
    }
    // Sanity: candidates() must be cheap and never open any of the
    // returned files. We call it twice and assert the second call is
    // bytewise identical (no time-dependent filesystem state).
    let again = candidates();
    assert_eq!(list, again, "candidates() must be a pure function");
    let _ = Path::new(&list[0]); // avoid unused-import warnings
}

#[tokio::test]
async fn token_value_exceeding_64kib_is_rejected() {
    let dir = TempDir::new().unwrap();
    let big = "a".repeat(65 * 1024);
    let path = build_state_db(
        &dir,
        "state.vscdb",
        &[
            (TOKEN_PRIMARY, big.as_str()),
            (MACHINE_PRIMARY, MACHINE_VALUE),
        ],
    );
    let err = import(Some(path)).await.err().expect("expected error");
    match err {
        OAuthError::Configuration(_) => {}
        other => panic!("expected Configuration error, got {:?}", other),
    }
}

fn jwt_fixture(exp: i64, sub: Option<&str>) -> String {
    let header = base64_url_encode(b"{\"alg\":\"HS256\",\"typ\":\"JWT\"}");
    let payload = match sub {
        Some(s) => format!("{{\"sub\":\"{}\",\"exp\":{}}}", s, exp),
        None => format!("{{\"exp\":{}}}", exp),
    };
    let body = base64_url_encode(payload.as_bytes());
    let sig = base64_url_encode(b"sig-bytes-not-validated");
    format!("{}.{}.{}", header, body, sig)
}

fn base64_url_encode(input: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(input)
}
