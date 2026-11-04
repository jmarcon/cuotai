use super::*;
use crate::{Credentials, Provider};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
struct MemoryStore(Mutex<HashMap<String, OAuthToken>>);
impl TokenStore for MemoryStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        Ok(self.0.lock().get(id).cloned())
    }
    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        self.0.lock().insert(id.into(), token.clone());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), OAuthError> {
        self.0.lock().remove(id);
        Ok(())
    }
}
struct NoSecrets;
impl Credentials for NoSecrets {
    fn get(&self, _: &str) -> Option<String> {
        None
    }
}

#[tokio::test]
async fn changed_client_cannot_reuse_stored_access_token() {
    let store = Arc::new(MemoryStore::default());
    let session = OAuthSession::new(store, Arc::new(NoSecrets)).unwrap();
    let config = OAuthConfig::new("own-google-client".into());
    let login = AuthorizedLogin {
        key: storage_key(Provider::GeminiApi, &config).unwrap(),
        token: OAuthToken {
            access_token: "access-one".into(),
            refresh_token: None,
            expires_at: None,
            metadata: Default::default(),
        },
    };
    session.save_login(&login).await.unwrap();
    assert_eq!(
        session
            .access_token(Provider::GeminiApi, &config)
            .await
            .unwrap(),
        "access-one"
    );
    let mut changed = config.clone();
    changed.client_id = "other-client".into();
    assert!(matches!(
        session.access_token(Provider::GeminiApi, &changed).await,
        Err(OAuthError::Configuration(_))
    ));
    assert!(matches!(
        session.access_token(Provider::AzureOpenAi, &config).await,
        Err(OAuthError::Configuration(_))
    ));
    session.logout(Provider::GeminiApi, &config).await.unwrap();
    assert!(matches!(
        session.access_token(Provider::GeminiApi, &config).await,
        Err(OAuthError::Configuration(_))
    ));
}

#[tokio::test]
async fn expired_token_without_refresh_requires_new_login() {
    let session = OAuthSession::new(Arc::new(MemoryStore::default()), Arc::new(NoSecrets)).unwrap();
    let config = OAuthConfig::new("own-google-client".into());
    session
        .save_login(&AuthorizedLogin {
            key: storage_key(Provider::GeminiApi, &config).unwrap(),
            token: OAuthToken {
                access_token: "old".into(),
                refresh_token: None,
                expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
                metadata: Default::default(),
            },
        })
        .await
        .unwrap();
    assert!(matches!(
        session.access_token(Provider::GeminiApi, &config).await,
        Err(OAuthError::Expired)
    ));
}

#[tokio::test]
async fn native_metadata_stays_secure_and_options_isolate_sessions() {
    let store = Arc::new(MemoryStore::default());
    let session = OAuthSession::new(store, Arc::new(NoSecrets)).unwrap();
    let mut config = OAuthConfig::new(String::new());
    config.options.insert("region".into(), "us-east-1".into());
    let login = AuthorizedLogin {
        key: storage_key(Provider::Kiro, &config).unwrap(),
        token: OAuthToken {
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            expires_at: None,
            metadata: [
                ("client_secret".into(), "secret-value".into()),
                ("profile_arn".into(), "arn:aws:profile/example".into()),
                ("auth_method".into(), "builder-id".into()),
            ]
            .into(),
        },
    };
    let projected = login.account_params(Provider::Kiro, &config);
    assert_eq!(
        projected.get("profile_arn").map(String::as_str),
        Some("arn:aws:profile/example")
    );
    assert!(!projected.contains_key("client_secret"));
    session.save_login(&login).await.unwrap();
    let token = session.credentials(Provider::Kiro, &config).await.unwrap();
    assert_eq!(
        token.metadata.get("client_secret").map(String::as_str),
        Some("secret-value")
    );
    config.options.insert("region".into(), "us-west-2".into());
    assert!(matches!(
        session.credentials(Provider::Kiro, &config).await,
        Err(OAuthError::Configuration(_))
    ));
}

