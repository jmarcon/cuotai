use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OpenAiApi,
    AnthropicApi,
    ChatGpt,
    Claude,
    GeminiApi,
    VertexAi,
    AwsBedrock,
    AzureOpenAi,
    Cursor,
    Copilot,
    Windsurf,
    MiniMax,
    Kimi,
    Qwen,
    Groq,
    Perplexity,
    Mistral,
    CloudflareAi,
    OllamaCloud,
    XiaomiMimo,
    OpenCodeGo,
    Qoder,
    GeminiAi,
    Grok,
    NvidiaNim,
    Poolside,
    Kiro,
    Cline,
    CodeBuddy,
    Zed,
    AlibabaModelStudio,
    AlibabaCodingPlan,
    AlibabaTokenPlan,
    AlibabaCloud,
    Antigravity,
    Kimchi,
    ElevenLabs,
    /// PRD 0036: a user's own 9router instance (an aggregator).
    NineRouter,
    /// A locally installed script extension. Dynamic identity lives in
    /// `Account::params["extension_id"]`; this variant is intentionally not
    /// part of [`Provider::ALL`], so a blank extension is never offered or
    /// created by the built-in account editor.
    Extension,
}

impl Provider {
    pub const ALL: [Self; 38] = [
        Self::OpenAiApi,
        Self::AnthropicApi,
        Self::ChatGpt,
        Self::Claude,
        Self::GeminiApi,
        Self::VertexAi,
        Self::AwsBedrock,
        Self::AzureOpenAi,
        Self::Cursor,
        Self::Copilot,
        Self::Windsurf,
        Self::MiniMax,
        Self::Kimi,
        Self::Qwen,
        Self::Groq,
        Self::Perplexity,
        Self::Mistral,
        Self::CloudflareAi,
        Self::OllamaCloud,
        Self::XiaomiMimo,
        Self::OpenCodeGo,
        Self::Qoder,
        Self::GeminiAi,
        Self::Grok,
        Self::NvidiaNim,
        Self::Poolside,
        Self::Kiro,
        Self::Cline,
        Self::CodeBuddy,
        Self::Zed,
        Self::AlibabaModelStudio,
        Self::AlibabaCodingPlan,
        Self::AlibabaTokenPlan,
        Self::AlibabaCloud,
        Self::Antigravity,
        Self::Kimchi,
        Self::ElevenLabs,
        Self::NineRouter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAiApi => "OpenAI API",
            Self::AnthropicApi => "Anthropic API",
            Self::ChatGpt => "ChatGPT / Codex",
            Self::Claude => "Claude",
            Self::GeminiApi => "Gemini (Cloud Monitoring)",
            Self::VertexAi => "Vertex AI",
            Self::AwsBedrock => "AWS Bedrock",
            Self::AzureOpenAi => "Azure OpenAI",
            Self::Cursor => "Cursor",
            Self::Copilot => "GitHub Copilot",
            Self::Windsurf => "Windsurf",
            Self::MiniMax => "Minimax.io",
            Self::Kimi => "Kimi",
            Self::Qwen => "Qwen",
            Self::Groq => "Groq",
            Self::Perplexity => "Perplexity",
            Self::Mistral => "Mistral",
            Self::CloudflareAi => "Cloudflare AI",
            Self::OllamaCloud => "Ollama Cloud",
            Self::XiaomiMimo => "Xiaomi MIMO",
            Self::OpenCodeGo => "Opencode Go",
            Self::Qoder => "Qoder",
            Self::GeminiAi => "Gemini AI (API key)",
            Self::Grok => "Grok (xAI)",
            Self::NvidiaNim => "NVidia NIM",
            Self::Poolside => "Poolside",
            Self::Kiro => "Kiro (AWS)",
            Self::Cline => "Cline",
            Self::CodeBuddy => "CodeBuddy",
            Self::Zed => "Zed",
            Self::AlibabaModelStudio => "Alibaba Model Studio / DashScope",
            Self::AlibabaCodingPlan => "Alibaba Coding Plan",
            Self::AlibabaTokenPlan => "Alibaba Token Plan (Qwen Cloud)",
            Self::AlibabaCloud => "Alibaba Cloud — faturamento",
            Self::Antigravity => "Antigravity",
            Self::Kimchi => "Kimchi",
            Self::ElevenLabs => "ElevenLabs",
            Self::NineRouter => "9router",
            Self::Extension => "Extension",
        }
    }

    /// True when the provider charges per outgoing query and a tight
    /// polling interval would burn money. Frontends use this to surface a
    /// cost advisory; configuration validation and effective-interval
    /// computation use the same classification. The list starts with
    /// `AwsBedrock` (Cost Explorer charges per query); every other provider
    /// is free to add itself here when evidence of billing surfaces, but no
    /// provider is metered without that proof.
    pub fn is_metered(self) -> bool {
        matches!(self, Self::AwsBedrock)
    }

    /// PRD 0036 RF1.1: true for providers that hold credentials for several
    /// upstream providers and relay their readings (today only 9router).
    pub fn is_aggregator(self) -> bool {
        matches!(self, Self::NineRouter)
    }

    /// PRD 0036 RF4.1: an aggregator account cannot be polled faster than
    /// this: every refresh makes the instance query its upstreams.
    pub const AGGREGATOR_MIN_REFRESH_SECONDS: u64 = 120;

    /// Default polling interval for a metered account without an override.
    pub const METERED_DEFAULT_REFRESH_SECONDS: u64 = 3600;

    /// Minimum effective polling interval for metered providers.
    pub const METERED_MIN_REFRESH_SECONDS: u64 = 900;

    /// Smallest metered override accepted from existing configuration.
    /// Values below the effective floor are raised at runtime; values below
    /// this compatibility boundary remain invalid.
    pub const METERED_MIN_CONFIGURED_REFRESH_SECONDS: u64 = 300;

    /// Stable snake_case slug (the serde representation of the variant).
    /// Used to build RF8 text ids such as `field.kiro.auth_method.label`
    /// and `credential_help.aws_bedrock`. Kept in lockstep with the
    /// `rename_all = "snake_case"` derive by a test in this module.
    pub fn slug(self) -> &'static str {
        match self {
            Self::OpenAiApi => "open_ai_api",
            Self::AnthropicApi => "anthropic_api",
            Self::ChatGpt => "chat_gpt",
            Self::Claude => "claude",
            Self::GeminiApi => "gemini_api",
            Self::VertexAi => "vertex_ai",
            Self::AwsBedrock => "aws_bedrock",
            Self::AzureOpenAi => "azure_open_ai",
            Self::Cursor => "cursor",
            Self::Copilot => "copilot",
            Self::Windsurf => "windsurf",
            Self::MiniMax => "mini_max",
            Self::Kimi => "kimi",
            Self::Qwen => "qwen",
            Self::Groq => "groq",
            Self::Perplexity => "perplexity",
            Self::Mistral => "mistral",
            Self::CloudflareAi => "cloudflare_ai",
            Self::OllamaCloud => "ollama_cloud",
            Self::XiaomiMimo => "xiaomi_mimo",
            Self::OpenCodeGo => "open_code_go",
            Self::Qoder => "qoder",
            Self::GeminiAi => "gemini_ai",
            Self::Grok => "grok",
            Self::NvidiaNim => "nvidia_nim",
            Self::Poolside => "poolside",
            Self::Kiro => "kiro",
            Self::Cline => "cline",
            Self::CodeBuddy => "code_buddy",
            Self::Zed => "zed",
            Self::AlibabaModelStudio => "alibaba_model_studio",
            Self::AlibabaCodingPlan => "alibaba_coding_plan",
            Self::AlibabaTokenPlan => "alibaba_token_plan",
            Self::AlibabaCloud => "alibaba_cloud",
            Self::Antigravity => "antigravity",
            Self::Kimchi => "kimchi",
            Self::ElevenLabs => "eleven_labs",
            Self::NineRouter => "nine_router",
            Self::Extension => "extension",
        }
    }

    /// Every variant, in serialization order. It includes
    /// [`Provider::Extension`], which is deliberately kept out of
    /// [`Provider::ALL`]: consumers that parse or route a provider name
    /// (hooks rules, CLI arguments, saved payloads) must accept it, while
    /// the account creation catalog keeps offering only real providers.
    pub const VARIANTS: [Self; 39] = [
        Self::OpenAiApi,
        Self::AnthropicApi,
        Self::ChatGpt,
        Self::Claude,
        Self::GeminiApi,
        Self::VertexAi,
        Self::AwsBedrock,
        Self::AzureOpenAi,
        Self::Cursor,
        Self::Copilot,
        Self::Windsurf,
        Self::MiniMax,
        Self::Kimi,
        Self::Qwen,
        Self::Groq,
        Self::Perplexity,
        Self::Mistral,
        Self::CloudflareAi,
        Self::OllamaCloud,
        Self::XiaomiMimo,
        Self::OpenCodeGo,
        Self::Qoder,
        Self::GeminiAi,
        Self::Grok,
        Self::NvidiaNim,
        Self::Poolside,
        Self::Kiro,
        Self::Cline,
        Self::CodeBuddy,
        Self::Zed,
        Self::AlibabaModelStudio,
        Self::AlibabaCodingPlan,
        Self::AlibabaTokenPlan,
        Self::AlibabaCloud,
        Self::Antigravity,
        Self::Kimchi,
        Self::ElevenLabs,
        Self::NineRouter,
        Self::Extension,
    ];

    /// The variant whose [`Self::slug`] is `slug`, trimmed.
    pub fn from_slug(slug: &str) -> Option<Self> {
        let slug = slug.trim();
        Self::VARIANTS
            .into_iter()
            .find(|provider| provider.slug() == slug)
    }
}

