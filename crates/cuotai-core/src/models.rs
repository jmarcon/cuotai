//! PRD 0002 RF2/RF3 — per-account model catalog, queried only on explicit
//! request ([`Monitor::load_models`] once, [`Monitor::sync_models`] on demand)
//! and persisted per configuration in `models.sqlite3`.
//!
//! A catalog lists the model IDs the provider returned for the configured
//! credential; it proves neither inference permission, price nor current
//! availability. Quota refresh (`Monitor::refresh`, `RefreshCycle`, `Report`,
//! `Snapshot`) never reads or writes this store.
//!
//! Every query runs under a per-account claim row (owner token + lease)
//! taken in `BEGIN IMMEDIATE`, so windows and processes sharing the file
//! coalesce onto one request; only the claim owner may publish its result.

use crate::config_file::{ConfigFile, ConfigIdentity};
use crate::text::{params, provider_l10n, Localized};
use crate::{providers, Account, Monitor, Provider, Status};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    /// A live claim (this or another window/process) is querying now;
    /// `previous` stays searchable.
    Fetching {
        note: Localized,
        since: DateTime<Utc>,
        previous: Option<ModelCatalog>,
    },
    /// The attempt started `at` never finished (cancelled, deadline or a
    /// crashed process); nothing partial was saved and it is not retried
    /// automatically, only by [`Monitor::sync_models`].
    Interrupted {
        note: Localized,
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

/// Catalog source of `account`, or why it cannot be listed (no network).
/// The published matrix lives in `docs/providers.md`.
pub fn support(account: &Account) -> Result<Localized, Localized> {
    let provider = account.provider;
    let product = |value: &str| account.params.get("product").is_some_and(|v| v == value);
    let source = |id: &'static str, text: &'static str| Ok(Localized::fixed(id, text));
    let reason = |id: &'static str, text: &str| {
        Err(Localized::with_params(
            id,
            params([("provider", provider_l10n(provider).into())]),
            format!("{}: {text} Nenhuma consulta é feita.", provider.label()),
        ))
    };
    match provider {
        Provider::Groq => source(
            "models.source.groq",
            "Groq GET /openai/v1/models com a chave da conta; não comprova permissão de inferência, preço ou disponibilidade atual.",
        ),
        Provider::AwsBedrock => source(
            "models.source.aws_bedrock",
            "AWS CLI bedrock list-foundation-models no perfil da conta, em params.region ou na região padrão do perfil; não usa Cost Explorer nem comprova acesso ao modelo.",
        ),
        Provider::Antigravity => source(
            "models.source.antigravity",
            "Antigravity fetchAvailableModels com a sessão da conta (planos pagos); não comprova quota, preço ou disponibilidade atual.",
        ),
        Provider::Poolside => source(
            "models.source.poolside",
            "Poolside GET /v1/models com a chave da conta; não comprova permissão de inferência, preço ou disponibilidade atual.",
        ),
        Provider::GeminiAi if account.oauth.is_none() && !product("cli") => source(
            "models.source.gemini_ai",
            "Gemini API models.list paginado com a chave AI Studio da conta; não comprova quota, preço ou disponibilidade atual.",
        ),
        Provider::Kimi if account.oauth.is_none() && !product("coding") => source(
            "models.source.kimi",
            "Kimi GET /v1/models (api.moonshot.ai) com a chave da conta; não comprova permissão de inferência, preço ou disponibilidade atual.",
        ),
        Provider::AlibabaModelStudio if providers::alibaba_plans::model_studio_lists(account) => {
            source(
                "models.source.alibaba_model_studio",
                "Model Studio GET /api/v1/models (Singapura) paginado com a chave da conta; não comprova permissão de inferência, preço ou disponibilidade atual.",
            )
        }
        Provider::GeminiAi | Provider::Kimi => reason(
            "models.unsupported.product",
            "o produto ou login configurado não usa a chave de API que lista modelos.",
        ),
        Provider::Qwen if account.oauth.is_some() => reason(
            "models.unsupported.product",
            "o produto ou login configurado não usa a chave de API que lista modelos.",
        ),
        Provider::Qwen | Provider::AlibabaModelStudio => reason(
            "models.unsupported.region",
            "a listagem sem WorkspaceId só existe na região internacional (Singapura) do Model Studio; use Alibaba Model Studio com params.region = intl.",
        ),
        Provider::OpenAiApi
        | Provider::AnthropicApi
        | Provider::Mistral
        | Provider::Grok
        | Provider::Windsurf => reason(
            "models.unsupported.admin_key",
            "a conta guarda credencial administrativa ou de faturamento, não a chave de inferência exigida para listar modelos.",
        ),
        Provider::GeminiApi
        | Provider::VertexAi
        | Provider::AzureOpenAi
        | Provider::CloudflareAi
        | Provider::AlibabaCloud => reason(
            "models.unsupported.cloud_credential",
            "a credencial da conta (escopo Cloud Monitoring, papel Cost Management Reader, AccessKey de faturamento ou sem account_id) não comprova permissão de listar modelos.",
        ),
        Provider::MiniMax | Provider::AlibabaCodingPlan => reason(
            "models.unsupported.plan_key",
            "a chave da conta pode ser de assinatura/plano, que o contrato oficial de listagem não aceita.",
        ),
        Provider::AlibabaTokenPlan
        | Provider::ChatGpt
        | Provider::Claude
        | Provider::Cursor
        | Provider::Copilot
        | Provider::Perplexity
        | Provider::OllamaCloud
        | Provider::XiaomiMimo
        | Provider::OpenCodeGo
        | Provider::Qoder
        | Provider::NvidiaNim
        | Provider::Kiro
        | Provider::Cline
        | Provider::CodeBuddy
        | Provider::Zed
        | Provider::Kimchi => reason(
            "models.unsupported",
            "nenhum endpoint oficial verificado lista os modelos desta conta com a credencial configurada.",
        ),
    }
}