#[tokio::test]
async fn separate_kiro_accounts_keep_their_tokens_when_one_is_removed() {
    let session = OAuthSession::new(Arc::new(MemoryStore::default()), Arc::new(NoSecrets)).unwrap();
    let first = OAuthConfig::new(String::new());
    let second = OAuthConfig::new(String::new());
    assert_ne!(
        storage_key(Provider::Kiro, &first).unwrap(),
        storage_key(Provider::Kiro, &second).unwrap()
    );

    for (config, access) in [(&first, "kiro-one"), (&second, "kiro-two")] {
        session
            .save_login(&AuthorizedLogin {
                key: storage_key(Provider::Kiro, config).unwrap(),
                token: OAuthToken {
                    access_token: access.into(),
                    refresh_token: None,
                    expires_at: None,
                    metadata: Default::default(),
                },
            })
            .await
            .unwrap();
    }
    assert_eq!(
        session.access_token(Provider::Kiro, &first).await.unwrap(),
        "kiro-one"
    );
    assert_eq!(
        session.access_token(Provider::Kiro, &second).await.unwrap(),
        "kiro-two"
    );
    session.logout(Provider::Kiro, &first).await.unwrap();
    assert!(matches!(
        session.access_token(Provider::Kiro, &first).await,
        Err(OAuthError::Configuration(_))
    ));
    assert_eq!(
        session.access_token(Provider::Kiro, &second).await.unwrap(),
        "kiro-two"
    );
}

fn secret_config(provider_client: &str) -> OAuthConfig {
    let mut config = OAuthConfig::new(provider_client.into());
    config.client_secret_env = Some("FAKE_CLIENT_SECRET".into());
    config
}

struct FixedSecret(Option<&'static str>);
impl Credentials for FixedSecret {
    fn get(&self, _: &str) -> Option<String> {
        self.0.map(str::to_owned)
    }
}

#[tokio::test]
async fn begin_rejects_a_missing_or_blank_client_secret_before_any_flow_starts() {
    let config = secret_config("own-google-client");
    for value in [None, Some(""), Some("   ")] {
        let session = OAuthSession::new(
            Arc::new(MemoryStore::default()),
            Arc::new(FixedSecret(value)),
        )
        .unwrap();
        for provider in [Provider::GeminiApi, Provider::AzureOpenAi] {
            let error = session.begin(provider, &config).await.err().expect("fails");
            assert!(
                matches!(&error, OAuthError::Configuration(l)
                    if l.id == "oauth_error.session.client_secret_env_missing"),
                "unexpected error for {provider:?}: {error}"
            );
        }
    }
}

#[tokio::test]
async fn begin_accepts_a_present_client_secret_and_keeps_only_a_lookup() {
    let session = OAuthSession::new(
        Arc::new(MemoryStore::default()),
        Arc::new(FixedSecret(Some("fake-secret-value"))),
    )
    .unwrap();
    let config = secret_config("own-google-client");
    let login = session.begin(Provider::GeminiApi, &config).await.unwrap();
    assert!(login
        .prompt()
        .url
        .starts_with("https://accounts.google.com/"));
    assert!(!login.prompt().url.contains("fake-secret-value"));
}

#[tokio::test]
async fn refresh_still_requires_the_client_secret() {
    let session = OAuthSession::new(
        Arc::new(MemoryStore::default()),
        Arc::new(FixedSecret(None)),
    )
    .unwrap();
    let config = secret_config("own-google-client");
    session
        .save_login(&AuthorizedLogin {
            key: storage_key(Provider::GeminiApi, &config).unwrap(),
            token: OAuthToken {
                access_token: "old".into(),
                refresh_token: Some("refresh".into()),
                expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
                metadata: Default::default(),
            },
        })
        .await
        .unwrap();
    let error = session
        .access_token(Provider::GeminiApi, &config)
        .await
        .expect_err("fails");
    assert!(
        matches!(&error, OAuthError::Configuration(l)
            if l.id == "oauth_error.session.client_secret_env_missing"),
        "got: {error}"
    );
}

// ---------------------------------------------------------------------------
// Refresh-token rotation: the rotated token must never be dropped. Every
// token below is a fixed fake; the token endpoint is a local wiremock.
// ---------------------------------------------------------------------------

/// A memory store whose next `fail_saves` writes fail like a locked vault.
#[derive(Default)]
struct FlakyStore {
    inner: MemoryStore,
    fail_saves: Mutex<usize>,
}
impl FlakyStore {
    fn fail_next_saves(&self, count: usize) {
        *self.fail_saves.lock() = count;
    }
    fn refresh_token(&self, key: &str) -> Option<String> {
        self.inner
            .load(key)
            .unwrap()
            .and_then(|token| token.refresh_token)
    }
}
impl TokenStore for FlakyStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        self.inner.load(id)
    }
    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        let mut remaining = self.fail_saves.lock();
        if *remaining > 0 {
            *remaining -= 1;
            return Err(storage_error());
        }
        drop(remaining);
        self.inner.save(id, token)
    }
    fn delete(&self, id: &str) -> Result<(), OAuthError> {
        self.inner.delete(id)
    }
}

