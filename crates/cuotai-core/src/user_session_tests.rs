use super::*;
use crate::config_file::ConfigFile;
use crate::oauth::{OAuthError, TokenStore};
use crate::{Monitor, Status};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

const SECRET: &str = "SECRET-SESSION-VALUE";
/// A MiMo platform session carrying [`SECRET`] with its required cookies,
/// as a variable holds it for a migration (SES-02: reduced on the way).
const MIGRATED: &str = "api-platform_serviceToken=SECRET-SESSION-VALUE; userId=7";

#[derive(Default)]
struct MemoryStore {
    entries: Mutex<HashMap<String, OAuthToken>>,
    fail_saves: bool,
    fail_deletes: bool,
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
        if self.fail_deletes {
            return Err(crate::oauth::session::storage_error());
        }
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
        id: None,
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
    let env = Env(vec![("MIMO_COOKIE", MIGRATED)]);
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
    assert_eq!(store.values(), [MIGRATED]);
    rollback(&oauth, staged).await.unwrap();
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
        .insert(key, token("OLD-SESSION".into(), None));
    account.session = Some(reference);
    let env = Env(vec![("MIMO_COOKIE", MIGRATED)]);

    let staged = stage_migration(&oauth, &env, &config(vec![account]), 0)
        .await
        .unwrap();
    assert_eq!(store.values(), [MIGRATED]);
    rollback(&oauth, staged).await.unwrap();
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
    // A platform cookie without the descriptor's required cookies (stored
    // before SES-02 reduced migrations) fails before any request; the
    // message proves the vault value was read.
    let store = Arc::new(MemoryStore::default());
    let mut account = mimo(&[("region", "sgp")]);
    let reference = SessionConfig {
        credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
        slots: ["platform_session_env".to_string()].into(),
    };
    let key = storage_key(account.provider, &reference, "platform_session_env").unwrap();
    store
        .entries
        .lock()
        .insert(key, token("other=SECRET-SESSION-VALUE".into(), None));
    account.session = Some(reference);
    let monitor = Monitor::with_oauth(
        config(vec![account]),
        Arc::new(Env(Vec::new())),
        Arc::new(session(&store)),
    )
    .unwrap();
    let report = monitor.refresh_account(0).await;
    assert_eq!(report.status, Status::NeedsConfiguration);
    assert_eq!(report.notes_l10n[0].id, "query.session.missing_cookies");
    assert!(report.notes[0].contains("api-platform_serviceToken, userId"));
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
        &Env(vec![("MIMO_VAR", "serviceToken=SECRET-SESSION-VALUE")]),
        &config(vec![mimo(&[("session_env", "MIMO_VAR")])]),
        0,
    )
    .await
    .unwrap();
    let reference = staged.config().accounts[0].session.clone().unwrap();
    let mut copied = Account {
        id: None,
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
        group_text: None,
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
        &Env(vec![("MIMO_VAR", MIGRATED)]),
        &config(vec![mimo(&[("platform_session_env", "MIMO_VAR")])]),
        0,
    )
    .await
    .unwrap();
    let account = staged.config().accounts[0].clone();
    drop(oauth);
    let reopened = OAuthSession::development_sqlite(&db).unwrap();
    let sessions = load(&reopened, &account).await.unwrap();
    let loaded: Vec<_> = sessions
        .iter()
        .map(|session| (session.slot, session.value.as_str()))
        .collect();
    assert_eq!(loaded, [("platform_session_env", MIGRATED)]);
}

// ------------------------------------------------------------ RF2.2: paste

const PLATFORM: &str = "platform_session_env";
const KEPT: &str = "api-platform_serviceToken=SECRET-TOKEN; userId=7";

fn now() -> DateTime<Utc> {
    use chrono::TimeZone;
    Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap()
}

#[test]
fn every_paste_format_keeps_only_the_accepted_cookies() {
    let cases = [
        (
            "Cookie line",
            "Cookie: _ga=GA1.2.3; api-platform_serviceToken=SECRET-TOKEN; userId=7; tracking=DISCARDED-VALUE".to_string(),
        ),
        (
            "name=value list",
            "userId=7; tracking=DISCARDED-VALUE; api-platform_serviceToken=SECRET-TOKEN; _ga=x".into(),
        ),
        (
            "request headers",
            "Host: platform.xiaomimimo.com\nAccept: text/html;q=0.9\ncookie: api-platform_serviceToken=SECRET-TOKEN; tracking=DISCARDED-VALUE; userId=7; _ga=x\nUser-Agent: test".into(),
        ),
        (
            "inspector table",
            "Name\tValue\tDomain\tPath\tExpires / Max-Age\tSize\n\
             api-platform_serviceToken\tSECRET-TOKEN\t.xiaomimimo.com\t/\tSession\t38\n\
             userId\t7\t.xiaomimimo.com\t/\t2027-01-01T00:00:00.000Z\t7\n\
             tracking\tDISCARDED-VALUE\t.xiaomimimo.com\t/\tSession\t23\n\
             _ga\tx\t.xiaomimimo.com\t/\tSession\t4"
                .into(),
        ),
        (
            "cURL with -H",
            "curl 'https://platform.xiaomimimo.com/api/v1/balance' \\\n  -H 'accept: application/json' \\\n  -H 'cookie: api-platform_serviceToken=SECRET-TOKEN; userId=7; tracking=DISCARDED-VALUE; _ga=x' \\\n  --compressed".into(),
        ),
        (
            "cURL with -b",
            "curl \"https://platform.xiaomimimo.com/api/v1/balance\" -b \"tracking=DISCARDED-VALUE; userId=7; api-platform_serviceToken=SECRET-TOKEN; _ga=x\" -H \"accept: */*\"".into(),
        ),
    ];
    for (format, paste) in cases {
        // A terminal delivers a pasted line break as a carriage return;
        // Windows copies carry CRLF.
        for paste in [
            paste.clone(),
            paste.replace('\n', "\r"),
            paste.replace('\n', "\r\n"),
        ] {
            let reduced = reduce(Provider::XiaomiMimo, PLATFORM, &paste, now())
                .unwrap_or_else(|error| panic!("{format} {paste:?}: {}", error.fallback));
            assert_eq!(reduced.cookie(), KEPT, "{format}");
            assert_eq!(
                reduced.kept(),
                ["api-platform_serviceToken", "userId"],
                "{format}"
            );
            assert_eq!(reduced.discarded(), 2, "{format}");
            let shown = format!("{reduced:?} {}", reduced.summary(now()).resolve("en-US"));
            assert!(
                !shown.contains("SECRET") && !shown.contains("DISCARDED"),
                "{format}: {shown}"
            );
        }
    }
}