/// Configuration holds credential references, never credential values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    /// PRD 0029 RF1: stable identity, first key of the table. `None` only
    /// for an account built in memory without one; [`Account::id`] then
    /// derives it from the legacy key. `ConfigFile` fills it when loading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<crate::account_id::AccountId>,
    pub name: String,
    pub provider: Provider,
    #[serde(default)]
    pub credential_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<crate::oauth::OAuthConfig>,
    /// PRD 0016 RF1: vault reference of the user-supplied web sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<crate::user_session::SessionConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    /// Per-account refresh override. Non-metered accounts with `None` use
    /// `Config::refresh_seconds`; metered accounts with `None` use their
    /// provider-specific default. Any explicit value must be `>= 30`
    /// (enforced by `Config::validate`); metered accounts apply their
    /// additional configured and effective boundaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_seconds: Option<u64>,
}

/// Account fields accepted from configuration; `deny_unknown_fields` keeps
/// rejecting typos.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountConfig {
    #[serde(default)]
    id: Option<crate::account_id::AccountId>,
    #[serde(default)]
    name: String,
    provider: Provider,
    #[serde(default)]
    credential_env: Option<String>,
    #[serde(default)]
    oauth: Option<crate::oauth::OAuthConfig>,
    #[serde(default)]
    session: Option<crate::user_session::SessionConfig>,
    #[serde(default)]
    params: BTreeMap<String, String>,
    #[serde(default)]
    refresh_seconds: Option<u64>,
}

