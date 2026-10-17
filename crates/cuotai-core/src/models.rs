//! PRD 0002 RF2 — per-account model catalog, queried only on explicit
//! request ([`Monitor::load_models`] once, [`Monitor::sync_models`] on demand)
//! and persisted per configuration in `models.sqlite3`.
//!
//! A catalog lists the model IDs the provider returned for the configured
//! credential; it proves neither inference permission, price nor current
//! availability. Quota refresh (`Monitor::refresh`, `RefreshCycle`, `Report`,
//! `Snapshot`) never reads or writes this store.

use crate::config_file::{ConfigFile, ConfigIdentity};
use crate::text::{params, provider_l10n, Localized};
use crate::{providers, Account, Monitor, Provider, Status};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Real provider ID, never rewritten.
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalog {
    /// Deduplicated and sorted by `id` for presentation.
    pub models: Vec<ModelInfo>,
    /// Endpoint and scope; never a URL with credentials.
    pub source: Localized,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelState {
    /// No attempt recorded for this account identity.
    NotFetched,
    /// Last attempt succeeded; an empty list is still `Ready`.
    Ready(ModelCatalog),
    /// Last attempt failed (`status` reuses the quota query classification);
    /// `previous` is the last successful catalog, still searchable.
    Failed {
        status: Status,
        error: Localized,
        at: DateTime<Utc>,
        previous: Option<ModelCatalog>,
    },
    /// No verified listing contract; no network is used.
    Unsupported { reason: Localized },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("Conta inexistente nesta configuração.")]
    InvalidIndex,
    #[error("Cache de modelos não configurado para esta configuração.")]
    NoCache,
    #[error("Falha ao ler ou gravar o cache de modelos; nenhum resultado foi salvo.")]
    Storage,
}

impl ModelError {
    pub fn l10n(&self) -> Localized {
        let id = match self {
            Self::InvalidIndex => "models.error.invalid_index",
            Self::NoCache => "models.error.no_cache",
            Self::Storage => "models.error.storage",
        };
        Localized::fixed(id, self.to_string())
    }
}

impl From<rusqlite::Error> for ModelError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}

/// Catalog source for `provider`, `None` without a verified listing contract.
pub(crate) fn source(provider: Provider) -> Option<Localized> {
    match provider {
        Provider::Groq => Some(Localized::fixed(
            "models.source.groq",
            "Groq GET /openai/v1/models com a chave da conta; não comprova permissão de inferência, preço ou disponibilidade atual.",
        )),
        _ => None,
    }
}

pub(crate) fn unsupported_reason(provider: Provider) -> Localized {
    Localized::with_params(
        "models.unsupported",
        params([("provider", provider_l10n(provider).into())]),
        format!(
            "{}: listagem de modelos não verificada para esta conta; nenhuma consulta é feita.",
            provider.label()
        ),
    )
}

const SCHEMA_VERSION: i64 = 1;
/// `status` is free text so RF3 can add `fetching`/`interrupted` and claim
/// columns without rewriting rows; `catalog` is the last full success.
const SCHEMA_V1: &str = "CREATE TABLE IF NOT EXISTS model_catalogs (
    config_key TEXT NOT NULL,
    account_key TEXT NOT NULL,
    status TEXT NOT NULL,
    catalog TEXT,
    error TEXT,
    attempted_at TEXT NOT NULL,
    PRIMARY KEY (config_key, account_key)
) WITHOUT ROWID;
PRAGMA user_version = 1;";

/// Last failure as stored in `model_catalogs.error`.
#[derive(Serialize, Deserialize)]
struct Failure {
    status: Status,
    error: Localized,
    at: DateTime<Utc>,
}

/// Model catalog store for one configuration. Holds only the database path
/// and the configuration digest; the file is opened per operation, so
/// building a cache never touches disk and errors surface on use.
#[derive(Debug, Clone)]
pub struct ModelCache {
    path: PathBuf,
    config_key: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Everything that selects credential, product or region; refresh cadence
/// is excluded. Any edit or rename yields a new key (conservative).
fn account_key(account: &Account) -> String {
    let identity = serde_json::to_vec(&(
        account.provider,
        &account.name,
        &account.credential_env,
        &account.oauth,
        &account.params,
    ))
    .expect("account identity serializes");
    digest(&identity)
}

impl ModelCache {
    /// Store at `path`, isolated by `config` (only its digest is written).
    pub fn new(path: PathBuf, config: &ConfigIdentity) -> Self {
        let identity = match config {
            ConfigIdentity::File(file) => {
                [b"file:".as_slice(), file.as_os_str().as_encoded_bytes()].concat()
            }
            ConfigIdentity::Explicit(id) => [b"explicit:".as_slice(), id.as_bytes()].concat(),
        };
        Self {
            path,
            config_key: digest(&identity),
        }
    }

