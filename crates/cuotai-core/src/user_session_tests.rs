use super::*;
use crate::config_file::ConfigFile;
use crate::oauth::{OAuthError, TokenStore};
use crate::{Monitor, Status};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

const SECRET: &str = "SECRET-SESSION-VALUE";

#[derive(Default)]
struct MemoryStore {
    entries: Mutex<HashMap<String, OAuthToken>>,
    fail_saves: bool,
}
impl TokenStore for MemoryStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        Ok(self.entries.lock().get(id).cloned())
    }
    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        if self.fail_saves {
            return Err(crate::oauth::session::storage_error());
        }
        self.entries.lock().insert(id.into(), token.clone());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), OAuthError> {
        self.entries.lock().remove(id);
        Ok(())
    }
}
impl MemoryStore {
    fn values(&self) -> Vec<String> {
        self.entries
            .lock()
            .values()
            .map(|token| token.access_token.clone())
            .collect()
    }
}

struct Env(Vec<(&'static str, &'static str)>);
impl Credentials for Env {
    fn get(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
    }
}

fn session(store: &Arc<MemoryStore>) -> OAuthSession {
    OAuthSession::new(store.clone(), Arc::new(Env(Vec::new()))).unwrap()
}

fn mimo(params: &[(&str, &str)]) -> Account {
    Account {
        name: "MiMo".into(),
        provider: Provider::XiaomiMimo,
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..Default::default()
    }
}

fn config(accounts: Vec<Account>) -> Config {
    Config {
        accounts,
        ..Config::default()
    }
}

/// A saved TOML whose MiMo account reads its platform session from
/// `MIMO_COOKIE`, as accounts did before PRD 0016.
fn legacy_file(dir: &tempfile::TempDir) -> ConfigFile {
    let path = dir.path().join("cuotai.toml");
    std::fs::write(
        &path,
        "# mine\n[[accounts]]\nname = \"MiMo\"\nprovider = \"xiaomi_mimo\"\nparams = { platform_session_env = \"MIMO_COOKIE\", region = \"sgp\" }\n",
    )
    .unwrap();
    ConfigFile::load(Some(path), false).unwrap()
}

#[tokio::test]
async fn migration_moves_the_session_into_the_vault_and_only_a_reference_into_the_toml() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = legacy_file(&dir);
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let env = Env(vec![(
        "MIMO_COOKIE",
        " api-platform_serviceToken=SECRET-SESSION-VALUE; userId=7 ",
    )]);
    assert_eq!(
        migration_candidates(&file.config.accounts[0])
            .iter()
            .map(|c| (c.slot, c.env.as_str()))
            .collect::<Vec<_>>(),
        [("platform_session_env", "MIMO_COOKIE")]
    );

    let staged = stage_migration(&oauth, &env, &file.config, 0)
        .await
        .unwrap();
    file.save(staged.config().clone()).unwrap();

    assert_eq!(
        store.values(),
        ["api-platform_serviceToken=SECRET-SESSION-VALUE; userId=7"]
    );
    let text = std::fs::read_to_string(&file.path).unwrap();
    assert!(!text.contains(SECRET), "{text}");
    assert!(!text.contains("MIMO_COOKIE"), "{text}");
    assert!(text.contains("credential_ref"), "{text}");
    let reloaded = ConfigFile::load(Some(file.path.clone()), false).unwrap();
    let account = &reloaded.config.accounts[0];
    assert_eq!(
        account.params.get("region").map(String::as_str),
        Some("sgp")
    );
    assert!(!account.params.contains_key("platform_session_env"));
    let stored = account.session.as_ref().expect("vault reference");
    assert_eq!(
        stored.slots.iter().collect::<Vec<_>>(),
        ["platform_session_env"]
    );
    assert!(migration_candidates(account).is_empty());
    assert!(!format!("{:?}", reloaded.config).contains(SECRET));
}

#[tokio::test]
async fn missing_variable_writes_nothing_and_names_only_the_variable() {
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let both = config(vec![mimo(&[
        ("session_env", "MIMO_SESSION"),
        ("platform_session_env", "MIMO_COOKIE"),
    ])]);
    // One of the two variables is set: nothing moves until both are.
    let env = Env(vec![("MIMO_COOKIE", SECRET), ("MIMO_SESSION", "  ")]);
    let error = stage_migration(&oauth, &env, &both, 0)
        .await
        .err()
        .expect("unset variable refuses");
    assert_eq!(error.id, "user_session.migrate.env_missing");
    assert!(error.fallback.contains("MIMO_SESSION"));
    assert!(!error.fallback.contains(SECRET));
    assert!(store.values().is_empty());

    let error = stage_migration(&oauth, &env, &config(vec![mimo(&[])]), 0)
        .await
        .err()
        .expect("nothing to migrate");
    assert_eq!(error.id, "user_session.migrate.nothing");
}