impl<'de> Deserialize<'de> for Account {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let account = AccountConfig::deserialize(deserializer)?;
        Ok(Self {
            id: account.id,
            name: account.name,
            provider: account.provider,
            credential_env: account.credential_env,
            oauth: account.oauth,
            session: account.session,
            params: account.params,
            refresh_seconds: account.refresh_seconds,
        })
    }
}

impl Account {
    /// Minimum supported refresh interval, shared with the wizard. Mirrors
    /// `Config::refresh_seconds` bounds so the wizard and the validator can
    /// agree without duplicating magic numbers.
    pub const MIN_REFRESH_SECONDS: u64 = 30;

    /// Effective polling interval for this account. Non-metered accounts use
    /// their override or the supplied global fallback. Metered accounts use
    /// their one-hour default when no override exists, and accepted overrides
    /// below the effective floor are raised to that floor.
    pub fn effective_refresh_seconds(&self, global_refresh_seconds: u64) -> u64 {
        if self.provider.is_aggregator() {
            return self
                .refresh_seconds
                .unwrap_or(global_refresh_seconds)
                .max(Provider::AGGREGATOR_MIN_REFRESH_SECONDS);
        }
        if !self.provider.is_metered() {
            return self.refresh_seconds.unwrap_or(global_refresh_seconds);
        }
        self.refresh_seconds
            .map(|seconds| seconds.max(Provider::METERED_MIN_REFRESH_SECONDS))
            .unwrap_or(Provider::METERED_DEFAULT_REFRESH_SECONDS)
    }

