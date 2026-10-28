use super::*;
use crate::account_edit::{build_account, build_account_with_secrets, AccountDraft};
use crate::config_file::ConfigFile;
use crate::oauth::{OAuthError, OAuthSession, OAuthToken, TokenStore};
use crate::user_session::{
    rollback, stage_pasted, stage_vault_migration, test_before_save, SaveClearance,
};
use crate::{Config, Credentials, Monitor, Status};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

const KEY: &str = "gsk_TYPED-SECRET-VALUE";

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

fn typed(slot: &str, value: &str) -> SecretValue {
    SecretValue {
        slot: slot.into(),
        value: value.into(),
    }
}

fn draft<'a>(provider: Provider, credential: &'a str) -> AccountDraft<'a> {
    AccountDraft {
        provider,
        name: "",
        credential,
        auth_mode: account_edit::AUTH_MODE_CREDENTIAL,
        oauth_mode: false,
        oauth_existing: None,
        oauth_authorized: None,
        oauth_login_params: Default::default(),
        existing_params: None,
        credential_params: None,
        fields: &[],
        interval_text: "",
        group_text: None,
    }
}

/// A Groq monitor over `config` whose usage endpoint answers only the
/// typed key, plus its vault.
async fn groq(
    config: Config,
    env: Env,
    store: Arc<MemoryStore>,
) -> (wiremock::MockServer, Monitor, Arc<OAuthSession>) {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::header(
            "authorization",
            format!("Bearer {KEY}").as_str(),
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })),
        )
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let creds: Arc<dyn Credentials> = Arc::new(env);
    let oauth = Arc::new(OAuthSession::new(store, Arc::clone(&creds)).unwrap());
    let mut monitor = Monitor::with_login_session(config, creds, Arc::clone(&oauth)).unwrap();
    monitor.groq_url = Some(server.uri());
    (server, monitor, oauth)
}

fn file_with(dir: &tempfile::TempDir, toml: &str) -> ConfigFile {
    let path = dir.path().join("cuotai.toml");
    std::fs::write(&path, toml).unwrap();
    ConfigFile::load(Some(path), false).unwrap()
}

/// G1: no input the GUI renders names a variable.
#[test]
fn gui_inputs_never_mention_an_environment_variable() {
    for provider in Provider::ALL {
        let inputs = secret_inputs(provider);
        if has_credential(provider) {
            assert_eq!(inputs[0].slot, CREDENTIAL_SLOT, "{provider:?}");
            assert_eq!(inputs[0].default_env, account_edit::default_env(provider));
        }
        // Messages name the provider and the input's position only: the
        // inputs describe secrets, so their contents stay out of the output.
        let mut slots = BTreeSet::new();
        for (at, input) in inputs.iter().enumerate() {
            assert!(slots.insert(input.slot), "{provider:?} input {at}");
            assert!(
                user_session::vault_descriptor(provider, input.slot).is_some(),
                "{provider:?} input {at}"
            );
            for text in [&input.label.fallback, &input.help.fallback] {
                let lower = text.to_lowercase();
                assert!(
                    !lower.contains("variable") && !lower.contains("_env") && !text.contains("$"),
                    "{provider:?} input {at} names a variable"
                );
            }
        }
    }
    assert!(secret_inputs(Provider::AwsBedrock).is_empty());
    assert!(secret_inputs(Provider::Extension).is_empty());
    let cloud: Vec<_> = secret_inputs(Provider::AlibabaCloud)
        .iter()
        .map(|i| i.slot)
        .collect();
    assert_eq!(cloud, [CREDENTIAL_SLOT, "secret_env", "security_token_env"]);
    let mimo = secret_inputs(Provider::XiaomiMimo);
    assert!(mimo
        .iter()
        .any(|i| i.slot == "platform_session_env" && i.cookie));
}

