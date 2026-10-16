//! Read-only AI usage monitoring, independent of terminal and windowing frameworks.
//! Call `Monitor::refresh` from an async runtime. A failed account does not fail
//! other accounts; reports retain configuration order and never contain secrets.
pub mod metrics;
mod model;
pub mod oauth;
mod providers;

use chrono::Utc;
use futures_util::{stream, StreamExt};
pub use model::*;
use std::{borrow::Cow, collections::HashSet, sync::Arc, time::Duration};

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
        "refresh_seconds deve ser >= 30, concurrency entre 1 e 32 e timeout_seconds entre 1 e 300."
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
        Ok(Self {
            config,
            http,
            credentials,
            oauth,
        })
    }

    pub async fn refresh(&self) -> Snapshot {
        let started_at = Utc::now();
        let mut indexed = stream::iter(0..self.config.accounts.len())
            .map(|index| async move { (index, self.refresh_account(index).await) })
            .buffer_unordered(self.config.concurrency)
            .collect::<Vec<_>>()
            .await;
        indexed.sort_unstable_by_key(|(index, _)| *index);
        Snapshot {
            started_at,
            finished_at: Utc::now(),
            reports: indexed.into_iter().map(|(_, report)| report).collect(),
        }
    }

    /// Refresh a single account by its configuration index, returning one report.
    /// `index` must be in `0..accounts.len()`; out-of-range is a programming error.
    pub async fn refresh_account(&self, index: usize) -> Report {
        debug_assert!(
            index < self.config.accounts.len(),
            "refresh_account index {} out of bounds ({} accounts)",
            index,
            self.config.accounts.len()
        );
        self.query(&self.config.accounts[index]).await
    }

    async fn query(&self, account: &Account) -> Report {
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
                            message: error.to_string(),
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
            match account.provider {
                Provider::OpenAiApi | Provider::AnthropicApi => {
                    providers::api::fetch(account, &ctx).await
                }
                Provider::GeminiApi
                | Provider::VertexAi
                | Provider::AwsBedrock
                | Provider::AzureOpenAi => providers::cloud::fetch(account, &ctx).await,
                Provider::Copilot
                    if account
                        .params
                        .get("query_mode")
                        .is_some_and(|v| v == "quota") =>
                {
                    providers::router_existing::fetch(account, &ctx).await
                }
                Provider::GeminiAi if account.params.get("product").is_some_and(|v| v == "cli") => {
                    providers::router_existing::fetch(account, &ctx).await
                }
                Provider::Kimi if account.params.get("product").is_some_and(|v| v == "coding") => {
                    providers::router_keys::fetch(account, &ctx).await
                }
                Provider::Grok if account.params.get("product").is_some_and(|v| v == "cli") => {
                    providers::router_grok::fetch(account, &ctx).await
                }
                Provider::Cursor
                    if account
                        .params
                        .get("query_mode")
                        .is_some_and(|v| v == "personal") =>
                {
                    providers::cursor_personal::fetch(account, &ctx).await
                }
                Provider::ChatGpt
                | Provider::Claude
                | Provider::Cursor
                | Provider::Copilot
                | Provider::Windsurf => providers::subscriptions::fetch(account, &ctx).await,
                Provider::Qwen if account.oauth.is_some() => Err(providers::QueryError::unsupported("Qwen Code OAuth: sessão autenticada, mas não há endpoint de quota confirmado. O plano OAuth gratuito foi descontinuado pelo provedor em 15/04/2026; não consultamos DashScope com este token.")),
                Provider::MiniMax | Provider::Kimi | Provider::Qwen => {
                    providers::asia::fetch(account, &ctx).await
                }
                Provider::Perplexity
                | Provider::Mistral
                | Provider::Grok
                | Provider::NvidiaNim
                | Provider::Poolside => providers::inference::fetch(account, &ctx).await,
                Provider::CloudflareAi | Provider::GeminiAi => {
                    providers::cloud_keys::fetch(account, &ctx).await
                }
                Provider::OpenCodeGo | Provider::Groq | Provider::XiaomiMimo => {
                    providers::router_keys::fetch(account, &ctx).await
                }
                Provider::Qoder | Provider::OllamaCloud => {
                    providers::router_accounts::fetch(account, &ctx).await
                }
                Provider::Kiro | Provider::CodeBuddy | Provider::Zed => {
                    providers::router_sessions::fetch(account, &ctx).await
                }
                Provider::Cline => providers::coding::fetch(account, &ctx).await,
                Provider::AlibabaModelStudio | Provider::AlibabaCodingPlan | Provider::AlibabaTokenPlan => providers::alibaba_plans::fetch(account, &ctx).await,
                Provider::AlibabaCloud => providers::alibaba_billing::fetch(account, &ctx).await,
                Provider::Antigravity => providers::antigravity::fetch(account, &ctx).await,
                Provider::Kimchi => providers::kimchi::fetch(account, &ctx).await,
            }
        };
        let result = tokio::time::timeout(timeout, fetch)
            .await
            .unwrap_or_else(|_| Err(providers::QueryError::transport()));
        let (status, metrics, notes, source) = match result {
            Ok(data) => (Status::Ready, data.metrics, data.notes, data.source),
            Err(error) => (error.status, Vec::new(), vec![error.message], String::new()),
        };
        Report {
            account: account.name.clone(),
            provider: account.provider,
            checked_at: Utc::now(),
            status,
            metrics,
            notes,
            source,
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
}