    /// User-visible explanation when a compatible metered override is raised
    /// to the effective floor. Invalid values below 300 seconds never reach a
    /// report because configuration validation rejects them.
    pub fn metered_interval_note(&self) -> Option<crate::text::Localized> {
        let configured = self.refresh_seconds?;
        if !self.provider.is_metered()
            || !(Provider::METERED_MIN_CONFIGURED_REFRESH_SECONDS
                ..Provider::METERED_MIN_REFRESH_SECONDS)
                .contains(&configured)
        {
            return None;
        }
        Some(crate::text::Localized::with_params(
            "note.metered.interval_raised",
            crate::text::params([
                ("configured", crate::text::ParamValue::from(configured)),
                (
                    "effective",
                    crate::text::ParamValue::from(Provider::METERED_MIN_REFRESH_SECONDS),
                ),
            ]),
            format!(
                "O intervalo configurado de {configured} s foi elevado para {} s porque o provedor é cobrado por consulta.",
                Provider::METERED_MIN_REFRESH_SECONDS
            ),
        ))
    }

    /// The dynamic extension id for an extension-backed account.
    pub fn extension_id(&self) -> Option<&str> {
        (self.provider == Provider::Extension)
            .then(|| self.params.get("extension_id").map(String::as_str))
            .flatten()
    }

    /// Stable account key. Built-ins use `provider:name`; extension accounts
    /// include their dynamic id (`extension:<id>:<name>`) so two discovered
    /// extensions can never collide.
    pub fn key(&self) -> String {
        match self.extension_id() {
            Some(id) => format!("extension:{id}:{}", self.name),
            None => account_key(self.provider, &self.name),
        }
    }
}

impl Account {
    /// PRD 0029 RF1: the account's id, or the one derived from its legacy
    /// key when it has none (an account built in memory).
    pub fn id(&self) -> crate::account_id::AccountId {
        self.id
            .clone()
            .unwrap_or_else(|| crate::account_id::AccountId::derive(&self.key(), None))
    }

    /// PRD 0029 RF2.1: the key every piece of per-account state uses (bells,
    /// mutes, ledger and alert marks, history series): the id, which
    /// survives renames and parameter edits.
    pub fn state_key(&self) -> String {
        self.id().as_str().to_owned()
    }
}

fn account_key(provider: Provider, name: &str) -> String {
    format!("{}:{name}", provider.slug())
}

impl Report {
    /// PRD 0033 RF2.1: a copy for showing as a preserved or restored
    /// reading at `now`, with every expired metric collapsed.
    pub fn collapse_expired(&self, now: DateTime<Utc>) -> Self {
        let mut copy = self.clone();
        for metric in &mut copy.metrics {
            if metric.is_expired(now) {
                *metric = metric.collapsed();
            }
        }
        copy
    }

    /// [`Account::key`] of the account this report belongs to.
    pub fn account_key(&self) -> String {
        if self.provider == Provider::Extension {
            if let Some(id) = self.source.strip_prefix("extension:") {
                return format!("extension:{id}:{}", self.account);
            }
        }
        account_key(self.provider, &self.account)
    }

