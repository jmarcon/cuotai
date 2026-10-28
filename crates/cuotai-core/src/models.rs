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
use crate::text::{params, provider_l10n, Localized, ParamValue};
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

impl ModelState {
    /// Catalog still shown for this state: the current one when `Ready`,
    /// otherwise the last successful one kept across a failure or a query
    /// in flight. `None` when the account was never synced or cannot list.
    pub fn catalog(&self) -> Option<&ModelCatalog> {
        match self {
            Self::Ready(catalog) => Some(catalog),
            Self::Failed { previous, .. }
            | Self::Fetching { previous, .. }
            | Self::Interrupted { previous, .. } => previous.as_ref(),
            Self::NotFetched | Self::Unsupported { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("Account does not exist in this configuration.")]
    InvalidIndex,
    #[error("Model cache not configured for this configuration.")]
    NoCache,
    #[error("Could not read or write the model cache; no result was saved.")]
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

/// PRD 0028 RF10.1: why a listing interface is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceBasis {
    /// The vendor documents the listing endpoint for this credential.
    Documented,
    /// Undocumented, but confirmed against a real account; the reference is
    /// the public contract the quota query already follows.
    VerifiedLive,
}

/// PRD 0028 RF10.1: the evidence behind an account's model listing. A
/// listing without one is `Unsupported`; there is no embedded fallback list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModelEvidence {
    pub basis: EvidenceBasis,
    pub evidence: crate::access::Evidence,
    /// ISO date of the last check of `evidence`.
    pub verified_on: &'static str,
}

const MODELS_VERIFIED_ON: &str = "2026-10-20";

/// Evidence behind `account`'s model listing; `Some` exactly when
/// [`support`] is `Ok`.
pub fn evidence(account: &Account) -> Option<ModelEvidence> {
    contract(account).ok().map(|(_, evidence)| evidence)
}

/// Catalog source of `account`, or why it cannot be listed (no network).
/// The published matrix lives in `docs/providers.md`.
///
/// A provider lists models only where the listing interface is documented
/// or was verified live (PRD 0028 RF10.1); otherwise the capability is
/// `Unsupported` and nothing is queried or invented.
pub fn support(account: &Account) -> Result<Localized, Localized> {
    contract(account).map(|(source, _)| source)
}

fn contract(account: &Account) -> Result<(Localized, ModelEvidence), Localized> {
    use crate::access::Evidence::Documentation;
    let provider = account.provider;
    let product = |value: &str| account.params.get("product").is_some_and(|v| v == value);
    let documented = |url: &'static str| ModelEvidence {
        basis: EvidenceBasis::Documented,
        evidence: Documentation(url),
        verified_on: MODELS_VERIFIED_ON,
    };
    let verified_live = || ModelEvidence {
        basis: EvidenceBasis::VerifiedLive,
        // The model listing of a Cursor account uses the personal session, not
        // the Teams admin key the default quota mode cites.
        evidence: if provider == Provider::Cursor {
            crate::access::classify(provider, true, &Default::default()).evidence
        } else {
            crate::access::describe(account).evidence
        },
        verified_on: MODELS_VERIFIED_ON,
    };
    let source = |id: &'static str, text: &'static str, evidence: ModelEvidence| {
        Ok((Localized::fixed(id, text), evidence))
    };
    let provider_param = || params([("provider", provider_l10n(provider).into())]);
    // Live query; `undocumented` marks interfaces the vendor does not document for this credential.
    let live = |endpoint: &'static str, undocumented: bool, evidence: ModelEvidence| {
        let (id, template) = if undocumented {
            (
                "models.source.undocumented",
                "{provider} {endpoint} with the account credential (interface not documented for this credential, may change without notice); does not prove price or current availability either.",
            )
        } else {
            (
                "models.source.live",
                "{provider} {endpoint} with the account credential (does not prove inference permission); does not prove price or current availability either.",
            )
        };
        let mut values = provider_param();
        values.insert("endpoint".into(), ParamValue::from(endpoint));
        Ok((Localized::with_params(id, values, template), evidence))
    };
    // `models.unsupported` names the reason as a nested
    // `models.unsupported.reason.<slug>` text.
    let reason = |slug: &'static str, text: &'static str| {
        let mut values = provider_param();
        values.insert(
            "reason".into(),
            Localized::fixed(format!("models.unsupported.reason.{slug}"), text).into(),
        );
        Err(Localized::with_params(
            "models.unsupported",
            values,
            "{provider}: {reason} No query is made.",
        ))
    };
    let no_evidence = || {
        Err(Localized::with_params(
            "models.unsupported_no_evidence",
            provider_param(),
            "{provider}: the interface that lists models is neither documented nor verified against a real account for this credential. No query is made.",
        ))
    };
    match provider {
        Provider::Groq => source(
            "models.source.groq",
            "Groq GET /openai/v1/models with the account key; does not prove inference permission, price or current availability.",
            documented("https://console.groq.com/docs/api-reference#models-list"),
        ),
        Provider::AwsBedrock => source(
            "models.source.aws_bedrock",
            "AWS CLI bedrock list-foundation-models on the account profile, in params.region or the profile default region; does not use Cost Explorer nor prove model access.",
            documented("https://docs.aws.amazon.com/cli/latest/reference/bedrock/list-foundation-models.html"),
        ),
        Provider::Antigravity => source(
            "models.source.antigravity",
            "Antigravity fetchAvailableModels with the account session (paid plans); does not prove quota, price or current availability.",
            verified_live(),
        ),
        Provider::Poolside => source(
            "models.source.poolside",
            "Poolside GET /v1/models with the account key; does not prove inference permission, price or current availability.",
            documented("https://docs.poolside.ai/"),
        ),
        Provider::Claude => source(
            "models.source.claude",
            "Claude GET /v1/models with the subscription OAuth token (interface undocumented for this token); does not prove quota, price or current availability.",
            verified_live(),
        ),
        Provider::ChatGpt => source(
            "models.source.chat_gpt",
            "Codex GET /codex/models with the ChatGPT session token (internal, undocumented Codex CLI interface); does not prove quota, price or current availability.",
            verified_live(),
        ),
        Provider::GeminiAi if account.oauth.is_none() && !product("cli") => source(
            "models.source.gemini_ai",
            "Gemini API models.list, paginated, with the account AI Studio key; does not prove quota, price or current availability.",
            documented("https://ai.google.dev/api/models#method:-models.list"),
        ),
        Provider::GeminiAi => no_evidence(),
        Provider::Kimi if account.oauth.is_none() && !product("coding") => source(
            "models.source.kimi",
            "Kimi GET /v1/models (api.moonshot.ai) with the account key; does not prove inference permission, price or current availability.",
            documented("https://platform.kimi.ai/docs"),
        ),
        Provider::Kimi => live("GET /coding/v1/models", true, verified_live()),
        Provider::AlibabaModelStudio if providers::alibaba_plans::model_studio_lists(account) => {
            source(
                "models.source.alibaba_model_studio",
                "Model Studio GET /api/v1/models (Singapore), paginated, with the account key; does not prove inference permission, price or current availability.",
                documented("https://www.alibabacloud.com/help/en/model-studio/"),
            )
        }
        Provider::AlibabaModelStudio => no_evidence(),
        Provider::OpenAiApi => live(
            "GET /v1/models",
            false,
            documented("https://platform.openai.com/docs/api-reference/models/list"),
        ),
        Provider::AnthropicApi => live(
            "GET /v1/models",
            false,
            documented("https://docs.anthropic.com/en/api/models-list"),
        ),
        Provider::Mistral => live(
            "GET /v1/models",
            false,
            documented("https://docs.mistral.ai/api/"),
        ),
        Provider::Perplexity => live(
            "GET /v1/models",
            false,
            documented("https://docs.perplexity.ai/"),
        ),
        Provider::NvidiaNim => live(
            "GET /v1/models",
            false,
            documented("https://docs.api.nvidia.com/nim/reference/llm-apis"),
        ),
        Provider::Grok if product("cli") || account.oauth.is_some() => no_evidence(),
        Provider::Grok => live(
            "GET /v1/models",
            false,
            documented("https://docs.x.ai/docs/api-reference"),
        ),
        Provider::Cline => live(
            "GET /api/v1/models",
            false,
            documented("https://docs.cline.bot/"),
        ),
        Provider::MiniMax => live(
            "GET /v1/models",
            false,
            documented("https://platform.minimax.io/docs"),
        ),
        Provider::OpenCodeGo => live("GET /zen/go/v1/models", true, verified_live()),
        Provider::OllamaCloud => live(
            "GET /v1/models",
            false,
            documented("https://docs.ollama.com/api/openai-compatibility"),
        ),
        Provider::AlibabaCodingPlan => live(
            "GET /v1/models",
            false,
            documented("https://www.alibabacloud.com/help/en/model-studio/"),
        ),
        Provider::Cursor => live("GetUsableModels", true, verified_live()),
        Provider::Kiro => live("ListAvailableModels", true, verified_live()),
        Provider::Zed => live("GET /models", true, verified_live()),
        Provider::GeminiApi => live(
            "GET /v1beta/models",
            false,
            documented("https://ai.google.dev/api/models#method:-models.list"),
        ),
        Provider::VertexAi => live(
            "GET /v1beta1/publishers/google/models",
            false,
            documented(
                "https://cloud.google.com/vertex-ai/docs/reference/rest/v1beta1/publishers.models/list",
            ),
        ),
        Provider::AzureOpenAi => live(
            "GET …/accounts/{recurso}/models",
            false,
            documented(
                "https://learn.microsoft.com/en-us/rest/api/aiservices/accountmanagement/models/list",
            ),
        ),
        Provider::CloudflareAi => live(
            "GET /ai/models/search",
            false,
            documented(
                "https://developers.cloudflare.com/api/resources/ai/subresources/models/methods/search/",
            ),
        ),
        // PRD 0028 RF10.2/RF10.3: no embedded static lists and no Copilot
        // token exchange; these providers have no evidenced listing.
        Provider::Qwen
        | Provider::Kimchi
        | Provider::Copilot
        | Provider::CodeBuddy
        | Provider::Windsurf
        | Provider::XiaomiMimo
        | Provider::Qoder
        | Provider::AlibabaTokenPlan => no_evidence(),
        Provider::OpenRouter if product("account") => reason(
            "management_key",
            "a management key is not documented for listing models; use an inference key (product key).",
        ),
        Provider::OpenRouter => live(
            "GET /api/v1/models",
            false,
            documented("https://openrouter.ai/docs/api/api-reference/models/get-models"),
        ),
        Provider::Meta => source(
            "models.source.meta",
            "Meta Model API GET /v1/models with the account key; does not prove inference permission, price or current availability.",
            documented("https://dev.meta.ai/docs/api-reference/models/list-models"),
        ),
        Provider::ElevenLabs => reason(
            "eleven_labs",
            "ElevenLabs offers no model catalog for quota queries; no request is made.",
        ),
        Provider::LiteLlm => reason(
            "litellm",
            "LiteLLM is monitored through its per-provider budgets; no model list is queried.",
        ),
        Provider::NineRouter => reason(
            "nine_router",
            "9router is monitored through the quotas of its connections; no model list is queried.",
        ),
        Provider::AlibabaCloud => reason(
            "alibaba_cloud",
            "the credential is a billing AccessKey and Alibaba Cloud has no models endpoint for it.",
        ),
        Provider::Extension => reason(
            "extension",
            "extensions report metrics, not a model catalog.",
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
            "Model query in progress in this or another window; the last catalog stays searchable.",
        )
    } else {
        Localized::fixed(
            "models.interrupted",
            "The last model query was interrupted before finishing; nothing partial was saved. Sync to try again.",
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

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Params that only filter or date the quota view (usage model filter,
/// invoice month); they never select the credential or the listed scope.
const DISPLAY_PARAMS: [&str; 2] = ["models", "billing_cycle"];

/// Provider, name, credential reference (env name or OAuth config) and the
/// params selecting credential, product, region or profile; refresh cadence
/// and [`DISPLAY_PARAMS`] are excluded. Any other edit or a rename yields a
/// new key (conservative); the account index never takes part. Shared with
/// [`crate::history`] so both stores isolate accounts the same way.
pub(crate) fn account_key(account: &Account) -> String {
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
fn private(path: &Path, dir: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() != dir {
        return Err(std::io::Error::other("unexpected file type"));
    }
    let mode = meta.permissions().mode();
    if mode & 0o077 != 0 {
        let owner = if dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & owner))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn private(_: &Path, _: bool) -> std::io::Result<()> {
    Ok(())
}

/// Create (or keep) a database file whose parent directory is restricted to
/// the owner (0700) and the file to 0600 on Unix, even when they already
/// existed; give each store a directory of its own.
pub(crate) fn create_private_file(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
        private(parent, true)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    private(path, false)
}

fn encode<T: Serialize>(value: &T) -> Result<String, ModelError> {
    serde_json::to_string(value).map_err(|_| ModelError::Storage)
}

impl ModelCache {
    /// Store at `path`, isolated by `config` (only its digest is written).
    pub fn new(path: PathBuf, config: &ConfigIdentity) -> Self {
        Self {
            path,
            config_key: config.digest(),
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

    /// Open (creating privately, see [`create_private_file`]) the store.
    fn open(&self) -> Result<Connection, ModelError> {
        create_private_file(&self.path).map_err(|_| ModelError::Storage)?;
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

    /// Number of models in the persisted catalog of account `index`, for
    /// cards; `None` when nothing was synced (never zero for "unknown").
    /// Never uses the network.
    pub fn model_count(&self, index: usize) -> Option<usize> {
        let state = self.model_state(index).ok()?;
        state.catalog().map(|catalog| catalog.models.len())
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
            Provider::Kimi
                if account.oauth.is_some()
                    || account.params.get("product").is_some_and(|v| v == "coding") =>
            {
                providers::catalog::kimi_coding_models(
                    account,
                    ctx,
                    providers::catalog::KIMI_CODING_BASE,
                )
                .await
            }
            Provider::Kimi => {
                let key = secret(account, ctx, "KIMI_API_KEY")?;
                openai_models(ctx, providers::asia::KIMI_MODELS_URL, &key).await
            }
            Provider::Claude => {
                providers::subscriptions::claude_models(
                    account,
                    ctx,
                    providers::subscriptions::CLAUDE_BASE,
                )
                .await
            }
            Provider::ChatGpt => {
                providers::subscriptions::chatgpt_models(
                    account,
                    ctx,
                    providers::subscriptions::CHATGPT_BASE,
                )
                .await
            }
            Provider::Meta => {
                let key = secret(account, ctx, "META_API_KEY")?;
                openai_models(ctx, providers::meta::META_MODELS_URL, &key).await
            }
            Provider::Poolside => {
                let key = secret(account, ctx, "POOLSIDE_API_KEY")?;
                openai_models(ctx, providers::inference::POOLSIDE_MODELS_URL, &key).await
            }
            Provider::GeminiAi
                if account.oauth.is_some()
                    || account.params.get("product").is_some_and(|v| v == "cli") =>
            {
                providers::catalog::bearer_models(
                    account,
                    ctx,
                    format!(
                        "{}/v1beta/models?pageSize=1000",
                        providers::catalog::GEMINI_BASE
                    ),
                    "GEMINI_API_KEY",
                )
                .await
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
            Provider::AlibabaModelStudio
                if providers::alibaba_plans::model_studio_lists(account) =>
            {
                let url = providers::alibaba_plans::MODEL_STUDIO_MODELS_URL;
                providers::alibaba_plans::model_studio_models(account, ctx, url).await
            }
            Provider::OpenAiApi => {
                providers::catalog::bearer_models(
                    account,
                    ctx,
                    format!("{}/v1/models", providers::catalog::OPENAI_BASE),
                    "OPENAI_ADMIN_KEY",
                )
                .await
            }
            Provider::AnthropicApi => {
                providers::catalog::anthropic_key_models(account, ctx, "https://api.anthropic.com")
                    .await
            }
            Provider::Mistral => {
                providers::catalog::bearer_models(
                    account,
                    ctx,
                    format!("{}/v1/models", providers::catalog::MISTRAL_BASE),
                    "MISTRAL_API_KEY",
                )
                .await
            }
            Provider::Perplexity => {
                providers::catalog::bearer_models(
                    account,
                    ctx,
                    format!("{}/v1/models", providers::catalog::PERPLEXITY_BASE),
                    "PERPLEXITY_API_KEY",
                )
                .await
            }
            Provider::NvidiaNim => {
                providers::catalog::bearer_models(
                    account,
                    ctx,
                    format!("{}/v1/models", providers::catalog::NVIDIA_BASE),
                    "NVIDIA_NIM_API_KEY",
                )
                .await
            }
            Provider::Grok => {
                providers::catalog::grok_models(
                    account,
                    ctx,
                    providers::catalog::GROK_CLI_BASE,
                    providers::catalog::XAI_BASE,
                )
                .await
            }
            Provider::OpenRouter => {
                providers::open_router::models(account, ctx, providers::open_router::BASE).await
            }
            Provider::Cline => {
                providers::catalog::cline_models(account, ctx, providers::catalog::CLINE_BASE).await
            }
            Provider::MiniMax => {
                let base = match account.params.get("region").map(String::as_str) {
                    Some("cn") => providers::catalog::MINIMAX_CN_BASE,
                    _ => providers::catalog::MINIMAX_BASE,
                };
                providers::catalog::minimax_models(account, ctx, base).await
            }
            Provider::OpenCodeGo => {
                providers::catalog::bearer_models(
                    account,
                    ctx,
                    format!("{}/v1/models", providers::catalog::OPENCODE_GO_BASE),
                    "OPENCODE_GO_API_KEY",
                )
                .await
            }
            Provider::OllamaCloud => {
                providers::catalog::bearer_models(
                    account,
                    ctx,
                    format!("{}/v1/models", providers::catalog::OLLAMA_BASE),
                    "OLLAMA_API_KEY",
                )
                .await
            }
            Provider::AlibabaCodingPlan => {
                providers::catalog::alibaba_coding_plan_models(
                    account,
                    ctx,
                    providers::catalog::CODING_PLAN_INTL,
                    providers::catalog::CODING_PLAN_CN,
                )
                .await
            }
            Provider::Cursor => {
                providers::catalog::cursor_models(account, ctx, providers::catalog::CURSOR_BASE)
                    .await
            }
            Provider::Kiro => providers::catalog::kiro_models(account, ctx, None).await,
            Provider::Zed => {
                providers::catalog::zed_models(account, ctx, providers::catalog::ZED_BASE).await
            }
            Provider::GeminiApi => {
                providers::catalog::gemini_api_models(account, ctx, providers::catalog::GEMINI_BASE)
                    .await
            }
            Provider::VertexAi => {
                providers::catalog::vertex_models(account, ctx, providers::catalog::VERTEX_BASE)
                    .await
            }
            Provider::AzureOpenAi => {
                providers::catalog::azure_models(account, ctx, providers::catalog::AZURE_BASE).await
            }
            Provider::CloudflareAi => {
                providers::catalog::cloudflare_models(
                    account,
                    ctx,
                    providers::catalog::CLOUDFLARE_BASE,
                )
                .await
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
provider = "alibaba_cloud"
credential_env = "ALIBABA_CLOUD_ACCESS_KEY_ID"
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

    /// Variant name only, so a failed assertion never formats the state.
    fn state_variant(state: &ModelState) -> &'static str {
        match state {
            ModelState::NotFetched => "NotFetched",
            ModelState::Ready(_) => "Ready",
            ModelState::Failed { .. } => "Failed",
            ModelState::Fetching { .. } => "Fetching",
            ModelState::Interrupted { .. } => "Interrupted",
            ModelState::Unsupported { .. } => "Unsupported",
        }
    }

    fn ids(state: &ModelState) -> Vec<&str> {
        match state {
            ModelState::Ready(catalog) => catalog.models.iter().map(|m| m.id.as_str()).collect(),
            other => panic!("expected Ready, got {}", state_variant(other)),
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
        assert_eq!(
            first.model_count(0),
            None,
            "never synced is unknown, not zero"
        );

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
        assert_eq!(
            first.model_count(0),
            Some(0),
            "empty catalog is a real zero"
        );
        assert_eq!(first.model_count(1), None, "failure without catalog");

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
        assert_eq!(
            first.model_count(0),
            Some(1),
            "count survives a failed sync"
        );

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

    /// Mirrors the matrix in docs/providers.md: every provider lists (live or built-in) except
    /// the one without any endpoint, and the product/region/login variants pick the right source.
    #[tokio::test]
    async fn support_matches_published_matrix_and_unsupported_never_queries() {
        use Provider::*;
        // PRD 0028 RF10: no embedded lists and no listing without evidence.
        let unsupported = [
            AlibabaCloud,
            CodeBuddy,
            Windsurf,
            XiaomiMimo,
            Qoder,
            AlibabaTokenPlan,
            Qwen,
            Kimchi,
            Copilot,
            ElevenLabs,
            NineRouter,
            LiteLlm,
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
            let provider = account.provider;
            let source = support(account);
            assert_eq!(
                source.is_err(),
                unsupported.contains(&provider),
                "{provider:?}"
            );
            if let Err(_reason) = &source {
                for state in [all.load_models(index).await, all.sync_models(index).await] {
                    assert!(matches!(state.unwrap(), ModelState::Unsupported { .. }));
                }
            }
        }
        assert_eq!(requests(&server).await, 0, "Unsupported uses no network");

        let variant = |provider, key: &str, value: &str| Account {
            id: None,
            provider,
            params: [(key.to_string(), value.to_string())].into(),
            ..Default::default()
        };
        let oauth =
            Some(serde_json::from_value(serde_json::json!({ "credential_id": "c" })).unwrap());
        let verified = variant(Kimi, "product", "coding");
        assert_eq!(
            &*support(&verified).unwrap().id,
            "models.source.undocumented"
        );
        // Undocumented variants never confirmed against a real account list nothing.
        for unverified in [
            variant(GeminiAi, "product", "cli"),
            variant(AlibabaModelStudio, "region", "cn"),
            variant(Grok, "product", "cli"),
            Account {
                id: None,
                provider: GeminiAi,
                oauth: oauth.clone(),
                ..Default::default()
            },
        ] {
            let reason = support(&unverified).unwrap_err();
            assert_eq!(&*reason.id, "models.unsupported_no_evidence");
            assert_eq!(evidence(&unverified), None);
        }
        assert_eq!(
            &*support(&variant(Grok, "x", "y")).unwrap().id,
            "models.source.live"
        );
    }

    /// Every account variant that lists has an evidence entry, and only
    /// those; the entry is documented, or verified live with the public
    /// contract of the quota interface.
    #[test]
    fn every_listing_has_evidence_and_no_static_list_remains() {
        for case in matrix_variants() {
            let at = format!(
                "{:?} {:?} {}",
                case.provider,
                case.params,
                case.oauth.is_some()
            );
            let listed = support(&case).is_ok();
            let found = evidence(&case);
            assert_eq!(found.is_some(), listed, "{at}");
            if let Some(found) = found {
                let url = found.evidence.url();
                assert!(url.starts_with("https://") && !url.contains('@'), "{at}");
                assert_eq!(found.verified_on.len(), 10, "{at}");
                if found.basis == EvidenceBasis::Documented {
                    assert!(
                        matches!(found.evidence, crate::access::Evidence::Documentation(_)),
                        "{at}"
                    );
                }
            }
        }
        for source in [
            include_str!("models.rs"),
            include_str!("providers/catalog.rs"),
            include_str!("providers/mod.rs"),
            include_str!("lib.rs"),
        ] {
            assert!(!source.contains(concat!("builtin", "_models::")));
        }
        assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/providers/builtin_models.rs")
            .exists());
        // RF10.3: the Copilot token exchange is never requested.
        assert!(!include_str!("providers/catalog.rs")
            .contains(concat!("copilot_internal", "/v2/token")));
    }

    /// RF10.3: Copilot lists nothing, and nothing is requested.
    #[tokio::test]
    async fn copilot_is_unsupported_and_never_queries() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let toml = "[[accounts]]\nname = \"gh\"\nprovider = \"copilot\"\ncredential_env = \"GITHUB_TOKEN\"\n";
        let monitor = monitor_with(
            &server,
            &dir.path().join("models.sqlite3"),
            &ConfigIdentity::Explicit("copilot".into()),
            toml,
        );
        for state in [monitor.load_models(0).await, monitor.sync_models(0).await] {
            let ModelState::Unsupported { reason } = state.unwrap() else {
                panic!("Copilot is unsupported");
            };
            assert_eq!(&*reason.id, "models.unsupported_no_evidence");
        }
        assert_eq!(requests(&server).await, 0);
    }

    /// Accounts for every provider and interface-switching mode.
    fn matrix_variants() -> Vec<Account> {
        let modes: [&[(&str, &str)]; 5] = [
            &[],
            &[("product", "cli")],
            &[("product", "coding")],
            &[("region", "cn")],
            &[("region", "intl")],
        ];
        let oauth = serde_json::from_value(serde_json::json!({ "credential_id": "c" })).ok();
        Provider::ALL
            .into_iter()
            .flat_map(|provider| {
                let oauth = oauth.clone();
                modes.into_iter().flat_map(move |params| {
                    [None, oauth.clone()].map(|oauth| Account {
                        provider,
                        params: params
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect(),
                        oauth,
                        ..Default::default()
                    })
                })
            })
            .collect()
    }

    const MODELS_MATRIX_START: &str = "<!-- models-matrix:start -->";
    const MODELS_MATRIX_END: &str = "<!-- models-matrix:end -->";

    fn models_matrix() -> String {
        let mut out = String::from(
            "| Identifier | Mode | Listing | Evidence | Verified on |\n|---|---|---|---|---|\n",
        );
        for provider in Provider::ALL {
            let mut rows: Vec<(Vec<String>, String)> = Vec::new();
            for account in matrix_variants()
                .into_iter()
                .filter(|a| a.provider == provider)
            {
                let label = if account.oauth.is_some() {
                    "native login".to_string()
                } else if account.params.is_empty() {
                    "default".to_string()
                } else {
                    account
                        .params
                        .iter()
                        .map(|(k, v)| format!("{k} = {v}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                if account.oauth.is_some() && !account.params.is_empty() {
                    continue;
                }
                let cells = match evidence(&account) {
                    None => "none: `Unsupported`".to_string() + " | — | —",
                    Some(found) => {
                        let basis = match found.basis {
                            EvidenceBasis::Documented => "documented",
                            EvidenceBasis::VerifiedLive => "verified live",
                        };
                        format!(
                            "{basis} | [reference]({}) | {}",
                            found.evidence.url(),
                            found.verified_on
                        )
                    }
                };
                match rows.iter_mut().find(|(_, row)| *row == cells) {
                    Some((labels, _)) => labels.push(label),
                    None => rows.push((vec![label], cells)),
                }
            }
            // The default row stands for a provider whose modes all agree.
            let all_agree = rows.len() == 1;
            for (labels, cells) in rows {
                let mode = if all_agree {
                    "any".to_string()
                } else if labels.iter().any(|l| l == "default") {
                    "default".to_string()
                } else {
                    labels.join(" or ")
                };
                out.push_str(&format!("| `{}` | {mode} | {cells} |\n", provider.slug()));
            }
        }
        out
    }

    /// RF10.1: `docs/providers.md` shows the evidence of every listing.
    #[test]
    fn providers_doc_models_matrix_matches_the_evidence() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/providers.md");
        let doc = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let (head, rest) = doc
            .split_once(MODELS_MATRIX_START)
            .expect("models matrix start");
        let (current, tail) = rest
            .split_once(MODELS_MATRIX_END)
            .expect("models matrix end");
        let expected = format!("\n{}", models_matrix());
        if std::env::var_os("CUOTAI_REGENERATE_DOCS").is_some() {
            let updated = format!("{head}{MODELS_MATRIX_START}{expected}{MODELS_MATRIX_END}{tail}");
            std::fs::write(&path, updated).unwrap();
            return;
        }
        assert_eq!(
            current, expected,
            "docs/providers.md is out of date with models::evidence. Regenerate it with \
             `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_models_matrix` and commit the result."
        );
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
