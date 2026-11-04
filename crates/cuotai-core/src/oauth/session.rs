use super::process_lock::ProcessLock;
use super::{
    flows, reference, requires_client_registration, LoginPrompt, OAuthConfig, OAuthError,
    OAuthToken, SystemTokenStore, TokenStore,
};
use crate::{Credentials, EnvironmentCredentials, Provider};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, PoisonError};
use tokio::sync::{Mutex, OwnedMutexGuard};

/// Rotated tokens this process obtained but could not write to the vault yet,
/// by storage key. The provider already invalidated the stored refresh token,
/// so these copies are newer than the vault and win over it until a later
/// write succeeds. Memory only: never logged, never `Debug`.
type Unsaved = Arc<std::sync::Mutex<HashMap<String, OAuthToken>>>;

/// Tests only: stands in for the provider dispatch of a refresh.
#[cfg(test)]
pub(super) type TestRefresher = Arc<
    dyn Fn(
            reqwest::Client,
            OAuthToken,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<OAuthToken, OAuthError>> + Send>,
        > + Send
        + Sync,
>;

/// Owns persistent credentials; no browser/terminal effects. Share one instance
/// across monitors to serialize rotating refresh tokens within the application.
pub struct OAuthSession {
    http: reqwest::Client,
    store: Arc<dyn TokenStore>,
    secrets: Arc<dyn Credentials>,
    refresh_lock: Arc<Mutex<()>>,
    unsaved: Unsaved,
    /// Where the cross-process locks of this session's vault entries live
    /// ([`super::process_lock`]); `None` keeps the in-process guard only.
    process_locks: Option<std::path::PathBuf>,
    /// PRD 0029 RF6: the index of the entries this session's store creates.
    vault_index: Option<std::path::PathBuf>,
    /// Tests only: replaces the provider dispatch of a refresh, so a local
    /// mock can stand in for the fixed provider endpoints.
    #[cfg(test)]
    test_refresher: Option<TestRefresher>,
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
        Ok(session.with_system_process_locks())
    }

    /// Guard the OS vault entries against rotation by other Cuotai processes
    /// (TUI, `--serve`, desktop) with the shared lock directory.
    pub(crate) fn with_system_process_locks(mut self) -> Self {
        self.process_locks = super::process_lock::default_dir();
        self
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
        session.process_locks = Some(super::process_lock::development_dir(path));
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
        // PRD 0052 RF6.1: OS roots, environment proxies, no redirects.
        let http = crate::network::client(
            crate::network::ClientPurpose::OAuth,
            std::time::Duration::from_secs(30),
        )
        .ok_or(OAuthError::Network)?;
        Ok(Self {
            http,
            store,
            secrets,
            refresh_lock: Arc::new(Mutex::new(())),
            unsaved: Arc::default(),
            process_locks: None,
            vault_index: None,
            #[cfg(test)]
            test_refresher: None,
        })
    }

    #[cfg(test)]
    pub(super) fn with_test_refresher(mut self, refresher: TestRefresher) -> Self {
        self.test_refresher = Some(refresher);
        self
    }

    #[cfg(test)]
    pub(super) fn with_process_locks(mut self, dir: std::path::PathBuf) -> Self {
        self.process_locks = Some(dir);
        self
    }

    /// Exclusive access to `key` for a vault write: this session's lock, then
    /// the cross-process lock of the entry, always in that order.
    async fn exclusive(
        &self,
        key: &str,
    ) -> Result<(OwnedMutexGuard<()>, Option<ProcessLock>), OAuthError> {
        let guard = Arc::clone(&self.refresh_lock).lock_owned().await;
        let process = self.lock_process(key).await?;
        Ok((guard, process))
    }

    async fn lock_process(&self, key: &str) -> Result<Option<ProcessLock>, OAuthError> {
        match &self.process_locks {
            Some(dir) => super::process_lock::acquire(dir, key, super::process_lock::WAIT).await,
            None => Ok(None),
        }
    }

    /// The rotated token this process holds for `key` but could not persist.
    fn unsaved_token(&self, key: &str) -> Option<OAuthToken> {
        self.unsaved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    /// A write for `key` replaced whatever rotation was waiting to be saved.
    fn forget_unsaved(&self, key: &str) {
        self.unsaved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
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
        let _locks = self.exclusive(&login.key).await?;
        self.write(&login.key, &login.token).await
    }

    /// Save a new login and return the token it replaced, read under the
    /// same lock as the write (RF2.4 staged commit). A refresh can no longer
    /// rotate the prior token between the read and the write, and a rotation
    /// this process could not persist counts as the prior token, so a rollback
    /// restores the newest refresh token instead of one the provider already
    /// invalidated. A stored token that no longer decodes counts as absent.
    pub(crate) async fn save_login_replacing(
        &self,
        login: &AuthorizedLogin,
    ) -> Result<Option<OAuthToken>, OAuthError> {
        let _locks = self.exclusive(&login.key).await?;
        let prior = match self.unsaved_token(&login.key) {
            Some(token) => Some(token),
            None => match self.read(&login.key).await {
                Ok(prior) => prior,
                Err(OAuthError::Storage(note)) if note.id == super::store::TOKEN_UNREADABLE => None,
                Err(error) => return Err(error),
            },
        };
        self.write(&login.key, &login.token).await?;
        Ok(prior)
    }

    /// Load the token under `key`: a rotation this process could not persist
    /// yet, else the stored one (RF2.4 staged commit and vault cleanup).
    pub(crate) async fn load_by_key(&self, key: &str) -> Result<Option<OAuthToken>, OAuthError> {
        if let Some(token) = self.unsaved_token(key) {
            return Ok(Some(token));
        }
        self.read(key).await
    }

    async fn read(&self, key: &str) -> Result<Option<OAuthToken>, OAuthError> {
        let store = Arc::clone(&self.store);
        let key = key.to_string();
        tokio::task::spawn_blocking(move || store.load(&key))
            .await
            .map_err(|_| storage_error())?
    }

    /// Write `token` under `key`; the caller holds the refresh lock. A
    /// successful write supersedes any rotation waiting to be saved.
    async fn write(&self, key: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        persist(&self.store, key, token).await?;
        self.forget_unsaved(key);
        Ok(())
    }

    /// Delete the entry under `key`; the caller holds the refresh lock.
    async fn remove(&self, key: &str) -> Result<(), OAuthError> {
        let store = Arc::clone(&self.store);
        let id = key.to_string();
        tokio::task::spawn_blocking(move || store.delete(&id))
            .await
            .map_err(|_| storage_error())??;
        self.forget_unsaved(key);
        Ok(())
    }

    /// Overwrite the token under `key`. Used only to RESTORE a prior token
    /// during commit compensation; new logins go through `save_login`.
    pub(crate) async fn restore_by_key(
        &self,
        key: String,
        token: OAuthToken,
    ) -> Result<(), OAuthError> {
        let _locks = self.exclusive(&key).await?;
        self.write(&key, &token).await
    }

    /// Delete the token under `key`. Commit compensation only: called when a
    /// brand-new credential was written but the config save failed, and never
    /// when a prior token was replaced without a successful restore.
    pub(crate) async fn delete_by_key(&self, key: String) -> Result<(), OAuthError> {
        let _locks = self.exclusive(&key).await?;
        self.remove(&key).await
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
        let unsaved = self.unsaved_token(&key);
        if unsaved.is_none() {
            // Fast path: a fresh token needs no cross-process lock.
            let token = self.read(&key).await?.ok_or_else(login_not_found)?;
            if is_fresh(&token) {
                return usable(token);
            }
        }
        // Any write below happens under the entry's cross-process lock.
        let process = self.lock_process(&key).await?;
        let token = match unsaved {
            // A rotation an earlier call could not persist is newer than the
            // vault: retry the write first, and keep the token in memory until
            // the vault accepts it.
            Some(token) => {
                self.write(&key, &token).await?;
                token
            }
            // Read again under the lock: another process may have rotated
            // the token while this one waited, and its stored result must be
            // used instead of sending the refresh token it invalidated.
            None => self.read(&key).await?.ok_or_else(login_not_found)?,
        };
        if is_fresh(&token) {
            return usable(token);
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
        let unsaved = Arc::clone(&self.unsaved);
        #[cfg(test)]
        let test_refresher = self.test_refresher.clone();
        // Once token rotation starts, complete persistence even if the caller
        // cancels its quota query; otherwise a rotated refresh token is lost.
        tokio::spawn(async move {
            let _locks = (guard, process);
            let refresh = async {
                #[cfg(test)]
                if let Some(refresher) = test_refresher {
                    return refresher(http.clone(), token.clone()).await;
                }
                dispatch_refresh(&http, provider, &config, secret.as_deref(), &token).await
            };
            let (refreshed, observed) = super::rotation::capture(refresh).await;
            let mut renewed = match refreshed {
                Ok(renewed) => renewed,
                Err(error) => {
                    // The endpoint may have rotated the refresh token in an
                    // answer the refresh then rejected; the old one is
                    // already invalid. Keep the rotated one with the expired
                    // access token, so the next call refreshes with it.
                    if let Some(rotated) =
                        observed.filter(|rotated| token.refresh_token.as_ref() != Some(rotated))
                    {
                        let salvaged = OAuthToken {
                            refresh_token: Some(rotated),
                            ..token
                        };
                        // A vault failure here is secondary: the caller gets
                        // the refresh error, and the token waits in memory.
                        let _ = keep(&store, &unsaved, key, salvaged).await;
                    }
                    return Err(error);
                }
            };
            for (key, value) in &token.metadata {
                renewed
                    .metadata
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
            keep(&store, &unsaved, key, renewed.clone()).await?;
            Ok(renewed)
        })
        .await
        .map_err(|_| storage_error())?
    }

    pub async fn logout(&self, provider: Provider, config: &OAuthConfig) -> Result<(), OAuthError> {
        let key = storage_key(provider, config)?;
        let _locks = self.exclusive(&key).await?;
        self.remove(&key).await
    }
}

/// A fresh token is returned only with a usable access token.
fn usable(token: OAuthToken) -> Result<OAuthToken, OAuthError> {
    if token.access_token.trim().is_empty() {
        return Err(OAuthError::InvalidResponse);
    }
    Ok(token)
}

/// An access token that stays valid for more than a minute, or never expires.
fn is_fresh(token: &OAuthToken) -> bool {
    token
        .expires_at
        .is_none_or(|expiry| expiry > Utc::now() + chrono::Duration::seconds(60))
}

fn login_not_found() -> OAuthError {
    OAuthError::Configuration(crate::text::Localized::fixed(
        "oauth_error.session.login_not_found",
        "Login not found in the secure store. Open the wizard and authenticate again.",
    ))
}

async fn persist(
    store: &Arc<dyn TokenStore>,
    key: &str,
    token: &OAuthToken,
) -> Result<(), OAuthError> {
    let store = Arc::clone(store);
    let key = key.to_string();
    let token = token.clone();
    tokio::task::spawn_blocking(move || store.save(&key, &token))
        .await
        .map_err(|_| storage_error())?
}

/// Write a rotated token. The provider already invalidated the stored refresh
/// token, so when the vault refuses the write the token waits in `unsaved`
/// for the next call instead of being dropped as the only valid copy.
async fn keep(
    store: &Arc<dyn TokenStore>,
    unsaved: &Unsaved,
    key: String,
    token: OAuthToken,
) -> Result<(), OAuthError> {
    match persist(store, &key, &token).await {
        Ok(()) => Ok(()),
        Err(error) => {
            unsaved
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key, token);
            Err(error)
        }
    }
}

async fn dispatch_refresh(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    secret: Option<&str>,
    token: &OAuthToken,
) -> Result<OAuthToken, OAuthError> {
    if requires_client_registration(provider) {
        flows::refresh(http, provider, config, secret, token).await
    } else {
        reference::refresh(http, provider, config, token).await
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
                            "Client secret variable is missing; export it before starting the app.",
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
        "Operation with the system vault failed.",
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
        errors: tokio::sync::mpsc::Sender<crate::text::Localized>,
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