/// Reject blank ids, then sort by id and keep the first of each duplicate.
fn normalize(mut models: Vec<ModelInfo>) -> Result<Vec<ModelInfo>, providers::QueryError> {
    if models.iter().any(|m| m.id.trim().is_empty()) {
        return Err(providers::QueryError::schema());
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|later, first| later.id == first.id);
    Ok(models)
}

/// Note of a claimed attempt: `live` while it runs, otherwise interrupted.
pub(crate) fn claim_note(live: bool) -> Localized {
    if live {
        Localized::fixed(
            "models.fetching",
            "Consulta de modelos em andamento nesta ou em outra janela; o último catálogo continua pesquisável.",
        )
    } else {
        Localized::fixed(
            "models.interrupted",
            "A última consulta de modelos foi interrompida antes de concluir; nada parcial foi salvo. Sincronize para tentar de novo.",
        )
    }
}

const SCHEMA_VERSION: i64 = 1;
/// `catalog` is the last full success; `status` is `ready`, `failed` or
/// `fetching`, and a `fetching` row past `lease_until` (unix ms) reads as
/// `Interrupted`. `owner` is the token of the attempt holding the claim.
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS model_catalogs (
    config_key TEXT NOT NULL,
    account_key TEXT NOT NULL,
    status TEXT NOT NULL,
    catalog TEXT,
    error TEXT,
    attempted_at TEXT NOT NULL,
    owner TEXT,
    lease_until INTEGER,
    PRIMARY KEY (config_key, account_key)
) WITHOUT ROWID;
PRAGMA user_version = 1;";
/// How often a caller waiting on another claim re-reads the store.
const POLL: Duration = Duration::from_millis(100);

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

/// Params that only filter or date the quota view (usage model filter,
/// invoice month); they never select the credential or the listed scope.
const DISPLAY_PARAMS: [&str; 2] = ["models", "billing_cycle"];

/// Provider, name, credential reference (env name or OAuth config) and the
/// params selecting credential, product, region or profile; refresh cadence
/// and [`DISPLAY_PARAMS`] are excluded. Any other edit or a rename yields a
/// new key (conservative); the account index never takes part.
fn account_key(account: &Account) -> String {
    let params: std::collections::BTreeMap<_, _> = account
        .params
        .iter()
        .filter(|(key, _)| !DISPLAY_PARAMS.contains(&key.as_str()))
        .collect();
    let identity = serde_json::to_vec(&(
        account.provider,
        &account.name,
        &account.credential_env,
        &account.oauth,
        params,
    ))
    .expect("account identity serializes");
    digest(&identity)
}

