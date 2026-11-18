//! PRD 0047 S-06: a variable name that is not a usable name (it may be a
//! pasted key) stops only its own account, before any credential is read,
//! and is never repeated in a note, a snapshot or a migration offer.

use crate::account_edit::{
    build_account, invalid_env_field, resolve_credential_env, validate_param, AccountDraft,
    AUTH_MODE_CREDENTIAL,
};
use crate::oauth::{OAuthError, OAuthSession, OAuthToken, TokenStore};
use crate::secret_input::{secret_states, vault_migration_candidates, SecretSource};
use crate::{Config, Credentials, Monitor, Provider, Status};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A vault with nothing in it that refuses writes.
struct EmptyVault;
impl TokenStore for EmptyVault {
    fn load(&self, _: &str) -> Result<Option<OAuthToken>, OAuthError> {
        Ok(None)
    }
    fn save(&self, _: &str, _: &OAuthToken) -> Result<(), OAuthError> {
        unreachable!("these tests never write the vault")
    }
    fn delete(&self, _: &str) -> Result<(), OAuthError> {
        unreachable!("these tests never write the vault")
    }
}

/// Answers every variable, so a query that read one would not stop at
/// "missing credential" — except the one valid name, left unset.
struct EveryVariable;
impl Credentials for EveryVariable {
    fn get(&self, name: &str) -> Option<String> {
        (name != "FAKE_GROQ_0047_UNSET").then(|| "sk-FAKE-0047-variable-value".to_owned())
    }
}

fn gsk() -> String {
    format!("gsk_FAKE0047{}", "a1".repeat(24))
}

fn config() -> Config {
    Config::from_toml(&format!(
        "[[accounts]]\nname = \"pasted\"\nprovider = \"groq\"\ncredential_env = \"sk-FAKE-0047-pasted-value\"\n\n\
         [[accounts]]\nname = \"posix\"\nprovider = \"groq\"\ncredential_env = \"{}\"\n\n\
         [[accounts]]\nname = \"admin\"\nprovider = \"mistral\"\n[accounts.params]\nadmin_key_env = \"sk-FAKE-0047-admin-value\"\n\n\
         [[accounts]]\nname = \"fine\"\nprovider = \"groq\"\ncredential_env = \"FAKE_GROQ_0047_UNSET\"\n",
        gsk()
    ))
    .unwrap()
}

fn monitor(config: Config) -> Monitor {
    let creds: Arc<dyn Credentials> = Arc::new(EveryVariable);
    let session = Arc::new(OAuthSession::new(Arc::new(EmptyVault), Arc::clone(&creds)).unwrap());
    Monitor::with_login_session(config, creds, session).unwrap()
}

fn leaks(text: &str) -> bool {
    text.contains("FAKE-0047") || text.contains("gsk_FAKE")
}

#[test]
fn invalid_env_field_names_the_field_never_the_value() {
    let config = config();
    let fields: Vec<_> = config.accounts.iter().map(invalid_env_field).collect();
    assert_eq!(
        fields,
        [
            Some("credential_env".to_owned()),
            Some("credential_env".to_owned()),
            Some("params.admin_key_env".to_owned()),
            None,
        ]
    );
    // An empty name reads as absent; a reserved one is the config's error.
    let mut account = config.accounts[3].clone();
    account.credential_env = Some(" ".into());
    assert_eq!(invalid_env_field(&account), None);
    account.credential_env = Some("__CUOTAI_SESSION".into());
    assert_eq!(invalid_env_field(&account), None);
}

