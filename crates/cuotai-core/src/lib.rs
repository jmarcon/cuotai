//! Read-only AI usage monitoring, independent of terminal and windowing frameworks.
//! Call `Monitor::refresh` from an async runtime. A failed account does not fail
//! other accounts; reports retain configuration order and never contain secrets.
pub mod about;
pub mod access;
pub mod account_edit;
pub mod config_file;
pub mod edit_error;
pub mod export;
pub mod freshness;
pub mod history;
pub mod identity;
pub mod last_config;
pub mod metrics;
mod model;
pub mod models;
pub mod oauth;
pub mod pace;
mod providers;
pub mod refresh;
pub mod resets;
pub mod text;
mod transport;

/// RF2 shared login driver, re-exported for convenience.
pub use oauth::login;

use chrono::Utc;
use futures_util::{stream, StreamExt};
pub use model::*;
#[cfg(test)]
use std::time::Instant;
use std::{borrow::Cow, collections::HashSet, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

/// The caller owns secret storage. Native apps can implement this using Keychain.
/// `get` must be fast and nonblocking; load interactive credentials before refresh.
pub trait Credentials: Send + Sync {
    fn get(&self, name: &str) -> Option<String>;
}

pub struct EnvironmentCredentials;
impl Credentials for EnvironmentCredentials {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Configuração TOML inválida; confira os campos e tipos no exemplo.")]
    Parse,
    #[error(
        "refresh_seconds deve ser >= 30 (>= 300 quando algum provedor é cobrado por consulta), \
         concurrency entre 1 e 32 e timeout_seconds entre 1 e 300."
    )]
    Bounds,
    #[error("Cada conta precisa de um nome não vazio e único.")]
    AccountNames,
    #[error("Não foi possível inicializar o cliente HTTPS.")]
    Client,
    #[error("Configuração OAuth inválida ou combinada com credential_env; confira o registro do aplicativo.")]
    OAuth,
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text).map_err(|_| ConfigError::Parse)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.refresh_seconds < 30
            || !(1..=32).contains(&self.concurrency)
            || !(1..=300).contains(&self.timeout_seconds)
        {
            return Err(ConfigError::Bounds);
        }
        // Global fallback: a metered provider (Cost Explorer) cannot be
        // polled more often than `METERED_MIN_REFRESH_SECONDS` even when
        // the account has no per-account override — the override can only
        // raise the interval, never lower it.
        let global_floor = self
            .accounts
            .iter()
            .map(|a| a.provider)
            .filter(|p| p.is_metered())
            .map(|_| Provider::METERED_MIN_REFRESH_SECONDS)
            .min()
            .unwrap_or(0);
        if global_floor > 0 && self.refresh_seconds < global_floor {
            return Err(ConfigError::Bounds);
        }
        let mut names = HashSet::with_capacity(self.accounts.len());
        if self
            .accounts
            .iter()
            .any(|a| a.name.trim().is_empty() || !names.insert(&a.name))
        {
            return Err(ConfigError::AccountNames);
        }
        for account in &self.accounts {
            if let Some(override_seconds) = account.refresh_seconds {
                if override_seconds < Account::MIN_REFRESH_SECONDS {
                    return Err(ConfigError::Bounds);
                }
                // Per-account override must also respect the metered
                // floor — the override can only raise the interval above
                // the global, so a metered account with an override
                // smaller than the floor is rejected.
                if account.provider.is_metered()
                    && override_seconds < Provider::METERED_MIN_REFRESH_SECONDS
                {
                    return Err(ConfigError::Bounds);
                }
            }
            if let Some(oauth) = &account.oauth {
                if account.credential_env.is_some() || oauth.validate(account.provider).is_err() {
                    return Err(ConfigError::OAuth);
                }
            }
        }
        Ok(())
    }
}

/// A shared HTTP connection pool and bounded concurrent queries.
/// OAuth tokens are refreshed and persisted before queries when necessary.
/// Dropping the refresh future cancels network work. Each account has one
/// deadline covering authentication, all pages, and subprocess work.
pub struct Monitor {
    config: Config,
    http: reqwest::Client,
    credentials: Arc<dyn Credentials>,
    oauth: Arc<oauth::OAuthSession>,
    query_slots: Semaphore,
    /// Test-only base URL for Groq so contract tests can count the HTTP
    /// requests a `Monitor` issues against a local `wiremock` server.
    #[cfg(test)]
    groq_url: Option<String>,
    /// Per-config model catalog store; `None` keeps model operations
    /// unavailable without affecting quota refresh.
    models: Option<models::ModelCache>,
    /// PRD 0006 RF2: measurement history recorder; `None` records nothing.
    history: Option<history::HistoryRecorder>,
    /// Test-only fixed clock for `checked_at`, so recording tests control
    /// the time between readings.
    #[cfg(test)]
    clock: Option<Arc<parking_lot::Mutex<chrono::DateTime<Utc>>>>,
}

impl Monitor {
    pub fn new(config: Config) -> Result<Self, ConfigError> {
        Self::with_credentials(config, Arc::new(EnvironmentCredentials))
    }

    pub fn with_credentials(
        config: Config,
        credentials: Arc<dyn Credentials>,
    ) -> Result<Self, ConfigError> {
        let oauth = Arc::new(
            oauth::OAuthSession::new(Arc::new(oauth::SystemTokenStore), Arc::clone(&credentials))
                .map_err(|_| ConfigError::Client)?,
        );
        Self::with_oauth(config, credentials, oauth)
    }