#[test]
fn typed_values_are_checked_and_never_printed() {
    let value = typed(CREDENTIAL_SLOT, KEY);
    assert!(!format!("{value:?}").contains(KEY));
    let unknown = reduce_secrets(Provider::Groq, &[typed("profile", KEY)]).unwrap_err();
    assert_eq!(unknown.id, "secret_input.unknown_slot");
    let twice = reduce_secrets(
        Provider::Groq,
        &[typed(CREDENTIAL_SLOT, KEY), typed(CREDENTIAL_SLOT, KEY)],
    )
    .unwrap_err();
    assert!(!format!("{twice:?}").contains(KEY));
    let bad = reduce_secrets(Provider::Groq, &[typed(CREDENTIAL_SLOT, "a\u{7}b")]).unwrap_err();
    assert!(!format!("{bad:?}").contains("a\u{7}b"));
    assert!(
        reduce_secrets(Provider::Groq, &[typed(CREDENTIAL_SLOT, "  ")])
            .unwrap()
            .is_empty()
    );
    let reduced = reduce_secrets(Provider::Groq, &[typed(CREDENTIAL_SLOT, " gsk_x ")]).unwrap();
    assert_eq!(reduced[0].cookie(), "gsk_x");
    assert!(!format!("{:?}", reduced[0]).contains("gsk_x"));
}

/// G2/G3/G4: a typed key is tested, written to the vault, referenced by
/// the TOML and read by the query; it never reaches the file, the errors
/// or the report.
#[tokio::test]
async fn a_typed_key_is_tested_vaulted_and_queried_without_reaching_the_toml() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = file_with(&dir, "# mine\naccounts = []\n");
    let store = Arc::new(MemoryStore::default());
    let (_server, monitor, oauth) =
        groq(file.config.clone(), Env(Vec::new()), Arc::clone(&store)).await;

    // Without the vault slot, the draft still needs a variable name.
    assert!(build_account(&draft(Provider::Groq, ""), &file.config, None).is_err());
    let secrets = reduce_secrets(Provider::Groq, &[typed(CREDENTIAL_SLOT, KEY)]).unwrap();
    let account = build_account_with_secrets(
        &draft(Provider::Groq, "GROQ_API_KEY"),
        &file.config,
        None,
        &[CREDENTIAL_SLOT],
    )
    .unwrap();
    assert_eq!(account.credential_env, None);

    // SES-02: a wrong key is refused before anything is stored.
    let wrong = reduce_secrets(Provider::Groq, &[typed(CREDENTIAL_SLOT, "gsk_WRONG")]).unwrap();
    let refused = test_before_save(&monitor, &account, &wrong, true)
        .await
        .unwrap_err();
    assert!(!refused.save_anyway);
    assert!(!format!("{:?}", refused.note).contains("gsk_WRONG"));
    assert!(store.values().is_empty());

    let clearance = test_before_save(&monitor, &account, &secrets, false)
        .await
        .unwrap();
    let updated = file.config.upsert_account(account, None).unwrap();
    let staged = stage_pasted(&oauth, &updated, 0, &secrets, clearance.as_ref())
        .await
        .unwrap();
    file.save(staged.config().clone()).unwrap();

    assert_eq!(store.values(), [KEY]);
    let text = std::fs::read_to_string(&file.path).unwrap();
    assert!(!text.contains(KEY), "{text}");
    assert!(!text.contains("credential_env ="), "{text}");
    assert!(text.contains("credential_ref"), "{text}");

    let reloaded = ConfigFile::load(Some(file.path.clone()), false).unwrap();
    let saved = &reloaded.config.accounts[0];
    assert_eq!(secret_states(saved)[0].source, SecretSource::Vault);
    let monitor = monitor.with_config(reloaded.config.clone()).unwrap();
    let report = monitor.refresh_account(0).await;
    assert_eq!(report.status, Status::Ready, "the vault key authenticates");
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains(KEY));
    assert!(
        user_session::notice(saved).is_none(),
        "an API key is no web session"
    );

    // Editing the account keeps the vault key without asking for it again.
    let edited =
        build_account_with_secrets(&draft(Provider::Groq, ""), &reloaded.config, Some(0), &[])
            .unwrap();
    assert_eq!(edited.credential_env, None);
    assert_eq!(edited.session, saved.session);
}