#[test]
fn fragments_out_of_order_are_reassembled_and_the_expiry_is_the_earliest_known() {
    let reduced = reduce(
        Provider::XiaomiMimo,
        "session_env",
        "serviceToken.2=CC; userId=9; serviceToken.0=AA; other.0=x; serviceToken.1=BB",
        now(),
    )
    .unwrap();
    assert_eq!(reduced.cookie(), "serviceToken=AABBCC; userId=9");
    assert_eq!(reduced.discarded(), 1);
    assert_eq!(reduced.expires_at(), None);

    // The table's expiry of the required cookie bounds the session; an
    // optional cookie expiring sooner does not.
    let table = "serviceToken.1\tBB\t.x\t/\tTue, 03 Nov 2026 10:00:00 GMT\n\
                 serviceToken.0\tAA\t.x\t/\t2026-11-20T00:00:00Z\n\
                 userId\t9\t.x\t/\t2026-10-22T00:00:00Z";
    let reduced = reduce(Provider::XiaomiMimo, "session_env", table, now()).unwrap();
    assert_eq!(reduced.cookie(), "serviceToken=AABB; userId=9");
    let at = DateTime::parse_from_rfc3339("2026-11-03T10:00:00Z").unwrap();
    assert_eq!(reduced.expires_at(), Some(at.with_timezone(&Utc)));
    assert_eq!(expiry(at.with_timezone(&Utc), now()).days, 12);
}

#[test]
fn malformed_pastes_name_the_cookie_and_never_a_value() {
    let expired = "api-platform_serviceToken\tSECRET-A\t.x\t/\t2026-10-01T00:00:00Z\n\
                   userId\tSECRET-B\t.x\t/\tSession";
    let cases = [
        (
            Provider::XiaomiMimo,
            PLATFORM,
            "userId=SECRET-A; other=SECRET-B",
            "user_session.paste.missing",
            "api-platform_serviceToken",
        ),
        (
            Provider::XiaomiMimo,
            "session_env",
            "serviceToken.0=SECRET-A; serviceToken.2=SECRET-B",
            "user_session.paste.fragment_missing",
            "serviceToken",
        ),
        (
            Provider::XiaomiMimo,
            "session_env",
            "serviceToken=SECRET-A; serviceToken=SECRET-B",
            "user_session.paste.duplicate",
            "serviceToken",
        ),
        (
            Provider::XiaomiMimo,
            "session_env",
            "serviceToken=SECRET-A; serviceToken.0=SECRET-B",
            "user_session.paste.duplicate",
            "serviceToken",
        ),
        (
            Provider::XiaomiMimo,
            PLATFORM,
            "api-platform_serviceToken=; userId=SECRET-A",
            "user_session.paste.invalid_value",
            "api-platform_serviceToken",
        ),
        (
            Provider::AlibabaTokenPlan,
            "session_env",
            "_ga=SECRET-A; foo=SECRET-B",
            "user_session.paste.none_accepted",
            "login_aliyunid_ticket",
        ),
        (
            Provider::XiaomiMimo,
            PLATFORM,
            "Host: SECRET-A\nAccept: SECRET-B",
            "user_session.paste.empty",
            "",
        ),
        (
            Provider::XiaomiMimo,
            PLATFORM,
            expired,
            "user_session.paste.expired",
            "2026-10-01",
        ),
    ];
    for (provider, slot, paste, id, named) in cases {
        let error = reduce(provider, slot, paste, now()).expect_err(id);
        assert_eq!(error.id, id, "{paste}");
        assert!(error.fallback.contains(named), "{}", error.fallback);
        for locale in ["pt-BR", "en-US", "es", "it"] {
            let text = error.resolve(locale);
            assert!(!text.contains("SECRET"), "{locale}: {text}");
            assert!(!format!("{error:?}").contains("SECRET"));
        }
        for locale in ["pt-BR", "es", "it"] {
            let catalog = crate::text::core_catalog(locale).unwrap();
            assert!(catalog.contains_key(id), "{locale} lacks {id}");
        }
    }
}

#[tokio::test]
async fn only_the_reduced_cookies_reach_the_vault_and_the_toml_keeps_the_reference() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = legacy_file(&dir);
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let reduced = reduce(
        Provider::XiaomiMimo,
        PLATFORM,
        "Cookie: api-platform_serviceToken=SECRET-TOKEN; userId=7; tracking=DISCARDED-VALUE",
        now(),
    )
    .unwrap();
    let staged = stage_pasted(&oauth, &file.config, 0, &[reduced], Some(&cleared()))
        .await
        .unwrap();
    file.save(staged.config().clone()).unwrap();

    assert_eq!(store.values(), [KEPT]);
    let text = std::fs::read_to_string(&file.path).unwrap();
    for absent in ["SECRET", "DISCARDED", "MIMO_COOKIE"] {
        assert!(!text.contains(absent), "{absent}: {text}");
    }
    assert!(text.contains("credential_ref"), "{text}");
}