#[tokio::test]
async fn an_invalid_variable_name_stops_only_its_account() {
    let config = config();
    config
        .validate()
        .expect("an invalid name never fails the load");
    let snapshot = monitor(config).refresh().await;
    let ids: Vec<_> = snapshot
        .reports
        .iter()
        .map(|report| {
            (
                report.status,
                report.notes_l10n.first().map(|note| note.id.to_string()),
                report.notes_l10n.first().and_then(|note| {
                    note.params.get("field").map(|field| match field {
                        crate::text::ParamValue::Text(text) => text.clone(),
                        _ => String::new(),
                    })
                }),
            )
        })
        .collect();
    let invalid = |field: &str| {
        (
            Status::NeedsConfiguration,
            Some("query.env_name_invalid".to_owned()),
            Some(field.to_owned()),
        )
    };
    assert_eq!(ids[0], invalid("credential_env"));
    assert_eq!(ids[1], invalid("credential_env"));
    assert_eq!(ids[2], invalid("params.admin_key_env"));
    assert_eq!(ids[3].1.as_deref(), Some("query.missing_credential"));
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(!leaks(&json), "a pasted name in the snapshot");
    let text: String = snapshot
        .reports
        .iter()
        .flat_map(|report| report.notes_l10n.iter())
        .flat_map(|note| [note.resolve("en-US"), note.resolve("pt-BR")])
        .collect();
    assert!(!leaks(&text), "a pasted name in a note");
}

#[tokio::test]
async fn test_session_refuses_an_invalid_name_without_echo() {
    let config = config();
    let monitor = monitor(config.clone());
    for account in &config.accounts[..3] {
        let report = monitor.test_session(account, &[]).await;
        assert_eq!(report.status, Status::NeedsConfiguration);
        assert_eq!(report.notes_l10n[0].id, "query.env_name_invalid");
        assert!(!leaks(&serde_json::to_string(&report).unwrap()));
    }
}

#[test]
fn an_invalid_name_is_unset_and_never_offered_for_the_move() {
    for account in &config().accounts[..3] {
        let states = secret_states(account);
        assert!(
            states
                .iter()
                .all(|state| state.source == SecretSource::Unset),
            "{}",
            account.name
        );
        assert!(vault_migration_candidates(account).is_empty());
        assert!(crate::user_session::migration_candidates(account).is_empty());
    }
    let fine = &config().accounts[3];
    assert_eq!(secret_states(fine)[0].source, SecretSource::Environment);
    assert_eq!(vault_migration_candidates(fine).len(), 1);
}

#[test]
fn a_migration_candidate_never_serializes_its_variable_name() {
    let candidates = vault_migration_candidates(&config().accounts[3]);
    let json = serde_json::to_value(&candidates).unwrap();
    assert!(json[0].get("env").is_none(), "{json}");
    assert!(!json.to_string().contains("FAKE_GROQ_0047_UNSET"));
}

#[test]
fn the_editors_refuse_a_name_the_loader_flags() {
    let key = gsk();
    let refused = resolve_credential_env(Provider::Groq, &key, None).unwrap_err();
    assert_eq!(refused.id, "edit_error.env_invalid");
    assert!(!leaks(&refused.resolve("en-US")));
    assert_eq!(
        resolve_credential_env(Provider::Groq, "GROQ_API_KEY_WORK", None).unwrap(),
        Some("GROQ_API_KEY_WORK".to_owned())
    );
    let refused = validate_param("admin_key_env", &key, &BTreeMap::new()).unwrap_err();
    assert_eq!(refused.id, "edit_error.param.env_name");
    assert!(validate_param("admin_key_env", "MISTRAL_ADMIN_KEY", &BTreeMap::new()).is_ok());

    // The wizard's save path (a draft through `build_account`) refuses it too.
    let draft = AccountDraft {
        provider: Provider::Groq,
        name: "",
        credential: &key,
        auth_mode: AUTH_MODE_CREDENTIAL,
        oauth_mode: false,
        oauth_existing: None,
        oauth_authorized: None,
        oauth_login_params: Default::default(),
        existing_params: None,
        credential_params: None,
        fields: &[],
        interval_text: "",
        group_text: None,
    };
    assert!(build_account(&draft, &Config::default(), None).is_err());
}