fn kiro_config() -> OAuthConfig {
    OAuthConfig::new(String::new())
}

/// A Kiro builder-id login whose access token expired an hour ago.
fn stale_kiro_token(refresh: &str) -> OAuthToken {
    OAuthToken {
        access_token: "fake-access-old".into(),
        refresh_token: Some(refresh.into()),
        expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
        metadata: [
            ("client_id".into(), "fake-client-id".into()),
            ("client_secret".into(), "fake-client-secret".into()),
            ("auth_method".into(), "builder-id".into()),
            ("region".into(), "us-east-1".into()),
            // Present, so the refresh never backfills it over the network.
            ("profile_arn".into(), "arn:aws:profile/example".into()),
        ]
        .into(),
    }
}

/// Refreshes go to the Kiro token endpoint of `server`.
fn kiro_refresher(server: &wiremock::MockServer) -> TestRefresher {
    let url = format!("{}/token", server.uri());
    Arc::new(move |http, token| {
        let url = url.clone();
        Box::pin(async move {
            super::super::reference_device::refresh_kiro_at(&http, &token, &url).await
        })
    })
}

/// The token endpoint rotates `old` into `new`, exactly `times` times.
async fn mount_rotation(server: &wiremock::MockServer, old: &str, new: &str, times: u64) {
    use wiremock::matchers::{body_string_contains, method, path};
    wiremock::Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(old))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"accessToken": "fake-access-new", "refreshToken": new, "expiresIn": 3600}),
        ))
        .expect(times)
        .mount(server)
        .await;
}

fn kiro_session(
    store: Arc<dyn TokenStore>,
    server: &wiremock::MockServer,
    config: &OAuthConfig,
) -> (OAuthSession, String) {
    let session = OAuthSession::new(store, Arc::new(NoSecrets))
        .unwrap()
        .with_test_refresher(kiro_refresher(server));
    let key = storage_key(Provider::Kiro, config).unwrap();
    (session, key)
}

#[tokio::test]
async fn a_rotation_the_vault_rejects_is_kept_and_written_on_the_next_call() {
    let server = wiremock::MockServer::start().await;
    mount_rotation(&server, "fake-refresh-old", "fake-refresh-new", 1).await;
    let store = Arc::new(FlakyStore::default());
    let config = kiro_config();
    let (session, key) = kiro_session(store.clone(), &server, &config);
    store
        .inner
        .save(&key, &stale_kiro_token("fake-refresh-old"))
        .unwrap();

    store.fail_next_saves(1);
    let error = session
        .credentials(Provider::Kiro, &config)
        .await
        .err()
        .expect("the vault write fails");
    assert!(matches!(error, OAuthError::Storage(_)), "got: {error}");
    assert!(!error.to_string().contains("fake-"));
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-old")
    );

    // The next call writes the kept rotation without a second refresh: the
    // old refresh token is already invalid at the provider.
    let token = session.credentials(Provider::Kiro, &config).await.unwrap();
    assert_eq!(token.access_token, "fake-access-new");
    assert_eq!(token.refresh_token.as_deref(), Some("fake-refresh-new"));
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-new")
    );
}