#[tokio::test]
async fn a_new_paste_replaces_the_session_in_the_same_vault_entry() {
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let first = reduce(Provider::XiaomiMimo, PLATFORM, KEPT, now()).unwrap();
    let saved = stage_pasted(
        &oauth,
        &config(vec![mimo(&[])]),
        0,
        &[first],
        Some(&cleared()),
    )
    .await
    .unwrap()
    .config()
    .clone();
    let reference = saved.accounts[0].session.clone().unwrap();

    let table = "api-platform_serviceToken\tNEW-TOKEN\t.x\t/\t2026-12-01T00:00:00Z\nuserId\t7\t.x\t/\tSession";
    let second = reduce(Provider::XiaomiMimo, PLATFORM, table, now()).unwrap();
    let replaced = stage_pasted(&oauth, &saved, 0, &[second], Some(&cleared()))
        .await
        .unwrap();

    assert_eq!(replaced.config().accounts.len(), 1);
    assert_eq!(
        replaced.config().accounts[0].session.as_ref(),
        Some(&reference)
    );
    let entries = store.entries.lock();
    assert_eq!(entries.len(), 1);
    let token = entries.values().next().unwrap();
    assert_eq!(
        token.access_token,
        "api-platform_serviceToken=NEW-TOKEN; userId=7"
    );
    assert_eq!(
        token.expires_at,
        Some(
            DateTime::parse_from_rfc3339("2026-12-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        )
    );
}

// --------------------------------------------- RF3: rejection, hosts, expiry

/// A MiMo account whose platform session was pasted from `paste`, and its
/// monitor (no environment variable can answer).
async fn pasted(paste: &str) -> (Account, Monitor) {
    let store = Arc::new(MemoryStore::default());
    let oauth = Arc::new(session(&store));
    let reduced = reduce(Provider::XiaomiMimo, PLATFORM, paste, Utc::now()).unwrap();
    let staged = stage_pasted(
        &oauth,
        &config(vec![mimo(&[])]),
        0,
        &[reduced],
        Some(&cleared()),
    )
    .await
    .unwrap();
    let config = staged.config().clone();
    let account = config.accounts[0].clone();
    let monitor = Monitor::with_oauth(config, Arc::new(Env(Vec::new())), oauth).unwrap();
    (account, monitor)
}

/// The platform query of `account` against `base`, its session read from
/// the query's credentials as the adapter reads it in production.
async fn query_platform(
    monitor: &Monitor,
    account: &Account,
    pasted: &[ReducedSession],
    base: &str,
) -> (
    Result<crate::providers::Data, crate::providers::QueryError>,
    Option<DateTime<Utc>>,
) {
    monitor
        .with_sessions(
            account,
            pasted,
            async |account: &Account, ctx: &crate::providers::Context<'_>| {
                let name = account.params.get(PLATFORM).expect("slot param");
                let cookie = ctx.credentials.get(name).expect("session");
                crate::providers::mimo_platform::fetch(ctx, base, &cookie).await
            },
        )
        .await
}

async fn balance_server() -> wiremock::MockServer {
    use wiremock::matchers::{header, method, path};
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("GET"))
        .and(path("/balance"))
        .and(header("Cookie", KEPT))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "balance": "12.50", "currency": "USD" }
            })),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_rejected_session_asks_for_a_new_paste_without_body_or_url() {
    use wiremock::matchers::method;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(401).set_body_string("SECRET-BODY"))
        .mount(&server)
        .await;
    let (account, monitor) = pasted(KEPT).await;
    let (result, _) = query_platform(&monitor, &account, &[], &server.uri()).await;
    let error = result.expect_err("401");
    assert_eq!(error.status, Status::AuthenticationError);
    assert_eq!(error.note.id, "user_session.rejected");
    let text = format!("{} {:?}", error.note.resolve("en-US"), error.note);
    assert!(
        !text.contains("SECRET") && !text.contains(&server.uri()),
        "{text}"
    );
}

#[tokio::test]
async fn a_redirect_to_another_host_never_carries_the_session() {
    use wiremock::matchers::{any, method};
    let elsewhere = wiremock::MockServer::start().await;
    wiremock::Mock::given(any())
        .respond_with(wiremock::ResponseTemplate::new(200))
        .expect(0)
        .mount(&elsewhere)
        .await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("GET"))
        .respond_with(
            wiremock::ResponseTemplate::new(302)
                .insert_header("Location", format!("{}/login", elsewhere.uri())),
        )
        .mount(&server)
        .await;
    let (account, monitor) = pasted(KEPT).await;
    let (result, _) = query_platform(&monitor, &account, &[], &server.uri()).await;
    let error = result.expect_err("redirect");
    assert_eq!(error.status, Status::AuthenticationError);
    assert_eq!(error.note.id, "user_session.rejected");
    assert!(!error.note.fallback.contains(&elsewhere.uri()));
    // `elsewhere` verifies on drop that it received nothing.
}

#[tokio::test]
async fn the_session_only_goes_to_its_declared_hosts_over_https() {
    let allowed = [
        (
            Provider::XiaomiMimo,
            PLATFORM,
            "https://platform.xiaomimimo.com/api/v1/balance",
        ),
        (
            Provider::XiaomiMimo,
            "session_env",
            "https://mimo-server-sgp.xiaomimimo.com/api/user/usage",
        ),
        (
            Provider::AlibabaCodingPlan,
            "session_env",
            "https://bailian.console.aliyun.com/data/api.json",
        ),
        (
            Provider::AlibabaCodingPlan,
            "session_env",
            "https://bailian-cs.console.aliyun.com/data/api.json",
        ),
        (
            Provider::AlibabaCodingPlan,
            "session_env",
            "https://modelstudio.console.alibabacloud.com/tool/user/info.json",
        ),
        (
            Provider::AlibabaCodingPlan,
            "session_env",
            "https://bailian-singapore-cs.alibabacloud.com/data/api.json",
        ),
        (
            Provider::AlibabaTokenPlan,
            "session_env",
            "https://home.qwencloud.com/tool/user/info.json",
        ),
        (
            Provider::AlibabaTokenPlan,
            "session_env",
            "https://cs-data.qwencloud.com/data/api.json",
        ),
    ];
    for (provider, slot, url) in allowed {
        assert!(
            may_receive(provider, slot, &url::Url::parse(url).unwrap()),
            "{url}"
        );
    }
    let refused = [
        // Plain HTTP to the declared host, another host, a look-alike, and
        // the other slot's or provider's host.
        (
            Provider::XiaomiMimo,
            PLATFORM,
            "http://platform.xiaomimimo.com/api/v1/balance",
        ),
        (
            Provider::XiaomiMimo,
            PLATFORM,
            "https://evil.example/api/v1/balance",
        ),
        (
            Provider::XiaomiMimo,
            PLATFORM,
            "https://platform.xiaomimimo.com.evil.example/",
        ),
        (
            Provider::XiaomiMimo,
            PLATFORM,
            "https://mimo-server-sgp.xiaomimimo.com/api/user/usage",
        ),
        (
            Provider::AlibabaTokenPlan,
            "session_env",
            "https://bailian.console.aliyun.com/data/api.json",
        ),
        (
            Provider::OpenAiApi,
            "session_env",
            "https://platform.xiaomimimo.com/",
        ),
    ];
    for (provider, slot, url) in refused {
        assert!(
            !may_receive(provider, slot, &url::Url::parse(url).unwrap()),
            "{url}"
        );
    }
    let http = reqwest::Client::new();
    let error = crate::providers::send_session(
        http.get("https://evil.example/").header("Cookie", KEPT),
        Provider::XiaomiMimo,
        PLATFORM,
    )
    .await
    .expect_err("refused before the network");
    assert_eq!(error.note.id, "query.session_host_refused");
}

