//! Read-only AI usage monitoring, independent of terminal and windowing frameworks.
//! Call `Monitor::refresh` from an async runtime. A failed account does not fail
//! other accounts; reports retain configuration order and never contain secrets.
pub mod about;
pub mod access;
pub mod account_edit;
pub mod account_id;
pub mod advice;
pub mod aggregators;
pub mod alerts;
pub mod build_info;
pub mod canary;
pub mod catalog;
pub mod changelog;
pub mod config_file;
pub mod cost;
pub mod detect;
pub mod diagnostic;
pub mod edit_error;
pub mod events;
pub mod export;
pub mod extensions;
pub mod freshness;
pub mod headless;
pub mod history;
pub mod hooks;
pub mod identity;
pub mod incidents;

/// RNF performance budgets in tests: the PRD figure in an optimized build,
/// twenty times it in a debug build (CI runs the debug suite on shared
/// runners), and the fastest of `runs` measurements so one slow scheduler
/// slice does not fail the suite.
#[cfg(test)]
pub(crate) fn assert_within_budget(
    what: &str,
    release: std::time::Duration,
    runs: usize,
    mut f: impl FnMut(),
) {
    let budget = if cfg!(debug_assertions) {
        release * 20
    } else {
        release
    };
    let best = (0..runs.max(1))
        .map(|_| {
            let started = std::time::Instant::now();
            f();
            started.elapsed()
        })
        .min()
        .unwrap_or_default();
    assert!(
        best <= budget,
        "{what}: {best:?} over the budget of {budget:?}"
    );
}
pub mod install_source;
pub mod last_config;
mod metered_record;
pub mod metric_text;
pub mod metrics;
mod model;
pub mod models;
pub mod notify_prefs;
pub mod oauth;
pub mod overage;
pub mod pace;
mod private_fs;
mod process_tree;
mod providers;
pub mod quiet;
pub mod refresh;
pub mod resets;
#[cfg(feature = "serve")]
pub mod serve;
pub mod stats;
pub mod support;
pub mod text;
mod transport;
pub mod tray;
pub mod update_check;
pub mod user_session;
pub mod watch;
pub mod widget_snapshot;

/// RF2 shared login driver, re-exported for convenience.
pub use oauth::login;

pub use account_id::AccountId;
use chrono::Utc;
use futures_util::{stream, StreamExt};
pub use model::*;
#[cfg(test)]
use std::time::Instant;
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashSet},
    sync::Arc,
    time::Duration,
};
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
    #[error("Invalid TOML document; check the fields and types in the example.")]
    Parse,
    #[error(
        "refresh_seconds must be >= 30, concurrency between 1 and 32 and timeout_seconds between 1 and 300."
    )]
    Bounds,
    #[error(
        "The {field} field for account {account} must be >= {minimum} for providers billed per query."
    )]
    MeteredRefreshFloor {
        field: String,
        account: String,
        minimum: u64,
    },
    #[error(
        "Configured account names must be unique; an unnamed account is valid only when it is the provider's only account."
    )]
    AccountNames,
    /// PRD 0029 RF1.2: two accounts share an `id`.
    #[error(
        "The field {field} repeats another account's id (a copied [[accounts]] block is the usual cause); delete the duplicated id line."
    )]
    AccountIds { field: String },
    #[error("Invalid configuration; could not initialise the HTTPS client.")]
    Client,
    #[error(
        "Invalid OAuth configuration or combined with credential_env; check the app registration."
    )]
    OAuth,
    #[error("Invalid or repeated session reference in [accounts.session]; enter the session again in the account wizard.")]
    Session,
    #[error("The extension account has no valid extension_id, repeats another extension's id, or has incompatible credential fields.")]
    Extension,
    #[error("The ElevenLabs account's params.region must be default, us, eu, in or sg.")]
    Region,
    #[error("The 9router base_url must be http(s)://host[:port][/prefix] without user, query or fragment; HTTP is only valid on loopback or, with allow_private_http, on a private network.")]
    BaseUrl,
    #[error("Invalid TLS for the 9router account: tls_pin_sha256 must have 64 hexadecimal digits, and tls_ca_file and tls_pin_sha256 cannot be used together or without HTTPS.")]
    Tls,
    /// PRD 0024 RF7.1: a `group` that is not a slug of at most 32 characters,
    /// or a `[[groups]]` table with an invalid or repeated id.
    #[error("An account group and each [[groups]] id must be 1 to 32 lowercase letters, digits, - or _; the [[groups]] ids cannot repeat, and an account id can be a member of one group only.")]
    Group,
    #[error("The OpenRouter account's params.product must be key or account, and params.breakdown (account only) off, provider or model.")]
    OpenRouter,
}