/// Drop group/other permission bits of our directory or database file.
#[cfg(unix)]
fn private(path: &Path, dir: bool) -> Result<(), ModelError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|_| ModelError::Storage)?;
    if meta.is_dir() != dir {
        return Err(ModelError::Storage);
    }
    let mode = meta.permissions().mode();
    if mode & 0o077 != 0 {
        let owner = if dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & owner))
            .map_err(|_| ModelError::Storage)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn private(_: &Path, _: bool) -> Result<(), ModelError> {
    Ok(())
}

fn encode<T: Serialize>(value: &T) -> Result<String, ModelError> {
    serde_json::to_string(value).map_err(|_| ModelError::Storage)
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

    /// `<OS persistent data dir>/cuotai/models/models.sqlite3` keyed by
    /// [`ConfigFile::identity`]; frontends sharing a TOML share entries.
    /// The store owns its directory, so making it private touches nothing else.
    pub fn in_data_dir(file: &ConfigFile) -> Result<Self, ModelError> {
        let identity = file.identity().map_err(|_| ModelError::Storage)?;
        let dir = dirs::data_local_dir().ok_or(ModelError::Storage)?;
        Ok(Self::new(
            dir.join("cuotai").join("models").join("models.sqlite3"),
            &identity,
        ))
    }

    /// Open (creating privately) the store. The parent directory is
    /// restricted to the owner (0700) and the file to 0600 on Unix, even
    /// when they already existed; give the store a directory of its own.
    fn open(&self) -> Result<Connection, ModelError> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|_| ModelError::Storage)?;
            private(parent, true)?;
        }
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
        private(&self.path, false)?;
        let mut db = Connection::open(&self.path)?;
        db.busy_timeout(Duration::from_secs(5))?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match tx.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? {
            0 => tx.execute_batch(SCHEMA)?,
            SCHEMA_VERSION => {}
            // Written by a newer Cuotai: never guess its layout.
            _ => return Err(ModelError::Storage),
        }
        tx.commit()?;
        Ok(db)
    }

    fn state(&self, account: &Account) -> Result<Option<ModelState>, ModelError> {
        read(&self.open()?, &self.config_key, &account_key(account))
    }

    /// Take the per-account claim inside `BEGIN IMMEDIATE`. A live claim
    /// of anyone yields `Busy`; with `first`, a recorded attempt (success,
    /// failure or interruption) is returned as is instead of claiming.
    fn claim(&self, account: &Account, lease: Duration, first: bool) -> Result<Claim, ModelError> {
        let key = account_key(account);
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match read(&tx, &self.config_key, &key)? {
            Some(ModelState::Fetching { .. }) => return Ok(Claim::Busy),
            Some(state) if first => return Ok(Claim::Settled(state)),
            _ => {}
        }
        let token = uuid::Uuid::new_v4().to_string();
        let lease_until = Utc::now().timestamp_millis()
            + i64::try_from(lease.as_millis()).map_err(|_| ModelError::Storage)?;
        tx.execute(
            "INSERT INTO model_catalogs (config_key, account_key, status, attempted_at, owner, lease_until)
             VALUES (?1, ?2, 'fetching', ?3, ?4, ?5)
             ON CONFLICT (config_key, account_key) DO UPDATE SET
                status = 'fetching',
                attempted_at = excluded.attempted_at,
                owner = excluded.owner,
                lease_until = excluded.lease_until",
            rusqlite::params![self.config_key, key, Utc::now().to_rfc3339(), token, lease_until],
        )?;
        tx.commit()?;
        Ok(Claim::Owned(Attempt {
            cache: self.clone(),
            key,
            token,
            done: false,
        }))
    }

    /// Re-read until another claim settles; bounded by its lease, after
    /// which the row reads as `Interrupted`.
    /// Takes `self` by value: see `run_models` on the `Send` bound.
    async fn wait(self, account: &Account) -> Result<ModelState, ModelError> {
        loop {
            tokio::time::sleep(POLL).await;
            match self.state(account)? {
                Some(ModelState::Fetching { .. }) => continue,
                state => return Ok(state.unwrap_or(ModelState::NotFetched)),
            }
        }
    }

    /// Delete entries of this configuration whose account identity is not
    /// in `saved`. Pass only the configuration just loaded from or saved to
    /// the TOML: another window's live accounts are in that file too.
    fn prune(&self, saved: &crate::Config) -> Result<(), ModelError> {
        // Nothing stored yet: do not create the store just to prune it.
        if !self.path.try_exists().map_err(|_| ModelError::Storage)? {
            return Ok(());
        }
        let keep = encode(&saved.accounts.iter().map(account_key).collect::<Vec<_>>())?;
        self.open()?.execute(
            "DELETE FROM model_catalogs WHERE config_key = ?1
             AND account_key NOT IN (SELECT value FROM json_each(?2))",
            [&self.config_key, &keep],
        )?;
        Ok(())
    }
}