    /// [`Account::state_key`] of the account this report belongs to: its
    /// `account_id`, or the id derived from the legacy key for a report
    /// built without one (as [`Account::id`] does for such an account).
    pub fn state_key(&self) -> String {
        match &self.account_id {
            Some(id) => id.as_str().to_owned(),
            None => crate::account_id::AccountId::derive(&self.account_key(), None)
                .as_str()
                .to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_refresh")]
    pub refresh_seconds: u64,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    pub accounts: Vec<Account>,
}

impl Config {
    /// Return a copy with the account at `from` moved to its final position
    /// `to`. Every account keeps its complete identity and settings; global
    /// settings are untouched. Indices are checked against the original
    /// configuration, so `to` is in `0..accounts.len()` rather than an
    /// insertion position after removal.
    pub fn move_account(
        &self,
        from: usize,
        to: usize,
    ) -> Result<Config, crate::edit_error::EditError> {
        for index in [from, to] {
            if index >= self.accounts.len() {
                return Err(crate::edit_error::index_out_of_range(
                    index,
                    self.accounts.len(),
                ));
            }
        }
        let mut updated = self.clone();
        // Rotate the affected span in place: equivalent to remove + insert,
        // and a no-op when `from == to`.
        if from < to {
            updated.accounts[from..=to].rotate_left(1);
        } else {
            updated.accounts[to..=from].rotate_right(1);
        }
        Ok(updated)
    }
}

fn default_refresh() -> u64 {
    300
}
fn default_concurrency() -> usize {
    4
}
fn default_timeout() -> u64 {
    30
}

impl Default for Account {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            provider: Provider::OpenAiApi,
            credential_env: None,
            oauth: None,
            session: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_seconds: default_refresh(),
            concurrency: default_concurrency(),
            timeout_seconds: default_timeout(),
            accounts: Provider::ALL
                .into_iter()
                .map(|provider| Account {
                    id: None,
                    name: provider.label().into(),
                    provider,
                    credential_env: None,
                    oauth: None,
                    session: None,
                    params: BTreeMap::new(),
                    refresh_seconds: None,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ready,
    NeedsConfiguration,
    Unsupported,
    AuthenticationError,
    RateLimited,
    Error,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Atualizado",
            Self::NeedsConfiguration => "Configurar",
            Self::Unsupported => "Indisponível",
            Self::AuthenticationError => "Autenticação",
            Self::RateLimited => "Rate limit",
            Self::Error => "Erro",
        }
    }
}

/// Semantic category assigned by the provider adapter, never inferred from display text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    /// Consumption or allowance associated with a plan or service quota.
    Quota,
    /// A short request/token throughput window, distinct from a plan quota.
    RateLimit,
    /// A current monetary account position, such as available cash or debt.
    Balance,
    /// Provider-issued credit balance, grant, or consumption outside a plan quota.
    Credits,
    /// Monetary cost or accrued spend in a billing period.
    Spend,
    /// Resource consumption or a count without plan-quota semantics.
    Usage,
}

/// Recurrence of the provider window. `Other` covers provider-specific or
/// unspecified cadences; `None` on [`Metric::window`] means no window applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricWindow {
    #[serde(rename = "window_5h")]
    Window5h,
    Daily,
    Weekly,
    Monthly,
    Other,
}

/// A missing value means unknown, not zero. Currency units use ISO codes.
/// Values are for monitoring, not financial settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub label: String,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    pub unit: String,
    pub kind: MetricKind,
    pub window: Option<MetricWindow>,
    pub period: Option<String>,
    pub resets_at: Option<DateTime<Utc>>,
    /// Whether the amount is part of the plan or billed on top of it.
    #[serde(default, skip_serializing_if = "Billing::is_included")]
    pub billing: Billing,
    /// Set when the metric limits one model (or another sub-scope) rather
    /// than the whole account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
}

/// Whether a metric is covered by the plan or is paid extra usage (overage).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Billing {
    /// Part of the plan's included allowance (the default).
    #[default]
    Included,
    /// Billed on top of the plan (extra usage, on-demand, overage).
    Overage,
}

impl Billing {
    pub fn is_included(&self) -> bool {
        *self == Billing::Included
    }
}

/// The sub-scope a metric is limited to, such as one model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub kind: ScopeKind,
    pub id: String,
    pub label: String,
}

impl Scope {
    pub fn model(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            kind: ScopeKind::Model,
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Model,
}

impl Metric {
    /// PRD 0033 RF2.1: the metric's window renewed before `now`, so a
    /// preserved or restored value of it no longer describes anything.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.resets_at.is_some_and(|at| at < now)
    }

    /// PRD 0033 RF2.1: the metric without value, limit or remaining, kept
    /// only as the marker that its window renewed.
    pub fn collapsed(&self) -> Self {
        Self {
            used: None,
            limit: None,
            remaining: None,
            ..self.clone()
        }
    }