#[tokio::test]
async fn a_known_expiry_reaches_the_report_and_an_unknown_one_stays_unknown() {
    let server = balance_server().await;
    let at = Utc::now() + chrono::TimeDelta::days(2);
    let table = format!(
        "api-platform_serviceToken\tSECRET-TOKEN\t.x\t/\t{}\nuserId\t7\t.x\t/\tSession",
        at.to_rfc3339()
    );
    let (account, monitor) = pasted(&table).await;
    let (result, expires_at) = query_platform(&monitor, &account, &[], &server.uri()).await;
    result.expect("balance");
    let expires_at = expires_at.expect("known expiry");
    assert_eq!(expires_at.timestamp(), at.timestamp());
    let expiry = expiry(expires_at, Utc::now());
    assert!(expiry.soon);
    assert_eq!(expiry.label.id, "user_session.expiry.one_day");

    let (account, monitor) = pasted(KEPT).await;
    let (result, expires_at) = query_platform(&monitor, &account, &[], &server.uri()).await;
    result.expect("balance");
    assert_eq!(expires_at, None);
}

#[tokio::test]
async fn a_test_query_uses_the_paste_and_stores_nothing() {
    let server = balance_server().await;
    let store = Arc::new(MemoryStore::default());
    let draft = mimo(&[]);
    let monitor = Monitor::with_oauth(
        config(vec![draft.clone()]),
        Arc::new(Env(Vec::new())),
        Arc::new(session(&store)),
    )
    .unwrap();
    let paste = reduce(Provider::XiaomiMimo, PLATFORM, KEPT, now()).unwrap();
    let (result, _) = query_platform(&monitor, &draft, &[paste], &server.uri()).await;
    result.expect("pasted session answered");
    assert!(store.values().is_empty());
    let requests_before_invalid = server.received_requests().await.unwrap().len();
    // A genuinely invalid draft is still refused without a query. A missing
    // name is valid for a lone provider under PRD 0029 RF0, so use a polling
    // interval below the existing boundary instead.
    let mut invalid = draft;
    invalid.refresh_seconds = Some(1);
    let report = monitor
        .test_session(
            &invalid,
            &[reduce(Provider::XiaomiMimo, PLATFORM, KEPT, now()).unwrap()],
        )
        .await;
    assert_eq!(report.status, Status::NeedsConfiguration);
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        requests_before_invalid,
        "an invalid draft must not reach the provider"
    );
}

// ------------------------------------------- RF4: warning and forgetting

#[test]
fn the_card_warns_about_a_vault_or_variable_session_and_nothing_else() {
    let mut vault = mimo(&[]);
    vault.session = Some(SessionConfig {
        credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
        slots: [PLATFORM.to_string()].into(),
    });
    let notice = notice(&vault).expect("vault session");
    assert_eq!(notice.level.id, "access.credential.user_session");
    assert_eq!(notice.warning.id, "user_session.warning");
    assert_eq!(
        notice.warning.resolve("en-US"),
        "This is a full login session. Cuotai stores it in the system vault and uses it only to query this service."
    );
    assert_eq!(
        super::notice(&mimo(&[(PLATFORM, "MIMO_COOKIE")]))
            .expect("variable session")
            .warning
            .id,
        "user_session.warning_env"
    );
    // An API key alone, a blank variable or a provider without sessions.
    assert_eq!(super::notice(&mimo(&[(PLATFORM, " ")])), None);
    let openai = Account {
        id: None,
        provider: Provider::OpenAiApi,
        params: [(PLATFORM.to_string(), "X".to_string())].into(),
        ..Default::default()
    };
    assert_eq!(super::notice(&openai), None);
    assert_eq!(paste_notice(Provider::OpenAiApi), None);
    assert_eq!(
        paste_notice(Provider::AlibabaTokenPlan).map(|n| n.warning.id),
        Some("user_session.warning".into())
    );
}

/// Two MiMo accounts with their own pasted platform sessions, saved.
async fn two_pasted(store: &Arc<MemoryStore>, dir: &tempfile::TempDir) -> ConfigFile {
    let oauth = session(store);
    let path = dir.path().join("cuotai.toml");
    std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();
    let mut file = ConfigFile::load(Some(path), false).unwrap();
    let mut accounts = config(vec![
        mimo(&[]),
        Account {
            name: "Other".into(),
            ..mimo(&[])
        },
    ]);
    for (index, token) in [(0, "SECRET-A"), (1, "SECRET-B")] {
        let paste = format!("api-platform_serviceToken={token}; userId=7");
        let reduced = reduce(Provider::XiaomiMimo, PLATFORM, &paste, now()).unwrap();
        accounts = stage_pasted(&oauth, &accounts, index, &[reduced], Some(&cleared()))
            .await
            .unwrap()
            .config()
            .clone();
    }
    file.save(accounts).unwrap();
    file
}

#[tokio::test]
async fn forgetting_deletes_the_vault_entry_and_the_reference_of_that_account_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let mut file = two_pasted(&store, &dir).await;
    let kept = file.config.accounts[1].session.clone();

    let staged = stage_forget(&oauth, &file.config, 0).await.unwrap();
    file.save(staged.config().clone()).unwrap();

    assert_eq!(
        store.values(),
        ["api-platform_serviceToken=SECRET-B; userId=7"]
    );
    let reloaded = ConfigFile::load(Some(file.path.clone()), false).unwrap();
    assert_eq!(reloaded.config.accounts[0].session, None);
    assert_eq!(reloaded.config.accounts[1].session, kept);
    assert_eq!(super::notice(&reloaded.config.accounts[0]), None);
    // Nothing left to forget.
    let error = stage_forget(&oauth, &reloaded.config, 0)
        .await
        .err()
        .expect("no session");
    assert_eq!(error.id, "user_session.forget.nothing");
    assert_eq!(store.values().len(), 1);
}

/// Every vault entry as key, value and expiry.
fn stored(store: &MemoryStore) -> BTreeMap<String, (String, Option<DateTime<Utc>>)> {
    store
        .entries
        .lock()
        .iter()
        .map(|(key, token)| (key.clone(), (token.access_token.clone(), token.expires_at)))
        .collect()
}