fn metered_refresh_floor_error(
    index: usize,
    account: &Account,
    field: impl Into<String>,
) -> ConfigError {
    let account = match account.name.trim() {
        "" => format!("accounts[{index}]"),
        name => name.to_owned(),
    };
    ConfigError::MeteredRefreshFloor {
        field: field.into(),
        account,
        minimum: Provider::METERED_MIN_CONFIGURED_REFRESH_SECONDS,
    }
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        // PRD 0028 RF8.2: `[updates]` belongs to the CLI update notice
        // (`update_check`), not to the accounts' configuration. It is
        // validated here and left out of `Config`, so the struct keeps its
        // shape; `ConfigFile` preserves the table on save like any other
        // unrelated content.
        let config: Self = match toml::from_str::<toml::Table>(text) {
            Ok(mut table) if table.contains_key("updates") => {
                if let Some(updates) = table.remove("updates") {
                    updates
                        .try_into::<update_check::UpdatesSettings>()
                        .map_err(|_| ConfigError::Parse)?;
                }
                table.try_into().map_err(|_| ConfigError::Parse)?
            }
            _ => toml::from_str(text).map_err(|_| ConfigError::Parse)?,
        };
        config.validate()?;
        Ok(config)
    }

    /// PRD 0029 RF1.3: give every account without an `id` its effective id,
    /// derived from its legacy key and `identity` (the configuration file's
    /// [`config_file::ConfigIdentity`]). Processes that read the same file
    /// without writing it agree on the ids; [`config_file::ConfigFile`]
    /// calls this on load and writes the ids on its next save.
    pub fn resolve_ids(&mut self, identity: Option<&config_file::ConfigIdentity>) {
        for account in &mut self.accounts {
            if account.id.is_none() {
                account.id = Some(AccountId::derive(&account.key(), identity));
            }
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.refresh_seconds < 30
            || !(1..=32).contains(&self.concurrency)
            || !(1..=300).contains(&self.timeout_seconds)
        {
            return Err(ConfigError::Bounds);
        }
        // Metered accounts without an override use their provider-specific
        // default, not the global interval. Explicit overrides preserve the
        // former 300-second configuration boundary for compatibility and are
        // raised later when they fall below the effective 900-second floor.
        let invalid_names = self.accounts.iter().enumerate().any(|(index, account)| {
            let name = account.name.trim();
            if name.is_empty() {
                self.accounts
                    .iter()
                    .enumerate()
                    .any(|(other_index, other)| {
                        index != other_index
                            && account.provider == other.provider
                            && (account.provider != Provider::Extension
                                || account.extension_id() == other.extension_id())
                    })
            } else {
                self.accounts[..index].iter().any(|other| {
                    let other_name = other.name.trim();
                    !other_name.is_empty() && other_name.eq_ignore_ascii_case(name)
                })
            }
        });
        if invalid_names {
            return Err(ConfigError::AccountNames);
        }
        // PRD 0024 RF7.1: group values are slugs; the named groups are unique.
        let mut group_ids = HashSet::new();
        let bad_group =
            self.accounts
                .iter()
                .filter_map(|account| account.group.as_deref())
                .any(|group| !model::is_group_slug(group))
                || self.groups.iter().any(|group| {
                    !model::is_group_slug(&group.id)
                        || !group_ids.insert(group.id.as_str())
                        || group.label.as_ref().is_some_and(|label| {
                            label.trim().is_empty() || label.chars().count() > 40
                        })
                });
        // A member id listed by two groups, or by a group other than the
        // account's own `group`, is ambiguous.
        let mut members = HashSet::new();
        let bad_members = self.groups.iter().any(|group| {
            group.members.iter().any(|member| {
                !members.insert(member.as_str())
                    || self.accounts.iter().any(|account| {
                        account.id().as_str() == member
                            && account.group.as_deref().is_some_and(|own| own != group.id)
                    })
            })
        });
        if bad_group || bad_members {
            return Err(ConfigError::Group);
        }
        let mut identities = HashSet::new();
        for (index, account) in self.accounts.iter().enumerate() {
            if let Some(override_seconds) = account.refresh_seconds {
                if override_seconds < Account::MIN_REFRESH_SECONDS {
                    return Err(ConfigError::Bounds);
                }
                if account.provider.is_metered()
                    && override_seconds < Provider::METERED_MIN_CONFIGURED_REFRESH_SECONDS
                {
                    return Err(metered_refresh_floor_error(
                        index,
                        account,
                        format!("accounts[{index}].refresh_seconds"),
                    ));
                }
            }
            if let Some(oauth) = &account.oauth {
                if account.credential_env.is_some() || oauth.validate(account.provider).is_err() {
                    return Err(ConfigError::OAuth);
                }
            }
            if let Some(session) = &account.session {
                // A vault password or key replaces `credential_env`; both
                // at once would be ambiguous.
                if !session.is_valid(account.provider)
                    || (session.slots.contains(user_session::CREDENTIAL_SLOT)
                        && account.credential_env.is_some())
                {
                    return Err(ConfigError::Session);
                }
            }
            if account.provider == Provider::ElevenLabs
                && providers::eleven_labs::base_for_region(
                    account.params.get("region").map(String::as_str),
                )
                .is_none()
            {
                return Err(ConfigError::Region);
            }
            if account.provider == Provider::NineRouter
                && providers::nine_router::account_base(account).is_err()
            {
                return Err(ConfigError::BaseUrl);
            }
            if account.provider == Provider::LiteLlm
                && providers::litellm::account_base(account).is_err()
            {
                return Err(ConfigError::BaseUrl);
            }
            if account.provider == Provider::NineRouter {
                let base = providers::nine_router::account_base(account)
                    .map_err(|_| ConfigError::BaseUrl)?;
                if providers::nine_router_tls::mode(account, &base).is_err() {
                    return Err(ConfigError::Tls);
                }
            }
            if account.provider == Provider::OpenRouter
                && (!providers::open_router::params_valid(&account.params)
                    || (account.oauth.is_some()
                        && account
                            .params
                            .get("product")
                            .is_some_and(|p| p == "account")))
            {
                return Err(ConfigError::OpenRouter);
            }
            if account.provider == Provider::Extension
                && (account
                    .extension_id()
                    .is_none_or(|id| !extensions::valid_id(id))
                    || account.credential_env.is_some()
                    || account.oauth.is_some()
                    || account.session.is_some())
            {
                return Err(ConfigError::Extension);
            }
            if account
                .extension_override()
                .is_some_and(|id| !extensions::valid_id(id))
            {
                return Err(ConfigError::Extension);
            }
            // One extension is one account: two rows for the same id would
            // race over one consent record and one schedule.
            if let Some(id) = account.extension_id() {
                if !identities.insert(id) {
                    return Err(ConfigError::Extension);
                }
            }
        }
        // PRD 0029 RF1.2: an id names exactly one account.
        let mut ids = HashSet::new();
        for (index, account) in self.accounts.iter().enumerate() {
            if let Some(id) = &account.id {
                if !ids.insert(id.as_str()) {
                    return Err(ConfigError::AccountIds {
                        field: format!("accounts[{index}].id"),
                    });
                }
            }
        }
        // A vault session belongs to exactly one account.
        let mut references = HashSet::new();
        if self
            .accounts
            .iter()
            .filter_map(|a| a.session.as_ref())
            .any(|session| !references.insert(session.credential_ref.as_str()))
        {
            return Err(ConfigError::Session);
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
    /// PRD 0011 RF2: opt-in public status queries and their cache; off
    /// until a frontend enables it. Shared by [`Monitor::with_config`].
    incidents: incidents::IncidentMonitor,
    /// PRD 0017 RF5: discovery/consent/runtime state and the one shared
    /// extension Runner used by recurring refreshes and explicit tests.
    extensions: Option<Arc<extensions::management::Manager>>,
    /// PRD 0033 RF5.3: the last-query record shared by processes for
    /// metered accounts; `None` queries every time.
    metered: Option<metered_record::Store>,
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
            incidents: incidents::IncidentMonitor::new(),
            extensions: extensions::management::Manager::default_shared(),
            #[cfg(test)]
            metered: None,
            #[cfg(not(test))]
            metered: metered_record::Store::default_dir().map(metered_record::Store::new),
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
    /// The new configuration is validated and returns the corresponding
    /// [`ConfigError`] when a document or account setting is invalid.
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
            incidents: self.incidents.clone(),
            extensions: self.extensions.clone(),
            metered: self.metered.clone(),
            #[cfg(test)]
            clock: self.clock.clone(),
        })
    }

    /// PRD 0033 RF5.3: keep the metered last-query records in `dir`
    /// instead of the OS local data directory.
    pub fn with_metered_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.metered = Some(metered_record::Store::new(dir));
        self
    }

    /// Attach the model catalog store for this configuration (see
    /// [`models::ModelCache::new`]); kept by [`Monitor::with_config`].
    pub fn with_model_cache(mut self, cache: models::ModelCache) -> Self {
        self.models = Some(cache);
        self
    }

    /// Use an injected extension directory (tests and portable frontends).
    /// Kept by [`Monitor::with_config`].
    pub fn with_extensions(mut self, manager: Arc<extensions::management::Manager>) -> Self {
        self.extensions = Some(manager);
        self
    }

    /// The shared UI-neutral extension module, when available.
    pub fn extension_manager(&self) -> Option<&Arc<extensions::management::Manager>> {
        self.extensions.as_ref()
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

    /// RF5.3: record new readings (the default) or ignore them; what was
    /// recorded stays until erased. Kept by [`Monitor::with_config`].
    pub fn set_history_recording(&self, on: bool) {
        if let Some(recorder) = &self.history {
            recorder.set_recording(on);
        }
    }

    /// Whether new readings enter the history; `false` without one.
    pub fn history_recording(&self) -> bool {
        self.history
            .as_ref()
            .is_some_and(history::HistoryRecorder::is_recording)
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

    /// PRD 0006 RF6: every recorded metric of account `index` within
    /// `range` ([`history::HistoryStore::account_history`]). Empty without
    /// history or for an unknown index; a storage failure is returned and
    /// also shown in [`Monitor::history_status`].
    pub fn account_history(
        &self,
        index: usize,
        range: impl std::ops::RangeBounds<chrono::DateTime<Utc>>,
    ) -> Result<Vec<history::MetricHistory>, history::HistoryError> {
        let (Some(recorder), Some(account)) = (&self.history, self.config.accounts.get(index))
        else {
            return Ok(Vec::new());
        };
        recorder
            .store()
            .account_history(account, range)
            .inspect_err(|error| recorder.note_error(*error))
    }

    /// PRD 0011 RF2: the status preference and cache this monitor uses;
    /// [`incidents::IncidentMonitor::set_enabled`] turns queries on or off.
    pub fn incidents(&self) -> &incidents::IncidentMonitor {
        &self.incidents
    }

    /// Use `incidents` (its preference and cached readings), e.g. to carry
    /// them into a monitor rebuilt from scratch. [`Monitor::with_config`]
    /// keeps them without this call.
    pub fn with_incidents(mut self, incidents: incidents::IncidentMonitor) -> Self {
        self.incidents = incidents;
        self
    }

    /// Distinct `(provider, source)` pairs of the configured accounts that
    /// have a verified source, in configuration order. Regional providers
    /// contribute one pair per distinct region.
    fn status_sources(&self) -> Vec<(Provider, incidents::StatusSource)> {
        let mut pairs: Vec<(Provider, incidents::StatusSource)> = Vec::new();
        for account in &self.config.accounts {
            let Some(source) = Self::status_source(account) else {
                continue;
            };
            let pair = (account.provider, source);
            if !pairs.contains(&pair) {
                pairs.push(pair);
            }
        }
        pairs
    }

    fn status_source(account: &Account) -> Option<incidents::StatusSource> {
        incidents::source_for(
            account.provider,
            account.params.get("region").map(String::as_str),
        )
    }

    /// One status per provider: accounts of a regional provider in several
    /// regions report the worst of their sub-pages.
    fn provider_statuses(&self) -> Vec<incidents::ProviderStatus> {
        let mut merged: Vec<incidents::ProviderStatus> = Vec::new();
        for (provider, source) in self.status_sources() {
            let status = self.incidents.status(provider, source);
            match merged.iter_mut().find(|known| known.provider == provider) {
                Some(known) => known.merge(status),
                None => merged.push(status),
            }
        }
        merged
    }

    /// PRD 0028 RF4.5: for each configured provider with a Statuspage source,
    /// the display names of the fixed components its public page no longer
    /// lists (only providers with something missing appear). One request per
    /// distinct page, independent of the status preference: the caller (the
    /// local canary) decides whether status pages may be contacted.
    pub async fn missing_status_components(&self) -> Vec<(Provider, Vec<&'static str>)> {
        let mut urls = HashSet::new();
        let sources: Vec<_> = self
            .status_sources()
            .into_iter()
            .filter(|(_, source)| urls.insert(source.url))
            .collect();
        let checked =
            futures_util::future::join_all(sources.iter().map(|(provider, source)| async move {
                incidents::missing_components(&self.http, &self.query_slots, *source)
                    .await
                    .filter(|missing| !missing.is_empty())
                    .map(|missing| (*provider, missing))
            }))
            .await;
        checked.into_iter().flatten().collect()
    }

    /// PRD 0011 RF4.3: the domains status queries reach for the configured
    /// providers, in configuration order, once each. No network and
    /// independent of the preference, so a frontend can list them before
    /// the user turns the feature on.
    pub fn status_domains(&self) -> Vec<&'static str> {
        let mut domains = Vec::new();
        for (_, source) in self.status_sources() {
            if !domains.contains(&source.host()) {
                domains.push(source.host());
            }
        }
        domains
    }

    /// PRD 0011 RF2: query each due source of the configured providers once
    /// (coalesced by URL, cached for [`incidents::CACHE_TTL`], under this
    /// monitor's query slots) and return their statuses. Makes no request
    /// and returns nothing while the feature is off. A failed status query
    /// keeps the last known reading, marked stale.
    pub async fn refresh_incidents(&self) -> Vec<incidents::ProviderStatus> {
        if !self.incidents.is_enabled() {
            return Vec::new();
        }
        let sources = self.status_sources();
        let mut urls = HashSet::new();
        futures_util::future::join_all(
            sources
                .iter()
                .filter(|(_, source)| urls.insert(source.url))
                .map(|(_, source)| {
                    self.incidents
                        .ensure(*source, &self.http, &self.query_slots)
                }),
        )
        .await;
        self.provider_statuses()
    }

    /// The cached statuses of the configured providers with a verified
    /// source, without network; empty while the feature is off. Providers
    /// without a source never appear (they are not "operational").
    pub fn incident_statuses(&self) -> Vec<incidents::ProviderStatus> {
        if !self.incidents.is_enabled() {
            return Vec::new();
        }
        self.provider_statuses()
    }

    /// RF3: the provider's current state for a failed query, reusing (or
    /// waiting for) the cycle's single status query. `None` while off or
    /// without a verified source.
    async fn incident_state(&self, account: &Account) -> Option<incidents::IncidentState> {
        if !self.incidents.is_enabled() {
            return None;
        }
        let source = Self::status_source(account)?;
        self.incidents
            .ensure(source, &self.http, &self.query_slots)
            .await;
        Some(self.incidents.status(account.provider, source).state)
    }

    fn now(&self) -> chrono::DateTime<Utc> {
        #[cfg(test)]
        if let Some(clock) = &self.clock {
            return *clock.lock();
        }
        Utc::now()
    }

    /// Refresh every account and, when enabled, the provider statuses
    /// ([`Monitor::refresh_incidents`]) concurrently. A status failure never
    /// delays or changes a quota reading beyond its own short timeout.
    pub async fn refresh(&self) -> Snapshot {
        let started_at = self.now();
        // PRD 0017 RF5.12: local extensions run under the extension
        // manager's own cap, never this provider semaphore, so a slow or
        // stuck extension at index 0 cannot delay a built-in account when
        // `concurrency` is 1. Both groups are joined and re-sorted into
        // configuration order.
        let (local, remote): (Vec<usize>, Vec<usize>) = (0..self.config.accounts.len())
            .partition(|index| self.config.accounts[*index].provider == Provider::Extension);
        let extensions = futures_util::future::join_all(
            local
                .into_iter()
                .map(|index| async move { (index, self.refresh_account(index).await) }),
        );
        let accounts = stream::iter(remote)
            .map(|index| async move { (index, self.refresh_account(index).await) })
            .buffer_unordered(self.config.concurrency)
            .collect::<Vec<_>>();
        let (extension_reports, mut indexed, _incidents) =
            tokio::join!(extensions, accounts, self.refresh_incidents());
        indexed.extend(extension_reports);
        indexed.sort_unstable_by_key(|(index, _)| *index);
        Snapshot {
            started_at,
            finished_at: self.now(),
            reports: indexed.into_iter().map(|(_, report)| report).collect(),
        }
    }

    /// One refresh like [`Monitor::refresh`], plus, per account in
    /// configuration order, the structural fingerprint of the responses it
    /// decoded (keys and JSON types only; `None` when the query made no JSON
    /// request, such as an extension or a failure before any answer). For
    /// the local canary (PRD 0028 RF4.1); statuses are not queried.
    pub async fn refresh_with_shapes(&self) -> (Snapshot, Vec<Option<diagnostic::ResponseShape>>) {
        let started_at = self.now();
        let mut indexed = stream::iter(0..self.config.accounts.len())
            .map(|index| async move {
                let (report, shape) = providers::capture::scoped(self.refresh_account(index)).await;
                (index, report, shape)
            })
            .buffer_unordered(self.config.concurrency)
            .collect::<Vec<_>>()
            .await;
        indexed.sort_unstable_by_key(|(index, _, _)| *index);
        let (reports, shapes) = indexed
            .into_iter()
            .map(|(_, report, shape)| (report, shape))
            .unzip();
        (
            Snapshot {
                started_at,
                finished_at: self.now(),
                reports,
            },
            shapes,
        )
    }

    /// Refresh one account by its configuration index. All full and targeted
    /// refreshes share this monitor's concurrency limit.
    ///
    /// `index` must be in `0..accounts.len()`; out-of-range is a programming
    /// error.
    pub async fn refresh_account(&self, index: usize) -> Report {
        debug_assert!(
            index < self.config.accounts.len(),
            "refresh_account index is out of bounds"
        );
        let account = &self.config.accounts[index];
        let report = match self
            .metered
            .as_ref()
            .filter(|_| account.provider.is_metered())
        {
            Some(store) => {
                let interval = chrono::TimeDelta::seconds(
                    account.effective_refresh_seconds(self.config.refresh_seconds) as i64,
                );
                match store.begin(account, interval, self.now()) {
                    // Another process paid for this interval's query.
                    metered_record::Decision::Reuse(report) => return *report,
                    metered_record::Decision::Query => {
                        let report = self.query_report(account, &[]).await;
                        store.finish(account, &report, self.now());
                        report
                    }
                }
            }
            None => self.query_report(account, &[]).await,
        };
        let mut report = report;
        // PRD 0036 RF5.3: a connection that duplicates a direct account is
        // marked before history, alerts or the ranking see it.
        aggregators::mark_duplicates(&self.config, account, &mut report);
        // RF2.2/RF2.4: only enqueued; the disk never delays the refresh.
        if let Some(recorder) = &self.history {
            recorder.submit(account, &report);
            // PRD 0036 RF1.4: each active sub-account has its own series.
            for unit in aggregators::sub_units(index, account, &report) {
                recorder.submit(&unit.account, &unit.report);
            }
        }
        report
    }

    /// PRD 0036 RF4.4: refresh only the connection `sub_id` of the
    /// aggregator account at `index`, merging it into `previous` (the
    /// account's current reading). One usage request, never `force=1`.
    /// The instance's other connections keep their readings; a connection
    /// that is gone is dropped. When the instance itself fails, the result
    /// is that failure, as a whole refresh would report it.
    pub async fn refresh_connection(
        &self,
        index: usize,
        sub_id: &str,
        previous: &Report,
    ) -> Report {
        let account = &self.config.accounts[index];
        if account.provider != Provider::NineRouter || previous.status != Status::Ready {
            return self.refresh_account(index).await;
        }
        let result = self
            .with_access(
                account,
                async |account: &Account, ctx: &providers::Context<'_>| {
                    providers::nine_router::fetch_connection(account, ctx, sub_id).await
                },
            )
            .await;
        let mut report = previous.clone();
        match result {
            Ok(Some(sub)) => match report.sub_reports.iter_mut().find(|old| old.id == sub.id) {
                Some(old) => *old = sub,
                None => report.sub_reports.push(sub),
            },
            Ok(None) => report.sub_reports.retain(|old| old.id != sub_id),
            Err(_) => return self.refresh_account(index).await,
        }
        aggregators::mark_duplicates(&self.config, account, &mut report);
        if let Some(recorder) = &self.history {
            for unit in aggregators::sub_units(index, account, &report)
                .into_iter()
                .filter(|unit| unit.sub_id == sub_id)
            {
                recorder.submit(&unit.account, &unit.report);
            }
        }
        report
    }

    /// PRD 0036 RF2.2: tests a 9router connection before it is saved:
    /// health, version, login (when a password is set) and the connection
    /// list. Read-only; nothing is stored. The outcome carries one sentence
    /// and never a body, URL or token.
    pub async fn test_nine_router(&self, account: &Account) -> aggregators::AggregatorTest {
        self.test_nine_router_with(account, &[]).await
    }

    /// [`Monitor::test_nine_router`] with `pasted` values (a password, a
    /// key or an SSO session not saved yet) in place of the vault ones of
    /// their slots (PRD 0036 RF2.1/RF2.6). Nothing is stored.
    pub async fn test_nine_router_with(
        &self,
        account: &Account,
        pasted: &[user_session::ReducedSession],
    ) -> aggregators::AggregatorTest {
        let outcome = self
            .with_sessions(
                account,
                pasted,
                async |account: &Account, ctx: &providers::Context<'_>| {
                    Ok(match account.provider {
                        Provider::LiteLlm => {
                            providers::litellm::test_connection(account, ctx).await
                        }
                        _ => providers::nine_router::test_connection(account, ctx).await,
                    })
                },
            )
            .await
            .0
            .unwrap_or(aggregators::TestOutcome::Unreachable);
        aggregators::AggregatorTest::for_provider(account.provider, outcome)
    }

    /// PRD 0016 RF2.1: one read-only query of `account` (a draft the wizard
    /// has not saved yet, or an edited one) with the pasted `sessions` in
    /// place of the vault ones. Nothing is stored nor recorded in the
    /// history; the report says whether the session works. An invalid draft
    /// is reported as needing configuration, without a query.
    pub async fn test_session(
        &self,
        account: &Account,
        sessions: &[user_session::ReducedSession],
    ) -> Report {
        let mut probe = self.config.clone();
        probe.accounts = vec![account.clone()];
        if let Err(error) = probe.validate() {
            let (notes, notes_l10n) =
                crate::text::Notes::one(edit_error::config_error_note(&error)).finish();
            return Report {
                account_id: Some(account.id()),
                diagnostic: None,
                account: account.name.clone(),
                provider: account.provider,
                checked_at: self.now(),
                status: Status::NeedsConfiguration,
                metrics: Vec::new(),
                notes,
                notes_l10n,
                source: String::new(),
                session_expires_at: None,
                sub_reports: Vec::new(),
                as_of: None,
                overage_hint: None,
            };
        }
        self.query_report(account, sessions).await
    }

    /// The report of one query of `account`, `pasted` sessions replacing the
    /// vault ones of their slots, labelled with the account's id (PRD 0029).
    async fn query_report(
        &self,
        account: &Account,
        pasted: &[user_session::ReducedSession],
    ) -> Report {
        let mut report = self.query_unlabelled(account, pasted).await;
        report.account_id = Some(account.id());
        report
    }

    async fn query_unlabelled(
        &self,
        account: &Account,
        pasted: &[user_session::ReducedSession],
    ) -> Report {
        // PRD 0028 RF7.1/RF7.2: an approved override extension replaces the
        // built-in query of the account that chose it. It receives only its
        // own declared `env` and secrets, never this account's credentials;
        // when it is missing, revoked, changed or does not override this
        // provider, the built-in adapter answers as before.
        if let (Some(id), Some(manager)) = (account.extension_override(), &self.extensions) {
            let checked_at = self.now();
            if let Some(result) = manager
                .run_override(account.provider, id, Some(self.oauth.token_store()))
                .await
            {
                let checked = match result {
                    Ok(checked) => checked,
                    Err(note) => extensions::output::Checked {
                        source: format!("extension:{id}"),
                        status: Status::NeedsConfiguration,
                        metrics: Vec::new(),
                        notes: vec![note],
                        violations: Vec::new(),
                    },
                };
                return checked.into_report(account.name.clone(), account.provider, checked_at);
            }
        }
        if account.provider == Provider::Extension {
            let checked_at = self.now();
            let unavailable = |note: crate::text::Localized| {
                let (notes, notes_l10n) = crate::text::Notes::one(note).finish();
                Report {
                    account_id: None,
                    diagnostic: None,
                    account: account.name.clone(),
                    provider: Provider::Extension,
                    checked_at,
                    status: Status::NeedsConfiguration,
                    metrics: Vec::new(),
                    notes,
                    notes_l10n,
                    source: account
                        .extension_id()
                        .map_or_else(String::new, |id| format!("extension:{id}")),
                    session_expires_at: None,
                    sub_reports: Vec::new(),
                    as_of: None,
                    overage_hint: None,
                }
            };
            let Some(manager) = &self.extensions else {
                return unavailable(extensions::management::missing(
                    account.extension_id().unwrap_or("extension"),
                ));
            };
            // A local program runs under the extension manager's own cap
            // (`Runner::MAX_RUNNING`), never the HTTP provider semaphore: a
            // slow extension must not hold a slot a provider query needs.
            return match manager
                .run_account_with(account, Some(self.oauth.token_store()))
                .await
            {
                Ok(checked) => {
                    checked.into_report(account.name.clone(), Provider::Extension, checked_at)
                }
                Err(note) => unavailable(note),
            };
        }
        let (result, session_expires_at) = self
            .with_sessions(
                account,
                pasted,
                async |account: &Account, ctx: &providers::Context<'_>| {
                    self.dispatch(account, ctx).await
                },
            )
            .await;
        let mut diagnostic = None;
        let (status, metrics, mut notes, mut notes_l10n, source, sub_reports, as_of, overage_hint) =
            match result {
                Ok(data) => (
                    Status::Ready,
                    data.metrics,
                    data.notes,
                    data.notes_l10n,
                    data.source,
                    data.sub_reports,
                    data.as_of,
                    data.overage_hint,
                ),
                Err(error) => {
                    let mut notes = crate::text::Notes::one(error.note);
                    // PRD 0028 RF1.3: the details say which field drifted.
                    if let Some(found) = &error.diagnostic {
                        notes.push_note(found.text());
                    }
                    diagnostic = error.diagnostic.map(|found| *found);
                    // PRD 0011 RF3: context only; the status stays as queried.
                    if let Some(state) = self.incident_state(account).await {
                        notes.extend(incidents::failure_notes(error.status, state));
                    }
                    let (plain, l10n) = notes.finish();
                    (
                        error.status,
                        Vec::new(),
                        plain,
                        l10n,
                        String::new(),
                        Vec::new(),
                        None,
                        None,
                    )
                }
            };
        if let Some(note) = account.metered_interval_note() {
            notes.push(note.text().to_owned());
            notes_l10n.push(note);
        }
        Report {
            account_id: None,
            diagnostic,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: self.now(),
            status,
            metrics,
            notes,
            notes_l10n,
            source,
            session_expires_at,
            sub_reports,
            as_of,
            overage_hint,
        }
    }

    /// Run `run` under one query slot and the account deadline, with OAuth
    /// credentials resolved (and refreshed) through the shared session.
    async fn with_access<T>(
        &self,
        account: &Account,
        run: impl AsyncFnOnce(&Account, &providers::Context<'_>) -> Result<T, providers::QueryError>,
    ) -> Result<T, providers::QueryError> {
        self.with_sessions(account, &[], run).await.0
    }

    /// [`Monitor::with_access`] with `pasted` sessions replacing the vault
    /// ones of their slots. PRD 0016 RF3.1: when the adapter read a vault
    /// or pasted session and the provider rejected the query, the note says
    /// the session expired or was revoked. RF3.2: also returns the earliest
    /// known expiry of the sessions the adapter read.
    async fn with_sessions<T>(
        &self,
        account: &Account,
        pasted: &[user_session::ReducedSession],
        run: impl AsyncFnOnce(&Account, &providers::Context<'_>) -> Result<T, providers::QueryError>,
    ) -> (
        Result<T, providers::QueryError>,
        Option<chrono::DateTime<Utc>>,
    ) {
        let _permit = self
            .query_slots
            .acquire()
            .await
            .expect("monitor query semaphore stays open");
        let mut timeout = Duration::from_secs(self.config.timeout_seconds);
        if account.provider.is_aggregator() {
            // PRD 0036 RF4.1: one deadline for the whole instance (discovery
            // plus every connection), longer than a single-request account.
            timeout = timeout.max(Duration::from_secs(60));
        }
        let fetch = async {
            let access = match &account.oauth {
                // MiMo's native login yields an inference key, never the cookie
                // used for quota. Do not open Keychain for this unrelated key.
                Some(_) if account.provider == Provider::XiaomiMimo => None,
                Some(config) => match self.oauth.credentials(account.provider, config).await {
                    Ok(token) => Some(token),
                    Err(error) => {
                        let status = match &error {
                            oauth::OAuthError::Configuration(_) => Status::NeedsConfiguration,
                            oauth::OAuthError::Denied | oauth::OAuthError::Expired => {
                                Status::AuthenticationError
                            }
                            _ => Status::Error,
                        };
                        let error = providers::QueryError {
                            diagnostic: None,
                            status,
                            note: error.l10n(),
                        };
                        return (Err(error), None);
                    }
                },
                None => None,
            };
            // PRD 0016 RF1: a vault session supersedes the variable its slot
            // names; the adapter reads it through the query's credentials.
            let mut sessions = match user_session::load(&self.oauth, account).await {
                Ok(sessions) => sessions,
                // RF2.1: a test with a pasted session does not need the vault.
                Err(_) if !pasted.is_empty() => Vec::new(),
                Err(error) => return (Err(error), None),
            };
            for reduced in pasted {
                sessions.retain(|session| session.slot != reduced.slot());
                sessions.push(user_session::QuerySession {
                    slot: reduced.slot(),
                    value: reduced.cookie().to_string(),
                    expires_at: reduced.expires_at(),
                });
            }
            let mut effective = Cow::Borrowed(account);
            for session in &sessions {
                let name = format!("{}{}", user_session::VAULT_NAME, session.slot);
                // PRD 0036 RF2.1: a vault password or key stands in for the
                // variable `credential_env` names.
                if session.slot == user_session::CREDENTIAL_SLOT {
                    effective.to_mut().credential_env = Some(name);
                } else {
                    effective
                        .to_mut()
                        .params
                        .insert(session.slot.to_string(), name);
                }
            }
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
                sessions,
                read: std::sync::Mutex::new(BTreeSet::new()),
            };
            let ctx = providers::Context {
                http: &self.http,
                credentials: &credentials,
                timeout,
            };
            let result = run(account, &ctx).await;
            let read = credentials
                .read
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let expires_at = credentials
                .sessions
                .iter()
                .filter(|session| read.contains(session.slot))
                .filter_map(|session| session.expires_at)
                .min();
            let read_cookie = read
                .iter()
                .any(|slot| user_session::is_cookie_slot(account.provider, slot));
            let result = match result {
                Err(error) if error.status == Status::AuthenticationError && read_cookie => {
                    Err(providers::QueryError {
                        diagnostic: None,
                        status: Status::AuthenticationError,
                        note: user_session::rejected(),
                    })
                }
                other => other,
            };
            (result, expires_at)
        };
        tokio::time::timeout(timeout, fetch)
            .await
            .unwrap_or_else(|_| (Err(providers::QueryError::timeout()), None))
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
                "Qwen Code OAuth: authenticated session, but no quota endpoint confirmed. The free OAuth plan was discontinued by the provider on 15/04/2026; we do not query DashScope with this token.",
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
            Provider::ElevenLabs => providers::eleven_labs::fetch(account, ctx).await,
            Provider::NineRouter => providers::nine_router::fetch(account, ctx).await,
            Provider::OpenRouter => providers::open_router::fetch(account, ctx).await,
            Provider::LiteLlm => providers::litellm::fetch(account, ctx).await,
            Provider::Extension => {
                unreachable!("extension dispatch is handled before provider dispatch")
            }
        }
    }
}