#[tokio::test]
async fn failed_vault_write_leaves_no_entry_and_rollback_undoes_a_staged_one() {
    let failing = Arc::new(MemoryStore {
        fail_saves: true,
        ..Default::default()
    });
    let env = Env(vec![("MIMO_COOKIE", SECRET)]);
    let legacy = config(vec![mimo(&[("platform_session_env", "MIMO_COOKIE")])]);
    let error = stage_migration(&session(&failing), &env, &legacy, 0)
        .await
        .err()
        .expect("vault refused");
    assert!(!error.fallback.contains(SECRET));
    assert!(failing.values().is_empty());

    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let staged = stage_migration(&oauth, &env, &legacy, 0).await.unwrap();
    assert_eq!(store.values(), [SECRET]);
    rollback_migration(&oauth, staged).await.unwrap();
    assert!(store.values().is_empty());
}

// An account that already keeps the slot in the vault and names a variable
// again: undoing the new move brings back the session it replaced.
#[tokio::test]
async fn rollback_restores_the_session_a_migration_replaced() {
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let mut account = mimo(&[("platform_session_env", "MIMO_COOKIE")]);
    let reference = SessionConfig {
        credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
        slots: ["platform_session_env".to_string()].into(),
    };
    let key = storage_key(account.provider, &reference, "platform_session_env").unwrap();
    store
        .entries
        .lock()
        .insert(key, token("OLD-SESSION".into()));
    account.session = Some(reference);
    let env = Env(vec![("MIMO_COOKIE", SECRET)]);

    let staged = stage_migration(&oauth, &env, &config(vec![account]), 0)
        .await
        .unwrap();
    assert_eq!(store.values(), [SECRET]);
    rollback_migration(&oauth, staged).await.unwrap();
    assert_eq!(store.values(), ["OLD-SESSION"]);
}

/// The account after a migration of `slot` holding `value`, its monitor and
/// the store behind both.
async fn migrated(slot: &'static str, value: &'static str) -> (Account, Monitor) {
    let store = Arc::new(MemoryStore::default());
    let oauth = Arc::new(session(&store));
    let legacy = config(vec![mimo(&[(slot, "MIMO_VAR"), ("region", "sgp")])]);
    let staged = stage_migration(&oauth, &Env(vec![("MIMO_VAR", value)]), &legacy, 0)
        .await
        .unwrap();
    let config = staged.config().clone();
    let account = config.accounts[0].clone();
    // The variable is gone from the process: only the vault can answer.
    let monitor = Monitor::with_oauth(config, Arc::new(Env(Vec::new())), oauth).unwrap();
    (account, monitor)
}

#[tokio::test]
async fn a_query_reads_the_vault_session_through_its_credentials() {
    let (account, monitor) = migrated("session_env", "serviceToken=SECRET-SESSION-VALUE").await;
    let read = monitor
        .with_access(
            &account,
            async |account: &Account, ctx: &crate::providers::Context<'_>| {
                let name = account.params.get("session_env").expect("slot param");
                Ok(ctx.credentials.get(name))
            },
        )
        .await
        .unwrap();
    assert_eq!(read.as_deref(), Some("serviceToken=SECRET-SESSION-VALUE"));
    // Another slot of the same reference was never stored.
    let other = monitor
        .with_access(
            &account,
            async |account: &Account, _: &crate::providers::Context<'_>| {
                Ok(account.params.get("platform_session_env").cloned())
            },
        )
        .await
        .unwrap();
    assert_eq!(other, None);
}

#[tokio::test]
async fn the_report_of_a_vault_session_never_carries_its_value() {
    // A platform cookie without the required fields fails before any
    // request; the adapter's message proves it read the vault value.
    let (_, monitor) = migrated("platform_session_env", "other=SECRET-SESSION-VALUE").await;
    let report = monitor.refresh_account(0).await;
    assert_eq!(report.status, Status::NeedsConfiguration);
    assert_eq!(
        report.notes_l10n[0].id,
        "query.mimo_platform.missing_cookie_fields"
    );
    let text = serde_json::to_string(&report).unwrap();
    assert!(!text.contains(SECRET), "{text}");
    assert!(!format!("{report:?}").contains(SECRET));
}