#[tokio::test]
async fn a_failed_save_brings_the_forgotten_session_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let file = two_pasted(&store, &dir).await;
    let before = stored(&store);

    let staged = stage_forget(&oauth, &file.config, 0).await.unwrap();
    assert_eq!(store.values().len(), 1);
    rollback(&oauth, staged).await.unwrap();
    assert_eq!(stored(&store), before);
    // The account reads its session again.
    assert_eq!(
        load(&oauth, &file.config.accounts[0]).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn a_refused_vault_delete_keeps_the_session_and_names_no_value() {
    let dir = tempfile::tempdir().unwrap();
    let seeded = Arc::new(MemoryStore::default());
    let file = two_pasted(&seeded, &dir).await;
    let failing = Arc::new(MemoryStore {
        entries: Mutex::new(seeded.entries.lock().clone()),
        fail_deletes: true,
        ..Default::default()
    });
    let error = stage_forget(&session(&failing), &file.config, 0)
        .await
        .err()
        .expect("vault refused");
    assert_eq!(failing.values().len(), 2);
    let text = format!("{} {error:?}", error.resolve("en-US"));
    assert!(!text.contains("SECRET"), "{text}");
}

/// FIX-06 / PRD 0016 acceptance: removing an account deletes its vault
/// session (and only its own), in the same operation as the TOML save.
#[tokio::test]
async fn removing_an_account_deletes_its_vault_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let mut file = two_pasted(&store, &dir).await;
    let kept = file.config.accounts[1].session.clone();

    let outcome = crate::account_edit::remove_account(&mut file, &oauth, 0, false)
        .await
        .unwrap();
    // PRD 0029 RF5.3: the deleted session is reported on its own.
    assert!(matches!(
        outcome,
        crate::account_edit::RemoveOutcome::RemovedNoCredential(
            crate::account_edit::RemovedAccount {
                sessions_removed: true,
                ..
            }
        )
    ));
    assert_eq!(
        store.values(),
        ["api-platform_serviceToken=SECRET-B; userId=7"]
    );
    let reloaded = ConfigFile::load(Some(file.path.clone()), false).unwrap();
    assert_eq!(reloaded.config.accounts.len(), 1);
    assert_eq!(reloaded.config.accounts[0].session, kept);
}

/// FIX-06: a removal whose TOML save fails puts the deleted session back,
/// so the account (still in the file) reads it as before.
#[tokio::test]
async fn a_removal_whose_save_fails_keeps_the_vault_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let mut file = two_pasted(&store, &dir).await;
    let before = stored(&store);
    let original = file.path.clone();
    file.path = dir.path().join("missing-parent").join("cuotai.toml");

    let Err(error) = crate::account_edit::remove_account(&mut file, &oauth, 0, true).await else {
        panic!("the save fails");
    };
    assert!(matches!(error, crate::account_edit::RemoveError::Save(_)));
    assert_eq!(stored(&store), before);
    file.path = original;
    let reloaded = ConfigFile::load(Some(file.path.clone()), false).unwrap();
    assert_eq!(reloaded.config.accounts.len(), 2);
    assert_eq!(
        load(&oauth, &reloaded.config.accounts[0])
            .await
            .unwrap()
            .len(),
        1
    );
}

/// FIX-06: a vault that refuses the delete fails the removal before
/// anything is saved, without a session value in the error.
#[tokio::test]
async fn a_refused_delete_fails_the_removal_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let seeded = Arc::new(MemoryStore::default());
    let mut file = two_pasted(&seeded, &dir).await;
    let failing = Arc::new(MemoryStore {
        entries: Mutex::new(seeded.entries.lock().clone()),
        fail_deletes: true,
        ..Default::default()
    });
    let Err(error) =
        crate::account_edit::remove_account(&mut file, &session(&failing), 0, true).await
    else {
        panic!("vault refused");
    };
    assert!(matches!(
        error,
        crate::account_edit::RemoveError::Session(_)
    ));
    assert_eq!(failing.values().len(), 2);
    assert_eq!(file.config.accounts.len(), 2);
    let reloaded = ConfigFile::load(Some(file.path.clone()), false).unwrap();
    assert_eq!(reloaded.config.accounts.len(), 2);
    let text = format!("{} {error:?}", error.l10n().resolve("en-US"));
    assert!(!text.contains("SECRET"), "{text}");
    assert!(text.contains("not removed"), "{text}");
}

/// FIX-06: a provider switch drops the account's session reference
/// (`build_account`); saving it deletes the entry, and a rollback (the save
/// failed) brings it back together with any paste staged with it.
#[tokio::test]
async fn a_provider_switch_deletes_the_dropped_session_and_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let file = two_pasted(&store, &dir).await;
    let before = stored(&store);
    let mut switched = file.config.clone();
    switched.accounts[0].provider = Provider::AlibabaCodingPlan;
    switched.accounts[0].session = None;

    let staged = stage_dropped(&oauth, &file.config, &switched, None)
        .await
        .unwrap()
        .expect("one entry dropped");
    assert_eq!(staged.config().accounts, switched.accounts);
    assert_eq!(
        store.values(),
        ["api-platform_serviceToken=SECRET-B; userId=7"]
    );
    rollback(&oauth, staged).await.unwrap();
    assert_eq!(stored(&store), before);

    // Nothing dropped: nothing staged, the vault untouched.
    assert!(stage_dropped(&oauth, &file.config, &file.config, None)
        .await
        .unwrap()
        .is_none());
    assert_eq!(stored(&store), before);
}

/// FIX-06: a failed drop undoes its own deletions and the paste handed to
/// it, so the caller aborts with the vault as it was.
#[tokio::test]
async fn a_failed_drop_undoes_the_paste_it_was_given() {
    let dir = tempfile::tempdir().unwrap();
    let seeded = Arc::new(MemoryStore::default());
    let file = two_pasted(&seeded, &dir).await;
    let failing = Arc::new(MemoryStore {
        entries: Mutex::new(seeded.entries.lock().clone()),
        fail_deletes: true,
        ..Default::default()
    });
    let oauth = session(&failing);
    let before = stored(&failing);
    // A paste for the second account, then a switch of the first.
    let paste = "api-platform_serviceToken=SECRET-C; userId=7";
    let reduced = reduce(Provider::XiaomiMimo, PLATFORM, paste, now()).unwrap();
    let staged = stage_pasted(&oauth, &file.config, 1, &[reduced], Some(&cleared()))
        .await
        .unwrap();
    let mut after = staged.config().clone();
    after.accounts[0].provider = Provider::AlibabaCodingPlan;
    after.accounts[0].session = None;
    assert!(stage_dropped(&oauth, &file.config, &after, Some(staged))
        .await
        .is_err());
    assert_eq!(stored(&failing), before, "the paste was undone too");
}

#[tokio::test]
async fn forgetting_a_reference_without_its_entry_only_drops_the_reference() {
    let store = Arc::new(MemoryStore::default());
    let mut account = mimo(&[]);
    account.session = Some(SessionConfig {
        credential_ref: uuid::Uuid::new_v4().to_string(),
        slots: [PLATFORM.to_string()].into(),
    });
    let staged = stage_forget(&session(&store), &config(vec![account]), 0)
        .await
        .unwrap();
    assert_eq!(staged.config().accounts[0].session, None);
    assert!(store.entries.lock().is_empty());
}

// ------------------------------------- SES-01, SES-02: test before save

/// A clearance for the tests that exercise the vault rather than the test
/// before save.
fn cleared() -> SaveClearance {
    SaveClearance { _private: () }
}