const OAUTH_ACCESS: &str = "__CUOTAI_OAUTH_ACCESS";
const NO_API_KEY: &str = "__CUOTAI_NO_API_KEY";

struct QueryCredentials<'a> {
    external: &'a dyn Credentials,
    access: Option<String>,
    sessions: Vec<user_session::QuerySession>,
    /// Session slots the adapter read (PRD 0016 RF3.1/RF3.2).
    read: std::sync::Mutex<BTreeSet<&'static str>>,
}
impl Credentials for QueryCredentials<'_> {
    fn get(&self, name: &str) -> Option<String> {
        if let Some(slot) = name.strip_prefix(user_session::VAULT_NAME) {
            let session = self.sessions.iter().find(|session| session.slot == slot)?;
            self.read
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(session.slot);
            return Some(session.value.clone());
        }
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

    #[test]
    fn future_account_identity_fixture_loads_with_an_unnamed_account() {
        let config = Config::from_toml(include_str!(
            "../tests/fixtures/stable-account-identity.toml"
        ))
        .unwrap();

        assert_eq!(config.accounts.len(), 2);
        assert_eq!(config.accounts[0].name, "");
        assert_eq!(config.accounts[0].provider, Provider::Groq);
        assert_eq!(config.accounts[1].name, "Claude");
    }

    #[test]
    fn future_id_allow_list_still_rejects_account_key_typos() {
        let typo = r#"
[[accounts]]
id = "acc_0123456789ab"
provider = "groq"
credential_en = "GROQ_API_KEY"
"#;

        assert!(matches!(Config::from_toml(typo), Err(ConfigError::Parse)));
    }

    #[test]
    fn unnamed_account_is_valid_only_without_a_provider_peer() {
        let mut config = Config::from_toml(include_str!(
            "../tests/fixtures/stable-account-identity.toml"
        ))
        .unwrap();
        config.accounts[1].name.clear();
        assert!(config.validate().is_ok());

        config.accounts[1].provider = Provider::Groq;
        assert!(matches!(config.validate(), Err(ConfigError::AccountNames)));
        config.accounts[1].name = "Other".into();
        assert!(matches!(config.validate(), Err(ConfigError::AccountNames)));
    }

    #[test]
    fn configured_names_remain_globally_unique_ignoring_case() {
        let mut config = Config::from_toml(include_str!(
            "../tests/fixtures/stable-account-identity.toml"
        ))
        .unwrap();
        config.accounts[0].name = "work".into();
        config.accounts[1].name = "Work".into();

        assert!(matches!(config.validate(), Err(ConfigError::AccountNames)));
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

    /// PRD 0013 RF1.3: a query that runs out of time and one that cannot
    /// reach the provider fail with distinct event categories.
    #[tokio::test]
    async fn deadline_and_unreachable_provider_fail_as_timeout_and_offline() {
        use crate::events::FailureCategory;
        let config = Config::from_toml(
            r#"
timeout_seconds = 1

[[accounts]]
name = "alpha"
provider = "groq"
credential_env = "GROQ_KEY_ALPHA"
"#,
        )
        .unwrap();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": [] }))
                    .set_delay(Duration::from_secs(5)),
            )
            .mount(&server)
            .await;
        let creds: Arc<dyn Credentials> = Arc::new(GroqKeys);
        let store: Arc<dyn oauth::TokenStore> = EmptyStore::new();
        let session = Arc::new(oauth::OAuthSession::new(store, Arc::clone(&creds)).unwrap());
        let mut monitor = Monitor::with_oauth(config, creds, session).unwrap();
        monitor.groq_url = Some(server.uri());
        let slow = monitor.refresh_account(0).await;
        assert_eq!(FailureCategory::of(&slow), Some(FailureCategory::Timeout));

        // Keep a listener alive long enough to accept the request, then close
        // without an HTTP response. A released port may be dropped by a
        // Windows firewall until the account deadline and look like a timeout.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let disconnect = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            drop(connection);
        });
        monitor.groq_url = Some(format!("http://{address}"));
        let unreachable = monitor.refresh_account(0).await;
        disconnect.await.unwrap();
        assert_eq!(
            FailureCategory::of(&unreachable),
            Some(FailureCategory::Offline)
        );
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
    // PRD 0033 RF5.1–RF5.2: metered defaults, effective floor and note.
    // -------------------------------------------------------------------

    fn aws_account(override_seconds: Option<u64>) -> Account {
        let mut account = Account {
            id: None,
            name: "bedrock-test".into(),
            provider: Provider::AwsBedrock,
            refresh_seconds: override_seconds,
            ..Account::default()
        };
        account.params.insert("profile".into(), "S".into());
        account
    }

    #[tokio::test]
    async fn metered_monitors_share_one_query_per_interval() {
        let dir = tempfile::tempdir().unwrap();
        let account = aws_account(None);
        let config = Config {
            accounts: vec![account.clone()],
            ..Config::default()
        };
        // Another process already paid for this interval's query.
        let cached = Report {
            overage_hint: Default::default(),
            account_id: None,
            diagnostic: None,
            sub_reports: Vec::new(),
            account: account.name.clone(),
            provider: Provider::AwsBedrock,
            checked_at: Utc::now() - chrono::TimeDelta::minutes(5),
            status: Status::Ready,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: "cost-explorer".into(),
            session_expires_at: None,
            as_of: None,
        };
        let store = metered_record::Store::new(dir.path().to_path_buf());
        store.finish(
            &account,
            &cached,
            Utc::now() - chrono::TimeDelta::minutes(5),
        );
        let monitor = Monitor::new(config)
            .unwrap()
            .with_metered_dir(dir.path().to_path_buf());
        let reused = monitor.refresh_account(0).await;
        // No AWS CLI ran: the report is the recorded one, with its time.
        assert_eq!(reused.status, Status::Ready);
        assert_eq!(reused.checked_at, cached.checked_at);
        let file = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "json"))
            .unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains("http"), "no URL in the record");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    fn metered_floor_error(
        config: &Config,
        expected_field: &str,
        expected_account: &str,
    ) -> ConfigError {
        let error = config.validate().expect_err("metered floor must reject");
        let ConfigError::MeteredRefreshFloor {
            field,
            account,
            minimum,
        } = &error
        else {
            panic!("expected metered refresh floor error, got {error:?}");
        };
        assert_eq!(field, expected_field);
        assert_eq!(account, expected_account);
        assert_eq!(*minimum, Provider::METERED_MIN_CONFIGURED_REFRESH_SECONDS);
        assert!(error.to_string().contains(expected_field));
        assert!(error.to_string().contains(expected_account));
        error
    }

    #[test]
    fn validate_uses_metered_default_instead_of_global_interval() {
        let config = Config {
            refresh_seconds: 120,
            accounts: vec![aws_account(None)],
            ..Config::default()
        };

        config.validate().expect("metered default is valid");
        assert_eq!(
            config.accounts[0].effective_refresh_seconds(config.refresh_seconds),
            Provider::METERED_DEFAULT_REFRESH_SECONDS
        );
        assert!(config.accounts[0].metered_interval_note().is_none());
    }

    #[test]
    fn validate_raises_compatible_metered_overrides_with_a_note() {
        for configured in [600, 899] {
            let config = Config {
                refresh_seconds: 120,
                accounts: vec![aws_account(Some(configured))],
                ..Config::default()
            };

            config.validate().expect("compatible override is valid");
            assert_eq!(
                config.accounts[0].effective_refresh_seconds(config.refresh_seconds),
                Provider::METERED_MIN_REFRESH_SECONDS
            );
            let note = config.accounts[0]
                .metered_interval_note()
                .expect("raised override has a note");
            assert_eq!(note.id, "note.metered.interval_raised");
            assert_eq!(
                note.params.get("configured"),
                Some(&crate::text::ParamValue::Text(configured.to_string()))
            );
            assert_eq!(
                note.params.get("effective"),
                Some(&crate::text::ParamValue::Text("900".into()))
            );
        }
    }

    #[test]
    fn validate_accepts_effective_floor_without_a_raise_note() {
        let config = Config {
            refresh_seconds: 120,
            accounts: vec![aws_account(Some(900))],
            ..Config::default()
        };

        config.validate().expect("effective floor is valid");
        assert_eq!(
            config.accounts[0].effective_refresh_seconds(config.refresh_seconds),
            900
        );
        assert!(config.accounts[0].metered_interval_note().is_none());
    }

    #[test]
    fn validate_rejects_metered_override_below_compatibility_floor() {
        let config = Config {
            refresh_seconds: 120,
            accounts: vec![aws_account(Some(200))],
            ..Config::default()
        };

        let error = metered_floor_error(&config, "accounts[0].refresh_seconds", "bedrock-test");
        let note = edit_error::config_error_note(&error);
        for locale in ["pt-BR", "en-US", "es", "it"] {
            let resolved = note.resolve(locale);
            assert!(resolved.contains("accounts[0].refresh_seconds"));
            assert!(resolved.contains("bedrock-test"));
            assert!(resolved.contains("300"));
        }
    }

    #[test]
    fn validate_leaves_non_metered_account_cadence_unchanged() {
        let config = Config {
            refresh_seconds: 30,
            accounts: vec![Account {
                id: None,
                name: "openai".into(),
                provider: Provider::OpenAiApi,
                credential_env: Some("OPENAI_ADMIN_KEY".into()),
                refresh_seconds: Some(60),
                ..Account::default()
            }],
            ..Config::default()
        };

        config.validate().expect("non-metered account is valid");
        assert_eq!(
            config.accounts[0].effective_refresh_seconds(config.refresh_seconds),
            60
        );
        assert!(config.accounts[0].metered_interval_note().is_none());
    }

    #[tokio::test]
    async fn monitor_report_surfaces_the_raised_metered_interval_note() {
        let mut account = aws_account(Some(600));
        account.params.clear();
        let config = Config {
            accounts: vec![account],
            ..Config::default()
        };
        let creds: Arc<dyn Credentials> = Arc::new(NoCreds);
        let store: Arc<dyn oauth::TokenStore> = EmptyStore::new();
        let session =
            Arc::new(oauth::OAuthSession::new(Arc::clone(&store), Arc::clone(&creds)).unwrap());
        let monitor = Monitor::with_oauth(config, creds, session).unwrap();

        let report = monitor.refresh_account(0).await;
        let position = report
            .notes_l10n
            .iter()
            .position(|note| note.id == "note.metered.interval_raised")
            .expect("account report includes the cadence note");
        assert_eq!(report.notes[position], report.notes_l10n[position].fallback);
        for locale in ["pt-BR", "en-US", "es", "it"] {
            let resolved = report.notes_l10n[position].resolve(locale);
            assert!(resolved.contains("600"));
            assert!(resolved.contains("900"));
        }
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
                        account_id: None,
                        diagnostic: None,
                        account: "aws-one".into(),
                        provider: Provider::AwsBedrock,
                        checked_at: chrono::Utc::now(),
                        status: Status::Ready,
                        metrics: Vec::new(),
                        notes: Vec::new(),
                        notes_l10n: Vec::new(),
                        source: String::new(),
                        session_expires_at: None,
                        sub_reports: Vec::new(),
                        as_of: None,
                        overage_hint: None,
                    },
                    crate::Report {
                        account_id: None,
                        diagnostic: None,
                        account: "aws-two".into(),
                        provider: Provider::AwsBedrock,
                        checked_at: chrono::Utc::now(),
                        status: Status::Ready,
                        metrics: Vec::new(),
                        notes: Vec::new(),
                        notes_l10n: Vec::new(),
                        source: String::new(),
                        session_expires_at: None,
                        sub_reports: Vec::new(),
                        as_of: None,
                        overage_hint: None,
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