    /// The metric is [`Self::collapsed`] by now: its window renewed and it
    /// carries no value (PRD 0033 RF2.1).
    pub fn is_collapsed(&self, now: DateTime<Utc>) -> bool {
        self.is_expired(now)
            && self.used.is_none()
            && self.limit.is_none()
            && self.remaining.is_none()
    }

    /// The `metric.renewed_waiting` text of an expired metric, with the
    /// renewal time in UTC (the machine-readable export).
    pub fn renewed_waiting(&self) -> Option<crate::text::Localized> {
        self.renewed_waiting_at(|at| at.format("%H:%M UTC").to_string())
    }

    /// [`Self::renewed_waiting`] with the renewal time formatted by the
    /// front end, normally in the viewer's local time.
    pub fn renewed_waiting_at(
        &self,
        time: impl FnOnce(DateTime<Utc>) -> String,
    ) -> Option<crate::text::Localized> {
        let time = time(self.resets_at?);
        Some(crate::text::Localized::with_params(
            "metric.renewed_waiting",
            crate::text::params([("time", crate::text::ParamValue::from(time.clone()))]),
            format!("Renovada às {time}; aguardando leitura."),
        ))
    }

    pub fn usage(
        label: impl Into<String>,
        used: f64,
        unit: impl Into<String>,
        kind: MetricKind,
        window: Option<MetricWindow>,
    ) -> Self {
        Self {
            label: label.into(),
            used: Some(used),
            limit: None,
            remaining: None,
            unit: unit.into(),
            kind,
            window,
            period: None,
            resets_at: None,
            billing: Billing::Included,
            scope: None,
        }
    }

    /// Marks the metric as paid extra usage.
    pub fn overage(mut self) -> Self {
        self.billing = Billing::Overage;
        self
    }