#[tokio::test]
async fn a_rotation_waiting_for_the_vault_survives_while_the_vault_stays_locked() {
    let server = wiremock::MockServer::start().await;
    mount_rotation(&server, "fake-refresh-old", "fake-refresh-new", 1).await;
    let store = Arc::new(FlakyStore::default());
    let config = kiro_config();
    let (session, key) = kiro_session(store.clone(), &server, &config);
    store
        .inner
        .save(&key, &stale_kiro_token("fake-refresh-old"))
        .unwrap();

    store.fail_next_saves(3);
    for _ in 0..3 {
        assert!(matches!(
            session.credentials(Provider::Kiro, &config).await,
            Err(OAuthError::Storage(_))
        ));
    }
    let loaded = session.load_by_key(&key).await.unwrap().unwrap();
    assert_eq!(loaded.refresh_token.as_deref(), Some("fake-refresh-new"));
    let token = session.credentials(Provider::Kiro, &config).await.unwrap();
    assert_eq!(token.refresh_token.as_deref(), Some("fake-refresh-new"));
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-new")
    );
}

#[tokio::test]
async fn a_login_rollback_restores_the_rotation_the_vault_had_rejected() {
    let server = wiremock::MockServer::start().await;
    mount_rotation(&server, "fake-refresh-old", "fake-refresh-new", 1).await;
    let store = Arc::new(FlakyStore::default());
    let config = kiro_config();
    let (session, key) = kiro_session(store.clone(), &server, &config);
    store
        .inner
        .save(&key, &stale_kiro_token("fake-refresh-old"))
        .unwrap();
    store.fail_next_saves(1);
    assert!(session.credentials(Provider::Kiro, &config).await.is_err());

    // A reauth replaces the credential, then its config save fails: the
    // rollback must restore the rotated token, not the invalidated one.
    let mut fresh = stale_kiro_token("fake-refresh-relogin");
    fresh.expires_at = None;
    let login = AuthorizedLogin::for_test(key.clone(), fresh);
    let prior = session
        .save_login_replacing(&login)
        .await
        .unwrap()
        .expect("a prior token");
    assert_eq!(prior.refresh_token.as_deref(), Some("fake-refresh-new"));
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-relogin")
    );
    session.restore_by_key(key.clone(), prior).await.unwrap();
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-new")
    );
}

#[tokio::test]
async fn a_new_login_supersedes_a_rotation_waiting_for_the_vault() {
    let server = wiremock::MockServer::start().await;
    mount_rotation(&server, "fake-refresh-old", "fake-refresh-new", 1).await;
    let store = Arc::new(FlakyStore::default());
    let config = kiro_config();
    let (session, key) = kiro_session(store.clone(), &server, &config);
    store
        .inner
        .save(&key, &stale_kiro_token("fake-refresh-old"))
        .unwrap();
    store.fail_next_saves(1);
    assert!(session.credentials(Provider::Kiro, &config).await.is_err());

    let mut fresh = stale_kiro_token("fake-refresh-relogin");
    fresh.expires_at = None;
    session
        .save_login(&AuthorizedLogin::for_test(key.clone(), fresh))
        .await
        .unwrap();
    let token = session.credentials(Provider::Kiro, &config).await.unwrap();
    assert_eq!(token.refresh_token.as_deref(), Some("fake-refresh-relogin"));
}