    /// `<OS persistent data dir>/cuotai/models.sqlite3` keyed by
    /// [`ConfigFile::identity`]; frontends sharing a TOML share entries.
    pub fn in_data_dir(file: &ConfigFile) -> Result<Self, ModelError> {
        let identity = file.identity().map_err(|_| ModelError::Storage)?;
        let dir = dirs::data_local_dir().ok_or(ModelError::Storage)?;
        Ok(Self::new(
            dir.join("cuotai").join("models.sqlite3"),
            &identity,
        ))
    }

    fn open(&self) -> Result<Connection, ModelError> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|_| ModelError::Storage)?;
        }
        // Create privately before SQLite opens it; never chmod an existing file.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&self.path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(ModelError::Storage),
        }
        let mut db = Connection::open(&self.path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match tx.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? {
            0 => tx.execute_batch(SCHEMA_V1)?,
            SCHEMA_VERSION => {}
            // Written by a newer Cuotai: never guess its layout.
            _ => return Err(ModelError::Storage),
        }
        tx.commit()?;
        Ok(db)
    }

    fn state(&self, account: &Account) -> Result<Option<ModelState>, ModelError> {
        let row = self
            .open()?
            .query_row(
                "SELECT status, catalog, error FROM model_catalogs
                 WHERE config_key = ?1 AND account_key = ?2",
                [&self.config_key, &account_key(account)],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((status, catalog, error)) = row else {
            return Ok(None);
        };
        let catalog: Option<ModelCatalog> = catalog
            .map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|_| ModelError::Storage)?;
        Ok(Some(match (status.as_str(), catalog, error) {
            ("ready", Some(catalog), _) => ModelState::Ready(catalog),
            ("failed", previous, Some(error)) => {
                let failure: Failure =
                    serde_json::from_str(&error).map_err(|_| ModelError::Storage)?;
                ModelState::Failed {
                    status: failure.status,
                    error: failure.error,
                    at: failure.at,
                    previous,
                }
            }
            _ => return Err(ModelError::Storage),
        }))
    }

    /// Persist the outcome of one attempt. A failure keeps the stored catalog.
    fn save(&self, account: &Account, state: &ModelState) -> Result<(), ModelError> {
        let encode =
            |value: Result<String, serde_json::Error>| value.map_err(|_| ModelError::Storage);
        let (status, catalog, error, at) = match state {
            ModelState::Ready(catalog) => (
                "ready",
                Some(encode(serde_json::to_string(catalog))?),
                None,
                catalog.fetched_at,
            ),
            ModelState::Failed {
                status, error, at, ..
            } => {
                let failure = Failure {
                    status: *status,
                    error: error.clone(),
                    at: *at,
                };
                (
                    "failed",
                    None,
                    Some(encode(serde_json::to_string(&failure))?),
                    *at,
                )
            }
            ModelState::NotFetched | ModelState::Unsupported { .. } => return Ok(()),
        };
        self.open()?.execute(
            "INSERT INTO model_catalogs (config_key, account_key, status, catalog, error, attempted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (config_key, account_key) DO UPDATE SET
                status = excluded.status,
                catalog = COALESCE(excluded.catalog, catalog),
                error = excluded.error,
                attempted_at = excluded.attempted_at",
            rusqlite::params![
                self.config_key,
                account_key(account),
                status,
                catalog,
                error,
                at.to_rfc3339()
            ],
        )?;
        Ok(())
    }
}

/// The account and cache of a supported provider, or its `Unsupported` state.
type Target<'a> = Result<(&'a Account, &'a ModelCache, Localized), ModelState>;