#[tokio::test]
async fn a_reference_without_its_vault_entry_asks_for_the_session_again() {
    let mut account = mimo(&[]);
    account.session = Some(SessionConfig {
        credential_ref: uuid::Uuid::new_v4().to_string(),
        slots: ["platform_session_env".to_string()].into(),
    });
    let store = Arc::new(MemoryStore::default());
    let monitor = Monitor::with_oauth(
        config(vec![account]),
        Arc::new(Env(Vec::new())),
        Arc::new(session(&store)),
    )
    .unwrap();
    let report = monitor.refresh_account(0).await;
    assert_eq!(report.status, Status::NeedsConfiguration);
    assert_eq!(report.notes_l10n[0].id, "user_session.not_found");
}

#[tokio::test]
async fn a_reference_moved_to_another_provider_or_slot_finds_nothing() {
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let staged = stage_migration(
        &oauth,
        &Env(vec![("MIMO_VAR", SECRET)]),
        &config(vec![mimo(&[("session_env", "MIMO_VAR")])]),
        0,
    )
    .await
    .unwrap();
    let reference = staged.config().accounts[0].session.clone().unwrap();
    let mut copied = Account {
        name: "Coding".into(),
        provider: Provider::AlibabaCodingPlan,
        session: Some(reference.clone()),
        ..Default::default()
    };
    assert_eq!(
        load(&oauth, &copied).await.err().unwrap().note.id,
        "user_session.not_found"
    );
    copied.provider = Provider::XiaomiMimo;
    copied.session = Some(SessionConfig {
        slots: ["platform_session_env".to_string()].into(),
        ..reference
    });
    assert_eq!(
        load(&oauth, &copied).await.err().unwrap().note.id,
        "user_session.not_found"
    );
}

#[test]
fn config_rejects_invalid_or_shared_session_references() {
    let reference = |slot: &str| SessionConfig {
        credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
        slots: [slot.to_string()].into(),
    };
    let with = |name: &str, session: SessionConfig| Account {
        name: name.into(),
        session: Some(session),
        ..mimo(&[])
    };
    assert!(config(vec![with("A", reference("session_env"))])
        .validate()
        .is_ok());
    for bad in [
        reference("unknown_slot"),
        SessionConfig {
            slots: BTreeSet::new(),
            ..reference("session_env")
        },
        SessionConfig {
            credential_ref: "not-a-uuid".into(),
            ..reference("session_env")
        },
    ] {
        assert!(matches!(
            config(vec![with("A", bad)]).validate(),
            Err(crate::ConfigError::Session)
        ));
    }
    assert!(matches!(
        config(vec![
            with("A", reference("session_env")),
            with("B", reference("platform_session_env")),
        ])
        .validate(),
        Err(crate::ConfigError::Session)
    ));
}

#[test]
fn editing_an_account_keeps_its_vault_reference_and_a_provider_switch_drops_it() {
    let mut account = mimo(&[]);
    account.session = Some(SessionConfig {
        credential_ref: uuid::Uuid::new_v4().to_string(),
        slots: ["session_env".to_string()].into(),
    });
    let base = config(vec![account.clone()]);
    let draft = |provider| crate::account_edit::AccountDraft {
        provider,
        name: "MiMo",
        credential: "XIAOMI_MIMO_API_KEY",
        auth_mode: crate::account_edit::AUTH_MODE_CREDENTIAL,
        oauth_mode: false,
        oauth_existing: None,
        oauth_authorized: None,
        oauth_login_params: BTreeMap::new(),
        existing_params: None,
        credential_params: None,
        fields: &[],
        interval_text: "",
    };
    let edited =
        crate::account_edit::build_account(&draft(Provider::XiaomiMimo), &base, Some(0)).unwrap();
    assert_eq!(edited.session, account.session);
    let switched =
        crate::account_edit::build_account(&draft(Provider::OpenCodeGo), &base, Some(0)).unwrap();
    assert_eq!(switched.session, None);
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn the_development_store_keeps_the_session_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("dev.sqlite3");
    let oauth = OAuthSession::development_sqlite(&db).unwrap();
    let staged = stage_migration(
        &oauth,
        &Env(vec![("MIMO_VAR", SECRET)]),
        &config(vec![mimo(&[("platform_session_env", "MIMO_VAR")])]),
        0,
    )
    .await
    .unwrap();
    let account = staged.config().accounts[0].clone();
    drop(oauth);
    let reopened = OAuthSession::development_sqlite(&db).unwrap();
    let sessions = load(&reopened, &account).await.unwrap();
    assert_eq!(sessions, [("platform_session_env", SECRET.to_string())]);
}