/// The test report `Monitor::test_session` builds from one query result.
fn test_report(
    account: &Account,
    result: Result<crate::providers::Data, crate::providers::QueryError>,
) -> crate::Report {
    let (status, metrics, notes_l10n) = match result {
        Ok(data) => (Status::Ready, data.metrics, data.notes_l10n),
        Err(error) => (error.status, Vec::new(), vec![error.note]),
    };
    crate::Report {
        account_id: None,
        diagnostic: None,
        account: account.name.clone(),
        provider: account.provider,
        checked_at: now(),
        status,
        metrics,
        notes: notes_l10n.iter().map(|n| n.fallback.to_string()).collect(),
        notes_l10n,
        source: String::new(),
        session_expires_at: None,
        sub_reports: Vec::new(),
        as_of: None,
        overage_hint: None,
    }
}

/// A MiMo draft without a vault session, its store and vault, its pasted
/// session (extra cookies included) and the test report of that paste
/// against `base`.
async fn tested(
    paste: &str,
    base: &str,
) -> (
    Account,
    Arc<MemoryStore>,
    Arc<OAuthSession>,
    ReducedSession,
    crate::Report,
) {
    let store = Arc::new(MemoryStore::default());
    let oauth = Arc::new(session(&store));
    let draft = mimo(&[]);
    let monitor = Monitor::with_oauth(
        config(vec![draft.clone()]),
        Arc::new(Env(Vec::new())),
        Arc::clone(&oauth),
    )
    .unwrap();
    let probe = reduce(Provider::XiaomiMimo, PLATFORM, paste, now()).unwrap();
    let (result, _) = query_platform(&monitor, &draft, &[probe], base).await;
    let report = test_report(&draft, result);
    let reduced = reduce(Provider::XiaomiMimo, PLATFORM, paste, now()).unwrap();
    (draft, store, oauth, reduced, report)
}

fn refusal_text(refused: &SaveRefused) -> String {
    ["pt-BR", "en-US", "es", "it"]
        .iter()
        .map(|locale| refused.note.resolve(locale))
        .chain([format!("{:?}", refused.note)])
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::test]
async fn a_passed_test_clears_the_save_and_only_the_descriptor_cookies_reach_the_vault() {
    let server = balance_server().await;
    let (draft, store, oauth, reduced, report) = tested(
        &format!("Cookie: {KEPT}; _ga=TRACKER; JSESSIONID=OTHER"),
        &server.uri(),
    )
    .await;
    assert_eq!(SessionCheck::of(&report), SessionCheck::Passed);
    let clearance = clear_for_save(&report, false).expect("passed test clears the save");
    stage_pasted(
        &oauth,
        &config(vec![draft]),
        0,
        &[reduced],
        Some(&clearance),
    )
    .await
    .unwrap();
    assert_eq!(store.values(), [KEPT]);
}

#[tokio::test]
async fn a_rejected_test_blocks_the_save_even_with_save_anyway() {
    use wiremock::matchers::method;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(401).set_body_string("SECRET-BODY"))
        .mount(&server)
        .await;
    let (draft, store, oauth, reduced, report) = tested(KEPT, &server.uri()).await;
    assert_eq!(report.status, Status::AuthenticationError);
    assert_eq!(SessionCheck::of(&report), SessionCheck::Rejected);
    for save_anyway in [false, true] {
        let refused = clear_for_save(&report, save_anyway).expect_err("rejected session");
        assert_eq!(refused.note.id, "user_session.save.rejected");
        assert!(!refused.save_anyway);
        let text = refusal_text(&refused);
        assert!(
            text.contains("revoked") && text.contains("revogada"),
            "{text}"
        );
        for absent in ["SECRET", server.uri().as_str()] {
            assert!(!text.contains(absent), "{absent}: {text}");
        }
    }
    let error = stage_pasted(&oauth, &config(vec![draft]), 0, &[reduced], None)
        .await
        .err()
        .expect("no clearance, no save");
    assert_eq!(error.id, "user_session.save.untested");
    assert!(store.values().is_empty());
}

#[tokio::test]
async fn a_transport_failure_offers_save_anyway_and_only_then_saves() {
    // A loopback port nothing listens on (wiremock pools its servers, so a
    // dropped one may still answer).
    let unreachable = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let (draft, store, oauth, reduced, report) = tested(KEPT, &unreachable).await;
    assert_eq!(
        SessionCheck::of(&report),
        SessionCheck::Unreachable,
        "{report:?}"
    );
    let refused = clear_for_save(&report, false).expect_err("not without the explicit choice");
    assert_eq!(refused.note.id, "user_session.save.unreachable");
    assert!(refused.save_anyway);
    let text = refusal_text(&refused);
    assert!(
        !text.contains("SECRET") && !text.contains(&unreachable),
        "{text}"
    );
    assert!(store.values().is_empty());

    let clearance = clear_for_save(&report, true).expect("save anyway after a transport failure");
    stage_pasted(
        &oauth,
        &config(vec![draft]),
        0,
        &[reduced],
        Some(&clearance),
    )
    .await
    .unwrap();
    assert_eq!(store.values(), [KEPT]);
}

#[tokio::test]
async fn an_incompatible_schema_blocks_the_save() {
    use wiremock::matchers::method;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("<html>SECRET</html>"))
        .mount(&server)
        .await;
    let (_, store, _, _, report) = tested(KEPT, &server.uri()).await;
    assert_eq!(report.status, Status::Error);
    assert_eq!(SessionCheck::of(&report), SessionCheck::Rejected);
    let refused = clear_for_save(&report, true).expect_err("schema failure");
    assert_eq!(refused.note.id, "user_session.save.rejected");
    assert!(!refused.save_anyway);
    assert!(!refusal_text(&refused).contains("SECRET"));
    assert!(store.values().is_empty());
}

#[test]
fn a_timeout_is_a_transport_failure_and_a_refused_endpoint_is_not() {
    let draft = mimo(&[]);
    let report = test_report(&draft, Err(crate::providers::QueryError::timeout()));
    assert_eq!(SessionCheck::of(&report), SessionCheck::Unreachable);
    let report = test_report(&draft, Err(crate::providers::QueryError::insecure()));
    assert_eq!(SessionCheck::of(&report), SessionCheck::Rejected);
}

#[tokio::test]
async fn the_test_before_save_refuses_an_invalid_draft_and_needs_nothing_without_a_paste() {
    let store = Arc::new(MemoryStore::default());
    let draft = mimo(&[]);
    let monitor = Monitor::with_oauth(
        config(vec![draft.clone()]),
        Arc::new(Env(Vec::new())),
        Arc::new(session(&store)),
    )
    .unwrap();
    assert!(test_before_save(&monitor, &draft, &[], false)
        .await
        .unwrap()
        .is_none());
    let mut invalid = draft;
    invalid.refresh_seconds = Some(1);
    let paste = reduce(Provider::XiaomiMimo, PLATFORM, KEPT, now()).unwrap();
    let refused = test_before_save(&monitor, &invalid, &[paste], true)
        .await
        .expect_err("an invalid draft is never saved");
    assert_eq!(refused.note.id, "user_session.save.rejected");
    assert!(!refused.save_anyway);
}

