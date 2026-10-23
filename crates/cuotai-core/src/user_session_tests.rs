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
    let env = Env(vec![("MIMO_COOKIE", SECRET)]);

    let staged = stage_migration(&oauth, &env, &config(vec![account]), 0)
        .await
        .unwrap();
    assert_eq!(store.values(), [SECRET]);
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
    // A platform cookie without the descriptor's required cookies fails
    // before any request; the message proves the vault value was read.
    let (_, monitor) = migrated("platform_session_env", "other=SECRET-SESSION-VALUE").await;
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
    let loaded: Vec<_> = sessions
        .iter()
        .map(|session| (session.slot, session.value.as_str()))
        .collect();
    assert_eq!(loaded, [("platform_session_env", SECRET)]);
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
        for locale in ["en-US", "es", "it"] {
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
    let staged = stage_pasted(&oauth, &file.config, 0, &[reduced])
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
    let saved = stage_pasted(&oauth, &config(vec![mimo(&[])]), 0, &[first])
        .await
        .unwrap()
        .config()
        .clone();
    let reference = saved.accounts[0].session.clone().unwrap();

    let table = "api-platform_serviceToken\tNEW-TOKEN\t.x\t/\t2026-12-01T00:00:00Z\nuserId\t7\t.x\t/\tSession";
    let second = reduce(Provider::XiaomiMimo, PLATFORM, table, now()).unwrap();
    let replaced = stage_pasted(&oauth, &saved, 0, &[second]).await.unwrap();

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
    let staged = stage_pasted(&oauth, &config(vec![mimo(&[])]), 0, &[reduced])
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
        accounts = stage_pasted(&oauth, &accounts, index, &[reduced])
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
    assert!(matches!(
        outcome,
        crate::account_edit::RemoveOutcome::RemovedNoCredential(_)
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
    let staged = stage_pasted(&oauth, &file.config, 1, &[reduced])
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