/// G2: a failed vault write stores nothing and the error names no value;
/// a failed TOML write is undone by the rollback.
#[tokio::test]
async fn a_failed_save_leaves_no_secret_behind() {
    let failing = Arc::new(MemoryStore {
        fail_saves: true,
        ..Default::default()
    });
    let oauth = OAuthSession::new(failing.clone(), Arc::new(Env(Vec::new()))).unwrap();
    let secrets = reduce_secrets(Provider::Groq, &[typed(CREDENTIAL_SLOT, KEY)]).unwrap();
    let account = build_account_with_secrets(
        &draft(Provider::Groq, ""),
        &empty(),
        None,
        &[CREDENTIAL_SLOT],
    )
    .unwrap();
    let config = empty().upsert_account(account, None).unwrap();
    let clearance = SaveClearance::for_tests();
    let error = stage_pasted(&oauth, &config, 0, &secrets, Some(&clearance))
        .await
        .err()
        .unwrap();
    assert!(!format!("{error:?}").contains(KEY));
    assert!(failing.values().is_empty());

    let store = Arc::new(MemoryStore::default());
    let oauth = OAuthSession::new(store.clone(), Arc::new(Env(Vec::new()))).unwrap();
    let staged = stage_pasted(&oauth, &config, 0, &secrets, Some(&clearance))
        .await
        .unwrap();
    assert_eq!(store.values(), [KEY]);
    rollback(&oauth, staged).await.unwrap();
    assert!(store.values().is_empty());
}

/// G5/T1: an account configured in the terminal keeps working unchanged,
/// reads as "environment" and moves to the vault only on request.
#[tokio::test]
async fn an_environment_account_works_unchanged_and_moves_to_the_vault() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = file_with(
        &dir,
        "[[accounts]]\nname = \"alpha\"\nprovider = \"groq\"\ncredential_env = \"GROQ_KEY_ALPHA\"\n",
    );
    let store = Arc::new(MemoryStore::default());
    let (_server, monitor, oauth) = groq(
        file.config.clone(),
        Env(vec![("GROQ_KEY_ALPHA", KEY)]),
        Arc::clone(&store),
    )
    .await;
    assert_eq!(monitor.refresh_account(0).await.status, Status::Ready);
    let account = &file.config.accounts[0];
    let states = secret_states(account);
    assert_eq!(states[0].source, SecretSource::Environment);
    assert!(!format!("{states:?}").contains("GROQ_KEY_ALPHA"));
    let candidates = vault_migration_candidates(account);
    assert_eq!(candidates.len(), 1);
    assert!(!candidates[0].consent.fallback.contains("GROQ_KEY_ALPHA"));
    // TUI path unchanged: the classic migration lists no API key.
    assert!(user_session::migration_candidates(account).is_empty());

    // An unset variable stores nothing.
    let missing = stage_vault_migration(&oauth, &Env(Vec::new()), &file.config, 0)
        .await
        .err()
        .unwrap();
    assert_eq!(missing.id, "user_session.migrate.env_missing");
    assert!(store.values().is_empty());

    let staged =
        stage_vault_migration(&oauth, &Env(vec![("GROQ_KEY_ALPHA", KEY)]), &file.config, 0)
            .await
            .unwrap();
    file.save(staged.config().clone()).unwrap();
    let text = std::fs::read_to_string(&file.path).unwrap();
    assert!(
        !text.contains(KEY) && !text.contains("GROQ_KEY_ALPHA"),
        "{text}"
    );
    assert_eq!(store.values(), [KEY]);
    let migrated = &file.config.accounts[0];
    assert!(vault_migration_candidates(migrated).is_empty());
    assert_eq!(secret_states(migrated)[0].source, SecretSource::Vault);
    // The vault key works without the variable.
    let (_server, monitor, _) = groq(file.config.clone(), Env(Vec::new()), store).await;
    assert_eq!(monitor.refresh_account(0).await.status, Status::Ready);
}

/// A parameter secret typed in the GUI is no longer required as a
/// variable name and is not written to `params`.
#[test]
fn a_typed_parameter_secret_replaces_its_variable_field() {
    use account_edit::{DraftField, DraftValue};
    let fields = [DraftField {
        key: "secret_env",
        value: DraftValue::Text("SOME_NAME".into()),
    }];
    let mut cloud = draft(Provider::AlibabaCloud, "");
    cloud.fields = &fields;
    let account =
        build_account_with_secrets(&cloud, &empty(), None, &[CREDENTIAL_SLOT, "secret_env"])
            .unwrap();
    assert_eq!(account.credential_env, None);
    assert!(!account.params.contains_key("secret_env"));
}