// SES-02 / RF5 decision: an Alibaba paste keeps only the descriptor's
// cookies, never the console's whole cookie jar.
#[tokio::test]
async fn an_alibaba_paste_never_stores_the_whole_cookie_jar() {
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let account = Account {
        id: None,
        name: "Coding".into(),
        provider: Provider::AlibabaCodingPlan,
        ..Default::default()
    };
    let jar = "Cookie: aliyun_choice=intl; login_aliyunid_ticket=TICKET; _ga=GA1; cna=CNA; JSESSIONID=J; aliyun_lang=en";
    let reduced = reduce(Provider::AlibabaCodingPlan, "session_env", jar, now()).unwrap();
    assert_eq!(reduced.discarded(), 4);
    stage_pasted(
        &oauth,
        &config(vec![account]),
        0,
        &[reduced],
        Some(&cleared()),
    )
    .await
    .unwrap();
    assert_eq!(store.values(), ["login_aliyunid_ticket=TICKET; cna=CNA"]);
}

#[tokio::test]
async fn a_migrated_variable_keeps_only_the_descriptor_cookies() {
    let store = Arc::new(MemoryStore::default());
    let oauth = session(&store);
    let env = Env(vec![(
        "MIMO_COOKIE",
        "_ga=TRACKER; api-platform_serviceToken=SECRET-TOKEN; userId=7; other=JAR",
    )]);
    let legacy = config(vec![mimo(&[(PLATFORM, "MIMO_COOKIE")])]);
    stage_migration(&oauth, &env, &legacy, 0).await.unwrap();
    assert_eq!(store.values(), [KEPT]);
}

#[test]
fn only_a_failed_vault_session_offers_a_new_paste() {
    let draft = mimo(&[]);
    let failed = |provider: Provider, status: Status, id: &'static str| crate::Report {
        provider,
        ..test_report(
            &draft,
            Err(crate::providers::QueryError {
                diagnostic: None,
                status,
                note: Localized::fixed(id, "x"),
            }),
        )
    };
    for id in [
        "user_session.rejected",
        "user_session.not_found",
        "query.session.missing_cookies",
    ] {
        for provider in [Provider::XiaomiMimo, Provider::AlibabaTokenPlan] {
            assert!(offers_new_paste(&failed(
                provider,
                Status::AuthenticationError,
                id
            )));
        }
    }
    assert!(!offers_new_paste(&failed(
        Provider::XiaomiMimo,
        Status::Error,
        "query.transport"
    )));
    assert!(!offers_new_paste(&failed(
        Provider::OpenAiApi,
        Status::AuthenticationError,
        "user_session.rejected"
    )));
    let mut ready = failed(
        Provider::XiaomiMimo,
        Status::AuthenticationError,
        "user_session.rejected",
    );
    ready.status = Status::Ready;
    assert!(!offers_new_paste(&ready));
}

// SES-02 review: each pasted slot is tested on its own, so a MiMo account
// session (which the adapter prefers) never vouches for, nor blocks, a
// pasted platform session.
#[test]
fn each_pasted_slot_is_tested_without_the_accounts_other_sessions() {
    let mut account = mimo(&[("session_env", "MIMO_ACCOUNT"), ("region", "sgp")]);
    account.session = Some(SessionConfig {
        credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
        slots: ["session_env".to_string(), PLATFORM.to_string()].into(),
    });
    let platform = only_slot(&account, PLATFORM);
    assert!(!platform.params.contains_key("session_env"));
    assert_eq!(
        platform.params.get("region").map(String::as_str),
        Some("sgp")
    );
    assert_eq!(
        platform
            .session
            .as_ref()
            .map(|s| s.slots.iter().cloned().collect::<Vec<_>>()),
        Some(vec![PLATFORM.to_string()])
    );
    let mut plain = mimo(&[("session_env", "MIMO_ACCOUNT")]);
    plain.session = None;
    let probe = only_slot(&plain, PLATFORM);
    assert!(probe.params.is_empty() && probe.session.is_none());
    let account_slot = only_slot(&account, "session_env");
    assert_eq!(
        account_slot.params.get("session_env").map(String::as_str),
        Some("MIMO_ACCOUNT")
    );
}

#[tokio::test]
async fn the_pasted_test_reports_the_worst_slot() {
    let store = Arc::new(MemoryStore::default());
    let mut invalid = mimo(&[]);
    invalid.refresh_seconds = Some(1);
    let monitor = Monitor::with_oauth(
        config(vec![mimo(&[])]),
        Arc::new(Env(Vec::new())),
        Arc::new(session(&store)),
    )
    .unwrap();
    let pastes = [
        reduce(Provider::XiaomiMimo, PLATFORM, KEPT, now()).unwrap(),
        reduce(
            Provider::XiaomiMimo,
            "session_env",
            "serviceToken=SECRET-SESSION-VALUE",
            now(),
        )
        .unwrap(),
    ];
    let report = test_pasted(&monitor, &invalid, &pastes).await;
    assert_eq!(report.status, Status::NeedsConfiguration);
    assert_eq!(SessionCheck::of(&report), SessionCheck::Rejected);
    assert!(store.values().is_empty());
}

// ------------------------------------------- PRD 0036 RF2.1 / RF2.6 (aggregators)

fn nine_router(base: &str) -> Account {
    Account {
        id: None,
        name: "9router".into(),
        provider: Provider::NineRouter,
        params: [("base_url".to_string(), base.to_string())].into(),
        ..Default::default()
    }
}

fn jwt(exp: i64) -> String {
    use base64::Engine as _;
    let encode = |value: serde_json::Value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_string())
    };
    format!(
        "{}.{}.sig",
        encode(serde_json::json!({"alg": "HS256"})),
        encode(serde_json::json!({"exp": exp, "sub": "SECRET-SUBJECT"}))
    )
}

fn empty_page() -> serde_json::Value {
    serde_json::json!({
        "connections": [], "providerOptions": [],
        "pagination": {"page": 1, "pageSize": 100, "totalPages": 1}, "totals": {}
    })
}