/// Stored state of one account, `None` without any attempt.
fn read(db: &Connection, config_key: &str, key: &str) -> Result<Option<ModelState>, ModelError> {
    let row = db
        .query_row(
            "SELECT status, catalog, error, attempted_at, lease_until FROM model_catalogs
             WHERE config_key = ?1 AND account_key = ?2",
            [config_key, key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((status, catalog, error, attempted_at, lease_until)) = row else {
        return Ok(None);
    };
    let previous: Option<ModelCatalog> = catalog
        .map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|_| ModelError::Storage)?;
    Ok(Some(
        match (status.as_str(), previous, error, lease_until) {
            ("ready", Some(catalog), _, _) => ModelState::Ready(catalog),
            ("failed", previous, Some(error), _) => {
                let failure: Failure =
                    serde_json::from_str(&error).map_err(|_| ModelError::Storage)?;
                ModelState::Failed {
                    status: failure.status,
                    error: failure.error,
                    at: failure.at,
                    previous,
                }
            }
            ("fetching", previous, _, Some(lease_until)) => {
                let at = DateTime::parse_from_rfc3339(&attempted_at)
                    .map_err(|_| ModelError::Storage)?
                    .with_timezone(&Utc);
                if lease_until > Utc::now().timestamp_millis() {
                    ModelState::Fetching {
                        note: claim_note(true),
                        since: at,
                        previous,
                    }
                } else {
                    ModelState::Interrupted {
                        note: claim_note(false),
                        at,
                        previous,
                    }
                }
            }
            _ => return Err(ModelError::Storage),
        },
    ))
}

enum Claim {
    Owned(Attempt),
    Settled(ModelState),
    Busy,
}

/// A claimed attempt. Only its owner token may publish; dropped unfinished
/// (cancelled future, panic) it expires its lease so the row reads as
/// `Interrupted` at once. A crashed process leaves the lease to expire.
/// Owns its cache handle: a borrowed one held across the fetch await makes
/// the model futures fail the higher-ranked `Send` check `tokio::spawn` needs.
struct Attempt {
    cache: ModelCache,
    key: String,
    token: String,
    done: bool,
}

impl Attempt {
    /// Publish a full success or a failure (keeping the stored catalog)
    /// if this attempt still owns the claim; a stale attempt changes
    /// nothing. Returns the stored state either way.
    fn finish(mut self, outcome: Result<ModelCatalog, Failure>) -> Result<ModelState, ModelError> {
        let (status, catalog, error, at) = match &outcome {
            Ok(catalog) => ("ready", Some(encode(catalog)?), None, catalog.fetched_at),
            Err(failure) => ("failed", None, Some(encode(failure)?), failure.at),
        };
        let cache = &self.cache;
        let mut db = cache.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE model_catalogs SET status = ?1, catalog = COALESCE(?2, catalog),
                error = ?3, attempted_at = ?4, owner = NULL, lease_until = NULL
             WHERE config_key = ?5 AND account_key = ?6 AND owner = ?7 AND status = 'fetching'",
            rusqlite::params![
                status,
                catalog,
                error,
                at.to_rfc3339(),
                cache.config_key,
                self.key,
                self.token
            ],
        )?;
        let state = read(&tx, &cache.config_key, &self.key)?;
        tx.commit()?;
        // Any earlier error leaves `done` unset: Drop expires the claim.
        self.done = true;
        Ok(state.unwrap_or(ModelState::NotFetched))
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        // Best effort: on failure the lease still expires on its own.
        let _ = self.cache.open().map(|db| {
            db.execute(
                "UPDATE model_catalogs SET lease_until = 0
                 WHERE config_key = ?1 AND account_key = ?2 AND owner = ?3 AND status = 'fetching'",
                [&self.cache.config_key, &self.key, &self.token],
            )
        });
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
        let source = match support(account) {
            Ok(source) => source,
            Err(reason) => return Ok(Err(ModelState::Unsupported { reason })),
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

    /// Delete stored catalogs of this configuration whose account identity
    /// is absent from it (removed, renamed or re-credentialed). Call it only
    /// on a monitor whose configuration was just loaded from or saved to the
    /// TOML, so accounts another window still shows are in it too.
    pub fn prune_models(&self) -> Result<(), ModelError> {
        match &self.models {
            Some(cache) => cache.prune(&self.config),
            None => Ok(()),
        }
    }

    /// First explicit opening: queries once when nothing is recorded; a
    /// recorded success (even empty), failure or interruption is returned
    /// as is. While another window/process holds the claim it waits for
    /// that result instead of querying.
    pub async fn load_models(&self, index: usize) -> Result<ModelState, ModelError> {
        self.run_models(index, true).await
    }

    /// Manual sync: one query, coalesced with a claim already in flight.
    /// Success replaces the catalog; failure records the error and keeps
    /// the previous catalog (and its date) searchable.
    pub async fn sync_models(&self, index: usize) -> Result<ModelState, ModelError> {
        self.run_models(index, false).await
    }

    async fn run_models(&self, index: usize, first: bool) -> Result<ModelState, ModelError> {
        let (account, cache, source) = match self.model_target(index)? {
            Ok(target) => target,
            Err(unsupported) => return Ok(unsupported),
        };
        // Owned account and cache handle: borrows returned through
        // `model_target` and held across the awaits below fail the
        // higher-ranked `Send` check `tokio::spawn` needs.
        let (account, cache) = (account.clone(), cache.clone());
        let account = &account;
        // The claim outlives the account deadline plus slot waiting; the
        // same bound cancels our fetch, so a live lease means a live query.
        let lease = Duration::from_secs(self.config.timeout_seconds + 30);
        let attempt = match cache.claim(account, lease, first)? {
            Claim::Owned(attempt) => attempt,
            Claim::Settled(state) => return Ok(state),
            Claim::Busy => return cache.wait(account).await,
        };
        let fetch = self.with_access(
            account,
            async |account: &Account, ctx: &providers::Context<'_>| {
                self.list_models(account, ctx).await
            },
        );
        let Ok(fetched) = tokio::time::timeout(lease, fetch).await else {
            drop(attempt);
            return Ok(cache.state(account)?.unwrap_or(ModelState::NotFetched));
        };
        attempt.finish(match fetched.and_then(normalize) {
            Ok(models) => Ok(ModelCatalog {
                models,
                source,
                fetched_at: Utc::now(),
            }),
            Err(error) => Err(Failure {
                status: error.status,
                error: error.note,
                at: Utc::now(),
            }),
        })
    }

    async fn list_models(
        &self,
        account: &Account,
        ctx: &providers::Context<'_>,
    ) -> Result<Vec<ModelInfo>, providers::QueryError> {
        use providers::{openai_models, secret};
        #[cfg(test)]
        let groq = self
            .groq_url
            .as_deref()
            .unwrap_or(providers::router_keys::GROQ_MODELS_URL);
        #[cfg(not(test))]
        let groq = providers::router_keys::GROQ_MODELS_URL;
        match account.provider {
            Provider::Groq => {
                openai_models(ctx, groq, &secret(account, ctx, "GROQ_API_KEY")?).await
            }
            Provider::Kimi => {
                let key = secret(account, ctx, "KIMI_API_KEY")?;
                openai_models(ctx, providers::asia::KIMI_MODELS_URL, &key).await
            }
            Provider::Poolside => {
                let key = secret(account, ctx, "POOLSIDE_API_KEY")?;
                openai_models(ctx, providers::inference::POOLSIDE_MODELS_URL, &key).await
            }
            Provider::GeminiAi => {
                let url = providers::cloud_keys::GEMINI_MODELS_URL;
                providers::cloud_keys::gemini_models(account, ctx, url).await
            }
            Provider::AwsBedrock => providers::cloud::bedrock_models(account, ctx, "aws").await,
            Provider::Antigravity => {
                use providers::antigravity::{
                    models, FETCH_AVAILABLE_MODELS_URL, LOAD_CODE_ASSIST_URL,
                };
                models(
                    account,
                    ctx,
                    LOAD_CODE_ASSIST_URL,
                    FETCH_AVAILABLE_MODELS_URL,
                )
                .await
            }
            Provider::AlibabaModelStudio => {
                let url = providers::alibaba_plans::MODEL_STUDIO_MODELS_URL;
                providers::alibaba_plans::model_studio_models(account, ctx, url).await
            }
            other => unreachable!("{other:?} has no model source"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{oauth, Config, Credentials};
    use std::sync::Arc;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// UIs spawn model operations on the multi-thread runtime (TUI tasks,
    /// Tauri commands), so their futures must stay `Send`; compile-time only.
    #[allow(dead_code)]
    fn model_futures_are_send(monitor: Arc<Monitor>) {
        fn send<T: Send>(_: T) {}
        let m = Arc::clone(&monitor);
        send(async move { m.load_models(0).await });
        send(async move { monitor.sync_models(0).await });
    }

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
        monitor_with(server, db, &ConfigIdentity::Explicit(config.into()), CONFIG)
    }

    /// An independent Monitor (another window or process) for `toml`.
    fn monitor_with(
        server: &MockServer,
        db: &std::path::Path,
        config: &ConfigIdentity,
        toml: &str,
    ) -> Monitor {
        let creds: Arc<dyn Credentials> = Arc::new(Keys);
        let session =
            Arc::new(oauth::OAuthSession::new(Arc::new(NoTokens), Arc::clone(&creds)).unwrap());
        let mut monitor = Monitor::with_oauth(Config::from_toml(toml).unwrap(), creds, session)
            .unwrap()
            .with_model_cache(ModelCache::new(db.to_path_buf(), config));
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
        let db = dir.path().join("models.sqlite3");
        std::fs::create_dir(&db).unwrap();
        let broken = monitor(&server, &db, "one");
        assert_eq!(broken.load_models(0).await, Err(ModelError::Storage));
        assert_eq!(requests(&server).await, 0);
    }

    /// Mirrors the matrix in docs/providers.md: default accounts of every
    /// provider, plus the product/region/login variants that switch it off.
    #[tokio::test]
    async fn support_matches_published_matrix_and_unsupported_never_queries() {
        use Provider::*;
        let supported = [
            Groq,
            AwsBedrock,
            Antigravity,
            Poolside,
            GeminiAi,
            Kimi,
            AlibabaModelStudio,
        ];
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let config = Config::default();
        let all = monitor_with(
            &server,
            &dir.path().join("models.sqlite3"),
            &ConfigIdentity::Explicit("matrix".into()),
            &toml::to_string(&config).unwrap(),
        );
        for (index, account) in config.accounts.iter().enumerate() {
            let listed = support(account).is_ok();
            let provider = account.provider;
            assert_eq!(listed, supported.contains(&provider), "{provider:?}");
            if !listed {
                for state in [all.load_models(index).await, all.sync_models(index).await] {
                    assert!(matches!(state.unwrap(), ModelState::Unsupported { .. }));
                }
            }
        }
        assert_eq!(requests(&server).await, 0, "Unsupported uses no network");

        let variant = |provider, key: &str, value: &str| Account {
            provider,
            params: [(key.to_string(), value.to_string())].into(),
            ..Default::default()
        };
        for off in [
            variant(GeminiAi, "product", "cli"),
            variant(Kimi, "product", "coding"),
            variant(AlibabaModelStudio, "region", "cn"),
            Account {
                provider: GeminiAi,
                oauth: Some(
                    serde_json::from_value(serde_json::json!({ "credential_id": "c" })).unwrap(),
                ),
                ..Default::default()
            },
        ] {
            assert!(support(&off).is_err(), "{off:?}");
        }
    }

    #[test]
    fn normalize_dedupes_and_sorts_without_rewriting_ids() {
        let info = |id: &str, name: &str| ModelInfo {
            id: id.into(),
            name: Some(name.into()),
            description: None,
        };
        let models = normalize(vec![info("b", "1"), info("A", "2"), info("b", "3")]).unwrap();
        assert_eq!(models, [info("A", "2"), info("b", "1")]);
        assert!(normalize(vec![info(" ", "blank")]).is_err());
    }

    fn slow(ids: &[&str]) -> ResponseTemplate {
        list(ids).set_delay(Duration::from_millis(300))
    }

    #[tokio::test]
    async fn two_instances_on_one_file_issue_one_request_and_coalesce_syncs() {
        let server = MockServer::start().await;
        reply(&server, "ALPHA", slow(&["llama"])).await;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("models.sqlite3");
        let (tui, desktop) = (monitor(&server, &db, "one"), monitor(&server, &db, "one"));

        let (a, b) = tokio::join!(tui.load_models(0), desktop.load_models(0));
        assert_eq!(ids(&a.unwrap()), ["llama"]);
        assert_eq!(
            ids(&b.unwrap()),
            ["llama"],
            "waiter reads the owner's result"
        );
        assert_eq!(requests(&server).await, 1);

        let (a, b) = tokio::join!(tui.sync_models(0), desktop.sync_models(0));
        assert_eq!(a.unwrap(), b.unwrap());
        assert_eq!(requests(&server).await, 2, "concurrent syncs coalesce");
    }

    #[tokio::test]
    async fn crashed_or_cancelled_claims_read_interrupted_until_manual_sync() {
        let server = MockServer::start().await;
        reply(&server, "ALPHA", list(&["llama"])).await;
        reply(&server, "BETA", slow(&["whisper"])).await;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("models.sqlite3");
        let first = monitor(&server, &db, "one");
        let ModelState::Ready(catalog) = first.load_models(0).await.unwrap() else {
            panic!("first load succeeds");
        };

        // Another process holds a live claim: reads show it, without network.
        let cache = first.models.as_ref().unwrap();
        let alpha = &first.config.accounts[0];
        let Ok(Claim::Owned(live)) = cache.claim(alpha, Duration::from_secs(60), false) else {
            panic!("claim is free");
        };
        assert!(matches!(
            &first.model_state(0).unwrap(),
            ModelState::Fetching { previous: Some(p), .. } if *p == catalog
        ));
        // That process crashes: no Drop runs and its lease runs out.
        std::mem::forget(live);
        cache
            .open()
            .unwrap()
            .execute("UPDATE model_catalogs SET lease_until = 1", [])
            .unwrap();

        let restarted = monitor(&server, &db, "one");
        let ModelState::Interrupted { previous, .. } = restarted.load_models(0).await.unwrap()
        else {
            panic!("abandoned claim reads as Interrupted");
        };
        assert_eq!(previous, Some(catalog), "last catalog and date kept");
        assert_eq!(requests(&server).await, 1, "no automatic refetch");
        assert_eq!(ids(&restarted.sync_models(0).await.unwrap()), ["llama"]);
        assert_eq!(requests(&server).await, 2, "manual sync recovers");

        // A cancelled first load publishes nothing and is not retried.
        let cancelled =
            tokio::time::timeout(Duration::from_millis(50), restarted.load_models(1)).await;
        assert!(cancelled.is_err());
        assert!(matches!(
            restarted.load_models(1).await.unwrap(),
            ModelState::Interrupted { previous: None, .. }
        ));
        assert_eq!(requests(&server).await, 3);
    }

    #[tokio::test]
    async fn stale_completion_never_overwrites_a_newer_result() {
        let server = MockServer::start().await;
        reply(&server, "ALPHA", list(&["new"])).await;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("models.sqlite3");
        let old = monitor(&server, &db, "one");
        let cache = old.models.as_ref().unwrap();
        let Ok(Claim::Owned(stale)) = cache.claim(&old.config.accounts[0], Duration::ZERO, false)
        else {
            panic!("claim is free");
        };
        assert_eq!(
            ids(&monitor(&server, &db, "one").sync_models(0).await.unwrap()),
            ["new"]
        );
        let late = ModelCatalog {
            models: Vec::new(),
            source: support(&old.config.accounts[0]).unwrap(),
            fetched_at: Utc::now(),
        };
        assert_eq!(ids(&stale.finish(Ok(late)).unwrap()), ["new"]);
        assert_eq!(ids(&old.model_state(0).unwrap()), ["new"]);
    }

    #[tokio::test]
    async fn identity_follows_moves_and_drops_edits_renames_and_removals() {
        let server = MockServer::start().await;
        reply(&server, "ALPHA", list(&["llama"])).await;
        reply(&server, "BETA", list(&["whisper"])).await;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("models.sqlite3");
        let id = ConfigIdentity::Explicit("one".into());
        let first = monitor(&server, &db, "one");
        first.load_models(0).await.unwrap();
        first.load_models(1).await.unwrap();
        monitor(&server, &db, "two").load_models(0).await.unwrap();
        let groq = |name: &str, env: &str, extra: &str| {
            format!("[[accounts]]\nname = \"{name}\"\nprovider = \"groq\"\ncredential_env = \"{env}\"\n{extra}")
        };
        let beta = groq("beta", "GROQ_KEY_BETA", "");

        let moved = monitor_with(
            &server,
            &db,
            &id,
            &(beta.clone() + &groq("alpha", "GROQ_KEY_ALPHA", "")),
        );
        assert_eq!(
            ids(&moved.model_state(1).unwrap()),
            ["llama"],
            "moves keep entries"
        );
        let filtered = groq("alpha", "GROQ_KEY_ALPHA", "params = { models = \"llama\" }");
        let filtered = monitor_with(&server, &db, &id, &filtered);
        assert_eq!(
            ids(&filtered.model_state(0).unwrap()),
            ["llama"],
            "display params ignored"
        );
        for edited in [
            groq("alpha", "GROQ_KEY_BETA", ""),
            groq("alpha2", "GROQ_KEY_ALPHA", ""),
            groq("alpha", "GROQ_KEY_ALPHA", "params = { region = \"eu\" }"),
        ] {
            let edited = monitor_with(&server, &db, &id, &edited);
            assert_eq!(edited.model_state(0).unwrap(), ModelState::NotFetched);
        }

        // Saved config without alpha: only alpha of this config is deleted.
        monitor_with(&server, &db, &id, &beta)
            .prune_models()
            .unwrap();
        let restarted = monitor(&server, &db, "one");
        assert_eq!(restarted.model_state(0).unwrap(), ModelState::NotFetched);
        assert_eq!(ids(&restarted.model_state(1).unwrap()), ["whisper"]);
        assert_eq!(
            ids(&monitor(&server, &db, "two").model_state(0).unwrap()),
            ["llama"]
        );
        assert_eq!(requests(&server).await, 3);
    }

    #[tokio::test]
    async fn same_toml_through_other_paths_shares_and_other_toml_isolates() {
        let server = MockServer::start().await;
        reply(&server, "ALPHA", list(&["llama"])).await;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("models.sqlite3");
        for sub in ["a", "b"] {
            std::fs::create_dir(dir.path().join(sub)).unwrap();
            std::fs::write(dir.path().join(sub).join("cuotai.toml"), CONFIG).unwrap();
        }
        let of = |path: PathBuf| ConfigIdentity::of_path(&path).unwrap();
        let tui = monitor_with(&server, &db, &of(dir.path().join("a/cuotai.toml")), CONFIG);
        let desktop = of(dir.path().join("b/../a/./cuotai.toml"));
        let desktop = monitor_with(&server, &db, &desktop, CONFIG);
        let other = monitor_with(&server, &db, &of(dir.path().join("b/cuotai.toml")), CONFIG);

        tui.load_models(0).await.unwrap();
        assert_eq!(ids(&desktop.load_models(0).await.unwrap()), ["llama"]);
        assert_eq!(other.model_state(0).unwrap(), ModelState::NotFetched);
        assert_eq!(requests(&server).await, 1);
    }

    #[tokio::test]
    async fn store_holds_digests_only() {
        let server = MockServer::start().await;
        reply(&server, "ALPHA", list(&["llama"])).await;
        reply(
            &server,
            "BETA",
            ResponseTemplate::new(500).set_body_string("gsk_BETA"),
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("models.sqlite3");
        let first = monitor(&server, &db, "secret-config-id");
        first.load_models(0).await.unwrap();
        first.load_models(1).await.unwrap();
        let bytes = String::from_utf8_lossy(&std::fs::read(&db).unwrap()).into_owned();
        for raw in [
            "gsk_",
            "GROQ_KEY",
            "alpha",
            "beta",
            "secret-config-id",
            "127.0.0.1",
        ] {
            assert!(!bytes.contains(raw), "{raw} stored in the model cache");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn store_dir_and_file_are_private_even_when_preexisting() {
        use std::os::unix::fs::PermissionsExt;
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let fresh = dir.path().join("fresh").join("models.sqlite3");
        monitor(&server, &fresh, "one").model_state(0).unwrap();
        assert_eq!(
            (mode(fresh.parent().unwrap()), mode(&fresh)),
            (0o700, 0o600)
        );

        let open = dir.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        let db = open.join("models.sqlite3");
        std::fs::write(&db, b"").unwrap();
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
        monitor(&server, &db, "one").model_state(0).unwrap();
        assert_eq!((mode(&open), mode(&db)), (0o700, 0o600));
    }
}