impl Monitor {
    fn model_target(&self, index: usize) -> Result<Target<'_>, ModelError> {
        let account = self
            .config
            .accounts
            .get(index)
            .ok_or(ModelError::InvalidIndex)?;
        let Some(source) = source(account.provider) else {
            return Ok(Err(ModelState::Unsupported {
                reason: unsupported_reason(account.provider),
            }));
        };
        let cache = self.models.as_ref().ok_or(ModelError::NoCache)?;
        Ok(Ok((account, cache, source)))
    }

    /// Persisted catalog state of account `index`; never uses the network.
    pub fn model_state(&self, index: usize) -> Result<ModelState, ModelError> {
        match self.model_target(index)? {
            Ok((account, cache, _)) => Ok(cache.state(account)?.unwrap_or(ModelState::NotFetched)),
            Err(unsupported) => Ok(unsupported),
        }
    }

    /// First explicit opening: queries once when nothing is recorded;
    /// a recorded success (even empty) or failure is returned as is.
    // ponytail: two concurrent first loads may both query; RF3.4 adds the
    // per-account SQLite claim.
    pub async fn load_models(&self, index: usize) -> Result<ModelState, ModelError> {
        match self.model_state(index)? {
            ModelState::NotFetched => self.sync_models(index).await,
            state => Ok(state),
        }
    }

    /// Manual sync: always one query. Success replaces the catalog; failure
    /// records the error and keeps the previous catalog searchable.
    pub async fn sync_models(&self, index: usize) -> Result<ModelState, ModelError> {
        let (account, cache, source) = match self.model_target(index)? {
            Ok(target) => target,
            Err(unsupported) => return Ok(unsupported),
        };
        let state = match self.fetch_models(account).await {
            Ok(models) => ModelState::Ready(ModelCatalog {
                models,
                source,
                fetched_at: Utc::now(),
            }),
            Err(error) => ModelState::Failed {
                status: error.status,
                error: error.note,
                at: Utc::now(),
                previous: None,
            },
        };
        cache.save(account, &state)?;
        Ok(cache.state(account)?.unwrap_or(ModelState::NotFetched))
    }

    async fn fetch_models(
        &self,
        account: &Account,
    ) -> Result<Vec<ModelInfo>, providers::QueryError> {
        #[cfg(test)]
        let url = self
            .groq_url
            .as_deref()
            .unwrap_or(providers::router_keys::GROQ_MODELS_URL);
        #[cfg(not(test))]
        let url = providers::router_keys::GROQ_MODELS_URL;
        self.with_access(
            account,
            async |account: &Account, ctx: &providers::Context<'_>| match account.provider {
                Provider::Groq => providers::router_keys::groq_models_at(account, ctx, url).await,
                other => unreachable!("{other:?} has no model source"),
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{oauth, Config, Credentials};
    use std::sync::Arc;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Keys;
    impl Credentials for Keys {
        fn get(&self, name: &str) -> Option<String> {
            name.strip_prefix("GROQ_KEY_").map(|a| format!("gsk_{a}"))
        }
    }
    struct NoTokens;
    impl oauth::TokenStore for NoTokens {
        fn load(&self, _: &str) -> Result<Option<oauth::OAuthToken>, oauth::OAuthError> {
            Ok(None)
        }
        fn save(&self, _: &str, _: &oauth::OAuthToken) -> Result<(), oauth::OAuthError> {
            unreachable!()
        }
        fn delete(&self, _: &str) -> Result<(), oauth::OAuthError> {
            unreachable!()
        }
    }

    const CONFIG: &str = r#"
[[accounts]]
name = "alpha"
provider = "groq"
credential_env = "GROQ_KEY_ALPHA"
[[accounts]]
name = "beta"
provider = "groq"
credential_env = "GROQ_KEY_BETA"
[[accounts]]
name = "openai"
provider = "open_ai_api"
credential_env = "OPENAI_ADMIN_KEY"
"#;

    /// A fresh Monitor (as after a restart) on the same database file.
    fn monitor(server: &MockServer, db: &std::path::Path, config: &str) -> Monitor {
        let creds: Arc<dyn Credentials> = Arc::new(Keys);
        let session =
            Arc::new(oauth::OAuthSession::new(Arc::new(NoTokens), Arc::clone(&creds)).unwrap());
        let mut monitor = Monitor::with_oauth(Config::from_toml(CONFIG).unwrap(), creds, session)
            .unwrap()
            .with_model_cache(ModelCache::new(
                db.to_path_buf(),
                &ConfigIdentity::Explicit(config.into()),
            ));
        monitor.groq_url = Some(server.uri());
        monitor
    }

    async fn reply(server: &MockServer, account: &str, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(header(
                "authorization",
                format!("Bearer gsk_{account}").as_str(),
            ))
            .respond_with(response)
            .mount(server)
            .await;
    }

    fn list(ids: &[&str]) -> ResponseTemplate {
        let data: Vec<_> = ids
            .iter()
            .map(|id| serde_json::json!({ "id": id, "object": "model", "owned_by": "Meta" }))
            .collect();
        ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": data }))
    }

    async fn requests(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    fn ids(state: &ModelState) -> Vec<&str> {
        match state {
            ModelState::Ready(catalog) => catalog.models.iter().map(|m| m.id.as_str()).collect(),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn first_load_queries_once_per_account_and_nothing_else_does() {
        let server = MockServer::start().await;
        reply(&server, "ALPHA", list(&["llama-b", "llama-a", "llama-b"])).await;
        reply(&server, "BETA", list(&["whisper"])).await;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("models.sqlite3");
        let first = monitor(&server, &db, "one");

        assert_eq!(first.model_state(0).unwrap(), ModelState::NotFetched);
        assert_eq!(
            ids(&first.load_models(0).await.unwrap()),
            ["llama-a", "llama-b"]
        );
        assert_eq!(requests(&server).await, 1);
        assert_eq!(ids(&first.load_models(1).await.unwrap()), ["whisper"]);
        assert_eq!(
            ids(&first.load_models(0).await.unwrap()),
            ["llama-a", "llama-b"]
        );
        assert_eq!(requests(&server).await, 2);

        let restarted = monitor(&server, &db, "one");
        assert_eq!(ids(&restarted.load_models(1).await.unwrap()), ["whisper"]);
        assert_eq!(requests(&server).await, 2, "restart reads the cache");
        restarted.refresh().await;
        assert_eq!(requests(&server).await, 4, "only the two quota calls");
        assert_eq!(
            ids(&restarted.model_state(0).unwrap()),
            ["llama-a", "llama-b"]
        );

        let other_config = monitor(&server, &db, "two");
        assert_eq!(other_config.model_state(0).unwrap(), ModelState::NotFetched);
        assert_eq!(requests(&server).await, 4);
    }

    #[tokio::test]
    async fn empty_and_failed_first_attempts_are_recorded_and_not_retried() {
        let server = MockServer::start().await;
        reply(&server, "ALPHA", list(&[])).await;
        reply(
            &server,
            "BETA",
            ResponseTemplate::new(401).set_body_string("gsk_BETA bad"),
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("models.sqlite3");
        let first = monitor(&server, &db, "one");

        let empty = first.load_models(0).await.unwrap();
        assert!(matches!(&empty, ModelState::Ready(c) if c.models.is_empty()));
        let ModelState::Failed {
            status,
            error,
            previous: None,
            ..
        } = first.load_models(1).await.unwrap()
        else {
            panic!("401 must be recorded as Failed without catalog");
        };
        assert_eq!(status, Status::AuthenticationError);
        assert!(!error.fallback.contains("gsk_"), "no body echo");
        assert_eq!(requests(&server).await, 2);

        let restarted = monitor(&server, &db, "one");
        assert_eq!(restarted.load_models(0).await.unwrap(), empty);
        assert!(matches!(
            restarted.load_models(1).await.unwrap(),
            ModelState::Failed { .. }
        ));
        assert_eq!(
            requests(&server).await,
            2,
            "recorded attempts are not retried"
        );
        restarted.sync_models(1).await.unwrap();
        assert_eq!(requests(&server).await, 3, "sync queries exactly once");
    }

    #[tokio::test]
    async fn failed_sync_keeps_previous_catalog() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(list(&["llama"]))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        reply(&server, "ALPHA", ResponseTemplate::new(429)).await;
        let dir = tempfile::tempdir().unwrap();
        let first = monitor(&server, &dir.path().join("models.sqlite3"), "one");
        let ModelState::Ready(catalog) = first.load_models(0).await.unwrap() else {
            panic!("first load succeeds");
        };
        let ModelState::Failed {
            status, previous, ..
        } = first.sync_models(0).await.unwrap()
        else {
            panic!("429 sync is recorded");
        };
        assert_eq!(status, Status::RateLimited);
        assert_eq!(
            previous,
            Some(catalog.clone()),
            "old catalog and fetched_at kept"
        );
        assert_eq!(requests(&server).await, 2);

        server.reset().await;
        reply(
            &server,
            "ALPHA",
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "data": [{ "object": "model" }] })),
        )
        .await;
        let ModelState::Failed {
            status, previous, ..
        } = first.sync_models(0).await.unwrap()
        else {
            panic!("malformed list is rejected");
        };
        assert_eq!(status, Status::Error);
        assert_eq!(previous, Some(catalog));
    }

    #[tokio::test]
    async fn unsupported_invalid_index_and_storage_errors_are_typed() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let monitor_ok = monitor(&server, &dir.path().join("models.sqlite3"), "one");
        assert!(matches!(
            monitor_ok.load_models(2).await.unwrap(),
            ModelState::Unsupported { .. }
        ));
        assert!(matches!(
            monitor_ok.sync_models(2).await.unwrap(),
            ModelState::Unsupported { .. }
        ));
        assert_eq!(
            monitor_ok.load_models(3).await,
            Err(ModelError::InvalidIndex)
        );
        assert_eq!(monitor_ok.model_state(3), Err(ModelError::InvalidIndex));

        // The database path is a directory: the error surfaces, nothing is queried.
        let broken = monitor(&server, dir.path(), "one");
        assert_eq!(broken.load_models(0).await, Err(ModelError::Storage));
        assert_eq!(requests(&server).await, 0);
    }
}