    /// Limits the metric to one model (or another sub-scope).
    pub fn scoped(mut self, scope: Scope) -> Self {
        self.scope = Some(scope);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub account: String,
    /// PRD 0029: [`Account::state_key`] of the account. Additive and skipped
    /// when absent, so fixtures of older releases stay byte-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<crate::account_id::AccountId>,
    pub provider: Provider,
    pub checked_at: DateTime<Utc>,
    pub status: Status,
    pub metrics: Vec<Metric>,
    pub notes: Vec<String>,
    /// RF8 structured sidecar for [`Report::notes`]: same length and order,
    /// each entry carrying the stable id and parameters of the note at the
    /// same index. Additive and skipped when empty so the legacy
    /// Snapshot/CLI JSON stays byte-compatible; frontends resolve the id in
    /// their catalog and render the Portuguese `fallback` when it is
    /// missing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes_l10n: Vec<crate::text::Localized>,
    pub source: String,
    /// PRD 0016 RF3.2: when the vault session the query used carries a
    /// known expiry (only the date is read, never the value). Additive and
    /// skipped when unknown, so the legacy JSON stays byte-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_expires_at: Option<DateTime<Utc>>,
    /// PRD 0028 RF1.3: what drifted in a response that no longer matched the
    /// interface (paths and types only). Not serialized in the legacy JSON.
    #[serde(skip)]
    pub diagnostic: Option<crate::diagnostic::Diagnostic>,
    /// PRD 0036 RF1.2: the readings of the sub-accounts an aggregator
    /// account relays (one per upstream connection it holds). Additive and
    /// skipped when empty, so every other report serializes as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_reports: Vec<SubReport>,
    /// PRD 0033 RF3.1: when the data itself is from, only for adapters
    /// whose provider states it (the end of the last complete cost
    /// bucket, say); `None` for every other adapter. Additive and skipped
    /// when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<DateTime<Utc>>,
}

/// PRD 0036 RF1.2: one upstream connection of an aggregator instance. It
/// never carries a credential, a connection UUID, an email or a response
/// body: `id` is a hash and the label comes from the aggregator's masked
/// display name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubReport {
    /// Stable, opaque id of the connection (a truncated hash).
    pub id: String,
    /// The aggregator's own provider slug, verbatim (for example
    /// `"gemini-cli"`).
    pub upstream: String,
    /// The mapped Cuotai provider; `None` for an upstream Cuotai does not
    /// know.
    #[serde(default)]
    pub provider: Option<Provider>,
    pub label: String,
    pub status: Status,
    #[serde(default)]
    pub metrics: Vec<Metric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes_l10n: Vec<crate::text::Localized>,
    pub checked_at: DateTime<Utc>,
    /// When the upstream measured the reading, if the aggregator says so;
    /// 9router exposes no age, so it is `None` there.
    #[serde(default)]
    pub as_of: Option<DateTime<Utc>>,
    /// The aggregator's own on/off flag for the connection.
    #[serde(default)]
    pub enabled_upstream: Option<bool>,
    /// Account key of the direct account this connection duplicates.
    #[serde(default)]
    pub duplicate_of: Option<String>,
    /// Plan name the aggregator reports, when it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Public file that defines the upstream reading in the aggregator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub reports: Vec<Report>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_serde_snake_case() {
        // RF8 ids are built from `slug`; if the serde rename ever
        // diverges the frontend catalogs would silently miss.
        for provider in Provider::ALL {
            let serialized = serde_json::to_value(provider).unwrap();
            assert_eq!(
                serialized.as_str().unwrap(),
                provider.slug(),
                "{provider:?}"
            );
        }
    }

    #[test]
    fn metric_kind_and_window_use_stable_serde_names() {
        for (kind, expected) in [
            (MetricKind::Quota, "quota"),
            (MetricKind::RateLimit, "rate_limit"),
            (MetricKind::Balance, "balance"),
            (MetricKind::Credits, "credits"),
            (MetricKind::Spend, "spend"),
            (MetricKind::Usage, "usage"),
        ] {
            let metric = Metric::usage("metric", 1.0, "req", kind, None);
            let json = serde_json::to_value(&metric).unwrap();
            assert_eq!(json["kind"], expected);
            assert!(json["window"].is_null());
            let back: Metric = serde_json::from_value(json).unwrap();
            assert_eq!(back.kind, kind);
            assert_eq!(back.window, None);
        }

        for (window, expected) in [
            (MetricWindow::Window5h, "window_5h"),
            (MetricWindow::Daily, "daily"),
            (MetricWindow::Weekly, "weekly"),
            (MetricWindow::Monthly, "monthly"),
            (MetricWindow::Other, "other"),
        ] {
            let metric = Metric::usage("metric", 1.0, "req", MetricKind::Quota, Some(window));
            let json = serde_json::to_value(&metric).unwrap();
            assert_eq!(json["window"], expected);
            let back: Metric = serde_json::from_value(json).unwrap();
            assert_eq!(back.window, Some(window));
        }
    }

    #[test]
    fn report_notes_l10n_is_additive_and_skipped_when_empty() {
        let report = Report {
            account_id: None,
            diagnostic: None,
            account: "a".into(),
            provider: Provider::Kimi,
            checked_at: Utc::now(),
            status: Status::Ready,
            metrics: Vec::new(),
            notes: vec!["Nota.".into()],
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert!(json.get("notes_l10n").is_none(), "empty sidecar must skip");
        assert_eq!(json["notes"], serde_json::json!(["Nota."]));
        let back: Report = serde_json::from_value(json).unwrap();
        assert!(back.notes_l10n.is_empty());
    }

    #[test]
    fn move_account_matches_remove_then_insert_for_every_pair() {
        let config = Config {
            accounts: ["a", "b", "c", "d"]
                .iter()
                .map(|name| Account {
                    id: None,
                    name: (*name).to_string(),
                    ..Account::default()
                })
                .collect(),
            ..Config::default()
        };
        let n = config.accounts.len();
        for from in 0..n {
            for to in 0..n {
                let mut expected: Vec<String> =
                    config.accounts.iter().map(|a| a.name.clone()).collect();
                let moved = expected.remove(from);
                expected.insert(to, moved);
                let got: Vec<String> = config
                    .move_account(from, to)
                    .unwrap()
                    .accounts
                    .iter()
                    .map(|a| a.name.clone())
                    .collect();
                assert_eq!(got, expected, "from {from} to {to}");
            }
        }
        assert!(config.move_account(n, 0).is_err());
        assert!(config.move_account(0, n).is_err());
    }
}