/// T2: a second account of a provider gets its own variable suggestion.
#[test]
fn the_terminal_suggests_a_distinct_variable_per_account() {
    use account_edit::suggest_env_for;
    let mut config = empty();
    assert_eq!(
        suggest_env_for(Provider::Groq, &config, "work", None),
        "GROQ_API_KEY"
    );
    config.accounts.push(Account {
        name: "home".into(),
        provider: Provider::Groq,
        credential_env: Some("GROQ_API_KEY".into()),
        ..Default::default()
    });
    assert_eq!(
        suggest_env_for(Provider::Groq, &config, "Work / EU", None),
        "GROQ_API_KEY_WORK_EU"
    );
    assert_eq!(
        suggest_env_for(Provider::Groq, &config, "home", Some(0)),
        "GROQ_API_KEY"
    );
    config.accounts.push(Account {
        name: "x".into(),
        provider: Provider::Groq,
        credential_env: Some("GROQ_API_KEY_WORK".into()),
        ..Default::default()
    });
    assert_eq!(
        suggest_env_for(Provider::Groq, &config, "work", None),
        "GROQ_API_KEY_WORK_2"
    );
    assert_eq!(
        suggest_env_for(Provider::Groq, &config, "", None),
        "GROQ_API_KEY_2"
    );
    assert!(account_edit::valid_env_name(&suggest_env_for(
        Provider::Groq,
        &config,
        "9 lives",
        None
    )));
    assert_eq!(
        suggest_env_for(Provider::AwsBedrock, &config, "a", None),
        ""
    );
    assert!(account_edit::is_terminal_only_field(
        Provider::AlibabaCloud,
        "secret_env"
    ));
    assert!(!account_edit::is_terminal_only_field(
        Provider::ElevenLabs,
        "region"
    ));
}

fn empty() -> Config {
    Config {
        accounts: Vec::new(),
        ..Config::default()
    }
}

/// Owner request: every key page is HTTPS, allow-listed exactly (no
/// prefix match) and listed once per slot.
#[test]
fn key_pages_are_https_and_exactly_allow_listed() {
    for (provider, slot, url) in KEY_PAGES {
        assert!(url.starts_with("https://"), "{url}");
        assert!(!url.contains(char::is_whitespace), "{url}");
        assert!(is_key_page_url(url));
        assert!(!is_key_page_url(&format!("{url}x")));
        assert!(!is_key_page_url(&url.replacen("https://", "http://", 1)));
        assert_eq!(key_page(*provider, slot), Some(*url));
        assert!(
            !NO_KEY_PAGE
                .iter()
                .any(|(p, s, _)| p == provider && s == slot),
            "{provider:?}/{slot} is both linked and exempt"
        );
    }
    assert!(!is_key_page_url("https://example.com/"));
    assert!(!is_key_page_url(""));
}

/// Owner request: every typed key of every provider either links its
/// creation page or is in the documented exemption list.
#[test]
fn every_typed_key_has_a_page_or_an_exemption() {
    let mut missing = Vec::new();
    for &provider in Provider::ALL.iter() {
        for input in secret_inputs(provider) {
            if input.cookie || user_session::descriptor(provider, input.slot).is_some() {
                continue;
            }
            let exempt = NO_KEY_PAGE
                .iter()
                .any(|(p, s, _)| *p == provider && *s == input.slot);
            if input.key_page.is_none() && !exempt {
                missing.push(format!("{provider:?}/{}", input.slot));
            }
            assert_eq!(input.key_page, key_page(provider, input.slot));
        }
    }
    assert!(missing.is_empty(), "no key page nor exemption: {missing:?}");
    for (provider, slot, _) in KEY_PAGES {
        assert!(
            secret_inputs(*provider)
                .iter()
                .any(|i| i.slot == *slot && i.key_page.is_some()),
            "{provider:?}/{slot} links a page no input shows"
        );
    }
}
