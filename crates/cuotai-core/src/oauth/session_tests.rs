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