/// The token endpoint rotates `old` into `new` once, in an answer the client
/// rejects (`expiresIn` is not a number).
async fn mount_rejected_rotation(server: &wiremock::MockServer, old: &str, new: &str) {
    use wiremock::matchers::{body_string_contains, method, path};
    wiremock::Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(old))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"accessToken": "fake-access-new", "refreshToken": new, "expiresIn": "soon"}),
        ))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_rotation_in_a_rejected_answer_is_kept_for_the_next_refresh() {
    let server = wiremock::MockServer::start().await;
    mount_rejected_rotation(&server, "fake-refresh-old", "fake-refresh-mid").await;
    mount_rotation(&server, "fake-refresh-mid", "fake-refresh-new", 1).await;
    let store = Arc::new(FlakyStore::default());
    let config = kiro_config();
    let (session, key) = kiro_session(store.clone(), &server, &config);
    store
        .inner
        .save(&key, &stale_kiro_token("fake-refresh-old"))
        .unwrap();

    let error = session
        .credentials(Provider::Kiro, &config)
        .await
        .err()
        .expect("the answer is rejected");
    assert!(matches!(error, OAuthError::InvalidResponse), "got: {error}");
    assert!(!error.to_string().contains("fake-"));
    // The rotated token is stored with the expired access token, so the next
    // call refreshes with it instead of the invalidated one.
    let stored = store.inner.load(&key).unwrap().unwrap();
    assert_eq!(stored.refresh_token.as_deref(), Some("fake-refresh-mid"));
    assert_eq!(stored.access_token, "fake-access-old");
    assert_eq!(
        stored.metadata.get("client_id").map(String::as_str),
        Some("fake-client-id")
    );

    let token = session.credentials(Provider::Kiro, &config).await.unwrap();
    assert_eq!(token.access_token, "fake-access-new");
    assert_eq!(token.refresh_token.as_deref(), Some("fake-refresh-new"));
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-new")
    );
}

#[tokio::test]
async fn a_rotation_in_a_rejected_answer_waits_in_memory_when_the_vault_fails() {
    let server = wiremock::MockServer::start().await;
    mount_rejected_rotation(&server, "fake-refresh-old", "fake-refresh-mid").await;
    mount_rotation(&server, "fake-refresh-mid", "fake-refresh-new", 1).await;
    let store = Arc::new(FlakyStore::default());
    let config = kiro_config();
    let (session, key) = kiro_session(store.clone(), &server, &config);
    store
        .inner
        .save(&key, &stale_kiro_token("fake-refresh-old"))
        .unwrap();

    store.fail_next_saves(1);
    // The refresh error wins over the vault error.
    assert!(matches!(
        session.credentials(Provider::Kiro, &config).await,
        Err(OAuthError::InvalidResponse)
    ));
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-old")
    );
    let token = session.credentials(Provider::Kiro, &config).await.unwrap();
    assert_eq!(token.refresh_token.as_deref(), Some("fake-refresh-new"));
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-new")
    );
}

#[tokio::test]
async fn a_rejected_answer_without_a_new_refresh_token_changes_nothing() {
    use wiremock::matchers::{method, path};
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"accessToken": "fake-access-new", "refreshToken": "fake-refresh-old", "expiresIn": "soon"}),
        ))
        .mount(&server)
        .await;
    let store = Arc::new(FlakyStore::default());
    let config = kiro_config();
    let (session, key) = kiro_session(store.clone(), &server, &config);
    store
        .inner
        .save(&key, &stale_kiro_token("fake-refresh-old"))
        .unwrap();
    // Any write would fail: none must be attempted.
    store.fail_next_saves(1);
    assert!(matches!(
        session.credentials(Provider::Kiro, &config).await,
        Err(OAuthError::InvalidResponse)
    ));
    assert!(session.unsaved_token(&key).is_none());
    assert_eq!(
        store.refresh_token(&key).as_deref(),
        Some("fake-refresh-old")
    );
}

/// Like [`mount_rotation`], with a slow answer so concurrent refreshes overlap.
async fn mount_slow_rotation(server: &wiremock::MockServer, old: &str, new: &str, times: u64) {
    use wiremock::matchers::{body_string_contains, method, path};
    wiremock::Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(old))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "accessToken": "fake-access-new", "refreshToken": new, "expiresIn": 3600
                }))
                .set_delay(std::time::Duration::from_millis(300)),
        )
        .expect(times)
        .mount(server)
        .await;
}

