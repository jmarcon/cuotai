use super::{
    flows, reference, requires_client_registration, LoginPrompt, OAuthConfig, OAuthError,
    OAuthToken, SystemTokenStore, TokenStore,
};
use crate::{Credentials, EnvironmentCredentials, Provider};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Owns persistent credentials; no browser/terminal effects. Share one instance
/// across monitors to serialize rotating refresh tokens within the application.
pub struct OAuthSession {
    http: reqwest::Client,
    store: Arc<dyn TokenStore>,
    secrets: Arc<dyn Credentials>,
    refresh_lock: Arc<Mutex<()>>,
    /// PRD 0029 RF6: the index of the entries this session's store creates.
    vault_index: Option<std::path::PathBuf>,
}

impl OAuthSession {
    /// The OS vault, with the index of the entries Cuotai creates
    /// ([`super::vault_index`], PRD 0029 RF6).
    pub fn system() -> Result<Self, OAuthError> {
        let index = super::vault_index::default_path();
        let store: Arc<dyn TokenStore> = Arc::new(SystemTokenStore);
        let mut session = Self::new(
            match &index {
                Some(path) => Arc::new(super::vault_index::IndexedStore::new(store, path.clone())),
                None => store,
            },
            Arc::new(EnvironmentCredentials),
        )?;
        session.vault_index = index;
        Ok(session)
    }

    /// Opt-in plaintext credentials for local development, never a fallback.
    /// Release builds reject this operation before touching the filesystem.
    pub fn development_sqlite(path: &std::path::Path) -> Result<Self, OAuthError> {
        // Its own index, next to the database, so the development and the
        // OS vault indexes never mix.
        let index = super::vault_index::development_path(path);
        let mut session = Self::new(
            Arc::new(super::vault_index::IndexedStore::new(
                super::store::development_store(path)?,
                index.clone(),
            )),
            Arc::new(EnvironmentCredentials),
        )?;
        session.vault_index = Some(index);
        Ok(session)
    }

    /// PRD 0029 RF6: where this session indexes the vault entries it
    /// creates (`None` for a store without the index, as in tests).
    pub fn vault_index_path(&self) -> Option<&std::path::Path> {
        self.vault_index.as_deref()
    }

    /// The vault this session reads and writes, shared with the extension
    /// secrets (PRD 0028 RF7.4).
    pub fn token_store(&self) -> Arc<dyn TokenStore> {
        Arc::clone(&self.store)
    }

    pub fn new(
        store: Arc<dyn TokenStore>,
        secrets: Arc<dyn Credentials>,
    ) -> Result<Self, OAuthError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("cuotai/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| OAuthError::Network)?;
        Ok(Self {
            http,
            store,
            secrets,
            refresh_lock: Arc::new(Mutex::new(())),
            vault_index: None,
        })
    }

    /// The returned operation is cancelled by dropping it or its finish future.
    /// Tokens remain in memory until explicitly committed by `save_login`.
    pub async fn begin(
        &self,
        provider: Provider,
        config: &OAuthConfig,
    ) -> Result<OAuthLogin, OAuthError> {
        config.validate(provider)?;
        let pending = if requires_client_registration(provider) {
            // The secret is only validated here; the pending login keeps the
            // lookup and reads the value again when the code is exchanged.
            let secrets = self.secret_lookup(config);
            secrets.ensure_available()?;
            Pending::Cloud(flows::begin(&self.http, provider, config, secrets).await?)
        } else {
            Pending::Native(reference::begin(&self.http, provider, config).await?)
        };
        Ok(OAuthLogin {
            pending,
            key: storage_key(provider, config)?,
        })
    }