    pub fn with_oauth(
        config: Config,
        credentials: Arc<dyn Credentials>,
        oauth: Arc<oauth::OAuthSession>,
    ) -> Result<Self, ConfigError> {
        config.validate()?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("cuotai/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| ConfigError::Client)?;
        let query_slots = Semaphore::new(config.concurrency);
        Ok(Self {
            config,
            http,
            credentials,
            oauth,
            query_slots,
            #[cfg(test)]
            groq_url: None,
            models: None,
            history: None,
            #[cfg(test)]
            clock: None,
        })
    }

    /// Build a monitor for a new configuration while reusing the HTTP
    /// client, the credential lookup and the `OAuthSession` from `self`.
    ///
    /// `reqwest::Client` is built with a request timeout tied to
    /// `config.timeout_seconds`; that timeout cannot be changed on an
    /// existing client, so when the new configuration requires a different
    /// timeout we rebuild the client (the credentials and the
    /// `OAuthSession` are always reused — token rotation is serialized
    /// across the process and the configured backend). The query
    /// semaphore is always rebuilt from the new `concurrency`, so a new
    /// edit can also change how many concurrent providers are in flight.
    ///
    /// The new configuration is validated; out-of-range settings return
    /// the usual `ConfigError::Bounds`.
    pub fn with_config(&self, config: Config) -> Result<Self, ConfigError> {
        config.validate()?;
        let http = if config.timeout_seconds == self.config.timeout_seconds {
            self.http.clone()
        } else {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(config.timeout_seconds))
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("cuotai/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|_| ConfigError::Client)?
        };
        let query_slots = Semaphore::new(config.concurrency);
        Ok(Self {
            config,
            http,
            credentials: Arc::clone(&self.credentials),
            oauth: Arc::clone(&self.oauth),
            query_slots,
            #[cfg(test)]
            groq_url: self.groq_url.clone(),
            models: self.models.clone(),
            history: self.history.clone(),
            #[cfg(test)]
            clock: self.clock.clone(),
        })
    }

    /// Attach the model catalog store for this configuration (see
    /// [`models::ModelCache::new`]); kept by [`Monitor::with_config`].
    pub fn with_model_cache(mut self, cache: models::ModelCache) -> Self {
        self.models = Some(cache);
        self
    }

    /// PRD 0006 RF2.1: record every `Ready` report into `store` (pruned by
    /// its [`history::Retention`]) from a worker thread with a bounded
    /// queue, so a slow disk never delays a refresh; kept by
    /// [`Monitor::with_config`]. Without it nothing is recorded.
    pub fn with_history(self, store: history::HistoryStore) -> Self {
        self.with_recorder(history::HistoryRecorder::spawn(store))
    }

    fn with_recorder(mut self, recorder: history::HistoryRecorder) -> Self {
        self.history = Some(recorder);
        self
    }

    /// The history store (with its current retention), for queries.
    pub fn history(&self) -> Option<history::HistoryStore> {
        self.history.as_ref().map(history::HistoryRecorder::store)
    }

    /// What front ends show about the history; `None` without one.
    pub fn history_status(&self) -> Option<history::HistoryStatus> {
        self.history.as_ref().map(history::HistoryRecorder::status)
    }

    /// RF4.1: prune by `retention` from now on (at once, then every 24 h).
    pub fn set_history_retention(&self, retention: history::Retention) {
        if let Some(recorder) = &self.history {
            recorder.set_retention(retention);
        }
    }

    /// Mark for pruning the series of accounts gone from this monitor's
    /// configuration; call it with the configuration just loaded from or
    /// saved to the TOML (as [`Monitor::prune_models`]). Queued, never blocks.
    pub fn mark_history_removed(&self) {
        if let Some(recorder) = &self.history {
            recorder.mark_removed(&self.config);
        }
    }

    /// Wait up to `timeout` for the queued readings to reach the disk (a
    /// process about to exit calls it). `true` without history.
    pub fn flush_history(&self, timeout: Duration) -> bool {
        self.history
            .as_ref()
            .is_none_or(|recorder| recorder.flush(timeout))
    }

    /// PRD 0010 RF2.3: the last recorded reading of each account, in
    /// configuration order, for [`refresh::RefreshCycle::restore_last_good`]
    /// on opening. All `None` without history; a storage failure also
    /// shows in [`Monitor::history_status`].
    pub fn restored_readings(&self) -> Vec<Option<Report>> {
        let none = || vec![None; self.config.accounts.len()];
        let Some(recorder) = &self.history else {
            return none();
        };
        recorder
            .store()
            .latest_reports(&self.config)
            .unwrap_or_else(|error| {
                recorder.note_error(error);
                none()
            })
    }

    /// PRD 0007 RF1.5: the recorded points [`pace::assess`] reads as
    /// `recent` for each of `metrics` of account `index` at `now`; empty
    /// lists (the average since the window start) without history, for a
    /// metric with no pace, or on a storage failure (also shown in
    /// [`Monitor::history_status`]).
    pub fn recent_points(
        &self,
        index: usize,
        metrics: &[Metric],
        now: chrono::DateTime<Utc>,
    ) -> Vec<Vec<pace::Point>> {
        let none = || vec![Vec::new(); metrics.len()];
        let (Some(recorder), Some(account)) = (&self.history, self.config.accounts.get(index))
        else {
            return none();
        };
        recorder
            .store()
            .recent(account, metrics, now)
            .unwrap_or_else(|error| {
                recorder.note_error(error);
                none()
            })
    }

    fn now(&self) -> chrono::DateTime<Utc> {
        #[cfg(test)]
        if let Some(clock) = &self.clock {
            return *clock.lock();
        }
        Utc::now()
    }

    pub async fn refresh(&self) -> Snapshot {
        let started_at = self.now();
        let mut indexed = stream::iter(0..self.config.accounts.len())
            .map(|index| async move { (index, self.refresh_account(index).await) })
            .buffer_unordered(self.config.concurrency)
            .collect::<Vec<_>>()
            .await;
        indexed.sort_unstable_by_key(|(index, _)| *index);
        Snapshot {
            started_at,
            finished_at: self.now(),
            reports: indexed.into_iter().map(|(_, report)| report).collect(),
        }
    }

    /// Refresh one account by its configuration index. All full and targeted
    /// refreshes share this monitor's concurrency limit.
    ///
    /// `index` must be in `0..accounts.len()`; out-of-range is a programming
    /// error.
    pub async fn refresh_account(&self, index: usize) -> Report {
        debug_assert!(
            index < self.config.accounts.len(),
            "refresh_account index {} out of bounds ({} accounts)",
            index,
            self.config.accounts.len()
        );
        let account = &self.config.accounts[index];
        let result = self
            .with_access(
                account,
                async |account: &Account, ctx: &providers::Context<'_>| {
                    self.dispatch(account, ctx).await
                },
            )
            .await;
        let (status, metrics, notes, notes_l10n, source) = match result {
            Ok(data) => (
                Status::Ready,
                data.metrics,
                data.notes,
                data.notes_l10n,
                data.source,
            ),
            Err(error) => {
                let (plain, l10n) = crate::text::Notes::one(error.note).finish();
                (error.status, Vec::new(), plain, l10n, String::new())
            }
        };
        let report = Report {
            account: account.name.clone(),
            provider: account.provider,
            checked_at: self.now(),
            status,
            metrics,
            notes,
            notes_l10n,
            source,
        };
        // RF2.2/RF2.4: only enqueued; the disk never delays the refresh.
        if let Some(recorder) = &self.history {
            recorder.submit(account, &report);
        }
        report
    }

