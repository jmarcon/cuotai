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
}

impl Provider {
    pub const ALL: [Self; 36] = [
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
        }
    }

    /// True when the provider charges per outgoing query and a tight
    /// polling interval would burn money. Frontends use this to surface a
    /// cost advisory and `Config::validate` uses it to clamp the
    /// effective interval (per-account override and global fallback) to
    /// at least [`Self::METERED_MIN_REFRESH_SECONDS`]. The list starts
    /// with `AwsBedrock` (Cost Explorer charges per query); every other
    /// provider is free to add itself here when evidence of billing
    /// surfaces, but no provider is metered without that proof.
    pub fn is_metered(self) -> bool {
        matches!(self, Self::AwsBedrock)
    }

    /// Minimum effective polling interval for metered providers. Same
    /// value applied to per-account overrides and the global fallback so
    /// a metered account is never polled more often than this regardless
    /// of how the user wires the configuration.
    pub const METERED_MIN_REFRESH_SECONDS: u64 = 300;

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
        }
    }
}

/// Configuration holds credential references, never credential values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
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
    /// Per-account override for the global `Config::refresh_seconds`. When
    /// `None`, the global interval applies; otherwise the value must be
    /// `>= 30` (enforced by `Config::validate`) so we never overwhelm a
    /// provider with frequent polling. Wizard edits this field; the global
    /// interval stays the fallback for accounts without an override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_seconds: Option<u64>,
}

impl Account {
    /// Minimum supported refresh interval, shared with the wizard. Mirrors
    /// `Config::refresh_seconds` bounds so the wizard and the validator can
    /// agree without duplicating magic numbers.
    pub const MIN_REFRESH_SECONDS: u64 = 30;

    /// Effective polling interval for this account: own override if set,
    /// otherwise the supplied `global_refresh_seconds` fallback.
    pub fn effective_refresh_seconds(&self, global_refresh_seconds: u64) -> u64 {
        self.refresh_seconds.unwrap_or(global_refresh_seconds)
    }

    /// Stable account key: provider slug + name (`claude:Work`). Survives
    /// reordering and credential edits; a rename yields a new key. Names
    /// are unique per configuration and slugs carry no `:`, so it is
    /// unambiguous.
    pub fn key(&self) -> String {
        account_key(self.provider, &self.name)
    }
}

fn account_key(provider: Provider, name: &str) -> String {
    format!("{}:{name}", provider.slug())
}

impl Report {
    /// [`Account::key`] of the account this report belongs to.
    pub fn account_key(&self) -> String {
        account_key(self.provider, &self.account)
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
        let account = updated.accounts.remove(from);
        updated.accounts.insert(to, account);
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
}

impl Metric {
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub account: String,
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
            account: "a".into(),
            provider: Provider::Kimi,
            checked_at: Utc::now(),
            status: Status::Ready,
            metrics: Vec::new(),
            notes: vec!["Nota.".into()],
            notes_l10n: Vec::new(),
            source: String::new(),
        };
        let json = serde_json::to_value(&report).unwrap();
        assert!(json.get("notes_l10n").is_none(), "empty sidecar must skip");
        assert_eq!(json["notes"], serde_json::json!(["Nota."]));
        let back: Report = serde_json::from_value(json).unwrap();
        assert!(back.notes_l10n.is_empty());
    }
}