/// Two sessions stand for two processes (the TUI and the desktop app): each
/// has its own in-process lock, and they share only the vault and the lock
/// directory. One rotation reaches the provider; the other session uses its
/// stored result instead of sending the invalidated refresh token.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sessions_sharing_a_vault_rotate_a_stale_token_once() {
    let server = wiremock::MockServer::start().await;
    mount_slow_rotation(&server, "fake-refresh-old", "fake-refresh-new", 1).await;
    let locks = tempfile::tempdir().unwrap();
    let store: Arc<dyn TokenStore> = Arc::new(MemoryStore::default());
    let config = kiro_config();
    let open = || {
        OAuthSession::new(Arc::clone(&store), Arc::new(NoSecrets))
            .unwrap()
            .with_test_refresher(kiro_refresher(&server))
            .with_process_locks(locks.path().to_path_buf())
    };
    let (first, second) = (open(), open());
    let key = storage_key(Provider::Kiro, &config).unwrap();
    first
        .save_login(&AuthorizedLogin::for_test(
            key,
            stale_kiro_token("fake-refresh-old"),
        ))
        .await
        .unwrap();

    let (a, b) = tokio::join!(
        first.credentials(Provider::Kiro, &config),
        second.credentials(Provider::Kiro, &config)
    );
    for token in [a.unwrap(), b.unwrap()] {
        assert_eq!(token.access_token, "fake-access-new");
        assert_eq!(token.refresh_token.as_deref(), Some("fake-refresh-new"));
    }
}

/// The same race on the debug SQLite store: two sessions opened on one
/// database file, as two `run.sh` processes would, with the lock directory
/// the store places beside the database.
#[cfg(debug_assertions)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn development_sqlite_sessions_rotate_a_stale_token_once() {
    let server = wiremock::MockServer::start().await;
    mount_slow_rotation(&server, "fake-refresh-old", "fake-refresh-new", 1).await;
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("cuotai-dev.sqlite3");
    let config = kiro_config();
    let open = || {
        OAuthSession::development_sqlite(&database)
            .unwrap()
            .with_test_refresher(kiro_refresher(&server))
    };
    let (first, second) = (open(), open());
    let key = storage_key(Provider::Kiro, &config).unwrap();
    first
        .save_login(&AuthorizedLogin::for_test(
            key.clone(),
            stale_kiro_token("fake-refresh-old"),
        ))
        .await
        .unwrap();
    assert!(super::super::process_lock::development_dir(&database)
        .join(format!("{key}.lock"))
        .is_file());

    let (a, b) = tokio::join!(
        first.credentials(Provider::Kiro, &config),
        second.credentials(Provider::Kiro, &config)
    );
    for token in [a.unwrap(), b.unwrap()] {
        assert_eq!(token.refresh_token.as_deref(), Some("fake-refresh-new"));
    }
    let stored = second.load_by_key(&key).await.unwrap().unwrap();
    assert_eq!(stored.refresh_token.as_deref(), Some("fake-refresh-new"));
}

/// A fresh token is read without touching the lock directory, so a session
/// whose lock directory is unusable keeps serving fresh tokens.
#[tokio::test]
async fn a_fresh_token_needs_no_process_lock() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("file");
    std::fs::write(&blocker, b"").unwrap();
    let store: Arc<dyn TokenStore> = Arc::new(MemoryStore::default());
    let config = kiro_config();
    let key = storage_key(Provider::Kiro, &config).unwrap();
    let mut token = stale_kiro_token("fake-refresh-old");
    token.expires_at = None;
    store.save(&key, &token).unwrap();
    let session = OAuthSession::new(store, Arc::new(NoSecrets))
        .unwrap()
        .with_process_locks(blocker.join("locks"));
    assert_eq!(
        session.access_token(Provider::Kiro, &config).await.unwrap(),
        "fake-access-old"
    );
    assert!(!blocker.join("locks").exists());
}