#[test]
fn a_pasted_secret_is_kept_whole_and_a_bad_one_is_never_echoed() {
    let kept = reduce(
        Provider::NineRouter,
        CREDENTIAL_SLOT,
        "  pw-SECRET-1 \n",
        now(),
    )
    .unwrap();
    assert_eq!(kept.cookie(), "pw-SECRET-1");
    assert_eq!(kept.expires_at(), None);
    assert!(!format!("{kept:?}").contains("SECRET"));
    for bad in ["", "   ", "pw\u{7}SECRET"] {
        let error = reduce(Provider::LiteLlm, CREDENTIAL_SLOT, bad, now()).unwrap_err();
        assert_eq!(error.id, "user_session.paste.secret_invalid");
        assert!(!error.fallback.contains("SECRET"));
    }
}

#[test]
fn the_sso_session_keeps_only_auth_token_and_dates_it_from_the_jwt() {
    let exp = now().timestamp() + 7_200;
    let token = jwt(exp);
    let paste = format!("Cookie: theme=dark; auth_token={token}; other=1");
    let reduced = reduce(Provider::NineRouter, NINE_ROUTER_SSO_SLOT, &paste, now()).unwrap();
    assert_eq!(reduced.cookie(), format!("auth_token={token}"));
    assert_eq!(reduced.expires_at().map(|at| at.timestamp()), Some(exp));
    let expired = format!("auth_token={}", jwt(now().timestamp() - 60));
    assert_eq!(
        reduce(Provider::NineRouter, NINE_ROUTER_SSO_SLOT, &expired, now())
            .unwrap_err()
            .id,
        "user_session.paste.expired"
    );
}

#[test]
fn account_host_sessions_never_go_through_the_declared_host_check() {
    for slot in [CREDENTIAL_SLOT, NINE_ROUTER_SSO_SLOT] {
        let descriptor = descriptor(Provider::NineRouter, slot).unwrap();
        assert!(descriptor.account_host && descriptor.hosts.is_empty());
        let url = url::Url::parse("https://example.com/api").unwrap();
        assert!(!may_receive(Provider::NineRouter, slot, &url));
    }
}

#[test]
fn a_password_variable_can_move_to_the_vault_and_names_the_base_host() {
    let mut account = nine_router("http://127.0.0.1:20128");
    account.credential_env = Some("NR_PASSWORD".into());
    let candidates = migration_candidates(&account);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].slot, CREDENTIAL_SLOT);
    assert_eq!(candidates[0].env, "NR_PASSWORD");
    assert!(candidates[0].consent.fallback.contains("127.0.0.1"));
}

#[test]
fn config_refuses_a_vault_password_next_to_a_password_variable() {
    let mut account = nine_router("http://127.0.0.1:20128");
    account.session = Some(SessionConfig {
        credential_ref: uuid::Uuid::new_v4().to_string(),
        slots: [CREDENTIAL_SLOT.to_string()].into(),
    });
    assert!(config(vec![account.clone()]).validate().is_ok());
    account.credential_env = Some("NR_PASSWORD".into());
    assert!(matches!(
        config(vec![account]).validate(),
        Err(crate::ConfigError::Session)
    ));
}

#[tokio::test]
async fn a_vault_password_logs_in_and_never_reaches_the_toml_or_the_report() {
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    crate::providers::nine_router::forget_login(&format!("{}/", server.uri())).await;
    let token = jwt(Utc::now().timestamp() + 86_400);
    Mock::given(method("POST"))
        .and(path("/api/auth/login"))
        .and(body_json(
            serde_json::json!({"password": "pw-SECRET-VAULT"}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", format!("auth_token={token}; Path=/"))
                .set_body_json(serde_json::json!({"success": true})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/providers/client"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_page()))
        .mount(&server)
        .await;
    let mut legacy = nine_router(&server.uri());
    legacy.credential_env = Some("NR_PASSWORD".into());
    let store = Arc::new(MemoryStore::default());
    let oauth = Arc::new(session(&store));
    let staged = stage_migration(
        &oauth,
        &Env(vec![("NR_PASSWORD", "pw-SECRET-VAULT")]),
        &config(vec![legacy]),
        0,
    )
    .await
    .unwrap();
    let saved = staged.config().clone();
    assert_eq!(saved.accounts[0].credential_env, None);
    assert_eq!(store.values(), ["pw-SECRET-VAULT"]);
    let toml = toml::to_string(&saved).unwrap();
    assert!(
        !toml.contains("SECRET") && !toml.contains("NR_PASSWORD"),
        "{toml}"
    );

    let monitor = Monitor::with_oauth(saved, Arc::new(Env(Vec::new())), oauth).unwrap();
    let snapshot = monitor.refresh().await;
    let report = &snapshot.reports[0];
    assert_eq!(report.status, Status::Ready, "{:?}", report.notes);
    let text = serde_json::to_string(report).unwrap();
    assert!(!text.contains("SECRET"), "{text}");
}

#[tokio::test]
async fn a_pasted_sso_session_replaces_the_login_and_a_rejected_one_says_so() {
    use crate::aggregators::TestOutcome;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let token = jwt(Utc::now().timestamp() + 3_600);
    let paste = format!("auth_token={token}");
    let reduced = || {
        reduce(
            Provider::NineRouter,
            NINE_ROUTER_SSO_SLOT,
            &paste,
            Utc::now(),
        )
        .unwrap()
    };
    let monitor_for = |account: &Account| {
        let store = Arc::new(MemoryStore::default());
        Monitor::with_oauth(
            config(vec![account.clone()]),
            Arc::new(Env(Vec::new())),
            Arc::new(session(&store)),
        )
        .unwrap()
    };

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(403))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/providers/client"))
        .and(header("cookie", paste.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_page()))
        .mount(&server)
        .await;
    let account = nine_router(&server.uri());
    let monitor = monitor_for(&account);
    let test = monitor.test_nine_router_with(&account, &[reduced()]).await;
    assert!(test.is_ok(), "{:?}", test.outcome);
    let report = monitor.test_session(&account, &[reduced()]).await;
    assert_eq!(report.status, Status::Ready, "{:?}", report.notes);
    assert!(report.session_expires_at.is_some());
    assert!(!serde_json::to_string(&report).unwrap().contains(&token));

    let revoked = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&revoked)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/providers/client"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&revoked)
        .await;
    let account = nine_router(&revoked.uri());
    let monitor = monitor_for(&account);
    let test = monitor.test_nine_router_with(&account, &[reduced()]).await;
    assert_eq!(test.outcome, TestOutcome::SessionRejected);
    let report = monitor.test_session(&account, &[reduced()]).await;
    assert_eq!(report.status, Status::AuthenticationError);
    assert!(report
        .notes_l10n
        .iter()
        .any(|note| note.id == "user_session.rejected"));
}