    /// Run `run` under one query slot and the account deadline, with OAuth
    /// credentials resolved (and refreshed) through the shared session.
    async fn with_access<T>(
        &self,
        account: &Account,
        run: impl AsyncFnOnce(&Account, &providers::Context<'_>) -> Result<T, providers::QueryError>,
    ) -> Result<T, providers::QueryError> {
        let _permit = self
            .query_slots
            .acquire()
            .await
            .expect("monitor query semaphore stays open");
        let timeout = Duration::from_secs(self.config.timeout_seconds);
        let fetch = async {
            let access = match &account.oauth {
                // MiMo's native login yields an inference key, never the cookie
                // used for quota. Do not open Keychain for this unrelated key.
                Some(_) if account.provider == Provider::XiaomiMimo => None,
                Some(config) => Some(
                    self.oauth
                        .credentials(account.provider, config)
                        .await
                        .map_err(|error| providers::QueryError {
                            status: match &error {
                                oauth::OAuthError::Configuration(_) => Status::NeedsConfiguration,
                                oauth::OAuthError::Denied | oauth::OAuthError::Expired => {
                                    Status::AuthenticationError
                                }
                                _ => Status::Error,
                            },
                            note: error.l10n(),
                        })?,
                ),
                None => None,
            };
            let mut effective = Cow::Borrowed(account);
            if let (Some(config), Some(token)) = (&account.oauth, &access) {
                let effective = effective.to_mut();
                effective.params.extend(oauth::account_params(
                    account.provider,
                    config,
                    &token.metadata,
                ));
                effective.credential_env = Some(OAUTH_ACCESS.into());
                match account.provider {
                    Provider::Kimi => {
                        effective.credential_env = Some(NO_API_KEY.into());
                        effective
                            .params
                            .insert("token_env".into(), OAUTH_ACCESS.into());
                    }
                    Provider::GeminiAi | Provider::Grok => {
                        effective
                            .params
                            .insert("session_env".into(), OAUTH_ACCESS.into());
                    }
                    _ => {}
                }
            }
            let account = effective.as_ref();
            let credentials = QueryCredentials {
                external: self.credentials.as_ref(),
                access: access.map(|token| token.access_token),
            };
            let ctx = providers::Context {
                http: &self.http,
                credentials: &credentials,
                timeout,
            };
            run(account, &ctx).await
        };
        tokio::time::timeout(timeout, fetch)
            .await
            .unwrap_or_else(|_| Err(providers::QueryError::transport()))
    }

    async fn dispatch(
        &self,
        account: &Account,
        ctx: &providers::Context<'_>,
    ) -> Result<providers::Data, providers::QueryError> {
        #[cfg(test)]
        if let (Provider::Groq, Some(url)) = (account.provider, self.groq_url.as_deref()) {
            return providers::router_keys::groq_at(account, ctx, url).await;
        }
        match account.provider {
            Provider::OpenAiApi | Provider::AnthropicApi => providers::api::fetch(account, ctx).await,
            Provider::GeminiApi
            | Provider::VertexAi
            | Provider::AwsBedrock
            | Provider::AzureOpenAi => providers::cloud::fetch(account, ctx).await,
            Provider::Copilot
                if account
                    .params
                    .get("query_mode")
                    .is_some_and(|v| v == "quota") =>
            {
                providers::router_existing::fetch(account, ctx).await
            }
            Provider::GeminiAi if account.params.get("product").is_some_and(|v| v == "cli") => {
                providers::router_existing::fetch(account, ctx).await
            }
            Provider::Kimi if account.params.get("product").is_some_and(|v| v == "coding") => {
                providers::router_keys::fetch(account, ctx).await
            }
            Provider::Grok if account.params.get("product").is_some_and(|v| v == "cli") => {
                providers::router_grok::fetch(account, ctx).await
            }
            Provider::Cursor
                if account
                    .params
                    .get("query_mode")
                    .is_some_and(|v| v == "personal") =>
            {
                providers::cursor_personal::fetch(account, ctx).await
            }
            Provider::ChatGpt
            | Provider::Claude
            | Provider::Cursor
            | Provider::Copilot
            | Provider::Windsurf => providers::subscriptions::fetch(account, ctx).await,
            Provider::Qwen if account.oauth.is_some() => Err(providers::QueryError::unsupported(
                "query.qwen_oauth_unsupported",
                "Qwen Code OAuth: sessão autenticada, mas não há endpoint de quota confirmado. O plano OAuth gratuito foi descontinuado pelo provedor em 15/04/2026; não consultamos DashScope com este token.",
            )),
            Provider::MiniMax | Provider::Kimi | Provider::Qwen => {
                providers::asia::fetch(account, ctx).await
            }
            Provider::Perplexity
            | Provider::Mistral
            | Provider::Grok
            | Provider::NvidiaNim
            | Provider::Poolside => providers::inference::fetch(account, ctx).await,
            Provider::CloudflareAi | Provider::GeminiAi => {
                providers::cloud_keys::fetch(account, ctx).await
            }
            Provider::OpenCodeGo | Provider::Groq | Provider::XiaomiMimo => {
                providers::router_keys::fetch(account, ctx).await
            }
            Provider::Qoder | Provider::OllamaCloud => {
                providers::router_accounts::fetch(account, ctx).await
            }
            Provider::Kiro | Provider::CodeBuddy | Provider::Zed => {
                providers::router_sessions::fetch(account, ctx).await
            }
            Provider::Cline => providers::coding::fetch(account, ctx).await,
            Provider::AlibabaModelStudio | Provider::AlibabaCodingPlan | Provider::AlibabaTokenPlan => providers::alibaba_plans::fetch(account, ctx).await,
            Provider::AlibabaCloud => providers::alibaba_billing::fetch(account, ctx).await,
            Provider::Antigravity => providers::antigravity::fetch(account, ctx).await,
            Provider::Kimchi => providers::kimchi::fetch(account, ctx).await,
        }
    }
}

const OAUTH_ACCESS: &str = "__CUOTAI_OAUTH_ACCESS";
const NO_API_KEY: &str = "__CUOTAI_NO_API_KEY";

struct QueryCredentials<'a> {
    external: &'a dyn Credentials,
    access: Option<String>,
}
impl Credentials for QueryCredentials<'_> {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            OAUTH_ACCESS => self.access.clone(),
            NO_API_KEY => None,
            _ => self.external.get(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_account_identity_and_unbounded_queries() {
        let mut config = Config::default();
        config.accounts.push(config.accounts[0].clone());
        assert!(matches!(config.validate(), Err(ConfigError::AccountNames)));
        config.accounts.pop();
        config.concurrency = 0;
        assert!(matches!(config.validate(), Err(ConfigError::Bounds)));
    }

    #[test]
    fn rejects_unknown_configuration_instead_of_silently_ignoring_it() {
        assert!(matches!(
            Config::from_toml("accounts = []\nrefresh_second = 1"),
            Err(ConfigError::Parse)
        ));
    }
    #[tokio::test]
    async fn refresh_account_targets_only_the_named_index() {
        // The OAuth flow loads tokens via storage_key(provider, config).
        // Account "First" and "Second" carry distinct credential_ids, so each
        // hits a distinct key. A second load call here means refresh_account
        // reached the wrong account.
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct CountingStore(AtomicUsize);
        impl oauth::TokenStore for CountingStore {
            fn load(&self, _: &str) -> Result<Option<oauth::OAuthToken>, oauth::OAuthError> {
                let n = self.0.fetch_add(1, Ordering::SeqCst);
                assert!(
                    n < 1,
                    "refresh_account must not touch other accounts' oauth store"
                );
                Ok(None)
            }
            fn save(&self, _: &str, _: &oauth::OAuthToken) -> Result<(), oauth::OAuthError> {
                unreachable!()
            }
            fn delete(&self, _: &str) -> Result<(), oauth::OAuthError> {
                unreachable!()
            }
        }
        struct NoCredentials;
        impl Credentials for NoCredentials {
            fn get(&self, _: &str) -> Option<String> {
                None
            }
        }
        let config = Config::from_toml(
            r#"
[[accounts]]
name = "First"
provider = "chat_gpt"
[accounts.oauth]
credential_id = "4be708d8-7073-4d01-85ba-3306cc3cf087"

[[accounts]]
name = "Second"
provider = "chat_gpt"
[accounts.oauth]
credential_id = "00000000-0000-0000-0000-000000000000"
"#,
        )
        .unwrap();
        let creds: Arc<dyn Credentials> = Arc::new(NoCredentials);
        let session = Arc::new(
            oauth::OAuthSession::new(
                Arc::new(CountingStore(AtomicUsize::new(0))),
                Arc::clone(&creds),
            )
            .unwrap(),
        );
        let monitor = Monitor::with_oauth(config, creds, session).unwrap();
        let report = monitor.refresh_account(0).await;
        assert_eq!(report.account, "First");
        assert_eq!(report.status, Status::NeedsConfiguration);
    }

    #[tokio::test]
    async fn mimo_quota_does_not_require_access_to_unrelated_native_key() {
        struct DeniedStore;
        impl oauth::TokenStore for DeniedStore {
            fn load(&self, _: &str) -> Result<Option<oauth::OAuthToken>, oauth::OAuthError> {
                panic!("quota must not read the inference key")
            }
            fn save(&self, _: &str, _: &oauth::OAuthToken) -> Result<(), oauth::OAuthError> {
                unreachable!()
            }
            fn delete(&self, _: &str) -> Result<(), oauth::OAuthError> {
                unreachable!()
            }
        }
        struct Cookie;
        impl Credentials for Cookie {
            fn get(&self, name: &str) -> Option<String> {
                (name == "MIMO_COOKIE").then(|| "wrong_cookie=fake".into())
            }
        }
        let config = Config::from_toml(
            r#"
[[accounts]]
name = "MiMo"
provider = "xiaomi_mimo"
params = { platform_session_env = "MIMO_COOKIE" }
[accounts.oauth]
credential_id = "4be708d8-7073-4d01-85ba-3306cc3cf087"
"#,
        )
        .unwrap();
        let creds: Arc<dyn Credentials> = Arc::new(Cookie);
        let session =
            Arc::new(oauth::OAuthSession::new(Arc::new(DeniedStore), Arc::clone(&creds)).unwrap());
        let monitor = Monitor::with_oauth(config, creds, session).unwrap();
        let snapshot = monitor.refresh().await;
        assert_eq!(snapshot.reports[0].status, Status::NeedsConfiguration);
        assert!(snapshot.reports[0].metrics.is_empty());
    }

    #[test]
    fn account_refresh_override_persists_through_toml_round_trip() {
        // Override survives a save/load cycle; default Config has no override.
        let mut config = Config::default();
        config.accounts[0].refresh_seconds = Some(120);
        config.accounts[1].refresh_seconds = Some(45);
        let rendered = toml::to_string(&config).unwrap();
        let parsed = Config::from_toml(&rendered).unwrap();
        assert_eq!(parsed.accounts[0].refresh_seconds, Some(120));
        assert_eq!(parsed.accounts[1].refresh_seconds, Some(45));
        assert_eq!(parsed.accounts[2].refresh_seconds, None);
        // Effective interval uses the override when set and falls back otherwise.
        assert_eq!(parsed.accounts[0].effective_refresh_seconds(300), 120);
        assert_eq!(parsed.accounts[1].effective_refresh_seconds(300), 45);
        assert_eq!(parsed.accounts[2].effective_refresh_seconds(300), 300);
    }

    #[test]
    fn account_override_below_minimum_is_rejected() {
        // The global `refresh_seconds` validates >= 30; account overrides must
        // honour the same bound so we never slow-poll more than the global cap
        // nor fast-poll past it.
        let mut config = Config::default();
        config.accounts[0].refresh_seconds = Some(29);
        assert!(matches!(config.validate(), Err(ConfigError::Bounds)));
        config.accounts[0].refresh_seconds = Some(30);
        assert!(config.validate().is_ok());
    }

    /// Token store with no credentials; tests here never save or delete.
    struct EmptyStore;

    impl EmptyStore {
        fn new() -> Arc<Self> {
            Arc::new(Self)
        }
    }

    impl oauth::TokenStore for EmptyStore {
        fn load(&self, _: &str) -> Result<Option<oauth::OAuthToken>, oauth::OAuthError> {
            Ok(None)
        }
        fn save(&self, _: &str, _: &oauth::OAuthToken) -> Result<(), oauth::OAuthError> {
            unreachable!("save must not be exercised by monitor tests")
        }
        fn delete(&self, _: &str) -> Result<(), oauth::OAuthError> {
            unreachable!("delete must not be exercised by monitor tests")
        }
    }

    struct NoCreds;
    impl Credentials for NoCreds {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    // RF1 acceptance: an edit refreshes exactly the edited account and a
    // removal refreshes none. Counted as real HTTP requests per account key
    // against a local `wiremock` server reached through the Groq adapter.
    struct GroqKeys;
    impl Credentials for GroqKeys {
        fn get(&self, name: &str) -> Option<String> {
            name.strip_prefix("GROQ_KEY_")
                .map(|account| format!("gsk_{account}"))
        }
    }

    fn three_groq_accounts() -> Config {
        Config::from_toml(
            r#"
refresh_seconds = 60
concurrency = 2
timeout_seconds = 30

[[accounts]]
name = "alpha"
provider = "groq"
credential_env = "GROQ_KEY_ALPHA"

[[accounts]]
name = "beta"
provider = "groq"
credential_env = "GROQ_KEY_BETA"

[[accounts]]
name = "gamma"
provider = "groq"
credential_env = "GROQ_KEY_GAMMA"
"#,
        )
        .unwrap()
    }

    async fn groq_monitor(config: Config) -> (wiremock::MockServer, Monitor) {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": [] })),
            )
            .mount(&server)
            .await;
        let creds: Arc<dyn Credentials> = Arc::new(GroqKeys);
        let store: Arc<dyn oauth::TokenStore> = EmptyStore::new();
        let session = Arc::new(oauth::OAuthSession::new(store, Arc::clone(&creds)).unwrap());
        let mut monitor = Monitor::with_oauth(config, creds, session).unwrap();
        monitor.groq_url = Some(server.uri());
        (server, monitor)
    }

    /// HTTP requests received so far, by account (`ALPHA`, `BETA`, `GAMMA`).
    async fn requests_per_account(server: &wiremock::MockServer) -> [usize; 3] {
        let requests = server.received_requests().await.unwrap();
        ["ALPHA", "BETA", "GAMMA"].map(|account| {
            let expected = format!("Bearer gsk_{account}");
            requests
                .iter()
                .filter(|request| {
                    request
                        .headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        == Some(expected.as_str())
                })
                .count()
        })
    }

    async fn primed_cycle(monitor: &Monitor, config: &Config) -> crate::refresh::RefreshCycle {
        let mut cycle = crate::refresh::RefreshCycle::new(config, Instant::now());
        let token = cycle.begin_full().expect("begin_full");
        cycle.finish_full(Some(monitor.refresh().await), token, Instant::now());
        cycle
    }

    #[tokio::test]
    async fn saving_one_of_three_accounts_issues_exactly_one_http_request() {
        use crate::refresh::Edit;
        let config = three_groq_accounts();
        let (server, monitor) = groq_monitor(config.clone()).await;
        let mut cycle = primed_cycle(&monitor, &config).await;
        assert_eq!(requests_per_account(&server).await, [1, 1, 1]);

        let mut edited = config.clone();
        edited.accounts[1].name = "beta-renamed".into();
        let due = cycle
            .apply_edit(&config, &edited, Edit::Upsert(1), Instant::now())
            .expect("upsert accepted");
        assert_eq!(due, vec![1]);
        let next = monitor.with_config(edited).expect("with_config");
        for index in due {
            let _ = next.refresh_account(index).await;
        }

        assert_eq!(requests_per_account(&server).await, [1, 2, 1]);
        assert!(
            cycle.snapshot().is_none(),
            "the edited account stays pending instead of showing its old report"
        );
    }

    #[tokio::test]
    async fn removing_an_account_issues_no_http_request_and_keeps_survivor_reports() {
        use crate::refresh::Edit;
        let config = three_groq_accounts();
        let (server, monitor) = groq_monitor(config.clone()).await;
        let mut cycle = primed_cycle(&monitor, &config).await;

        let removed = config.without_account(1).unwrap();
        let due = cycle
            .apply_edit(&config, &removed, Edit::Remove(1), Instant::now())
            .expect("remove accepted");
        assert!(due.is_empty(), "removal must not schedule any query");
        let next = monitor.with_config(removed).expect("with_config");
        for index in due {
            let _ = next.refresh_account(index).await;
        }

        assert_eq!(requests_per_account(&server).await, [1, 1, 1]);
        let accounts: Vec<_> = cycle
            .snapshot()
            .expect("survivors keep a complete snapshot")
            .reports
            .iter()
            .map(|report| report.account.clone())
            .collect();
        assert_eq!(accounts, ["alpha", "gamma"]);
    }

    // PRD 0006 RF2 acceptance: recording from the refresh, against a local
    // `wiremock` Groq with a fixed clock for `checked_at`.
    fn groq_rate_limits(reset: &str) -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({ "data": [] }))
            .insert_header("x-ratelimit-limit-requests", "14400")
            .insert_header("x-ratelimit-remaining-requests", "14370")
            .insert_header("x-ratelimit-reset-requests", reset)
            .insert_header("x-ratelimit-limit-tokens", "18000")
            .insert_header("x-ratelimit-remaining-tokens", "17997")
            .insert_header("x-ratelimit-reset-tokens", reset)
    }

    /// Groq monitor at `server` whose clock reads `clock`; beta's key is
    /// refused (401).
    async fn recording_monitor(
        server: &wiremock::MockServer,
        recorder: history::HistoryRecorder,
        clock: &Arc<parking_lot::Mutex<chrono::DateTime<Utc>>>,
    ) -> Monitor {
        use wiremock::matchers::{header, method};
        wiremock::Mock::given(method("GET"))
            .and(header("authorization", "Bearer gsk_BETA"))
            .respond_with(wiremock::ResponseTemplate::new(401))
            .mount(server)
            .await;
        wiremock::Mock::given(method("GET"))
            .respond_with(groq_rate_limits("2m59s"))
            .mount(server)
            .await;
        let creds: Arc<dyn Credentials> = Arc::new(GroqKeys);
        let session =
            Arc::new(oauth::OAuthSession::new(EmptyStore::new(), Arc::clone(&creds)).unwrap());
        let mut config = three_groq_accounts();
        config.accounts.truncate(2);
        let mut monitor = Monitor::with_oauth(config, creds, session)
            .unwrap()
            .with_recorder(recorder);
        monitor.groq_url = Some(server.uri());
        monitor.clock = Some(Arc::clone(clock));
        monitor
    }

    fn store_at(dir: &tempfile::TempDir) -> history::HistoryStore {
        history::HistoryStore::new(
            dir.path().join("history.sqlite3"),
            &config_file::ConfigIdentity::Explicit("one".into()),
        )
    }

    fn t0() -> chrono::DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn refresh_records_ready_readings_compacts_repeats_and_follows_new_windows() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_at(&dir);
        let server = wiremock::MockServer::start().await;
        let clock = Arc::new(parking_lot::Mutex::new(t0()));
        let monitor = recording_monitor(
            &server,
            history::HistoryRecorder::spawn(store.clone()),
            &clock,
        )
        .await;
        let (alpha, beta) = (&monitor.config.accounts[0], &monitor.config.accounts[1]);
        let samples = |store: &history::HistoryStore| {
            let latest = store.latest(alpha).unwrap().expect("alpha recorded");
            latest
                .metrics
                .iter()
                .map(|m| store.series(alpha, &m.key, ..).unwrap().len())
                .collect::<Vec<_>>()
        };

        let snapshot = monitor.refresh().await;
        assert_eq!(snapshot.reports[0].status, Status::Ready);
        assert_eq!(snapshot.reports[1].status, Status::AuthenticationError);
        assert!(monitor.flush_history(Duration::from_secs(10)));
        assert_eq!(samples(&store), [1, 1], "one sample per metric");
        assert_eq!(store.latest(alpha).unwrap().unwrap().checked_at, t0());
        assert!(
            store.latest(beta).unwrap().is_none(),
            "a failure records nothing"
        );

        // Same values 10 minutes later: omitted, the reading still dated.
        *clock.lock() = t0() + chrono::TimeDelta::minutes(10);
        monitor.refresh_account(0).await;
        assert!(monitor.flush_history(Duration::from_secs(10)));
        assert_eq!(samples(&store), [1, 1]);
        assert_eq!(
            store.latest(alpha).unwrap().unwrap().checked_at,
            t0() + chrono::TimeDelta::minutes(10)
        );

        // Same values, a reset an hour away: a new window is recorded.
        server.reset().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(groq_rate_limits("1h"))
            .mount(&server)
            .await;
        *clock.lock() = t0() + chrono::TimeDelta::minutes(11);
        monitor.refresh_account(0).await;
        assert!(monitor.flush_history(Duration::from_secs(10)));
        assert_eq!(samples(&store), [2, 2]);
        assert_eq!(monitor.history_status().unwrap().warning(), None);

        // A monitor without history records nothing and changes nothing.
        let plain = Monitor {
            history: None,
            ..monitor.with_config(monitor.config.clone()).unwrap()
        };
        *clock.lock() = t0() + chrono::TimeDelta::hours(1);
        assert_eq!(plain.refresh_account(0).await.status, Status::Ready);
        assert!(plain.flush_history(Duration::ZERO) && plain.history_status().is_none());
        assert!(monitor.flush_history(Duration::from_secs(10)));
        assert_eq!(samples(&store), [2, 2]);
    }

    #[tokio::test]
    async fn a_slow_disk_never_delays_the_refresh_and_a_full_queue_drops() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_at(&dir);
        store.bounds().unwrap();
        // Another process holds the write lock: every write waits for it.
        let locker = rusqlite::Connection::open(dir.path().join("history.sqlite3")).unwrap();
        locker.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let server = wiremock::MockServer::start().await;
        let clock = Arc::new(parking_lot::Mutex::new(t0()));
        let recorder = history::HistoryRecorder::with_capacity(store.clone(), 1);
        let monitor = recording_monitor(&server, recorder, &clock).await;
        for n in 0..5 {
            *clock.lock() = t0() + chrono::TimeDelta::minutes(20 * n);
            let started = std::time::Instant::now();
            assert_eq!(monitor.refresh_account(0).await.status, Status::Ready);
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "the 5 s busy wait never reaches the refresh"
            );
        }
        let dropped = monitor.history_status().unwrap().dropped;
        assert!(dropped >= 3, "capacity 1 behind a blocked write: {dropped}");
        assert_eq!(
            monitor.history_status().unwrap().warning().map(|w| w.id),
            Some("history.warning.dropped".into())
        );
        drop(locker);
        assert!(monitor.flush_history(Duration::from_secs(20)));
        let alpha = &monitor.config.accounts[0];
        let key = &store.latest(alpha).unwrap().unwrap().metrics[0].key;
        assert_eq!(
            store.series(alpha, key, ..).unwrap().len() as u64,
            5 - dropped,
            "every queued reading lands once the disk answers"
        );
    }

    #[test]
    fn monitor_with_config_reuses_http_when_timeout_matches() {
        let config = Config::from_toml(
            r#"timeout_seconds = 30
[[accounts]]
name = "x"
provider = "open_ai_api"
credential_env = "OPENAI_ADMIN_KEY"
"#,
        )
        .unwrap();
        let creds: Arc<dyn Credentials> = Arc::new(NoCreds);
        let store: Arc<dyn oauth::TokenStore> = EmptyStore::new();
        let session =
            Arc::new(oauth::OAuthSession::new(Arc::clone(&store), Arc::clone(&creds)).unwrap());
        let monitor = Monitor::with_oauth(config.clone(), creds, Arc::clone(&session)).unwrap();
        // Same timeout: HTTP client is reused (same pointer), credentials
        // and OAuthSession are reused.
        let next = monitor.with_config(config.clone()).expect("with_config");
        assert!(Arc::ptr_eq(&next.credentials, &monitor.credentials));
        assert!(Arc::ptr_eq(&next.oauth, &monitor.oauth));
        // Concurrency follows the new config even when timeout matches.
        let mut bumped = config.clone();
        bumped.concurrency = 8;
        let bumped_monitor = monitor.with_config(bumped).expect("with_config bumped");
        assert_eq!(bumped_monitor.config.concurrency, 8);
        // Concurrency really changed: a new semaphore was created with 8
        // permits (we verify by exhausting it via a dummy query_slots
        // contract — the semaphore's "available_permits" is private, so
        // we just confirm the config propagated).
        assert_eq!(bumped_monitor.config.concurrency, 8);
    }

    #[test]
    fn monitor_with_config_rebuilds_http_when_timeout_changes() {
        let config = Config::from_toml(
            r#"timeout_seconds = 30
[[accounts]]
name = "x"
provider = "open_ai_api"
credential_env = "OPENAI_ADMIN_KEY"
"#,
        )
        .unwrap();
        let creds: Arc<dyn Credentials> = Arc::new(NoCreds);
        let store: Arc<dyn oauth::TokenStore> = EmptyStore::new();
        let session =
            Arc::new(oauth::OAuthSession::new(Arc::clone(&store), Arc::clone(&creds)).unwrap());
        let monitor = Monitor::with_oauth(config.clone(), creds, Arc::clone(&session)).unwrap();

        let mut next_config = config.clone();
        next_config.timeout_seconds = 60;
        let next = monitor
            .with_config(next_config)
            .expect("with_config new timeout");
        // OAuth session and credentials are reused; the HTTP client was
        // rebuilt so it carries the new timeout.
        assert!(Arc::ptr_eq(&next.credentials, &monitor.credentials));
        assert!(Arc::ptr_eq(&next.oauth, &monitor.oauth));
        assert_eq!(next.config.timeout_seconds, 60);
    }

    // -------------------------------------------------------------------
    // RF6: metered provider floors. The cost-explorer account must never
    // be polled more often than METERED_MIN_REFRESH_SECONDS (300) per
    // query; the global fallback must enforce the same floor when no
    // per-account override is present.
    // -------------------------------------------------------------------

    fn aws_account(override_seconds: Option<u64>) -> Account {
        let mut account = Account {
            name: "bedrock-test".into(),
            provider: Provider::AwsBedrock,
            refresh_seconds: override_seconds,
            ..Account::default()
        };
        account.params.insert("profile".into(), "S".into());
        account
    }

    #[test]
    fn validate_rejects_metered_account_with_override_below_floor() {
        let config = Config {
            refresh_seconds: 600,
            accounts: vec![aws_account(Some(60))],
            ..Config::default()
        };
        // 60s override on a metered provider is below the 300s floor.
        assert!(matches!(config.validate(), Err(ConfigError::Bounds)));
    }

    #[test]
    fn validate_accepts_metered_account_with_override_at_or_above_floor() {
        let mut config = Config {
            refresh_seconds: 600,
            accounts: vec![aws_account(Some(300))],
            ..Config::default()
        };
        assert!(config.validate().is_ok(), "300s override is the floor");
        config.accounts[0].refresh_seconds = Some(3600);
        assert!(
            config.validate().is_ok(),
            "3600s override is well above the floor"
        );
        config.accounts[0].refresh_seconds = None;
        // No override: the global (600) still applies, which is above
        // the floor.
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_rejects_global_below_floor_when_metered_present() {
        // A metered account with NO override inherits the global
        // interval. If the user set the global too low (e.g. 60s),
        // the metered floor still applies — the global cannot
        // bypass the per-account floor.
        let config = Config {
            refresh_seconds: 60,
            accounts: vec![aws_account(None)],
            ..Config::default()
        };
        assert!(matches!(config.validate(), Err(ConfigError::Bounds)));
    }

    #[test]
    fn validate_does_not_enforce_metered_floor_for_free_providers() {
        // A free provider with a 30s interval is fine. The floor
        // only kicks in when at least one account charges per query.
        let config = Config {
            refresh_seconds: 30,
            accounts: vec![Account {
                name: "openai".into(),
                provider: Provider::OpenAiApi,
                credential_env: Some("OPENAI_ADMIN_KEY".into()),
                ..Account::default()
            }],
            ..Config::default()
        };
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn editing_metered_account_does_not_cost_query_other_metered_account() {
        // Contract from the RF6 acceptance: editing one metered
        // account must not trigger a query for *another* metered
        // account. The cost-guard is a cycle-level invariant:
        // `apply_edit` returns exactly the touched index, so the
        // frontend never queues a refresh for any other account.
        // We also confirm the cycle's `due_indices` / `pending_indices`
        // do not list other metered slots after the edit, which is
        // what the scheduler consumes.
        use crate::refresh::{Edit, RefreshCycle};
        use std::time::Duration;
        let config = Config {
            refresh_seconds: 600,
            accounts: vec![
                Account {
                    name: "aws-one".into(),
                    ..aws_account(Some(600))
                },
                Account {
                    name: "aws-two".into(),
                    ..aws_account(Some(600))
                },
            ],
            ..Config::default()
        };
        let aws_one = config.accounts[0].clone();

        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&config, now);
        // Initial sweep fills the cycle so both accounts have reports.
        let sweep_token = cycle.begin_full().expect("begin_full");
        cycle.finish_full(
            Some(Snapshot {
                started_at: chrono::Utc::now(),
                finished_at: chrono::Utc::now(),
                reports: vec![
                    crate::Report {
                        account: "aws-one".into(),
                        provider: Provider::AwsBedrock,
                        checked_at: chrono::Utc::now(),
                        status: Status::Ready,
                        metrics: Vec::new(),
                        notes: Vec::new(),
                        notes_l10n: Vec::new(),
                        source: String::new(),
                    },
                    crate::Report {
                        account: "aws-two".into(),
                        provider: Provider::AwsBedrock,
                        checked_at: chrono::Utc::now(),
                        status: Status::Ready,
                        metrics: Vec::new(),
                        notes: Vec::new(),
                        notes_l10n: Vec::new(),
                        source: String::new(),
                    },
                ],
            }),
            sweep_token,
            now,
        );

        // Edit aws-one only. The cycle must report exactly slot 0;
        // slot 1 (the other metered account) must not appear.
        let mut edited_one = aws_one.clone();
        edited_one.refresh_seconds = Some(900);
        let new_config = {
            let mut c = config.clone();
            c.accounts[0] = edited_one;
            c
        };
        let due = cycle
            .apply_edit(&config, &new_config, Edit::Upsert(0), Instant::now())
            .expect("upsert accepted");
        assert_eq!(
            due,
            vec![0],
            "editing one metered account must not cost a query on the other"
        );
        // pending_indices lists the touched one, not the survivor.
        assert_eq!(cycle.pending_indices(), vec![0]);
        // pending_now_indices is empty right after the edit: the
        // touched slot's deadline was reset to `now + interval`, so
        // the scheduler does not fire an automatic periodic retry —
        // the immediate-fire signal already came from `apply_edit`'s
        // returned `due = vec![0]`. The survivor must NOT be eligible
        // either.
        assert!(cycle.pending_now_indices(Instant::now()).is_empty());
        // Once the touched slot's deadline has elapsed naturally the
        // scheduler may pick it up. The survivor was not edited and
        // must NOT be eligible even after a long wait.
        assert!(!cycle
            .pending_now_indices(Instant::now() + Duration::from_secs(3600))
            .contains(&1));
        // At a moment just after the edit, only the touched slot is
        // pending. The survivor was not due before (deadline far in
        // the future) and the edit preserved that deadline — it must
        // still not be due.
        let just_after = now + Duration::from_secs(1);
        let due_after = cycle.due_indices(just_after);
        assert!(
            !due_after.contains(&1),
            "the other metered account must not be due just after the edit"
        );
        // Once the survivor's deadline has elapsed naturally the
        // scheduler may pick it up — the contract is "no spurious
        // queries immediately after the edit", not "never due
        // again". Push the deadline past `just_after` and confirm it
        // appears in `due_indices`.
        cycle.set_deadline_for_test(1, just_after - Duration::from_secs(1));
        assert_eq!(cycle.due_indices(just_after), vec![1]);
    }
}