    /// Explicit user-authorized import; never called during normal refresh/startup.
    pub async fn import_cursor(&self, config: &OAuthConfig) -> Result<AuthorizedLogin, OAuthError> {
        config.validate(Provider::Cursor)?;
        let path = config
            .options
            .get("database_path")
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from);
        let token = super::cursor_import::import(path).await?;
        Ok(AuthorizedLogin {
            key: storage_key(Provider::Cursor, config)?,
            token,
        })
    }

    fn secret_lookup(&self, config: &OAuthConfig) -> ClientSecretLookup {
        ClientSecretLookup::new(Arc::clone(&self.secrets), config)
    }

    pub async fn save_login(&self, login: &AuthorizedLogin) -> Result<(), OAuthError> {
        let guard = Arc::clone(&self.refresh_lock).lock_owned().await;
        let store = Arc::clone(&self.store);
        let key = login.key.clone();
        let token = login.token.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            store.save(&key, &token)
        })
        .await
        .map_err(|_| storage_error())?
    }

    /// Load the raw token stored under `key` (RF2.4 staged commit: captures
    /// the prior token before a reauth overwrites it, so a failed config save
    /// can restore it).
    pub(crate) async fn load_by_key(&self, key: &str) -> Result<Option<OAuthToken>, OAuthError> {
        let store = Arc::clone(&self.store);
        let key = key.to_string();
        tokio::task::spawn_blocking(move || store.load(&key))
            .await
            .map_err(|_| storage_error())?
    }

    /// Overwrite the token under `key`. Used only to RESTORE a prior token
    /// during commit compensation; new logins go through `save_login`.
    pub(crate) async fn restore_by_key(
        &self,
        key: String,
        token: OAuthToken,
    ) -> Result<(), OAuthError> {
        let guard = Arc::clone(&self.refresh_lock).lock_owned().await;
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            store.save(&key, &token)
        })
        .await
        .map_err(|_| storage_error())?
    }

    /// Delete the token under `key`. Commit compensation only: called when a
    /// brand-new credential was written but the config save failed, and never
    /// when a prior token was replaced without a successful restore.
    pub(crate) async fn delete_by_key(&self, key: String) -> Result<(), OAuthError> {
        let guard = Arc::clone(&self.refresh_lock).lock_owned().await;
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            store.delete(&key)
        })
        .await
        .map_err(|_| storage_error())?
    }

    pub async fn access_token(
        &self,
        provider: Provider,
        config: &OAuthConfig,
    ) -> Result<String, OAuthError> {
        Ok(self.credentials(provider, config).await?.access_token)
    }

    pub async fn credentials(
        &self,
        provider: Provider,
        config: &OAuthConfig,
    ) -> Result<OAuthToken, OAuthError> {
        config.validate(provider)?;
        let key = storage_key(provider, config)?;
        let guard = Arc::clone(&self.refresh_lock).lock_owned().await;
        let store = Arc::clone(&self.store);
        let load_key = key.clone();
        let token = tokio::task::spawn_blocking(move || store.load(&load_key)).await
            .map_err(|_| storage_error())??
            .ok_or_else(|| OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.session.login_not_found", "Login não encontrado no armazenamento seguro. Abra o wizard e autentique novamente.",
        )))?;
        if token
            .expires_at
            .is_none_or(|expiry| expiry > Utc::now() + chrono::Duration::seconds(60))
        {
            if token.access_token.trim().is_empty() {
                return Err(OAuthError::InvalidResponse);
            }
            return Ok(token);
        }
        if token
            .refresh_token
            .as_ref()
            .is_none_or(|t| t.trim().is_empty())
        {
            return Err(OAuthError::Expired);
        }
        let secret = self.secret_lookup(config).resolve()?;
        let http = self.http.clone();
        let config = config.clone();
        let store = Arc::clone(&self.store);
        // Once token rotation starts, complete persistence even if the caller
        // cancels its quota query; otherwise a rotated refresh token is lost.
        tokio::spawn(async move {
            let mut renewed = if requires_client_registration(provider) {
                flows::refresh(&http, provider, &config, secret.as_deref(), &token).await?
            } else {
                reference::refresh(&http, provider, &config, &token).await?
            };
            for (key, value) in &token.metadata {
                renewed
                    .metadata
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
            let returned = renewed.clone();
            tokio::task::spawn_blocking(move || {
                let _guard = guard;
                store.save(&key, &renewed)
            })
            .await
            .map_err(|_| storage_error())??;
            Ok(returned)
        })
        .await
        .map_err(|_| storage_error())?
    }

    pub async fn logout(&self, provider: Provider, config: &OAuthConfig) -> Result<(), OAuthError> {
        let key = storage_key(provider, config)?;
        let guard = Arc::clone(&self.refresh_lock).lock_owned().await;
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            store.delete(&key)
        })
        .await
        .map_err(|_| storage_error())?
    }
}

/// Deferred lookup of the optional OAuth client secret named by
/// [`OAuthConfig::client_secret_env`]. It owns the credential source and the
/// config, never the secret value: pending logins keep this handle and resolve
/// the value only while building the token-exchange request.
#[derive(Clone)]
pub(super) struct ClientSecretLookup {
    credentials: Arc<dyn Credentials>,
    config: Box<OAuthConfig>,
}

impl ClientSecretLookup {
    pub(super) fn new(credentials: Arc<dyn Credentials>, config: &OAuthConfig) -> Self {
        Self {
            credentials,
            config: Box::new(config.clone()),
        }
    }

    /// `Ok(None)` when no client secret is configured; a configured variable
    /// that is unset or blank is a configuration error.
    pub(super) fn resolve(&self) -> Result<Option<String>, OAuthError> {
        self.config
            .client_secret_env
            .as_ref()
            .map(|name| {
                self.credentials
                    .get(name)
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        OAuthError::Configuration(crate::text::Localized::fixed(
                            "oauth_error.session.client_secret_env_missing",
                            "Variável do client secret ausente; exporte-a antes de iniciar o aplicativo.",
                        ))
                    })
            })
            .transpose()
    }

    /// Fail early, without keeping the value, when the secret is missing.
    pub(super) fn ensure_available(&self) -> Result<(), OAuthError> {
        self.resolve().map(|_| ())
    }
}

pub(crate) fn storage_error() -> OAuthError {
    OAuthError::Storage(crate::text::Localized::fixed(
        "oauth_error.session.vault_operation_failed",
        "Falha na operação com o cofre do sistema.",
    ))
}

// Bind saved tokens to provider, registered app and tenant. Editing the TOML
// must not redirect a token to a different provider/application.
pub(crate) fn storage_key(provider: Provider, config: &OAuthConfig) -> Result<String, OAuthError> {
    config.validate(provider)?;
    let namespace =
        uuid::Uuid::parse_str(&config.credential_id).map_err(|_| OAuthError::InvalidResponse)?;
    let identity = if config.options.is_empty() {
        serde_json::to_vec(&(provider, &config.client_id, &config.tenant))
    } else {
        serde_json::to_vec(&(provider, &config.client_id, &config.tenant, &config.options))
    }
    .map_err(|_| OAuthError::InvalidResponse)?;
    Ok(uuid::Uuid::new_v5(&namespace, &identity).to_string())
}

enum Pending {
    Cloud(flows::PendingLogin),
    Native(reference::PendingLogin),
}
impl Pending {
    fn prompt(&self) -> &LoginPrompt {
        match self {
            Self::Cloud(p) => p.prompt(),
            Self::Native(p) => p.prompt(),
        }
    }
    async fn finish(self) -> Result<OAuthToken, OAuthError> {
        match self {
            Self::Cloud(p) => p.finish().await,
            Self::Native(p) => p.finish().await,
        }
    }
}

pub struct OAuthLogin {
    pending: Pending,
    key: String,
}
impl OAuthLogin {
    pub fn prompt(&self) -> &LoginPrompt {
        self.pending.prompt()
    }
    pub async fn finish(self) -> Result<AuthorizedLogin, OAuthError> {
        Ok(AuthorizedLogin {
            key: self.key,
            token: self.pending.finish().await?,
        })
    }

    /// Complete a pending browser flow with bounded manual code submissions.
    /// Invalid MiMo codes do not restart the flow or echo submitted data.
    pub async fn finish_with_codes(
        self,
        codes: tokio::sync::mpsc::Receiver<String>,
        errors: tokio::sync::mpsc::Sender<String>,
    ) -> Result<AuthorizedLogin, OAuthError> {
        let token = match self.pending {
            Pending::Cloud(pending) => pending.finish().await?,
            Pending::Native(pending) => pending.finish_with_codes(codes, errors).await?,
        };
        Ok(AuthorizedLogin {
            key: self.key,
            token,
        })
    }
}

/// Tokens intentionally opaque and non-Debug. Safe to hold until wizard commit.
pub struct AuthorizedLogin {
    key: String,
    token: OAuthToken,
}

impl AuthorizedLogin {
    /// Only whitelisted nonsecret quota metadata is exposed to the wizard.
    pub fn account_params(
        &self,
        provider: Provider,
        config: &OAuthConfig,
    ) -> std::collections::BTreeMap<String, String> {
        super::account_params(provider, config, &self.token.metadata)
    }
}

impl AuthorizedLogin {
    /// Vault key derived at `begin`; reused verbatim on reauth (RF2.8).
    pub(crate) fn key(&self) -> &str {
        &self.key
    }
}

#[cfg(test)]
impl AuthorizedLogin {
    pub(crate) fn for_test(key: String, token: OAuthToken) -> Self {
        Self { key, token }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
