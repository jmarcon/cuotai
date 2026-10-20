//! Provider-specific account editing policy shared by every frontend.
//!
//! The TUI wizard, the desktop app and any future frontend all funnel account
//! create/update through these rules: name uniqueness, OAuth identity
//! preservation, managed-key stripping, per-provider validation and per-account
//! refresh intervals. Keeping them in core guarantees the wizard never diverges
//! from the file format or the OAuth store semantics.
//!
//! `Config::upsert_account` and `Config::without_account` are the two
//! mutators frontends need; `AccountDraft` and `build_account` describe the
//! shape of an account edit without dragging UI types across the boundary.
//!
//! The provider-specific field schema (which params to ask for, their labels,
//! help text, requiredness and options) and visibility rules live here as
//! [`FieldDescriptor`]s and [`visible_field_indices`]. Frontends map each
//! descriptor to a UI widget of their choosing; `build_account` then refuses
//! to honour tampered `required` flags, out-of-range choice selections or
//! options arrays that diverge from the descriptor (silent key drops on
//! hidden fields included).

use crate::oauth::{requires_client_registration, OAuthConfig};
use crate::{Account, Config, Provider};
use std::collections::BTreeMap;

/// Minimum interval accepted for an individual account override. Matches the
/// global minimum so a per-account override can never be more aggressive than
/// the global cap.
pub const MIN_REFRESH_SECONDS: u64 = 30;
/// Maximum interval accepted for an individual account override. One day is
/// a sane upper bound for low-priority accounts without forcing a magic
/// number on every caller.
pub const MAX_REFRESH_SECONDS: u64 = 86_400;

/// Conventional env var name for each provider (pre-filled when adding).
pub fn default_env(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAiApi => "OPENAI_ADMIN_KEY",
        Provider::AnthropicApi => "ANTHROPIC_ADMIN_KEY",
        Provider::ChatGpt => "CHATGPT_ACCESS_TOKEN",
        Provider::Claude => "CLAUDE_OAUTH_TOKEN",
        Provider::GeminiApi | Provider::VertexAi => "GOOGLE_CLOUD_ACCESS_TOKEN",
        Provider::AwsBedrock => "",
        Provider::AzureOpenAi => "AZURE_ACCESS_TOKEN",
        Provider::Cursor => "CURSOR_ADMIN_API_KEY",
        Provider::Copilot => "GITHUB_TOKEN",
        Provider::Windsurf => "WINDSURF_SERVICE_KEY",
        Provider::MiniMax => "MINIMAX_IO_API_KEY",
        Provider::Kimi => "KIMI_API_KEY",
        Provider::Qwen => "QWEN_API_KEY",
        Provider::Groq => "GROQ_API_KEY",
        Provider::Perplexity => "PERPLEXITY_API_KEY",
        Provider::Mistral => "MISTRAL_API_KEY",
        Provider::CloudflareAi => "CLOUDFLARE_AI_API_KEY",
        Provider::OllamaCloud => "OLLAMA_CLOUD_API_KEY",
        Provider::XiaomiMimo => "XIAOMI_MIMO_API_KEY",
        Provider::OpenCodeGo => "OPENCODE_GO_API_KEY",
        Provider::Qoder => "QODER_API_KEY",
        Provider::GeminiAi => "GEMINI_API_KEY",
        Provider::Grok => "X_GROK_API_KEY",
        Provider::NvidiaNim => "NVIDIA_NIM_API_KEY",
        Provider::Poolside => "POOLSIDE_API_KEY",
        Provider::Cline => "CLINE_API_KEY",
        Provider::Kiro => "KIRO_ACCESS_TOKEN",
        Provider::CodeBuddy => "CODEBUDDY_ACCESS_TOKEN",
        Provider::Zed => "ZED_ACCESS_TOKEN",
        Provider::AlibabaModelStudio => "DASHSCOPE_API_KEY",
        Provider::AlibabaCodingPlan => "BAILIAN_CODING_PLAN_API_KEY",
        Provider::AlibabaTokenPlan => "QWEN_API_KEY",
        Provider::AlibabaCloud => "ALIBABA_CLOUD_ACCESS_KEY_ID",
        Provider::Antigravity => "ANTIGRAVITY_ACCESS_TOKEN",
        Provider::Kimchi => "KIMCHI_API_KEY",
    }
}

/// Provider-specific help shown next to the credential field.
///
/// RF8: the returned [`Localized`] carries the stable id
/// `credential_help.<provider-slug>`; the Portuguese text is the fallback
/// rendered when a frontend catalog lacks the id.
pub fn credential_help(provider: Provider) -> crate::text::Localized {
    crate::text::Localized::fixed(
        format!("credential_help.{}", provider.slug()),
        credential_help_text(provider),
    )
}

/// Portuguese default wording for [`credential_help`].
fn credential_help_text(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAiApi => {
            "Chave de ADMINISTRAÇÃO da organização (Admin API), não uma chave de projeto comum."
        }
        Provider::AnthropicApi => {
            "Chave de administração (sk-ant-admin01-...) da Usage & Cost Admin API."
        }
        Provider::ChatGpt => {
            "Use o ACCESS token OAuth (não o refresh token nem o ID token). Interface interna do Codex: cobre as janelas do agente, NÃO a quota do chat web; pode mudar sem aviso."
        }
        Provider::Claude => {
            "Token OAuth de acesso. Interface não documentada oficialmente, sujeita a mudanças."
        }
        Provider::GeminiApi | Provider::VertexAi => {
            "Access token OAuth do Google Cloud (ex.: gcloud auth application-default print-access-token)."
        }
        Provider::AzureOpenAi => "Access token Azure com permissão de leitura de custos.",
        Provider::Cursor => {
            "Admin API Teams/Enterprise: variável CURSOR_ADMIN_API_KEY. Para a conta pessoal, pressione Tab e confirme importar a sessão do Cursor local. Não abre OAuth nem lê tokens sem confirmação."
        }
        Provider::Copilot => {
            "Token GitHub da CONTA, nunca o copilotToken de inferência. Cobrança (padrão): PAT clássico. Quota pessoal (opt-in): token GitHub com acesso ao endpoint interno; OAuth/PAT aceito depende do provedor."
        }
        Provider::Windsurf => {
            "Requer Enterprise com permissões Billing Read e Teams Read-Only."
        }
        Provider::AwsBedrock => "",
        Provider::Mistral => "A consulta de consumo exige Admin API Key, não chave de geração. Use MISTRAL_API_KEY com a chave administrativa ou configure admin_key_env nos parâmetros.",
        Provider::Grok => "Token de acesso xAI da equipe (Management API ou CLI). Padrão Management API: Management Key + Team ID. Opt-in cli: token de sessão do Grok CLI. Não confundir com chave da API social X.",
        Provider::Perplexity => "PERPLEXITY_API_KEY é mantida. Sonar não expõe saldo por essa chave; Computer Enterprise Analytics é uma consulta distinta, opcional nos parâmetros.",
        Provider::GeminiAi => "Padrão: chave Gemini/AI Studio — esta chave NÃO consulta quota (use Gemini CLI/CodeAssist nos parâmetros). Opt-in cli: token de sessão do Gemini CLI.",
        Provider::Cline => "Chave da Cline API ou token de conta (CLINE_API_KEY). Consulta saldo Cline; BYOK deve ser acompanhado no provedor da chave. Organização opcional nos parâmetros.",
        Provider::Kimi => "KIMI_API_KEY (Moonshot/Kimi). Padrão: saldo da conta internacional. Opt-in coding: aceita a chave de plataforma OU um access token OAuth (informe o nome em token_env); a chave tem precedência.",
        Provider::Qoder => "QODER_API_KEY aceita PAT (pt-..., trocado por job token) ou um token Bearer/job já existente (jt-.../device). Escolha o tipo de credencial nos parâmetros.",
        Provider::OpenCodeGo => "OPENCODE_GO_API_KEY da conta; a própria chave do usuário autentica a consulta interna.",
        Provider::Groq => "GROQ_API_KEY da conta; a própria chave do usuário autentica a consulta interna.",
        Provider::OllamaCloud => "OLLAMA_CLOUD_API_KEY da conta (Bearer); uso em GET ollama.com/api/usage e plano em POST /api/me. Não é necessário importar cookies.",
        Provider::XiaomiMimo => "Saldo/Token Plan: configure platform_session_env com a sessão web platform.xiaomimimo.com. O código nativo entrega chave de inferência, não cookie. session_env é separado: quota semanal do MiMo Desktop.",
        Provider::Kiro => "Access token Kiro/CodeWhisperer (não refresh token). Com variável: auth_method google, github, import, api_key ou external_idp. Builder ID e Identity Center exigem login nativo para renovação.",
        Provider::CodeBuddy => "Access token de sessão do CodeBuddy (cn: copilot.tencent.com; intl: www.codebuddy.ai).",
        Provider::Zed => "ZED_ACCESS_TOKEN (cloud access token do fluxo de login nativo zed.dev). Informe também o user_id numérico — é enviado como `Authorization: <user_id> <token>`.",
        Provider::AlibabaModelStudio => "API key Alibaba Model Studio/DashScope. Pode apontar para QWEN_API_KEY. Selecione a região correspondente à conta; não é login OAuth Qwen Code.",
        Provider::AlibabaCodingPlan => "Chave da assinatura Alibaba Coding Plan (normalmente sk-sp-). Selecione China ou internacional; é diferente de uma API key padrão DashScope.",
        Provider::AlibabaTokenPlan => "A quota Token Plan consulta, por padrão, o perfil dedicado do `bl` (`params.cli_profile`, configurado pelo F2). `params.session_env` (Cookie de home.qwencloud.com) é override opcional. QWEN_API_KEY autentica apenas inferência e não é enviada à consulta.",
        Provider::AlibabaCloud => "Informe o NOME da variável com AccessKey ID da conta RAM de faturamento. Em Parâmetros informe a variável AccessKey Secret; os valores nunca entram no TOML.",
        Provider::Antigravity => "Sessão Google do Antigravity, distinta da API key Gemini. Pressione Tab para login nativo; o projeto Code Assist e as janelas de quota serão consultados automaticamente.",
        Provider::Kimchi => "KIMCHI_API_KEY da conta Kimchi; consulta créditos e orçamentos em llm.kimchi.dev, sem geração.",
        _ => "API key/PAT: informe apenas o nome da variável exportada. A chave de geração pode não permitir consultar consumo; confira a limitação do relatório.",
    }
}

/// Instructions to register your own OAuth app on the provider's console.
/// Only meaningful when `requires_client_registration(provider)` is true.
///
/// RF8 id: `oauth.registration_help.<provider-slug>`.
pub fn oauth_registration_help(provider: Provider) -> crate::text::Localized {
    crate::text::Localized::fixed(
        format!("oauth.registration_help.{}", provider.slug()),
        oauth_registration_help_text(provider),
    )
}

/// Portuguese default wording for [`oauth_registration_help`].
fn oauth_registration_help_text(provider: Provider) -> &'static str {
    match provider {
        Provider::GeminiApi | Provider::VertexAi => {
            "Registre um client ID de app Desktop em https://console.cloud.google.com/apis/credentials (tela de consentimento OAuth + cliente Desktop; o login usa PKCE com loopback local)."
        }
        Provider::AzureOpenAi => {
            "Registre um app público em https://portal.azure.com > Microsoft Entra > Registros de aplicativo, habilite o fluxo de código de dispositivo e conceda permissão delegada de gerenciamento/leitura do Azure."
        }
        _ => "Registre um aplicativo OAuth próprio no provedor e informe o client ID.",
    }
}

/// Reason why a provider lacks built-in OAuth (env var mode still works).
///
/// RF8 id: `oauth.unsupported_reason.<provider-slug>`.
pub fn oauth_unsupported_reason(provider: Provider) -> Option<crate::text::Localized> {
    oauth_unsupported_reason_text(provider).map(|text| {
        crate::text::Localized::fixed(
            format!("oauth.unsupported_reason.{}", provider.slug()),
            text,
        )
    })
}

/// Portuguese default wording for [`oauth_unsupported_reason`].
fn oauth_unsupported_reason_text(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::OpenAiApi => Some("a Admin API da OpenAI aceita apenas chaves de administração."),
        Provider::AnthropicApi => {
            Some("a Usage & Cost Admin API aceita apenas chaves de administração.")
        }
        Provider::Windsurf => Some("o Windsurf exige service key Enterprise; não há fluxo OAuth."),
        _ => None,
    }
}

/// POSIX env-var name: `[A-Za-z_][A-Za-z0-9_]*`. Never accept token values
/// here — only the NAMES of exported variables.
pub fn valid_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Keys managed by the editor: stripped from the cloned `params` on every
/// save so they get re-written from the current form. Includes native-login
/// exclusive keys (region/auth_method/...) — empty in env mode until a new
/// login (identity change).
pub fn managed_keys(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Qwen => &["product"],
        Provider::OpenAiApi => &["project_ids", "models"],
        Provider::AnthropicApi => &["workspace_ids", "models"],
        Provider::ChatGpt => &["account_id"],
        Provider::GeminiApi | Provider::VertexAi => &["project_id"],
        Provider::AwsBedrock => &["profile"],
        Provider::AzureOpenAi => &["subscription_id", "resource_id"],
        Provider::Copilot => &["mode", "org", "billing", "query_mode"],
        Provider::Cursor => &["query_mode", "device_id", "user_id"],
        Provider::Mistral => &["admin_key_env"],
        Provider::Kimi => &["product", "token_env", "device_id"],
        Provider::XiaomiMimo => &[
            "session_env",
            "platform_session_env",
            "region",
            "auth_method",
        ],
        Provider::Grok => &[
            "product",
            "session_env",
            "email",
            "user_id",
            "team_id",
            "management_key_env",
        ],
        Provider::Perplexity => &["product", "analytics_key_env"],
        Provider::Cline => &["organization_id"],
        Provider::GeminiAi => &["product", "session_env", "project_id"],
        Provider::Qoder => &["region", "credential_type"],
        Provider::Kiro => &["auth_method", "profile_arn", "region"],
        Provider::CodeBuddy => &["region"],
        Provider::Zed => &["user_id", "system_id"],
        Provider::Antigravity => &["project_id"],
        Provider::AlibabaModelStudio => &["region"],
        Provider::AlibabaCodingPlan => &["region", "session_env"],
        Provider::AlibabaTokenPlan => &["session_env"],
        Provider::AlibabaCloud => &[
            "region",
            "secret_env",
            "security_token_env",
            "billing_cycle",
        ],
        _ => &[],
    }
}

/// Params controlled by native login: hidden in the params step (must never
/// diverge from the authenticated identity) and rewritten from public login
/// metadata (or preserved when editing an already-authenticated account
/// without a new login). Mirrors the public projection in
/// `oauth::account_params`.
pub fn native_param_keys(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Qwen => &["product"],
        Provider::Cursor => &["query_mode", "device_id", "user_id"],
        Provider::ChatGpt => &["account_id"],
        Provider::Copilot => &["query_mode", "mode", "org", "billing"],
        Provider::Kimi => &["product", "token_env", "device_id"],
        Provider::Grok => &[
            "product",
            "session_env",
            "email",
            "user_id",
            "team_id",
            "management_key_env",
        ],
        Provider::GeminiAi => &["product", "session_env", "project_id"],
        Provider::Qoder => &["region", "credential_type"],
        Provider::CodeBuddy => &["region"],
        Provider::Kiro => &["auth_method", "profile_arn", "region"],
        Provider::Zed => &["user_id", "system_id"],
        Provider::XiaomiMimo => &["region", "auth_method"],
        _ => &[],
    }
}

/// Strip every managed key from `params` so the form always wins over the
/// clone. Callers normally work on a fresh clone of the editing account's
/// params; mutating the original would discard user edits.
pub fn strip_managed_keys(provider: Provider, params: &mut BTreeMap<String, String>) {
    for key in managed_keys(provider) {
        params.remove(*key);
    }
}

/// Per-key validation aligned to the adapter constraints (path_id, slug,
/// ARN shape, Azure ARM resource id, billing cycle format...). Empty values
/// are accepted as "field cleared" by the caller; `required` lives in the
/// draft.
///
/// RF8: errors are [`Localized`] with stable `edit_error.param.*` ids; the
/// Portuguese wording is the fallback and never echoes the typed value.
pub fn validate_param(
    key: &str,
    value: &str,
    params: &BTreeMap<String, String>,
) -> Result<(), crate::text::Localized> {
    use crate::text::{params as l10n_params, Localized as L10n, ParamValue};
    match key {
        "billing_cycle" => {
            if value.len() == 7
                && chrono::NaiveDate::parse_from_str(&format!("{value}-01"), "%Y-%m-%d").is_ok()
            {
                Ok(())
            } else {
                Err(L10n::fixed(
                    "edit_error.param.billing_cycle",
                    "Informe o ciclo no formato YYYY-MM.",
                ))
            }
        }
        k if k.ends_with("_env") => {
            if valid_env_name(value) {
                Ok(())
            } else {
                Err(L10n::fixed(
                    "edit_error.param.env_name",
                    "Informe apenas o nome da variável de ambiente, nunca o valor.",
                ))
            }
        }
        "team_id" | "organization_id" | "user_id" => {
            if !value.is_empty()
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                Ok(())
            } else {
                Err(L10n::fixed(
                    "edit_error.param.identifier",
                    "Identificador inválido: use letras, números, hífen ou sublinhado.",
                ))
            }
        }
        "system_id" => {
            if value.is_empty()
                || value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                Ok(())
            } else {
                Err(L10n::fixed(
                    "edit_error.param.system_id",
                    "System ID inválido: use letras, números, hífen ou sublinhado.",
                ))
            }
        }
        "email" => {
            if value.is_empty()
                || (value.contains('@')
                    && value.chars().all(|c| !c.is_control() && !c.is_whitespace()))
            {
                Ok(())
            } else {
                Err(L10n::fixed("edit_error.param.email", "E-mail inválido."))
            }
        }
        "profile_arn" => {
            if value.is_empty()
                || (value.starts_with("arn:")
                    && value.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '-' | '_' | '.')
                    }))
            {
                Ok(())
            } else {
                Err(L10n::fixed(
                    "edit_error.param.profile_arn",
                    "ARN inválido: deve começar com \"arn:\" e usar apenas caracteres AWS.",
                ))
            }
        }
        "product" => {
            if value == "computer" {
                Ok(())
            } else {
                Err(L10n::fixed(
                    "edit_error.param.product",
                    "Use computer para Computer Analytics, ou deixe vazio para Perplexity/Sonar.",
                ))
            }
        }
        "project_id" | "subscription_id" => {
            if value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
            {
                Ok(())
            } else {
                Err(L10n::with_param(
                    "edit_error.param.resource_id_chars",
                    "key",
                    key,
                    format!(
                        "params.{key} contém caracteres inválidos para identificador de recurso."
                    ),
                ))
            }
        }
        "resource_id" => {
            let subscription = params
                .get("subscription_id")
                .map(String::as_str)
                .unwrap_or_default();
            let segments: Vec<&str> = value.split('/').collect();
            let valid = !subscription.is_empty()
                && value.starts_with(&format!("/subscriptions/{subscription}/resourceGroups/"))
                && value.contains("/providers/Microsoft.CognitiveServices/accounts/")
                && !value.ends_with('/')
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
                && segments.iter().skip(1).all(|s| !s.is_empty());
            if valid {
                Ok(())
            } else {
                Err(L10n::with_params(
                    "edit_error.param.resource_id",
                    l10n_params([
                        ("name", ParamValue::from(key.to_string())),
                        ("rg", ParamValue::from("{rg}".to_string())),
                        ("sub", ParamValue::from("{sub}".to_string())),
                    ]),
                    "resource_id deve ser o ID ARM completo dentro da assinatura informada: /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.CognitiveServices/accounts/{nome}.",
                ))
            }
        }
        "org" => {
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                Ok(())
            } else {
                Err(L10n::fixed(
                    "edit_error.param.org",
                    "Organização inválida: use apenas letras, números e hífens.",
                ))
            }
        }
        _ => Ok(()),
    }
}

/// Parses the per-account interval text. Empty input means "no override"
/// (inherit the global). Range and parse errors return `Err` with a
/// [`Localized`] message (`edit_error.interval_*`); callers may surface it
/// directly to the user.
pub fn parse_refresh_seconds(text: &str) -> Result<Option<u64>, crate::text::Localized> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let seconds: u64 = trimmed.parse().map_err(|_| {
        crate::text::Localized::fixed("edit_error.interval_invalid", "Intervalo inválido.")
    })?;
    if !(MIN_REFRESH_SECONDS..=MAX_REFRESH_SECONDS).contains(&seconds) {
        return Err(crate::text::Localized::with_params(
            "edit_error.interval_range",
            crate::text::params([
                ("min", crate::text::ParamValue::from(MIN_REFRESH_SECONDS)),
                ("max", crate::text::ParamValue::from(MAX_REFRESH_SECONDS)),
            ]),
            format!(
                "Intervalo deve ficar entre {MIN_REFRESH_SECONDS} e {MAX_REFRESH_SECONDS} segundos."
            ),
        ));
    }
    Ok(Some(seconds))
}

/// `bl` profile names accept letters, digits, hyphen and underscore; up to
/// 32 chars; no leading hyphen.
pub fn is_valid_cli_profile(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Returns the `bl` profile to use for this account, creating one when none
/// is present. The `bl` CLI isolates credentials via `--config <name>`;
/// without a dedicated profile, two accounts would share `default` and
/// could read each other's data.
pub fn ensure_cli_profile(params: &mut BTreeMap<String, String>) -> String {
    const KEY: &str = "cli_profile";
    if let Some(existing) = params.get(KEY) {
        if !existing.is_empty() && is_valid_cli_profile(existing) {
            return existing.clone();
        }
    }
    let mut encoded = [0; 32];
    let id = uuid::Uuid::new_v4().as_simple().encode_lower(&mut encoded);
    let profile = format!("cuotai-{}", &id[..24]);
    params.insert(KEY.into(), profile.clone());
    profile
}

/// Authentication-mode id used by a manually supplied credential (environment
/// variable or provider-specific profile). Interactive mode ids come from
/// `oauth::login` (`oauth`, `aws_sso`, `bailian_console`).
pub const AUTH_MODE_CREDENTIAL: &str = "credential";

/// One machine value and label in a choice field. `auth_modes = None` means
/// the option applies to every authentication type. A restricted option also
/// carries the Portuguese fallback for its localizable unavailability reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChoiceOptionDescriptor {
    pub value: &'static str,
    pub label: &'static str,
    pub auth_modes: Option<&'static [&'static str]>,
    pub unavailable_reason: Option<&'static str>,
}

impl ChoiceOptionDescriptor {
    pub const fn new(value: &'static str, label: &'static str) -> Self {
        Self {
            value,
            label,
            auth_modes: None,
            unavailable_reason: None,
        }
    }

    pub const fn for_auth_modes(
        value: &'static str,
        label: &'static str,
        auth_modes: &'static [&'static str],
        unavailable_reason: &'static str,
    ) -> Self {
        Self {
            value,
            label,
            auth_modes: Some(auth_modes),
            unavailable_reason: Some(unavailable_reason),
        }
    }

    pub fn applies_to(&self, auth_mode: &str) -> bool {
        match self.auth_modes {
            Some(modes) => modes.contains(&auth_mode),
            None => true,
        }
    }
}

/// Descriptor for a choice field. `omit_default` skips writing the provider
/// key only when the first declared option is selected. If that option is not
/// applicable to the active authentication mode, the mode default is written
/// explicitly so an adapter's implicit default cannot silently change it.
#[derive(Debug, Clone, Copy)]
pub struct ChoiceDescriptor {
    pub options: &'static [ChoiceOptionDescriptor],
    pub omit_default: bool,
}

/// A choice option valid for one authentication mode. `index` always refers
/// to the complete trusted descriptor list, including options filtered out for
/// the mode. Drafts therefore remain unambiguous and `build_account` can reject
/// a tampered or stale selection instead of mistaking it for another option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldOption {
    pub index: usize,
    pub value: &'static str,
    pub label: &'static str,
}

/// Valid options for `(provider, field, auth mode)` and the default used when
/// the current value is absent or no longer applicable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldOptions {
    pub options: Vec<FieldOption>,
    pub default: FieldOption,
}

/// Result of reconciling a current choice with an authentication mode.
/// `notice` is present only when a known option had to be replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChoiceResolution {
    pub option: FieldOption,
    pub notice: Option<crate::text::Localized>,
}

/// Kind of a field descriptor (text or choice). Mirrors the visual shape the
/// frontend renders; the value/selection always comes from the user input,
/// not from here.
#[derive(Debug, Clone, Copy)]
pub enum FieldDescriptorKind {
    Text,
    Choice(ChoiceDescriptor),
}

/// Provider-owned schema for a single account-edit field. Frontends read
/// `key`, `label`, `help`, `required` and the `kind` to render their own
/// widget; `build_account` uses the same descriptor to enforce requiredness
/// and validate choice selections against the trusted options list, refusing
/// to honour tampered drafts. Native-only login inputs use this same type
/// with `required = false`; shared fields are never duplicated.
#[derive(Debug, Clone, Copy)]
pub struct FieldDescriptor {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub required: bool,
    pub kind: FieldDescriptorKind,
}

impl FieldDescriptor {
    /// Convenience constructor for a plain text field.
    pub const fn text(
        key: &'static str,
        label: &'static str,
        help: &'static str,
        required: bool,
    ) -> Self {
        Self {
            key,
            label,
            help,
            required,
            kind: FieldDescriptorKind::Text,
        }
    }

    /// Convenience constructor for a choice field. `omit_default = true` is
    /// the common case (skip writing the key when the first declared option
    /// is selected, treating it as the provider's implicit default).
    pub const fn choice(
        key: &'static str,
        label: &'static str,
        help: &'static str,
        options: &'static [ChoiceOptionDescriptor],
        omit_default: bool,
    ) -> Self {
        Self {
            key,
            label,
            help,
            required: false,
            kind: FieldDescriptorKind::Choice(ChoiceDescriptor {
                options,
                omit_default,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// RF8: stable text ids for the descriptor schema. Ids are derived from the
// provider slug and the descriptor key — both stable, non-textual data — so
// a frontend resolves `field.<provider>.<key>.label` in its catalog and
// falls back to the Portuguese `label`/`help` when the id is missing.
// ---------------------------------------------------------------------------

/// Stable id of a field label: `field.<provider>.<key>.label`.
pub fn field_label_id(provider: Provider, key: &str) -> String {
    format!("field.{}.{}.label", provider.slug(), key)
}

/// Stable id of a field help text: `field.<provider>.<key>.help`.
pub fn field_help_id(provider: Provider, key: &str) -> String {
    format!("field.{}.{}.help", provider.slug(), key)
}

/// Stable id of a choice option label:
/// `field.<provider>.<key>.option.<value>`.
pub fn field_option_id(provider: Provider, key: &str, option: &str) -> String {
    format!("field.{}.{}.option.{}", provider.slug(), key, option)
}

/// Stable id of an option's unavailability reason:
/// `field.<provider>.<key>.option.<value>.unavailable`.
pub fn field_option_unavailable_id(provider: Provider, key: &str, option: &str) -> String {
    format!(
        "field.{}.{}.option.{}.unavailable",
        provider.slug(),
        key,
        option
    )
}

/// Stable id of a native-login option label:
/// `native.<provider>.<key>.label`.
pub fn native_option_label_id(provider: Provider, key: &str) -> String {
    format!("native.{}.{}.label", provider.slug(), key)
}

/// Stable id of a native-login option help text:
/// `native.<provider>.<key>.help`.
pub fn native_option_help_id(provider: Provider, key: &str) -> String {
    format!("native.{}.{}.help", provider.slug(), key)
}

/// Stable id of a native-login choice option label:
/// `native.<provider>.<key>.option.<value>`.
pub fn native_option_option_id(provider: Provider, key: &str, option: &str) -> String {
    format!("native.{}.{}.option.{}", provider.slug(), key, option)
}

/// [`Localized`] label of a descriptor, for embedding as a nested
/// parameter in validation errors (`edit_error.field_required`, ...).
fn field_label_note(provider: Provider, descriptor: &FieldDescriptor) -> crate::text::Localized {
    crate::text::Localized::fixed(field_label_id(provider, descriptor.key), descriptor.label)
}

/// `edit_error.field_required` with the descriptor label as a nested
/// localizable parameter.
fn required_field_note(provider: Provider, descriptor: &FieldDescriptor) -> crate::text::Localized {
    crate::text::Localized::with_param(
        "edit_error.field_required",
        "label",
        field_label_note(provider, descriptor),
        format!("{} é obrigatório.", descriptor.label),
    )
}

/// Validation error whose only parameter is the offending field's label
/// (nested, localizable).
fn field_note(
    provider: Provider,
    descriptor: &FieldDescriptor,
    id: &'static str,
    fallback: String,
) -> crate::text::Localized {
    crate::text::Localized::with_param(
        id,
        "label",
        field_label_note(provider, descriptor),
        fallback,
    )
}

fn declared_option(
    provider: Provider,
    key: &str,
    value: &str,
) -> Option<(&'static FieldDescriptor, &'static ChoiceOptionDescriptor)> {
    let descriptor = descriptor(provider, key)?;
    let FieldDescriptorKind::Choice(choice) = descriptor.kind else {
        return None;
    };
    choice
        .options
        .iter()
        .find(|option| option.value == value)
        .map(|option| (descriptor, option))
}

fn option_label_note(
    provider: Provider,
    key: &str,
    option: &ChoiceOptionDescriptor,
) -> crate::text::Localized {
    crate::text::Localized::fixed(field_option_id(provider, key, option.value), option.label)
}

/// Localizable label of an authentication type used in validation errors and
/// substitution notices. Unknown ids use a generic, non-echoing label.
pub fn auth_mode_label(auth_mode: &str) -> crate::text::Localized {
    let (id, fallback) = match auth_mode {
        AUTH_MODE_CREDENTIAL => ("auth_mode.credential.label", "Credencial manual"),
        "oauth" => ("auth_mode.oauth.label", "Login nativo"),
        "aws_sso" => ("auth_mode.aws_sso.label", "AWS SSO"),
        "bailian_console" => ("auth_mode.bailian_console.label", "Console Bailian"),
        _ => (
            "auth_mode.unknown.label",
            "Tipo de autenticação desconhecido",
        ),
    };
    crate::text::Localized::fixed(id, fallback)
}

/// Reason declared for an option that is restricted to a subset of auth
/// modes. The id is stable even when the fallback is the generic reason.
pub fn field_option_unavailability(
    provider: Provider,
    key: &str,
    value: &str,
) -> Option<crate::text::Localized> {
    let (_, option) = declared_option(provider, key, value)?;
    option.auth_modes?;
    let fallback = option
        .unavailable_reason
        .unwrap_or("Esta opção não está disponível para o tipo de autenticação selecionado.");
    Some(crate::text::Localized::fixed(
        field_option_unavailable_id(provider, key, value),
        fallback,
    ))
}

/// Localized reason why `value` cannot be used with `auth_mode`, or `None`
/// when the option is valid (or is not a declared choice value).
pub fn field_option_unavailable_reason(
    provider: Provider,
    key: &str,
    value: &str,
    auth_mode: &str,
) -> Option<crate::text::Localized> {
    let (_, option) = declared_option(provider, key, value)?;
    if option.applies_to(auth_mode) {
        None
    } else {
        field_option_unavailability(provider, key, value)
    }
}

/// RF11.2: return the valid options and mode default for a provider field.
/// Indices refer to the complete descriptor, not the filtered vector. `None`
/// means the key is unknown, is not a choice, or has no valid option for the
/// supplied mode.
pub fn field_options(provider: Provider, key: &str, auth_mode: &str) -> Option<FieldOptions> {
    let descriptor = descriptor(provider, key)?;
    let FieldDescriptorKind::Choice(choice) = descriptor.kind else {
        return None;
    };
    let options: Vec<FieldOption> = choice
        .options
        .iter()
        .enumerate()
        .filter(|(_, option)| option.applies_to(auth_mode))
        .map(|(index, option)| FieldOption {
            index,
            value: option.value,
            label: option.label,
        })
        .collect();
    let default = *options.first()?;
    Some(FieldOptions { options, default })
}

fn option_not_valid_for_auth_mode_note(
    provider: Provider,
    descriptor: &FieldDescriptor,
    option: &ChoiceOptionDescriptor,
    auth_mode: &str,
) -> crate::text::Localized {
    let reason = field_option_unavailable_reason(provider, descriptor.key, option.value, auth_mode)
        .expect("invalid auth-mode combination must have an unavailability reason");
    crate::text::Localized::with_params(
        "edit_error.option_not_valid_for_auth_mode",
        crate::text::params([
            ("field", field_label_note(provider, descriptor).into()),
            (
                "value",
                option_label_note(provider, descriptor.key, option).into(),
            ),
            ("auth_mode", auth_mode_label(auth_mode).into()),
            ("reason", reason.clone().into()),
        ]),
        format!(
            "{}: a opção {} não é válida com {}. {}",
            descriptor.label,
            option.label,
            auth_mode_label(auth_mode).fallback,
            reason.fallback
        ),
    )
}

/// RF11.3: reconcile a machine value with an auth mode. A known value that
/// became invalid is replaced by the mode default and accompanied by a fully
/// localizable notice naming the field, old/new labels, auth type and reason.
/// Unknown values retain the legacy prefill behaviour (default without
/// echoing the unknown text); direct draft submission still rejects them.
pub fn resolve_field_choice(
    provider: Provider,
    key: &str,
    auth_mode: &str,
    current: Option<&str>,
) -> Option<FieldChoiceResolution> {
    let options = field_options(provider, key, auth_mode)?;
    if let Some(value) = current {
        if let Some(option) = options.options.iter().find(|option| option.value == value) {
            return Some(FieldChoiceResolution {
                option: *option,
                notice: None,
            });
        }
    }
    let notice = current.and_then(|value| {
        let (descriptor, previous) = declared_option(provider, key, value)?;
        if previous.applies_to(auth_mode) {
            return None;
        }
        let reason = field_option_unavailable_reason(provider, key, value, auth_mode)?;
        let replacement = choice_descriptor_option(descriptor, options.default.index)?;
        Some(crate::text::Localized::with_params(
            "edit_notice.option_replaced_for_auth_mode",
            crate::text::params([
                ("field", field_label_note(provider, descriptor).into()),
                ("value", option_label_note(provider, key, previous).into()),
                (
                    "replacement",
                    option_label_note(provider, key, replacement).into(),
                ),
                ("auth_mode", auth_mode_label(auth_mode).into()),
                ("reason", reason.clone().into()),
            ]),
            format!(
                "{}: {} não está disponível com {}; substituído por {}. {}",
                descriptor.label,
                previous.label,
                auth_mode_label(auth_mode).fallback,
                replacement.label,
                reason.fallback
            ),
        ))
    });
    Some(FieldChoiceResolution {
        option: options.default,
        notice,
    })
}

fn choice_descriptor_option(
    descriptor: &FieldDescriptor,
    index: usize,
) -> Option<&'static ChoiceOptionDescriptor> {
    let FieldDescriptorKind::Choice(choice) = descriptor.kind else {
        return None;
    };
    choice.options.get(index)
}

/// Inputs needed to decide which provider-owned fields are visible and which
/// choice values are valid for the active authentication mode.
#[derive(Debug)]
pub struct FieldVisibility<'a> {
    /// Stable id of the selected authentication type (`credential`, `oauth`,
    /// `aws_sso` or `bailian_console`).
    pub auth_mode: &'a str,
    /// True when the credential step is in OAuth mode AND the provider is
    /// native-login (no client registration). Native-param keys become
    /// hidden — the identity is rewritten from public login metadata.
    pub native_login: bool,
    /// `oauth_login_params` populated so far (used by Kiro: `profile_arn`
    /// becomes editable only when the login metadata did NOT already pin
    /// it; otherwise it stays hidden to mirror the auth result).
    pub login_params: &'a BTreeMap<String, String>,
    /// Currently selected full-descriptor choice index per `key`. Choices
    /// with no entry default to index `0`.
    pub choice_selections: &'a BTreeMap<String, usize>,
}

impl<'a> FieldVisibility<'a> {
    pub fn new(
        auth_mode: &'a str,
        login_params: &'a BTreeMap<String, String>,
        choice_selections: &'a BTreeMap<String, usize>,
    ) -> Self {
        Self {
            auth_mode,
            native_login: false,
            login_params,
            choice_selections,
        }
    }
}

/// Returns `Some(index)` if the key is present and within range, `None`
/// otherwise. Mirrors the check core applies when validating a draft.
fn descriptor_for<'a>(
    descriptors: &'a [FieldDescriptor],
    key: &str,
) -> Option<(usize, &'a FieldDescriptor)> {
    descriptors.iter().enumerate().find(|(_, d)| d.key == key)
}

/// Compute the list of visible descriptor indices for a provider given the
/// live visibility inputs. The list preserves descriptor order so the
/// wizard can iterate naturally and matches the indices used by
/// `apply_draft_fields` when validating a draft.
///
/// Hidden fields are NEVER persisted: `apply_draft_fields` only iterates the
/// descriptors, so even if the frontend forwards a value for a hidden key it
/// is silently ignored.
pub fn visible_field_indices(provider: Provider, visibility: &FieldVisibility<'_>) -> Vec<usize> {
    let descriptors = field_descriptors(provider);
    descriptors
        .iter()
        .enumerate()
        .filter_map(|(idx, descriptor)| {
            let selection = |key: &str| visibility.choice_selections.get(key).copied().unwrap_or(0);
            let mode_org = selection("mode") == 1;
            let quota_mode = selection("query_mode") == 1;
            let cli_product = selection("product") == 1
                && matches!(provider, Provider::GeminiAi | Provider::Grok);
            let editable_kiro_profile = provider == Provider::Kiro
                && descriptor.key == "profile_arn"
                && !visibility.login_params.contains_key("profile_arn");
            if visibility.native_login
                && native_param_keys(provider).contains(&descriptor.key)
                && !editable_kiro_profile
            {
                return None;
            }
            let keep = match descriptor.key {
                "org" => mode_org && !quota_mode,
                "mode" | "billing" if provider == Provider::Copilot => !quota_mode,
                "session_env" | "email" | "user_id"
                    if matches!(provider, Provider::GeminiAi | Provider::Grok) =>
                {
                    cli_product
                }
                "project_id" if provider == Provider::GeminiAi => cli_product,
                "token_env" if provider == Provider::Kimi => selection("product") == 1,
                "team_id" | "management_key_env" if provider == Provider::Grok => !cli_product,
                _ => true,
            };
            if keep {
                Some(idx)
            } else {
                None
            }
        })
        .collect()
}

/// Provider-owned schema for the params step. The wizard renders exactly
/// these fields (in this order) and forwards their current value/choice to
/// `build_account`. UI-only labels/help descriptors live here so the TUI and
/// any future frontend share a single source.
///
/// Each provider's array lives in a dedicated `const` block so the items are
/// constructed in const context (the constructor is `const fn`); the function
/// body just returns a `&'static` slice pointing at the right constant.
pub fn field_descriptors(provider: Provider) -> &'static [FieldDescriptor] {
    const OPENAI_API: [FieldDescriptor; 2] = [
        FieldDescriptor::text(
            "project_ids",
            "IDs de projetos",
            "Opcional; lista separada por vírgula para filtrar custos e uso.",
            false,
        ),
        FieldDescriptor::text(
            "models",
            "Modelos",
            "Opcional; lista separada por vírgula para filtrar o uso.",
            false,
        ),
    ];
    const ANTHROPIC_API: [FieldDescriptor; 2] = [
        FieldDescriptor::text(
            "workspace_ids",
            "IDs de workspaces",
            "Opcional; lista separada por vírgula para filtrar custos e uso.",
            false,
        ),
        FieldDescriptor::text(
            "models",
            "Modelos",
            "Opcional; lista separada por vírgula para filtrar o uso.",
            false,
        ),
    ];
    const CHATGPT: [FieldDescriptor; 1] = [FieldDescriptor::text(
        "account_id",
        "ID da conta",
        "Opcional; cabeçalho chatgpt-account-id para conta de equipe.",
        false,
    )];
    const EMPTY: [FieldDescriptor; 0] = [];
    const GEMINI_CLOUD: [FieldDescriptor; 1] = [FieldDescriptor::text(
        "project_id",
        "ID do projeto GCP",
        "Obrigatório; letras, números e - _ . :",
        true,
    )];
    const AZURE: [FieldDescriptor; 2] = [
        FieldDescriptor::text(
            "subscription_id",
            "ID da assinatura",
            "Obrigatório; assinatura Azure dona do recurso.",
            true,
        ),
        FieldDescriptor::text(
            "resource_id",
            "ID ARM do recurso",
            "Obrigatório; caminho completo /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.CognitiveServices/accounts/{nome}.",
            true,
        ),
    ];
    const COPILOT: [FieldDescriptor; 4] = [
        FieldDescriptor::choice(
            "query_mode",
            "Modo de consulta",
            "billing (padrão): APIs de cobrança documentadas. quota: quota pessoal (interface interna; exige token OAuth/PAT do GitHub, NÃO o copilotToken interno).",
            &[
                ChoiceOptionDescriptor::new("billing", "Cobrança documentada (padrão)"),
                ChoiceOptionDescriptor::new(
                    "quota",
                    "Quota pessoal (opt-in, interface interna)",
                ),
            ],
            true,
        ),
        FieldDescriptor::choice(
            "mode",
            "Escopo",
            "user: cobrança pessoal; org: cobrança da organização (requer admin). Sem efeito no modo quota.",
            &[
                ChoiceOptionDescriptor::new(
                    "user",
                    "Cobrança pessoal do usuário autenticado",
                ),
                ChoiceOptionDescriptor::new(
                    "org",
                    "Cobrança da organização (requer admin)",
                ),
            ],
            true,
        ),
        FieldDescriptor::text(
            "org",
            "Organização",
            "Obrigatório no modo org; slug da organização (letras, números e hífens).",
            true,
        ),
        FieldDescriptor::choice(
            "billing",
            "Produto",
            "premium_requests (padrão) ou ai_credits.",
            &[
                ChoiceOptionDescriptor::new("premium_requests", "Solicitações premium"),
                ChoiceOptionDescriptor::new("ai_credits", "Créditos de IA"),
            ],
            true,
        ),
    ];
    const MISTRAL: [FieldDescriptor; 1] = [FieldDescriptor::text(
        "admin_key_env",
        "Variável administrativa alternativa",
        "Opcional: nome da variável com Admin API Key. Vazio usa a variável da etapa Credencial, que deve conter chave administrativa.",
        false,
    )];
    const KIMI: [FieldDescriptor; 2] = [
        FieldDescriptor::choice(
            "product",
            "Produto",
            "Padrão: saldo da conta internacional Kimi/Moonshot. coding: opt-in para a quota do plano Kimi for Coding (interface interna, sujeita a mudanças).",
            &[
                ChoiceOptionDescriptor::new(
                    "default",
                    "Saldo da conta internacional (padrão)",
                ),
                ChoiceOptionDescriptor::new(
                    "coding",
                    "Kimi for Coding — quota do plano (opt-in, interno)",
                ),
            ],
            true,
        ),
        FieldDescriptor::text(
            "token_env",
            "Variável do token OAuth (opcional)",
            "Opcional; nome da variável com access token OAuth Kimi. Usado somente se a chave da etapa Credencial estiver ausente; a API key tem precedência, como no 9router.",
            false,
        ),
    ];
    const GROK: [FieldDescriptor; 6] = [
        FieldDescriptor::choice(
            "product",
            "Produto",
            "Padrão: Management API xAI (saldo/cobrança, exige Team ID). cli: opt-in para a quota do Grok CLI (interface interna) com token de sessão.",
            &[
                ChoiceOptionDescriptor::new("management", "Management API (padrão)"),
                ChoiceOptionDescriptor::new("cli", "Grok CLI (opt-in, interface interna)"),
            ],
            true,
        ),
        FieldDescriptor::text(
            "session_env",
            "Variável do token de sessão",
            "Nome (não valor) da variável com o access token do Grok CLI; o padrão é GROK_CLI_ACCESS_TOKEN — informe outro nome apenas se necessário. Usado somente no produto cli.",
            false,
        ),
        FieldDescriptor::text(
            "email",
            "E-mail (opcional)",
            "Opcional no produto cli; enviado como cabeçalho x-email.",
            false,
        ),
        FieldDescriptor::text(
            "user_id",
            "User ID (opcional)",
            "Opcional no produto cli; enviado como cabeçalho x-userid.",
            false,
        ),
        FieldDescriptor::text(
            "team_id",
            "Team ID xAI",
            "Obrigatório na Management API, disponível nas configurações da equipe xAI.",
            true,
        ),
        FieldDescriptor::text(
            "management_key_env",
            "Variável de Management Key alternativa",
            "Opcional: nome da variável com Management Key, não o valor. Vazio usa a variável da etapa Credencial.",
            false,
        ),
    ];
    const PERPLEXITY: [FieldDescriptor; 2] = [
        FieldDescriptor::text(
            "product",
            "Produto (opcional)",
            "Digite computer somente para Computer Enterprise Analytics. Vazio mantém Perplexity/Sonar com limitação explícita.",
            false,
        ),
        FieldDescriptor::text(
            "analytics_key_env",
            "Variável Analytics alternativa",
            "Opcional: variável com chave Computer Analytics. Não consulta consumo Sonar.",
            false,
        ),
    ];
    const CLINE: [FieldDescriptor; 1] = [FieldDescriptor::text(
        "organization_id",
        "Organização Cline (opcional)",
        "Vazio consulta a conta pessoal. Informe o ID da organização para consultar seu saldo, com a permissão correspondente.",
        false,
    )];
    const GEMINI_AI: [FieldDescriptor; 3] = [
        FieldDescriptor::choice(
            "product",
            "Produto",
            "A chave de API Gemini/AI Studio NÃO tem consulta de quota. cli: opt-in para a quota do Gemini CLI (CodeAssist, interface interna) com token de sessão.",
            &[
                ChoiceOptionDescriptor::new(
                    "default",
                    "Chave de API (sem consulta de quota)",
                ),
                ChoiceOptionDescriptor::new(
                    "cli",
                    "Gemini CLI / CodeAssist (opt-in, interno)",
                ),
            ],
            true,
        ),
        FieldDescriptor::text(
            "session_env",
            "Variável do token de sessão",
            "Nome (não valor) da variável com o access token do Gemini CLI; o padrão é GEMINI_CLI_ACCESS_TOKEN — informe outro nome apenas se necessário. Usado somente no produto cli.",
            false,
        ),
        FieldDescriptor::text(
            "project_id",
            "ID do projeto GCP (opcional)",
            "Opcional no produto cli; projeto CodeAssist. Vazio tenta resolução automática.",
            false,
        ),
    ];
    const QODER: [FieldDescriptor; 2] = [
        FieldDescriptor::choice(
            "region",
            "Região",
            "intl (padrão): openapi.qoder.sh. cn: openapi.qoder.com.cn.",
            &[
                ChoiceOptionDescriptor::new(
                    "intl",
                    "Internacional (openapi.qoder.sh)",
                ),
                ChoiceOptionDescriptor::new("cn", "China (openapi.qoder.com.cn)"),
            ],
            true,
        ),
        FieldDescriptor::choice(
            "credential_type",
            "Tipo de credencial",
            "auto (padrão): detecta o prefixo pt-. pat: Personal Access Token (trocado por job token). bearer: token Bearer/job já existente.",
            &[
                ChoiceOptionDescriptor::new("auto", "Automático (detecta pt-)"),
                ChoiceOptionDescriptor::new("pat", "Personal Access Token"),
                ChoiceOptionDescriptor::new("bearer", "Bearer/job token existente"),
            ],
            true,
        ),
    ];
    const KIRO: [FieldDescriptor; 2] = [
        FieldDescriptor::choice(
            "auth_method",
            "Método de autenticação",
            "No login nativo: builder-id (padrão) ou idc. Com credencial manual: google (padrão para novas contas), github, import, api_key ou external_idp.",
            &[
                ChoiceOptionDescriptor::for_auth_modes(
                    "builder-id",
                    "AWS Builder ID",
                    &["oauth"],
                    "O Cuotai não renova tokens curtos de Builder ID recebidos por variável de ambiente; use o login nativo.",
                ),
                ChoiceOptionDescriptor::for_auth_modes(
                    "idc",
                    "IAM Identity Center",
                    &["oauth"],
                    "O Cuotai não renova tokens curtos do Identity Center recebidos por variável de ambiente; use o login nativo.",
                ),
                ChoiceOptionDescriptor::for_auth_modes(
                    "google",
                    "Google",
                    &[AUTH_MODE_CREDENTIAL],
                    "O login nativo do Kiro aceita somente AWS Builder ID ou IAM Identity Center.",
                ),
                ChoiceOptionDescriptor::for_auth_modes(
                    "github",
                    "GitHub",
                    &[AUTH_MODE_CREDENTIAL],
                    "O login nativo do Kiro aceita somente AWS Builder ID ou IAM Identity Center.",
                ),
                ChoiceOptionDescriptor::for_auth_modes(
                    "import",
                    "Importado",
                    &[AUTH_MODE_CREDENTIAL],
                    "O login nativo do Kiro aceita somente AWS Builder ID ou IAM Identity Center.",
                ),
                ChoiceOptionDescriptor::for_auth_modes(
                    "api_key",
                    "API key",
                    &[AUTH_MODE_CREDENTIAL],
                    "O login nativo do Kiro aceita somente AWS Builder ID ou IAM Identity Center.",
                ),
                ChoiceOptionDescriptor::for_auth_modes(
                    "external_idp",
                    "IdP externo",
                    &[AUTH_MODE_CREDENTIAL],
                    "O login nativo do Kiro aceita somente AWS Builder ID ou IAM Identity Center.",
                ),
            ],
            true,
        ),
        FieldDescriptor::text(
            "profile_arn",
            "ARN do perfil CodeWhisperer (opcional)",
            "Perfil da sua conta; opcional no fluxo legado. Permite consultar créditos atuais no endpoint regional. Nunca use ARN de outra conta.",
            false,
        ),
    ];
    const CODEBUDDY: [FieldDescriptor; 1] = [FieldDescriptor::choice(
        "region",
        "Região",
        "cn (padrão): copilot.tencent.com (cabeçalhos CLI). intl: www.codebuddy.ai (cabeçalhos IDE).",
        &[
            ChoiceOptionDescriptor::new("cn", "China (copilot.tencent.com)"),
            ChoiceOptionDescriptor::new("intl", "Internacional (www.codebuddy.ai)"),
        ],
        true,
    )];
    const ZED: [FieldDescriptor; 2] = [
        FieldDescriptor::text(
            "user_id",
            "ID numérico do usuário Zed",
            "Obrigatório; enviado como \"Authorization: <user_id> <access_token>\".",
            true,
        ),
        FieldDescriptor::text(
            "system_id",
            "System ID (opcional)",
            "Opcional; enviado como cabeçalho x-zed-system-id quando definido.",
            false,
        ),
    ];
    const XIAOMI_MIMO: [FieldDescriptor; 3] = [
        FieldDescriptor::text(
            "platform_session_env",
            "Variável da sessão da plataforma (opcional)",
            "Nome da variável com Cookie de platform.xiaomimimo.com (api-platform_serviceToken e userId). Lê saldo/plano mensal; não cole cookies neste campo.",
            false,
        ),
        FieldDescriptor::text(
            "session_env",
            "Variável do cookie de sessão (opcional)",
            "Nome da variável com o cookie de sessão da conta Xiaomi (ex.: serviceToken=...), nunca o valor. Tem prioridade sobre a credencial principal.",
            false,
        ),
        FieldDescriptor::choice(
            "region",
            "Região",
            "Região do serviço MiMo: sgp (padrão), cn, ams, ru ou in.",
            &[
                ChoiceOptionDescriptor::new("sgp", "Singapura (padrão)"),
                ChoiceOptionDescriptor::new("cn", "China"),
                ChoiceOptionDescriptor::new("ams", "Amsterdã"),
                ChoiceOptionDescriptor::new("ru", "Rússia"),
                ChoiceOptionDescriptor::new("in", "Índia"),
            ],
            true,
        ),
    ];
    const ANTIGRAVITY: [FieldDescriptor; 1] = [FieldDescriptor::text(
        "project_id",
        "Projeto Code Assist (opcional)",
        "Vazio tenta obter o projeto pela sessão Antigravity. Não é obrigatório informar o projeto usado por uma API key Gemini.",
        false,
    )];
    const ALIBABA_MODEL_STUDIO: [FieldDescriptor; 1] = [FieldDescriptor::choice(
        "region",
        "Região Alibaba",
        "Use a região onde sua conta/chave foi criada; China e internacional possuem endpoints distintos.",
        &[
            ChoiceOptionDescriptor::new("intl", "Internacional"),
            ChoiceOptionDescriptor::new("cn", "China continental"),
        ],
        true,
    )];
    const ALIBABA_CODING_PLAN: [FieldDescriptor; 2] = [
        FieldDescriptor::choice(
            "region",
            "Região Alibaba",
            "Use a região da assinatura; a credencial não será enviada automaticamente para outra região.",
            &[
                ChoiceOptionDescriptor::new("intl", "Internacional"),
                ChoiceOptionDescriptor::new("cn", "China continental"),
            ],
            true,
        ),
        FieldDescriptor::text(
            "session_env",
            "Variável da sessão Alibaba (opcional)",
            "Se a API exigir login no console, informe o NOME da variável contendo o Cookie da sua sessão. Substitui a API key na consulta; nunca cole o cookie aqui.",
            false,
        ),
    ];
    const ALIBABA_TOKEN_PLAN: [FieldDescriptor; 1] = [FieldDescriptor::text(
        "session_env",
        "Variável do Cookie Qwen Cloud (opcional)",
        "Vazio = quota via perfil dedicado do `bl` (`params.cli_profile`). Se preferir a rota manual, informe o NOME da variável contendo o Cookie de home.qwencloud.com; nunca cole o Cookie aqui.",
        false,
    )];
    const ALIBABA_CLOUD: [FieldDescriptor; 4] = [
        FieldDescriptor::choice(
            "region",
            "Conta de faturamento",
            "Internacional ou China continental; corresponde à conta de cobrança Alibaba.",
            &[
                ChoiceOptionDescriptor::new("intl", "Internacional"),
                ChoiceOptionDescriptor::new("cn", "China continental"),
            ],
            true,
        ),
        FieldDescriptor::text(
            "secret_env",
            "Variável AccessKey Secret",
            "Opcional: padrão ALIBABA_CLOUD_ACCESS_KEY_SECRET. Informe apenas o nome da variável, nunca o segredo.",
            false,
        ),
        FieldDescriptor::text(
            "security_token_env",
            "Variável STS Security Token (opcional)",
            "Somente para credenciais temporárias STS; deixe vazio para AccessKey permanente.",
            false,
        ),
        FieldDescriptor::text(
            "billing_cycle",
            "Fatura mensal (opcional)",
            "YYYY-MM, por exemplo 2026-09. Vazio consulta apenas saldo. Fatura agrega todos os produtos da conta cloud, não somente AI.",
            false,
        ),
    ];
    match provider {
        Provider::OpenAiApi => &OPENAI_API,
        Provider::AnthropicApi => &ANTHROPIC_API,
        Provider::ChatGpt => &CHATGPT,
        Provider::Claude
        | Provider::Cursor
        | Provider::Windsurf
        | Provider::AwsBedrock
        | Provider::MiniMax
        | Provider::Qwen
        | Provider::Groq
        | Provider::NvidiaNim
        | Provider::Poolside
        | Provider::OpenCodeGo
        | Provider::CloudflareAi
        | Provider::OllamaCloud
        | Provider::Kimchi => &EMPTY,
        Provider::GeminiApi | Provider::VertexAi => &GEMINI_CLOUD,
        Provider::AzureOpenAi => &AZURE,
        Provider::Copilot => &COPILOT,
        Provider::Mistral => &MISTRAL,
        Provider::Kimi => &KIMI,
        Provider::Grok => &GROK,
        Provider::Perplexity => &PERPLEXITY,
        Provider::Cline => &CLINE,
        Provider::GeminiAi => &GEMINI_AI,
        Provider::Qoder => &QODER,
        Provider::Kiro => &KIRO,
        Provider::CodeBuddy => &CODEBUDDY,
        Provider::Zed => &ZED,
        Provider::XiaomiMimo => &XIAOMI_MIMO,
        Provider::Antigravity => &ANTIGRAVITY,
        Provider::AlibabaModelStudio => &ALIBABA_MODEL_STUDIO,
        Provider::AlibabaCodingPlan => &ALIBABA_CODING_PLAN,
        Provider::AlibabaTokenPlan => &ALIBABA_TOKEN_PLAN,
        Provider::AlibabaCloud => &ALIBABA_CLOUD,
    }
}

/// Provider-owned schema for non-secret native-login options. Fields shared
/// with the account form (`auth_method`/`region`) are borrowed from the single
/// [`field_descriptors`] declaration; only native-only inputs live in the
/// private slice below. The iterator avoids allocating while composing them.
pub fn native_option_descriptors(
    provider: Provider,
) -> impl Iterator<Item = &'static FieldDescriptor> {
    let shared_keys = native_shared_field_keys(provider);
    field_descriptors(provider)
        .iter()
        .filter(move |descriptor| shared_keys.contains(&descriptor.key))
        .chain(native_only_option_descriptors(provider).iter())
}

fn native_shared_field_keys(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Kiro => &["auth_method"],
        Provider::CodeBuddy | Provider::Qoder | Provider::XiaomiMimo => &["region"],
        _ => &[],
    }
}

fn native_only_option_descriptors(provider: Provider) -> &'static [FieldDescriptor] {
    const CURSOR: [FieldDescriptor; 1] = [FieldDescriptor::text(
        "database_path",
        "Banco local Cursor (opcional)",
        "Caminho completo de state.vscdb; vazio usa Cursor instalado. Nunca informe o token neste campo.",
        false,
    )];
    const KIRO: [FieldDescriptor; 2] = [
        FieldDescriptor::text(
            "region",
            "Região (opcional)",
            "Região AWS do Identity Center (ex.: us-east-1); vazio usa o padrão do fluxo.",
            false,
        ),
        FieldDescriptor::text(
            "start_url",
            "URL de início IDC (opcional)",
            "URL do portal IAM Identity Center; usada apenas no método idc.",
            false,
        ),
    ];
    const EMPTY: [FieldDescriptor; 0] = [];
    match provider {
        Provider::Cursor => &CURSOR,
        Provider::Kiro => &KIRO,
        _ => &EMPTY,
    }
}

/// Look up the descriptor for a provider-owned params key. Returns `None`
/// when the key is unknown to the provider (treat as hidden / not persisted).
pub fn descriptor(provider: Provider, key: &str) -> Option<&'static FieldDescriptor> {
    descriptor_for(field_descriptors(provider), key).map(|(_, d)| d)
}

/// UI-neutral field value consumed by [`build_account`]. The frontend sends
/// only the user's text or selected index; requiredness, options, defaults,
/// visibility, labels, and validation remain provider-owned core policy.
#[derive(Debug, Clone)]
pub enum DraftValue {
    Text(String),
    Choice(usize),
}

#[derive(Debug, Clone)]
pub struct DraftField {
    pub key: &'static str,
    pub value: DraftValue,
}

/// Everything `build_account` needs to assemble a final `Account`. Built
/// from the editing state by the frontend without leaking UI types.
#[derive(Debug)]
pub struct AccountDraft<'a> {
    pub provider: Provider,
    pub name: &'a str,
    /// Credential text: env-var name (validated), AWS profile, or whatever
    /// the active credential field is collecting.
    pub credential: &'a str,
    /// Stable authentication type selected by the frontend (`credential` or
    /// one of the ids returned by `login_modes`).
    pub auth_mode: &'a str,
    /// True if OAuth mode is active (provider supports login).
    pub oauth_mode: bool,
    pub oauth_existing: Option<&'a OAuthConfig>,
    pub oauth_authorized: Option<OAuthConfig>,
    /// Params returned by a successful native login in this session.
    pub oauth_login_params: BTreeMap<String, String>,
    /// Params cloned from the editing account (same provider) so user
    /// values for unmanaged keys survive an edit.
    pub existing_params: Option<&'a BTreeMap<String, String>>,
    /// Bailian F2 mutates params mid-edit to add `cli_profile`; when set,
    /// it overrides `existing_params`.
    pub credential_params: Option<BTreeMap<String, String>>,
    pub fields: &'a [DraftField],
    pub interval_text: &'a str,
}

/// Sanitize the account name and confirm it does not collide with another
/// account in `config`. `editing` excludes the slot being replaced so the
/// own name remains valid while editing.
///
/// The duplicate-name error is intentionally generic: the user's typed
/// value is never echoed back, so a tampered draft (or a hostile front-end)
/// cannot smuggle a misleading message into the displayed error.
pub fn validate_name(
    name: &str,
    config: &Config,
    editing: Option<usize>,
) -> Result<String, crate::text::Localized> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(crate::text::Localized::fixed(
            "edit_error.name_required",
            "Informe um nome para a conta.",
        ));
    }
    let collision = config
        .accounts
        .iter()
        .enumerate()
        .any(|(i, a)| Some(i) != editing && a.name.eq_ignore_ascii_case(trimmed));
    if collision {
        return Err(crate::text::Localized::fixed(
            "edit_error.duplicate_name",
            "Já existe uma conta com esse nome.",
        ));
    }
    Ok(trimmed.to_string())
}

/// Resolve the `credential_env` field for an account given the provider
/// semantics. Returns `None` for AWS (profile goes in params), OAuth
/// accounts (token lives in the secure store) and AlibabaTokenPlan (token
/// plan quota uses the dedicated `bl` profile). For env-var providers the
/// trimmed credential must be a valid env-var name.
pub fn resolve_credential_env(
    provider: Provider,
    credential_text: &str,
    oauth: Option<&OAuthConfig>,
) -> Result<Option<String>, crate::text::Localized> {
    if provider == Provider::AwsBedrock {
        // Profile goes into params in `apply_aws_profile`; nothing here.
        return Ok(None);
    }
    if oauth.is_some() || provider == Provider::AlibabaTokenPlan {
        // Token lives in the secure store / dedicated `bl` profile.
        return Ok(None);
    }
    let trimmed = credential_text.trim();
    if trimmed.is_empty() {
        return Err(crate::text::Localized::fixed(
            "edit_error.env_required",
            "Informe o NOME da variável de ambiente com a credencial.",
        ));
    }
    if !valid_env_name(trimmed) {
        return Err(crate::text::Localized::fixed(
            "edit_error.env_invalid",
            "Nome de variável inválido ([A-Za-z_][A-Za-z0-9_]*). Não cole o token: informe apenas o nome da variável exportada.",
        ));
    }
    Ok(Some(trimmed.to_string()))
}

/// Pick the OAuth config the account should be saved with. Order matters:
/// in-progress authorized login wins; existing config survives as long as
/// the identity was not changed (caller's responsibility to invalidate).
/// `oauth_mode = false` returns `None` — env-var accounts never carry it.
pub fn resolve_oauth(
    oauth_mode: bool,
    oauth_authorized: Option<OAuthConfig>,
    oauth_existing: Option<&OAuthConfig>,
) -> Result<Option<OAuthConfig>, crate::text::Localized> {
    if !oauth_mode {
        return Ok(None);
    }
    if let Some(config) = oauth_authorized.or_else(|| oauth_existing.cloned()) {
        return Ok(Some(config));
    }
    Err(crate::text::Localized::fixed(
        "edit_error.oauth_required",
        "Faça o login OAuth (Enter em \"Iniciar login\") antes de gravar, ou volte à variável de ambiente com Tab.",
    ))
}

fn selected_choice_option<'a>(
    provider: Provider,
    descriptor: &FieldDescriptor,
    choice: &'a ChoiceDescriptor,
    selected: usize,
    auth_mode: &str,
) -> Result<&'a ChoiceOptionDescriptor, crate::text::Localized> {
    let Some(option) = choice.options.get(selected) else {
        return Err(field_note(
            provider,
            descriptor,
            "edit_error.choice_invalid",
            format!("Seleção inválida para {}.", descriptor.label),
        ));
    };
    if !option.applies_to(auth_mode) {
        return Err(option_not_valid_for_auth_mode_note(
            provider, descriptor, option, auth_mode,
        ));
    }
    Ok(option)
}

fn write_choice(
    provider: Provider,
    descriptor: &FieldDescriptor,
    choice: &ChoiceDescriptor,
    selected: usize,
    auth_mode: &str,
    params: &mut BTreeMap<String, String>,
) -> Result<(), crate::text::Localized> {
    let option = selected_choice_option(provider, descriptor, choice, selected, auth_mode)?;
    if selected != 0 || !choice.omit_default {
        params.insert(descriptor.key.into(), option.value.into());
    }
    Ok(())
}

/// Apply managed-keys cleanup and write the active fields into `params`.
/// Choice indices always refer to the complete trusted descriptor. A choice
/// supplied for a hidden field is still checked for auth applicability before
/// being discarded, so a stale/tampered Kiro combination cannot evade RF11.4.
pub fn apply_draft_fields(
    provider: Provider,
    params: &mut BTreeMap<String, String>,
    fields: &[DraftField],
    visibility: &FieldVisibility<'_>,
) -> Result<(), crate::text::Localized> {
    strip_managed_keys(provider, params);
    let descriptors = field_descriptors(provider);
    let visible = visible_field_indices(provider, visibility);
    let draft_by_key: BTreeMap<&'static str, &DraftField> =
        fields.iter().map(|field| (field.key, field)).collect();

    // Validate auth applicability independently of visibility. Hidden values
    // remain non-persistent, but known invalid combinations are never accepted.
    for field in fields {
        let Some((_, descriptor)) = descriptor_for(descriptors, field.key) else {
            continue;
        };
        if let (FieldDescriptorKind::Choice(choice), DraftValue::Choice(selected)) =
            (&descriptor.kind, &field.value)
        {
            selected_choice_option(
                provider,
                descriptor,
                choice,
                *selected,
                visibility.auth_mode,
            )?;
        }
    }

    for idx in visible {
        let descriptor = &descriptors[idx];
        let Some(field) = draft_by_key.get(descriptor.key) else {
            if descriptor.required {
                return Err(required_field_note(provider, descriptor));
            }
            if let FieldDescriptorKind::Choice(choice) = descriptor.kind {
                let options = field_options(provider, descriptor.key, visibility.auth_mode)
                    .ok_or_else(|| {
                        field_note(
                            provider,
                            descriptor,
                            "edit_error.choice_invalid",
                            format!("Seleção inválida para {}.", descriptor.label),
                        )
                    })?;
                write_choice(
                    provider,
                    descriptor,
                    &choice,
                    options.default.index,
                    visibility.auth_mode,
                    params,
                )?;
            }
            continue;
        };
        match (&descriptor.kind, &field.value) {
            (FieldDescriptorKind::Text, DraftValue::Text(value)) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    if descriptor.required {
                        return Err(required_field_note(provider, descriptor));
                    }
                    continue;
                }
                validate_param(descriptor.key, trimmed, params)?;
                params.insert(descriptor.key.into(), trimmed.to_string());
            }
            (FieldDescriptorKind::Choice(choice), DraftValue::Choice(selected)) => {
                write_choice(
                    provider,
                    descriptor,
                    choice,
                    *selected,
                    visibility.auth_mode,
                    params,
                )?;
            }
            (FieldDescriptorKind::Text, DraftValue::Choice(_)) => {
                return Err(field_note(
                    provider,
                    descriptor,
                    "edit_error.field_expected_text",
                    format!(
                        "Campo {} enviado como escolha, mas o esquema espera texto.",
                        descriptor.label
                    ),
                ));
            }
            (FieldDescriptorKind::Choice(_), DraftValue::Text(_)) => {
                return Err(field_note(
                    provider,
                    descriptor,
                    "edit_error.field_expected_choice",
                    format!(
                        "Campo {} enviado como texto, mas o esquema espera uma escolha.",
                        descriptor.label
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Extend params with native-login metadata after the form has been
/// written. Authorized sessions use the freshly returned params; otherwise
/// preserve the editing account's values for native keys (identity
/// unchanged). Kiro's `profile_arn` is intentionally not preserved because
/// its adapter resolves it from the access token.
pub fn apply_native_login_params(
    provider: Provider,
    oauth_authorized: Option<&OAuthConfig>,
    oauth_login_params: &BTreeMap<String, String>,
    editing_account: Option<&Account>,
    params: &mut BTreeMap<String, String>,
) {
    if !requires_client_registration(provider) {
        if let Some(config) = oauth_authorized {
            params.extend(crate::oauth::account_params(
                provider,
                config,
                oauth_login_params,
            ));
        } else if let Some(account) = editing_account {
            for key in native_param_keys(provider) {
                if let Some(value) = account.params.get(*key) {
                    if *key == "profile_arn" && provider == Provider::Kiro {
                        continue;
                    }
                    params.insert((*key).into(), value.clone());
                }
            }
        }
    }
}

fn validate_account_choice_params(
    provider: Provider,
    params: &BTreeMap<String, String>,
    auth_mode: &str,
) -> Result<(), crate::text::Localized> {
    for descriptor in field_descriptors(provider) {
        let FieldDescriptorKind::Choice(choice) = descriptor.kind else {
            continue;
        };
        let Some(value) = params.get(descriptor.key) else {
            continue;
        };
        let Some(option) = choice.options.iter().find(|option| option.value == value) else {
            return Err(field_note(
                provider,
                descriptor,
                "edit_error.choice_invalid",
                format!("Seleção inválida para {}.", descriptor.label),
            ));
        };
        if !option.applies_to(auth_mode) {
            return Err(option_not_valid_for_auth_mode_note(
                provider, descriptor, option, auth_mode,
            ));
        }
    }
    Ok(())
}

/// Push the AWS profile (when present) into `params["profile"]`; the
/// credential_env slot is `None` for AWS.
pub fn apply_aws_profile(
    provider: Provider,
    credential_text: &str,
) -> Result<(), crate::text::Localized> {
    if provider == Provider::AwsBedrock && credential_text.trim().is_empty() {
        return Err(crate::text::Localized::fixed(
            "edit_error.aws_profile_required",
            "Informe o perfil AWS (AWS CLI v2).",
        ));
    }
    Ok(())
}

/// Shape of the credential field on the account-edit form. Mirrors the
/// actual `build_account` fetcher behavior:
///
/// * `AwsProfile` — credential text becomes `params["profile"]` and is
///   consumed by the AWS CLI v2 / SSO flow. No env var is ever written
///   to TOML. Only `AwsBedrock`.
/// * `None` — the form exposes no credential text; quota relies on
///   `params["cli_profile"]` (the `bl` profile) or, optionally,
///   `params["session_env"]`. Only `AlibabaTokenPlan`.
/// * `EnvVar` — credential text is the POSIX env-var name holding the
///   secret; persisted to TOML via `credential_env`. Default for every
///   other provider.
///
/// Empty `default_env` is NOT a heuristic for this decision: AWS has an
/// empty `default_env` precisely because it does not use one. Frontends
/// must use this descriptor and never infer the shape from
/// `default_env` / `default_env(...).is_empty()`.
/// Serialized as `"env"`, `"profile"` or `"none"` for GUI frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// POSIX env-var name (the common case). The TOML persists the name
    /// in `credential_env`; the actual value lives in the environment.
    #[serde(rename = "env")]
    EnvVar,
    /// AWS CLI v2 profile name. Persisted in `params["profile"]`.
    #[serde(rename = "profile")]
    AwsProfile,
    /// No credential text on the form (token plan quota relies on the
    /// dedicated `bl` profile in `params["cli_profile"]`).
    None,
}

/// Shape of the credential field for `provider`. Mirrors the actual
/// `build_account` fetcher behaviour so a frontend never has to
/// `match` `Provider` to render the credential input.
pub fn credential_kind(provider: Provider) -> CredentialKind {
    match provider {
        Provider::AwsBedrock => CredentialKind::AwsProfile,
        Provider::AlibabaTokenPlan => CredentialKind::None,
        _ => CredentialKind::EnvVar,
    }
}

/// Existing stored value shown on the credential step for an existing
/// account. Centralises the provider-specific read so the TUI and
/// desktop do not duplicate the policy:
/// * `AwsBedrock` → `params["profile"]` (the AWS CLI v2 profile name).
/// * `AlibabaTokenPlan` → `params["cli_profile"]` (the dedicated `bl`
///   profile), or empty when absent.
/// * Anything else → `credential_env` (POSIX env-var name). Falls back
///   to `default_env(provider)` when nothing is stored yet so the user
///   sees the conventional suggestion.
pub fn stored_credential(account: &Account) -> String {
    let provider = account.provider;
    match credential_kind(provider) {
        CredentialKind::AwsProfile => account.params.get("profile").cloned().unwrap_or_default(),
        CredentialKind::None => account
            .params
            .get("cli_profile")
            .cloned()
            .unwrap_or_default(),
        CredentialKind::EnvVar => account
            .credential_env
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| default_env(provider).to_string()),
    }
}

/// Conventional env-var name to prefill when creating a brand-new
/// account for `provider`. Delegates to `default_env` so callers do not
/// reach for the provider table directly; kept here so every frontend
/// uses the same constant.
pub fn suggested_credential(provider: Provider) -> &'static str {
    default_env(provider)
}

/// PRD 0005 RF3: how the usage of the account being edited will be read,
/// from the same inputs [`build_account`] takes (choice indices refer to the
/// full descriptor). Front ends render the picker label, the credential
/// notes, the cost advisory and the save confirmation from it, never from
/// the provider. Fields the provider does not declare are ignored.
pub fn draft_access(
    provider: Provider,
    auth_mode: &str,
    fields: &[DraftField],
) -> crate::access::UsageAccess {
    let params = fields
        .iter()
        .filter_map(|field| {
            let value = match (&descriptor(provider, field.key)?.kind, &field.value) {
                (FieldDescriptorKind::Choice(choice), DraftValue::Choice(index)) => {
                    choice.options.get(*index)?.value.to_string()
                }
                (FieldDescriptorKind::Text, DraftValue::Text(text)) => text.trim().to_string(),
                _ => return None,
            };
            Some((field.key.to_string(), value))
        })
        .collect();
    crate::access::classify(
        provider,
        auth_mode == crate::oauth::login::MODE_OAUTH,
        &params,
    )
}

/// Glue every step together. Returns the new account ready to be inserted
/// into `Config::upsert_account`. Validation errors are user-facing and
/// safe to display.
///
/// The visibility state is derived from the live draft itself (Choice
/// selections and `oauth_login_params` already in `AccountDraft`) — no
/// caller-provided map of choices is needed.
#[allow(clippy::too_many_arguments)]
pub fn build_account(
    draft: &AccountDraft<'_>,
    config: &Config,
    editing: Option<usize>,
) -> Result<Account, crate::edit_error::EditError> {
    let AccountDraft {
        provider,
        name,
        credential,
        auth_mode,
        oauth_mode,
        oauth_existing,
        oauth_authorized,
        oauth_login_params,
        existing_params,
        credential_params,
        fields,
        interval_text,
    } = draft;

    let name = validate_name(name, config, editing)
        .map_err(|message| build_account_field_error("name", message))?;
    apply_aws_profile(*provider, credential)
        .map_err(|message| build_account_field_error("credential", message))?;

    let oauth = resolve_oauth(*oauth_mode, oauth_authorized.clone(), *oauth_existing)
        .map_err(|message| build_account_field_error("oauth", message))?;

    let credential_env = resolve_credential_env(*provider, credential, oauth.as_ref())
        .map_err(|message| build_account_field_error("credential_env", message))?;

    // Start from the credential-enriched params when Bailian F2 mutated
    // them; otherwise clone the editing account's params (same provider
    // only — provider switches start clean).
    let mut params: BTreeMap<String, String> = credential_params.clone().unwrap_or_else(|| {
        match editing.and_then(|i| config.accounts.get(i)) {
            Some(a) if a.provider == *provider => existing_params
                .map(|params| (*params).clone())
                .unwrap_or_else(|| a.params.clone()),
            _ => BTreeMap::new(),
        }
    });

    // Derive visibility directly from the draft: a field is hidden iff
    // its native_param_keys entry is set AND we're in native-login mode.
    // Choice-driven visibility (org / mode / cli / coding) is folded into
    // the same map so callers don't have to plumb two sources of truth.
    let mut choice_selections: BTreeMap<String, usize> = BTreeMap::new();
    for field in fields.iter() {
        if let DraftValue::Choice(selected) = &field.value {
            choice_selections.insert(field.key.to_string(), *selected);
        }
    }
    // native_login = oauth mode active AND the provider supports login
    // natively (no client registration required).
    let native_login = *oauth_mode
        && !requires_client_registration(*provider)
        && crate::oauth::supports_login(*provider);
    let visibility = FieldVisibility {
        auth_mode,
        native_login,
        login_params: oauth_login_params,
        choice_selections: &choice_selections,
    };
    apply_draft_fields(*provider, &mut params, fields, &visibility)
        .map_err(|message| build_account_field_error("params", message))?;

    let editing_account = editing.and_then(|i| config.accounts.get(i));
    if oauth.is_some() {
        apply_native_login_params(
            *provider,
            oauth_authorized.as_ref(),
            oauth_login_params,
            editing_account.filter(|account| account.provider == *provider),
            &mut params,
        );
    }

    if *provider == Provider::AwsBedrock {
        params.insert("profile".into(), credential.trim().to_string());
    }

    validate_account_choice_params(*provider, &params, auth_mode)
        .map_err(|message| build_account_field_error("params", message))?;

    let refresh_seconds = parse_refresh_seconds(interval_text)
        .map_err(|message| build_account_field_error("interval", message))?;

    Ok(Account {
        name,
        provider: *provider,
        credential_env,
        oauth,
        params,
        refresh_seconds,
    })
}

/// Map a [`Localized`](crate::text::Localized) produced by the internal
/// `build_account` helpers into a typed
/// [`crate::edit_error::EditError::Validation`]. The `field` points at the
/// schema name (`"name"`, `"credential"`, ...) so a UI can highlight the
/// offending widget without parsing the message; the note carries the
/// stable RF8 id, structured params and the Portuguese fallback. Messages
/// never echo user input.
fn build_account_field_error(
    field: &'static str,
    note: crate::text::Localized,
) -> crate::edit_error::EditError {
    crate::edit_error::validation_note(Some(field), note)
}

/// Inherent `Config` mutators owned by the account-edit module so the TUI
/// wizard and other frontends share a single insertion / removal API. Lives
/// here (not in `lib.rs`) to keep the edit policy in one file; lib.rs still
/// exposes them via `pub mod account_edit`.
impl Config {
    /// Insert a brand-new account or replace the one at `editing`. Errors
    /// when `editing` is out of bounds, when the name collides with another
    /// account, or when the resulting config fails to validate. Global
    /// settings (`refresh_seconds`, `concurrency`, ...) are preserved.
    pub fn upsert_account(
        &self,
        account: Account,
        editing: Option<usize>,
    ) -> Result<Config, crate::edit_error::EditError> {
        if let Some(idx) = editing {
            if idx >= self.accounts.len() {
                return Err(crate::edit_error::index_out_of_range(
                    idx,
                    self.accounts.len(),
                ));
            }
        }
        let mut account = account;
        account.name = validate_name(&account.name, self, editing)
            .map_err(|message| build_account_field_error("name", message))?;
        let mut updated = self.clone();
        match editing {
            Some(idx) => updated.accounts[idx] = account,
            None => updated.accounts.push(account),
        }
        updated.validate().map_err(|e| {
            crate::edit_error::validation_note(None, crate::edit_error::config_error_note(&e))
        })?;
        Ok(updated)
    }

    /// Remove the account at `index`. Errors when the index is out of bounds.
    pub fn without_account(&self, index: usize) -> Result<Config, crate::edit_error::EditError> {
        if index >= self.accounts.len() {
            return Err(crate::edit_error::index_out_of_range(
                index,
                self.accounts.len(),
            ));
        }
        let mut updated = self.clone();
        updated.accounts.remove(index);
        Ok(updated)
    }
}

// ---------------------------------------------------------------------------
// RF3: coordinated removal — TOML save first, then optional vault purge.
// All frontends funnel through `remove_account` so policy (effective storage
// key, shared-ref guard, save-before-purge ordering, typed partial failure)
// lives in one place. The TUI and desktop no longer call
// `Config::without_account` / `OAuthSession::logout` separately.
// ---------------------------------------------------------------------------

/// Outcome of [`remove_account`]. The TOML was always saved before the
/// function returned; the variants describe what happened to the OAuth
/// credential so frontends can render truthful text. Splitting
/// `Removed` into "had no credential" and "credential purged" matters
/// because the user-visible phrasing differs: a no-credential removal
/// never touched the vault, while a successful purge did and the UI
/// should say so.
#[derive(Debug)]
pub enum RemoveOutcome {
    /// The account had no OAuth credential (env-var, AWS profile, or a
    /// malformed OAuth config the core could not derive a key for). The
    /// TOML was updated; the vault was never touched.
    RemovedNoCredential(RemovedAccount),
    /// The account had an OAuth credential, the user requested a purge,
    /// the storage key was unique among the survivors, and the vault
    /// delete succeeded. TOML and vault are both updated.
    RemovedFromVault(RemovedAccount),
    /// The TOML was updated but the credential was intentionally left in
    /// the vault. The reason is carried in [`KeptReason`] so the UI can
    /// say *why* without inventing policy.
    CredentialKept {
        account: RemovedAccount,
        reason: KeptReason,
    },
    /// The TOML was saved but the vault purge failed. The credential
    /// remains in the vault (the safer of the two outcomes: a stray
    /// TOML entry would force the user to re-link from scratch). The
    /// `vault_error` is an RF8 [`crate::text::Localized`] note the frontend
    /// translates via its catalog (the Portuguese fallback is what pre-RF8
    /// callers received as a fixed string); it never carries token material.
    /// The retained `account` is the recovery primitive the UI uses to
    /// retry via [`purge_credential`] against the current config.
    PartialFailure {
        account: RemovedAccount,
        vault_error: crate::text::Localized,
    },
}

/// Why `remove_account` left the OAuth credential in the vault. The two
/// cases produce very different copy in the UI: "conforme solicitado" is
/// honest when the user declined the purge; "outra conta usa esta
/// credencial" is the only honest reason when the user *did* ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeptReason {
    /// `purge_credentials = false`. The user (or the frontend's default)
    /// explicitly declined the purge.
    Declined,
    /// The credential's effective storage key is shared with another
    /// surviving account. The core refuses to delete it because the
    /// survivor would break on the next refresh. The survivor's account
    /// name is included so the dialog can name it.
    SharedWith { other_account: String },
}

/// Public-but-secret-free information about the removed account. Lets a
/// frontend render an honest status (e.g. "credential still in Keychain
/// because it is shared") and re-attempt the purge later using
/// `storage_key`. `storage_key` is the UUID the vault uses internally; it
/// is not a secret but exposing it does not leak anything either.
#[derive(Debug, Clone)]
pub struct RemovedAccount {
    pub provider: Provider,
    pub oauth: Option<OAuthConfig>,
    pub storage_key: Option<String>,
}

/// Pre-save failures: the TOML was not touched, the vault was not
/// touched, the live `ConfigFile` is unchanged. Frontends should surface
/// the message verbatim; it never contains user-entered secrets or token
/// material (the OAuth config in memory is the one the user typed, but
/// the only field `storage_key` ever operates on is the `credential_id`
/// UUID, which is not secret).
#[derive(Debug, thiserror::Error)]
pub enum RemoveError {
    #[error("Índice {0} fora de alcance.")]
    Index(usize),
    #[error("Falha ao salvar a configuração: {0}")]
    Save(crate::edit_error::EditError),
    /// The account has an OAuth config but it is structurally invalid
    /// (e.g. malformed `credential_id`, empty `client_id`). The core
    /// cannot derive the vault key, so it cannot guarantee the
    /// credential is reachable — but it may still exist in the vault.
    /// Refusing to remove the TOML entry would be wrong (the user
    /// explicitly asked to delete the account), and silently
    /// reporting success is also wrong (the credential is in limbo).
    /// The caller MUST surface this to the user; the TOML was NOT
    /// saved.
    #[error(
        "Configuração OAuth inválida; não é possível garantir a remoção completa. Detalhes: {0}"
    )]
    InvalidOAuth(String),
}

impl KeptReason {
    /// RF8 structured note: stable id + parameters + Portuguese fallback.
    /// Desktop localizes via the id; the TUI uses the fallback verbatim.
    pub fn l10n(&self) -> crate::text::Localized {
        match self {
            KeptReason::Declined => crate::text::Localized::fixed(
                "remove.kept.declined",
                "Você optou por manter a credencial no cofre do sistema.",
            ),
            KeptReason::SharedWith { other_account } => crate::text::Localized::with_param(
                "remove.kept.shared",
                "name",
                other_account.clone(),
                format!(
                    "A credencial foi mantida porque a conta \"{other_account}\" ainda a usa; \
                     apagá-la quebraria a sobrevivente."
                ),
            ),
        }
    }
}

impl RemoveError {
    /// RF8 structured form of the removal error. Frontends resolve the id
    /// in their catalog; the TUI uses the fallback verbatim.
    pub fn l10n(&self) -> crate::text::Localized {
        match self {
            RemoveError::Index(index) => crate::text::Localized::with_param(
                "remove_error.index",
                "index",
                *index,
                self.to_string(),
            ),
            RemoveError::Save(inner) => crate::text::Localized::with_param(
                "remove_error.save",
                "detail",
                inner.l10n(),
                self.to_string(),
            ),
            RemoveError::InvalidOAuth(detail) => crate::text::Localized::with_param(
                "remove_error.invalid_oauth",
                "detail",
                detail.clone(),
                self.to_string(),
            ),
        }
    }
}

/// Coordinated removal of a single account. Saves the TOML first; only
/// after the file write succeeds does it touch the vault, and even then
/// only when the caller asked for a purge **and** the credential is not
/// shared with another surviving account.
///
/// `file` is mutated in place: the on-disk file is replaced atomically
/// via `ConfigFile::save`, and the in-memory snapshot is updated to
/// match. The cycle / monitor that callers keep outside the
/// `ConfigFile` must be rebuilt from `file.config` afterwards (the TUI
/// `run_remove_account` helper and the desktop `Backend::delete_account`
/// both stage the cycle + monitor first and commit only on `Ok`).
///
/// Failure invariant: if the TOML save fails, the vault is not
/// consulted. If the vault delete fails, the TOML is already updated
/// and the credential is reported as still present via
/// `RemoveOutcome::PartialFailure`. The caller can re-attempt the
/// purge with `purge_credential(session, &removed_account.storage_key)`
/// without depending on the now-gone account index.
pub async fn remove_account(
    file: &mut crate::config_file::ConfigFile,
    session: &crate::oauth::OAuthSession,
    index: usize,
    purge_credentials: bool,
) -> Result<RemoveOutcome, RemoveError> {
    let staged = remove_account_save_only(file, index, purge_credentials)?;
    match staged {
        StagedRemoval::RemovedNoCredential(account) => {
            Ok(RemoveOutcome::RemovedNoCredential(account))
        }
        StagedRemoval::Kept { account, reason } => {
            Ok(RemoveOutcome::CredentialKept { account, reason })
        }
        StagedRemoval::ReadyForPurge {
            account,
            provider,
            oauth,
        } => match purge_credential(session, &file.config, provider, &oauth).await {
            Ok(()) => Ok(RemoveOutcome::RemovedFromVault(account)),
            Err(error) => Ok(RemoveOutcome::PartialFailure {
                account,
                vault_error: error,
            }),
        },
    }
}

/// Sync phase of [`remove_account`]: validates the index, derives the
/// effective vault key, runs the shared-key guard, writes the TOML
/// (or refuses on save failure), and decides what to do next. Does
/// NOT touch the vault — that is the caller's responsibility, which
/// is what lets the desktop release the inner mutex around the
/// `OAuthSession::logout` round-trip while keeping the rest of the
/// critical section as short as a TOML write.
///
/// Returned [`StagedRemoval`] covers the four documented outcomes:
/// * `RemovedNoCredential` — env-var or AWS profile, nothing in the
///   vault to purge.
/// * `Kept` — `purge_credentials = false` or the storage key is shared
///   with a surviving account.
/// * `ReadyForPurge` — TOML saved, the caller MUST call
///   [`purge_credential`] next (the shared-key guard is the only thing
///   protecting the survivor and must be re-run against the
///   *current* config in case another edit changed the survivor set).
pub fn remove_account_save_only(
    file: &mut crate::config_file::ConfigFile,
    index: usize,
    purge_credentials: bool,
) -> Result<StagedRemoval, RemoveError> {
    let prev = file.config.clone();
    if index >= prev.accounts.len() {
        return Err(RemoveError::Index(index));
    }
    let target = prev.accounts[index].clone();

    // 1) Compute the effective vault key (if any) and the set of surviving
    //    keys. The shared-ref guard runs BEFORE the TOML save so we can
    //    tell the caller up front whether a purge would actually happen.
    //
    //    A non-empty OAuth config that fails to derive a key is a hard
    //    pre-save failure: the credential may still exist in the vault
    //    under a key the core cannot compute, so we cannot guarantee the
    //    removal is complete. Refusing the operation (rather than
    //    silently claiming `RemovedNoCredential`) is the only honest
    //    answer. The TOML is NOT touched in this branch.
    let target_key = match target.oauth.as_ref() {
        None => None,
        Some(cfg) => match crate::oauth::session::storage_key(target.provider, cfg) {
            Ok(key) => Some(key),
            Err(_) => {
                return Err(RemoveError::InvalidOAuth(
                    "Configuração OAuth inválida; não é possível garantir a \
                     remoção completa. Remova a entrada manualmente do cofre \
                     do sistema se quiser garantir a limpeza."
                        .into(),
                ));
            }
        },
    };

    let shared_with = target_key.as_ref().and_then(|key| {
        prev.accounts
            .iter()
            .enumerate()
            .filter_map(|(i, account)| {
                if i == index {
                    None
                } else {
                    account_storage_key(account).map(|k| (k, account.name.clone()))
                }
            })
            .find(|(k, _)| k == key)
            .map(|(_, name)| name)
    });

    // 2) Build the new TOML.
    let updated = prev.without_account(index).map_err(RemoveError::Save)?;
    // 3) Persist FIRST.
    file.save(updated.clone()).map_err(RemoveError::Save)?;

    // 4) Decide whether to purge.
    let kept_account = || RemovedAccount {
        provider: target.provider,
        oauth: target.oauth.clone(),
        storage_key: target_key.clone(),
    };
    if target_key.is_none() {
        return Ok(StagedRemoval::RemovedNoCredential(RemovedAccount {
            provider: target.provider,
            oauth: target.oauth.clone(),
            storage_key: None,
        }));
    }
    if !purge_credentials {
        return Ok(StagedRemoval::Kept {
            account: kept_account(),
            reason: KeptReason::Declined,
        });
    }
    if let Some(other) = shared_with {
        return Ok(StagedRemoval::Kept {
            account: kept_account(),
            reason: KeptReason::SharedWith {
                other_account: other,
            },
        });
    }

    // 5) TOML saved, vault untouched. Caller drives the async purge.
    let provider = target.provider;
    let oauth = target.oauth.clone().expect("checked Some above");
    Ok(StagedRemoval::ReadyForPurge {
        account: kept_account(),
        provider,
        oauth,
    })
}

/// Sync staging result of [`remove_account_save_only`]. The
/// `ReadyForPurge` variant hands the caller the projection of
/// [`RemovedAccount`] they need to call [`purge_credential`] outside
/// the critical section that holds the lock on the live config.
#[derive(Debug)]
pub enum StagedRemoval {
    /// TOML saved; account had no OAuth credential. Nothing to do.
    RemovedNoCredential(RemovedAccount),
    /// TOML saved; credential kept in the vault (declined or shared).
    Kept {
        account: RemovedAccount,
        reason: KeptReason,
    },
    /// TOML saved; caller MUST call [`purge_credential`] next against
    /// the current config to actually delete the vault entry.
    ReadyForPurge {
        account: RemovedAccount,
        provider: Provider,
        oauth: crate::oauth::OAuthConfig,
    },
}

/// Best-effort purge of a credential, intended for retries after
/// `remove_account` returned `CredentialKept` (the user declined the
/// purge the first time, or the key was shared) or `PartialFailure`
/// (the vault was unavailable during the original removal). The TOML
/// no longer mentions the removed account; the OAuth config stored in
/// [`RemovedAccount`] is the only public, non-secret reference to the
/// credential, so the retry works without depending on the removed
/// account's index.
///
/// `current_config` is the *live* configuration (post-removal). The
/// shared-key guard runs against this snapshot on every call: a retry
/// after the survivor was dropped succeeds, a retry while a new shared
/// reference exists is refused, and a retry after the original
/// reference re-appeared is also refused. The guard is the only thing
/// preventing an eager retry from deleting the survivor's token.
///
/// Returns the fixed-string error message on failure — never the OAuth
/// config, the token, or any other secret material.
pub async fn purge_credential(
    session: &crate::oauth::OAuthSession,
    current_config: &crate::Config,
    provider: Provider,
    oauth: &crate::oauth::OAuthConfig,
) -> Result<(), crate::text::Localized> {
    // Re-derive the vault key from the OAuth config. A failure here
    // means the original `remove_account` would have refused the
    // removal in the first place (see the `InvalidOAuth` pre-save
    // check), so the retry is also invalid.
    let key = match crate::oauth::session::storage_key(provider, oauth) {
        Ok(key) => key,
        Err(_) => {
            // Same fixed-string rationale as in `remove_account`: do
            // NOT echo the typed `OAuthError` Debug (it leaks the
            // variant name and therefore hints at the OAuth shape).
            return Err(crate::text::Localized::fixed(
                "remove_error.purge_invalid_oauth",
                "Configuração OAuth inválida; não é possível apagar o cofre.",
            ));
        }
    };
    // Re-run the shared-key guard against the current configuration.
    let shared = current_config.accounts.iter().any(|account| {
        account.provider == provider && account_storage_key(account).as_ref() == Some(&key)
    });
    if shared {
        return Err(crate::text::Localized::fixed(
            "remove_error.purge_shared",
            "Outra conta ainda usa esta credencial; não é possível apagar o cofre sem quebrar a sobrevivente.",
        ));
    }
    match session.logout(provider, oauth).await {
        Ok(()) => Ok(()),
        Err(error) => Err(error.l10n_for_purge()),
    }
}

/// Return the effective vault key for `account`, or `None` when the
/// account has no OAuth credential or the credential is malformed. Used
/// by `remove_account` to compute the shared-key guard and the recovery
/// primitive in one place. Public so the TUI / desktop can render the
/// survivor-set diagnostic without re-implementing the derivation.
pub fn account_storage_key(account: &Account) -> Option<String> {
    let cfg = account.oauth.as_ref()?;
    crate::oauth::session::storage_key(account.provider, cfg).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::OAuthConfig;
    use crate::{Config, Provider};
    use std::collections::BTreeMap;

    fn config_with(accounts: Vec<Account>) -> Config {
        Config {
            refresh_seconds: 300,
            concurrency: 4,
            timeout_seconds: 30,
            accounts,
        }
    }

    fn env_account(name: &str, provider: Provider, env: &str) -> Account {
        Account {
            name: name.into(),
            provider,
            credential_env: Some(env.into()),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    fn text_field(key: &'static str, _label: &'static str, value: &str) -> DraftField {
        DraftField {
            key,
            value: DraftValue::Text(value.into()),
        }
    }

    fn oauth(provider: Provider) -> OAuthConfig {
        let mut config = OAuthConfig::new(String::new());
        config.credential_id = "4be708d8-7073-4d01-85ba-3306cc3cf087".into();
        config.validate(provider).unwrap();
        config
    }

    #[test]
    fn default_env_is_a_valid_env_name() {
        // The helper is only useful if every value passes its own validator;
        // otherwise the form would refuse its own pre-fill.
        for provider in Provider::ALL {
            if matches!(provider, Provider::AwsBedrock) {
                continue;
            }
            assert!(
                valid_env_name(default_env(provider)),
                "default_env({provider:?}) = {}",
                default_env(provider)
            );
        }
    }

    #[test]
    fn valid_env_name_rejects_typical_token_shapes() {
        assert!(!valid_env_name(""));
        assert!(!valid_env_name("1FOO"));
        assert!(!valid_env_name("FOO BAR"));
        assert!(!valid_env_name("FOO-BAR"));
        assert!(!valid_env_name("sk-ant-123"));
        assert!(valid_env_name("FOO"));
        assert!(valid_env_name("_FOO"));
        assert!(valid_env_name("FOO_123"));
    }

    #[test]
    fn ensure_cli_profile_is_stable_and_unique() {
        let mut first_params = BTreeMap::new();
        let first = ensure_cli_profile(&mut first_params);
        assert!(is_valid_cli_profile(&first));
        // Re-call: same params keep the same profile (idempotent).
        assert_eq!(first, ensure_cli_profile(&mut first_params));

        let mut second_params = BTreeMap::new();
        let second = ensure_cli_profile(&mut second_params);
        assert!(is_valid_cli_profile(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn ensure_cli_profile_replaces_invalid_existing_value() {
        let mut params = BTreeMap::from([("cli_profile".into(), "!!!".into())]);
        let fresh = ensure_cli_profile(&mut params);
        assert!(is_valid_cli_profile(&fresh));
        assert_ne!(fresh, "!!!");
    }

    #[test]
    fn parse_refresh_seconds_handles_blank_range_and_errors() {
        assert_eq!(parse_refresh_seconds("").unwrap(), None);
        assert_eq!(parse_refresh_seconds("60").unwrap(), Some(60));
        assert!(parse_refresh_seconds("29").is_err());
        assert!(parse_refresh_seconds("86401").is_err());
        assert!(parse_refresh_seconds("not a number").is_err());
    }

    #[test]
    fn validate_param_rejects_malformed_identifiers() {
        let mut params = BTreeMap::new();
        params.insert("subscription_id".into(), "abc".into());
        assert!(validate_param(
            "resource_id",
            "/subscriptions/abc/resourceGroups/rg",
            &params
        )
        .is_err());
        assert!(validate_param(
            "resource_id",
            "/subscriptions/abc/resourceGroups/rg/providers/Microsoft.CognitiveServices/accounts/foo",
            &params
        )
        .is_ok());
        assert!(validate_param("org", "valid-org", &BTreeMap::new()).is_ok());
        assert!(validate_param("org", "bad org", &BTreeMap::new()).is_err());
        assert!(validate_param("org", "with_under", &BTreeMap::new()).is_err());
        assert!(validate_param("billing_cycle", "2026-09", &BTreeMap::new()).is_ok());
        assert!(validate_param("billing_cycle", "2026/09", &BTreeMap::new()).is_err());
        assert!(validate_param("token_env", "TOKEN_NAME", &BTreeMap::new()).is_ok());
        assert!(validate_param("token_env", "bad name", &BTreeMap::new()).is_err());
    }

    #[test]
    fn strip_managed_keys_removes_provider_managed_keys() {
        let mut params = BTreeMap::from([
            ("project_ids".into(), "p1".into()),
            ("kept".into(), "yes".into()),
            ("profile".into(), "default".into()),
        ]);
        strip_managed_keys(Provider::OpenAiApi, &mut params);
        assert!(params.contains_key("kept"));
        assert!(!params.contains_key("project_ids"));

        let mut aws = BTreeMap::from([
            ("profile".into(), "default".into()),
            ("kept".into(), "y".into()),
        ]);
        strip_managed_keys(Provider::AwsBedrock, &mut aws);
        assert!(!aws.contains_key("profile"));
        assert!(aws.contains_key("kept"));
    }

    #[test]
    fn upsert_replaces_account_at_known_index() {
        let config = config_with(vec![env_account(
            "OpenAI",
            Provider::OpenAiApi,
            "OPENAI_ADMIN_KEY",
        )]);
        let new_account = Account {
            name: "OpenAI".into(),
            provider: Provider::OpenAiApi,
            credential_env: Some("OPENAI_ADMIN_KEY".into()),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: Some(120),
        };
        let updated = config.upsert_account(new_account.clone(), Some(0)).unwrap();
        assert_eq!(updated.accounts.len(), 1);
        assert_eq!(updated.accounts[0].refresh_seconds, Some(120));
        assert_eq!(updated.refresh_seconds, 300, "global must stay untouched");
    }

    #[test]
    fn upsert_appends_when_no_editing_index() {
        let config = config_with(vec![env_account(
            "OpenAI",
            Provider::OpenAiApi,
            "OPENAI_ADMIN_KEY",
        )]);
        let new_account = Account {
            name: "Anthropic".into(),
            provider: Provider::AnthropicApi,
            credential_env: Some("ANTHROPIC_ADMIN_KEY".into()),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        };
        let updated = config.upsert_account(new_account.clone(), None).unwrap();
        assert_eq!(updated.accounts.len(), 2);
        assert_eq!(updated.accounts[1].name, "Anthropic");
    }

    #[test]
    fn upsert_rejects_duplicate_name_in_remaining_accounts() {
        let config = config_with(vec![env_account(
            "OpenAI",
            Provider::OpenAiApi,
            "OPENAI_ADMIN_KEY",
        )]);
        let dup = Account {
            name: "OpenAI".into(),
            provider: Provider::AnthropicApi,
            credential_env: Some("ANTHROPIC_ADMIN_KEY".into()),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        };
        assert!(config.upsert_account(dup, None).is_err());
    }

    #[test]
    fn upsert_edit_rejects_case_insensitive_collision_but_keeps_own_name() {
        let config = config_with(vec![
            env_account("OpenAI", Provider::OpenAiApi, "OPENAI_ADMIN_KEY"),
            env_account("Anthropic", Provider::AnthropicApi, "ANTHROPIC_ADMIN_KEY"),
        ]);
        let mut edited = config.accounts[1].clone();
        edited.name = " oPeNaI ".into();
        let err = config.upsert_account(edited, Some(1)).unwrap_err();
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        assert_eq!(err.field(), Some("name"));
        assert_eq!(err.message(), "Já existe uma conta com esse nome.");
        assert_eq!(err.l10n().id, "edit_error.duplicate_name");

        let mut own = config.accounts[1].clone();
        own.name = " Anthropic ".into();
        let updated = config.upsert_account(own, Some(1)).unwrap();
        assert_eq!(updated.accounts[1].name, "Anthropic");
        assert_eq!(updated.accounts[0].name, "OpenAI");
    }

    #[test]
    fn upsert_preserves_existing_oauth_when_identity_unchanged() {
        // The wizard sets oauth_authorized = None and oauth_existing = Some when
        // the user did not re-login; this path must keep the credential_id and
        // options untouched (no keychain churn).
        let mut existing_oauth = oauth(Provider::ChatGpt);
        existing_oauth.client_id = "client-from-existing".into();
        let original_id = existing_oauth.credential_id.clone();
        let mut account = env_account("Codex", Provider::ChatGpt, "");
        account.oauth = Some(existing_oauth.clone());

        let config = config_with(vec![account.clone()]);
        let draft = AccountDraft {
            provider: Provider::ChatGpt,
            name: "Codex",
            credential: "",
            auth_mode: "oauth",
            oauth_mode: true,
            oauth_existing: account.oauth.as_ref(),
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: Some(&account.params),
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let built = build_account(&draft, &config, Some(0)).unwrap();
        assert_eq!(built.oauth.unwrap().credential_id, original_id);
    }

    #[test]
    fn upsert_uses_authorized_oauth_and_projects_safe_login_params() {
        // F2 login flow: oauth_authorized = Some, oauth_existing = None.
        // Core projects the caller-supplied metadata through the provider
        // whitelist before storing account params.
        let mut authorized = oauth(Provider::ChatGpt);
        authorized.client_id = "fresh-login".into();
        let mut login_params = BTreeMap::new();
        login_params.insert("account_id".into(), "acc-123".into());
        login_params.insert("injected_secret".into(), "must-not-persist".into());

        let config = config_with(Vec::new());
        let draft = AccountDraft {
            provider: Provider::ChatGpt,
            name: "Codex",
            credential: "",
            auth_mode: "oauth",
            oauth_mode: true,
            oauth_existing: None,
            oauth_authorized: Some(authorized.clone()),
            oauth_login_params: login_params.clone(),
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let built = build_account(&draft, &config, None).unwrap();
        let saved = built.oauth.unwrap();
        assert_eq!(saved.client_id, "fresh-login");
        assert_eq!(saved.credential_id, authorized.credential_id);
        assert_eq!(built.params.get("account_id").unwrap(), "acc-123");
        assert!(!built.params.contains_key("injected_secret"));
    }

    #[test]
    fn upsert_blocks_save_in_oauth_mode_without_login() {
        // The form must reject the save when the user keeps OAuth mode on
        // but never confirmed the login.
        let draft = AccountDraft {
            provider: Provider::ChatGpt,
            name: "Codex",
            credential: "",
            auth_mode: "oauth",
            oauth_mode: true,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let config = config_with(Vec::new());
        let err = build_account(&draft, &config, None).unwrap_err();
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        assert_eq!(err.field(), Some("oauth"));
        assert!(
            err.message().contains("login OAuth"),
            "unexpected error: {err}"
        );
        assert_eq!(err.l10n().id, "edit_error.oauth_required");
    }

    #[test]
    fn without_account_removes_only_the_index() {
        let config = config_with(vec![
            env_account("OpenAI", Provider::OpenAiApi, "OPENAI_ADMIN_KEY"),
            env_account("Anthropic", Provider::AnthropicApi, "ANTHROPIC_ADMIN_KEY"),
            env_account("Mistral", Provider::Mistral, "MISTRAL_API_KEY"),
        ]);
        let after = config.without_account(1).unwrap();
        assert_eq!(after.accounts.len(), 2);
        assert_eq!(after.accounts[0].name, "OpenAI");
        assert_eq!(after.accounts[1].name, "Mistral");
    }

    #[test]
    fn without_account_rejects_out_of_bounds_index() {
        let config = config_with(vec![env_account(
            "OpenAI",
            Provider::OpenAiApi,
            "OPENAI_ADMIN_KEY",
        )]);
        assert!(config.without_account(1).is_err());
        assert!(config.without_account(usize::MAX).is_err());
    }

    #[test]
    fn build_account_writes_refresh_override_without_touching_global() {
        // Same invariant as the wizard's: only account.refresh_seconds moves;
        // config.refresh_seconds stays so other accounts keep inheriting it.
        let config = config_with(vec![env_account(
            "OpenAI",
            Provider::OpenAiApi,
            "OPENAI_ADMIN_KEY",
        )]);
        let draft = AccountDraft {
            provider: Provider::OpenAiApi,
            name: "OpenAI",
            credential: "OPENAI_ADMIN_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "60",
        };
        let account = build_account(&draft, &config, Some(0)).unwrap();
        let updated = config.upsert_account(account, Some(0)).unwrap();
        assert_eq!(updated.refresh_seconds, 300);
        assert_eq!(updated.accounts[0].refresh_seconds, Some(60));
    }

    #[test]
    fn build_account_blank_interval_clears_existing_override() {
        let mut existing = env_account("OpenAI", Provider::OpenAiApi, "OPENAI_ADMIN_KEY");
        existing.refresh_seconds = Some(45);
        let config = config_with(vec![existing.clone()]);
        let draft = AccountDraft {
            provider: Provider::OpenAiApi,
            name: "OpenAI",
            credential: "OPENAI_ADMIN_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: Some(&existing.params),
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let account = build_account(&draft, &config, Some(0)).unwrap();
        assert_eq!(account.refresh_seconds, None);
    }

    #[test]
    fn build_account_provider_change_drops_incompatible_params() {
        // Switching providers wipes the edit clone: the new provider starts
        // from empty params, never inherits stale values from the old one.
        let existing = Account {
            name: "Multi".into(),
            provider: Provider::OpenAiApi,
            credential_env: Some("OPENAI_ADMIN_KEY".into()),
            oauth: None,
            params: BTreeMap::from([("project_ids".into(), "kept".into())]),
            refresh_seconds: None,
        };
        let config = config_with(vec![existing]);
        let draft = AccountDraft {
            provider: Provider::Mistral,
            name: "Multi",
            credential: "MISTRAL_API_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            // Wizard clears this when the provider changes.
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let account = build_account(&draft, &config, Some(0)).unwrap();
        assert_eq!(account.provider, Provider::Mistral);
        assert!(account.params.is_empty());
        assert_eq!(account.credential_env.as_deref(), Some("MISTRAL_API_KEY"));
    }

    #[test]
    fn build_account_preserves_unmanaged_params_when_provider_unchanged() {
        // Editing with the same provider keeps unrelated user params.
        let existing = Account {
            name: "OpenAI".into(),
            provider: Provider::OpenAiApi,
            credential_env: Some("OPENAI_ADMIN_KEY".into()),
            oauth: None,
            params: BTreeMap::from([
                ("kept".into(), "yes".into()),
                ("project_ids".into(), "old".into()),
            ]),
            refresh_seconds: None,
        };
        let config = config_with(vec![existing.clone()]);
        let draft = AccountDraft {
            provider: Provider::OpenAiApi,
            name: "OpenAI",
            credential: "OPENAI_ADMIN_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: Some(&existing.params),
            credential_params: None,
            fields: &[text_field("kept", "kept", "yes")],
            interval_text: "",
        };
        let account = build_account(&draft, &config, Some(0)).unwrap();
        assert_eq!(account.params.get("kept").unwrap(), "yes");
        // project_ids is managed, so it was wiped before the field was
        // re-applied — and the form did not provide a new value.
        assert!(!account.params.contains_key("project_ids"));
    }

    #[test]
    fn build_account_aws_profile_goes_into_params_not_credential_env() {
        // AWS profile lives in params["profile"]; credential_env stays empty.
        let draft = AccountDraft {
            provider: Provider::AwsBedrock,
            name: "Prod",
            credential: "default",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let account = build_account(&draft, &config_with(Vec::new()), None).unwrap();
        assert!(account.credential_env.is_none());
        assert_eq!(account.params.get("profile").unwrap(), "default");
    }

    #[test]
    fn build_account_alibaba_token_plan_drops_credential_env() {
        // Token plan quota uses the dedicated `bl` profile in params; the
        // env var the user typed on the credential step never reaches TOML.
        let draft = AccountDraft {
            provider: Provider::AlibabaTokenPlan,
            name: "Qwen Cloud",
            credential: "QWEN_API_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: Some(BTreeMap::from([(
                "cli_profile".into(),
                "cuotai-123".into(),
            )])),
            fields: &[],
            interval_text: "",
        };
        let account = build_account(&draft, &config_with(Vec::new()), None).unwrap();
        assert!(account.credential_env.is_none());
        assert_eq!(account.params.get("cli_profile").unwrap(), "cuotai-123");
    }

    #[test]
    fn build_account_rejects_duplicate_name_against_other_accounts() {
        let config = config_with(vec![env_account(
            "OpenAI",
            Provider::OpenAiApi,
            "OPENAI_ADMIN_KEY",
        )]);
        let draft = AccountDraft {
            provider: Provider::AnthropicApi,
            name: "OpenAI",
            credential: "ANTHROPIC_ADMIN_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        assert!(build_account(&draft, &config, None).is_err());
    }

    #[test]
    fn build_account_required_field_left_blank_returns_error() {
        let draft = AccountDraft {
            provider: Provider::GeminiApi,
            name: "Cloud",
            credential: "GOOGLE_CLOUD_ACCESS_TOKEN",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[DraftField {
                key: "project_id",
                value: DraftValue::Text(String::new()),
            }],
            interval_text: "",
        };
        let err = build_account(&draft, &config_with(Vec::new()), None).unwrap_err();
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        assert_eq!(err.field(), Some("params"));
        assert!(
            err.message().contains("obrigatório"),
            "unexpected error: {err}"
        );
        assert_eq!(err.l10n().id, "edit_error.field_required");
    }

    #[test]
    fn build_account_resource_id_must_match_subscription() {
        let draft = AccountDraft {
            provider: Provider::AzureOpenAi,
            name: "Azure",
            credential: "AZURE_ACCESS_TOKEN",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[
                text_field("subscription_id", "subscription", "sub-1"),
                text_field(
                    "resource_id",
                    "resource",
                    "/subscriptions/sub-1/resourceGroups/rg/providers/Microsoft.CognitiveServices/accounts/foo",
                ),
            ],
            interval_text: "",
        };
        let account = build_account(&draft, &config_with(Vec::new()), None).unwrap();
        assert_eq!(account.params.get("subscription_id").unwrap(), "sub-1");
        assert!(account
            .params
            .get("resource_id")
            .unwrap()
            .starts_with("/subscriptions/sub-1/"));

        // Cross-tenant resource_id is rejected.
        let bad = AccountDraft {
            fields: &[
                text_field("subscription_id", "subscription", "sub-1"),
                text_field(
                    "resource_id",
                    "resource",
                    "/subscriptions/sub-2/resourceGroups/rg/providers/Microsoft.CognitiveServices/accounts/foo",
                ),
            ],
            ..draft
        };
        assert!(build_account(&bad, &config_with(Vec::new()), None).is_err());
    }

    #[test]
    fn build_account_native_login_preserves_existing_params_without_new_login() {
        // Edit an existing native-login account without re-logging in: the
        // native keys must come from the editing account, NOT from an empty
        // login_params. Kiro's profile_arn is intentionally skipped.
        let mut existing_oauth = oauth(Provider::Kiro);
        existing_oauth
            .options
            .insert("region".into(), "us-east-1".into());
        let existing_account = Account {
            name: "Kiro".into(),
            provider: Provider::Kiro,
            credential_env: None,
            oauth: Some(existing_oauth.clone()),
            params: BTreeMap::from([
                ("auth_method".into(), "idc".into()),
                ("region".into(), "us-east-1".into()),
                (
                    "profile_arn".into(),
                    "arn:aws:codewhisperer:us-east-1:0:profile/abc".into(),
                ),
            ]),
            refresh_seconds: None,
        };
        let config = config_with(vec![existing_account.clone()]);
        let draft = AccountDraft {
            provider: Provider::Kiro,
            name: "Kiro",
            credential: "",
            auth_mode: "oauth",
            oauth_mode: true,
            oauth_existing: existing_account.oauth.as_ref(),
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: Some(&existing_account.params),
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let account = build_account(&draft, &config, Some(0)).unwrap();
        assert_eq!(account.params.get("auth_method").unwrap(), "idc");
        assert_eq!(account.params.get("region").unwrap(), "us-east-1");
        assert!(
            !account.params.contains_key("profile_arn"),
            "Kiro profile_arn must be resolved from the access token, not copied"
        );
    }

    #[test]
    fn credential_help_and_oauth_helpers_cover_all_providers_without_panicking() {
        // Smoke test: every provider returns a non-empty help string and
        // does not blow up when asked for an OAuth hint. Some hints are
        // intentionally `None` (env-only providers).
        for provider in Provider::ALL {
            assert!(
                !credential_help(provider).text().is_empty()
                    || matches!(provider, Provider::AwsBedrock),
                "credential_help empty for {provider:?}"
            );
            // oauth_registration_help is always defined; oauth_unsupported_reason
            // is None for the majority. Just touch both to make sure the
            // match stays exhaustive.
            let _ = oauth_registration_help(provider);
            let _ = oauth_unsupported_reason(provider);
        }
    }

    fn empty_visibility<'a>(
        login_params: &'a BTreeMap<String, String>,
        choice_selections: &'a BTreeMap<String, usize>,
    ) -> FieldVisibility<'a> {
        FieldVisibility {
            auth_mode: AUTH_MODE_CREDENTIAL,
            native_login: false,
            login_params,
            choice_selections,
        }
    }

    #[test]
    fn visibility_hides_copilot_org_outside_org_mode() {
        // `org` is required in org/billing mode and disappears entirely in
        // the quota personal mode (Copilot special-case). Core owns the
        // conditional rendering rule.
        let mut selections = BTreeMap::new();
        // default mode (0) + default query_mode (0) → org hidden.
        let descriptors = field_descriptors(Provider::Copilot);
        let org_index = descriptors
            .iter()
            .position(|d| d.key == "org")
            .expect("Copilot descriptor exposes org");
        assert!(!visible_field_indices(
            Provider::Copilot,
            &empty_visibility(&BTreeMap::new(), &selections)
        )
        .contains(&org_index));

        // user mode keeps org hidden (must be org mode to surface).
        selections.insert("mode".into(), 0);
        assert!(!visible_field_indices(
            Provider::Copilot,
            &empty_visibility(&BTreeMap::new(), &selections)
        )
        .contains(&org_index));

        // org mode (mode=1) with billing query_mode (0) surfaces org.
        selections.insert("mode".into(), 1);
        assert!(visible_field_indices(
            Provider::Copilot,
            &empty_visibility(&BTreeMap::new(), &selections)
        )
        .contains(&org_index));

        // org mode with quota (query_mode=1) hides org again.
        selections.insert("query_mode".into(), 1);
        assert!(!visible_field_indices(
            Provider::Copilot,
            &empty_visibility(&BTreeMap::new(), &selections)
        )
        .contains(&org_index));
    }

    #[test]
    fn visibility_hides_grok_management_fields_under_cli_product() {
        // product=1 (cli) is mutually exclusive with team_id /
        // management_key_env, which only make sense in the default
        // Management API mode.
        let mut selections = BTreeMap::new();
        let descriptors = field_descriptors(Provider::Grok);
        let team = descriptors
            .iter()
            .position(|d| d.key == "team_id")
            .expect("Grok descriptor exposes team_id");
        let session = descriptors
            .iter()
            .position(|d| d.key == "session_env")
            .expect("Grok descriptor exposes session_env");

        // product=0 (management) keeps team_id and hides session_env.
        assert!(visible_field_indices(
            Provider::Grok,
            &empty_visibility(&BTreeMap::new(), &selections)
        )
        .contains(&team));
        assert!(!visible_field_indices(
            Provider::Grok,
            &empty_visibility(&BTreeMap::new(), &selections)
        )
        .contains(&session));

        // product=1 (cli) flips it.
        selections.insert("product".into(), 1);
        let visible = visible_field_indices(
            Provider::Grok,
            &empty_visibility(&BTreeMap::new(), &selections),
        );
        assert!(!visible.contains(&team));
        assert!(visible.contains(&session));
    }

    #[test]
    fn visibility_hides_kimi_token_env_outside_coding_product() {
        let mut selections = BTreeMap::new();
        let descriptors = field_descriptors(Provider::Kimi);
        let token_env = descriptors
            .iter()
            .position(|d| d.key == "token_env")
            .expect("Kimi descriptor exposes token_env");

        // default product → hidden.
        assert!(!visible_field_indices(
            Provider::Kimi,
            &empty_visibility(&BTreeMap::new(), &selections)
        )
        .contains(&token_env));
        // coding (product=1) → visible.
        selections.insert("product".into(), 1);
        assert!(visible_field_indices(
            Provider::Kimi,
            &empty_visibility(&BTreeMap::new(), &selections)
        )
        .contains(&token_env));
    }

    #[test]
    fn visibility_hides_native_param_keys_in_native_login_mode() {
        // In native-login OAuth mode the provider's `native_param_keys`
        // (region, auth_method, etc.) MUST stay hidden so the form cannot
        // diverge from the authenticated identity.
        let descriptors = field_descriptors(Provider::Grok);
        let team_id = descriptors
            .iter()
            .position(|d| d.key == "team_id")
            .expect("Grok descriptor exposes team_id");
        let login_params = BTreeMap::new();
        let choices = BTreeMap::new();
        let mut visibility = empty_visibility(&login_params, &choices);

        // Default (non-native) → team_id visible (Grok product=0 default).
        assert!(visible_field_indices(Provider::Grok, &visibility).contains(&team_id));

        // Native-login mode → hidden (controlled by the auth source).
        visibility.native_login = true;
        assert!(!visible_field_indices(Provider::Grok, &visibility).contains(&team_id));
    }

    // ---------- RF5: credential_kind / stored_credential ----------

    #[test]
    fn credential_kind_matches_build_account_fetcher_behaviour() {
        // The decision MUST mirror the actual fetcher behaviour, not the
        // empty `default_env` heuristic. AWS Bedrock uses an AWS CLI v2
        // profile; AlibabaTokenPlan relies on the dedicated `bl` profile
        // (params.cli_profile); every other provider persists a POSIX
        // env-var name through `credential_env`.
        for provider in Provider::ALL {
            let kind = credential_kind(provider);
            let env = default_env(provider);
            match provider {
                Provider::AwsBedrock => {
                    assert_eq!(
                        kind,
                        CredentialKind::AwsProfile,
                        "AwsBedrock must use AwsProfile regardless of default_env: {provider:?}"
                    );
                }
                Provider::AlibabaTokenPlan => {
                    assert_eq!(
                        kind,
                        CredentialKind::None,
                        "AlibabaTokenPlan must use None (bl profile) regardless of default_env: {provider:?}"
                    );
                }
                _ => {
                    assert_eq!(kind, CredentialKind::EnvVar, "{provider:?} must use EnvVar");
                    assert!(
                        !env.is_empty(),
                        "EnvVar providers carry a non-empty default_env to prefill: {provider:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn stored_credential_reads_profile_for_aws() {
        // AWS: stored value lives in params["profile"]; credential_env is
        // always empty for AWS, so the EnvVar path would return empty.
        let account = Account {
            name: "Prod".into(),
            provider: Provider::AwsBedrock,
            credential_env: None,
            oauth: None,
            params: BTreeMap::from([("profile".into(), "default".into())]),
            refresh_seconds: None,
        };
        assert_eq!(stored_credential(&account), "default");

        // Empty params["profile"] → empty (never falls back to default_env).
        let account = Account {
            name: "Prod".into(),
            provider: Provider::AwsBedrock,
            credential_env: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        };
        assert_eq!(stored_credential(&account), "");
    }

    #[test]
    fn stored_credential_reads_cli_profile_for_token_plan() {
        // AlibabaTokenPlan: stored value lives in params["cli_profile"];
        // credential_env must always be empty here. The fallback does not
        // surface any default env (the credential kind is `None`).
        let account = Account {
            name: "Qwen".into(),
            provider: Provider::AlibabaTokenPlan,
            credential_env: None,
            oauth: None,
            params: BTreeMap::from([("cli_profile".into(), "qwen-personal".into())]),
            refresh_seconds: None,
        };
        assert_eq!(stored_credential(&account), "qwen-personal");

        let empty = Account {
            name: "Qwen".into(),
            provider: Provider::AlibabaTokenPlan,
            credential_env: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        };
        assert_eq!(stored_credential(&empty), "");
    }

    #[test]
    fn stored_credential_reads_credential_env_for_env_providers() {
        // Boundary: env-var providers expose credential_env; when it is
        // empty/missing, suggested_credential (default_env) fills in so the
        // form pre-populates a sane suggestion.
        let account = Account {
            name: "OpenAI".into(),
            provider: Provider::OpenAiApi,
            credential_env: Some("OPENAI_ADMIN_KEY".into()),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        };
        assert_eq!(stored_credential(&account), "OPENAI_ADMIN_KEY");

        // Empty credential_env (cleared by user) falls back to default.
        let empty = Account {
            name: "OpenAI".into(),
            provider: Provider::OpenAiApi,
            credential_env: Some(String::new()),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        };
        assert_eq!(stored_credential(&empty), default_env(Provider::OpenAiApi));

        // Missing credential_env (e.g. account created by an older schema)
        // also falls back to default.
        let missing = Account {
            name: "OpenAI".into(),
            provider: Provider::OpenAiApi,
            credential_env: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        };
        assert_eq!(
            stored_credential(&missing),
            default_env(Provider::OpenAiApi)
        );
    }

    #[test]
    fn stored_credential_ignores_params_for_env_providers() {
        // AWS stores its profile in params; env-var providers must not be
        // confused by a stray `params["profile"]` left over from a
        // provider switch.
        let account = Account {
            name: "OpenAI".into(),
            provider: Provider::OpenAiApi,
            credential_env: Some("OPENAI_ADMIN_KEY".into()),
            oauth: None,
            params: BTreeMap::from([("profile".into(), "stale".into())]),
            refresh_seconds: None,
        };
        assert_eq!(stored_credential(&account), "OPENAI_ADMIN_KEY");
    }

    #[test]
    fn credential_kind_agrees_with_resolve_credential_env_for_aws() {
        // Boundary: AwsBedrock must NOT accept an env-var name in the
        // credential step even if the user typed one. resolve_credential_env
        // returns None (the value goes into params["profile"]); the form
        // shape must agree with this contract.
        let result = resolve_credential_env(Provider::AwsBedrock, "OPENAI_ADMIN_KEY", None);
        assert!(
            result.is_ok() && result.unwrap().is_none(),
            "AwsBedrock credential_env must be None"
        );
        assert_eq!(
            credential_kind(Provider::AwsBedrock),
            CredentialKind::AwsProfile
        );
    }

    #[test]
    fn credential_kind_agrees_with_resolve_credential_env_for_token_plan() {
        // AlibabaTokenPlan: resolve_credential_env returns None even when
        // a non-empty credential text is supplied (the bl profile lives in
        // params). Credential kind must mirror that.
        let result = resolve_credential_env(Provider::AlibabaTokenPlan, "QWEN_API_KEY", None);
        assert!(
            result.is_ok() && result.unwrap().is_none(),
            "AlibabaTokenPlan credential_env must be None"
        );
        assert_eq!(
            credential_kind(Provider::AlibabaTokenPlan),
            CredentialKind::None
        );
    }

    #[test]
    fn credential_kind_agrees_with_resolve_credential_env_for_env_providers() {
        // EnvVar providers: resolve_credential_env returns Some(env) for a
        // valid env-var name. Credential kind agrees: the value persists to
        // TOML through credential_env.
        let result = resolve_credential_env(Provider::OpenAiApi, "OPENAI_ADMIN_KEY", None);
        assert_eq!(result.unwrap(), Some("OPENAI_ADMIN_KEY".into()));
        assert_eq!(credential_kind(Provider::OpenAiApi), CredentialKind::EnvVar);

        // Empty/invalid → Err (form refuses to save with no name).
        assert!(resolve_credential_env(Provider::OpenAiApi, "", None).is_err());
        assert!(resolve_credential_env(Provider::OpenAiApi, "1FOO", None).is_err());
    }

    #[test]
    fn suggested_credential_matches_default_env() {
        // The frontend helper is a thin alias for default_env; check that
        // it stays in lockstep so a future tweak to one does not silently
        // diverge from the other.
        for provider in Provider::ALL {
            assert_eq!(
                suggested_credential(provider),
                default_env(provider),
                "{provider:?}"
            );
        }
    }

    #[test]
    fn build_account_rejects_out_of_range_choice_selection() {
        // Tampered draft: choice.selected points past the descriptor's
        // option list. Core must reject instead of indexing into garbage.
        let draft = AccountDraft {
            provider: Provider::Copilot,
            name: "GH",
            credential: "GITHUB_TOKEN",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[DraftField {
                key: "mode",
                value: DraftValue::Choice(42),
            }],
            interval_text: "",
        };
        let err = build_account(&draft, &config_with(Vec::new()), None).unwrap_err();
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        assert_eq!(err.field(), Some("params"));
        assert!(
            err.message().contains("Seleção inválida"),
            "unexpected error: {err}"
        );
        assert_eq!(err.l10n().id, "edit_error.choice_invalid");
    }

    #[test]
    fn build_account_drops_drafts_for_keys_unknown_to_descriptor() {
        // Hidden-field attack: the wizard (or a buggy caller) forwards a
        // value for a key that the provider's descriptor does not list.
        // Core must silently drop it — the schema is the source of truth.
        let draft = AccountDraft {
            provider: Provider::OpenAiApi,
            name: "OpenAI",
            credential: "OPENAI_ADMIN_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[DraftField {
                key: "totally_secret_key",
                value: DraftValue::Text("leaked".into()),
            }],
            interval_text: "",
        };
        let account = build_account(&draft, &config_with(Vec::new()), None).unwrap();
        assert!(!account.params.contains_key("totally_secret_key"));
    }

    #[test]
    fn build_account_drops_hidden_known_fields_forwarded_by_caller() {
        // Copilot in quota mode (query_mode=1) hides `mode` and `billing`.
        // If a buggy/hostile caller forwards values for those hidden
        // descriptors anyway, core must drop them — visibility is decided
        // by core from the draft's Choice selections, not by what the
        // caller sends.
        let draft = AccountDraft {
            provider: Provider::Copilot,
            name: "GH",
            credential: "GITHUB_TOKEN",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[
                DraftField {
                    key: "query_mode",
                    value: DraftValue::Choice(1), // quota → mode/billing hidden
                },
                DraftField {
                    key: "mode",
                    value: DraftValue::Choice(1), // hidden but still forwarded
                },
                DraftField {
                    key: "billing",
                    value: DraftValue::Choice(1),
                },
            ],
            interval_text: "",
        };
        let account = build_account(&draft, &config_with(Vec::new()), None).unwrap();
        assert!(!account.params.contains_key("mode"));
        assert!(!account.params.contains_key("billing"));
    }

    // RF4: per-field error mapping. The desktop dialog uses
    // `EditError::field()` to point at the offending input; each
    // variant below asserts that the right field token is exposed
    // for the three named fields the UI cares most about (name,
    // interval, params). Document-wide rules (config-level
    // validation) keep `field = None` so the dialog surfaces the
    // error globally.
    #[test]
    fn build_account_blank_name_returns_named_field_error() {
        let draft = AccountDraft {
            provider: Provider::OpenAiApi,
            name: "   ",
            credential: "OPENAI_ADMIN_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let err = build_account(&draft, &config_with(Vec::new()), None).unwrap_err();
        assert_eq!(err.field(), Some("name"));
        assert_eq!(err.l10n().id, "edit_error.name_required");
    }

    #[test]
    fn build_account_bad_interval_returns_named_field_error() {
        let draft = AccountDraft {
            provider: Provider::OpenAiApi,
            name: "OpenAI",
            credential: "OPENAI_ADMIN_KEY",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "not-a-number",
        };
        let err = build_account(&draft, &config_with(Vec::new()), None).unwrap_err();
        assert_eq!(err.field(), Some("interval"));
        assert_eq!(err.l10n().id, "edit_error.interval_invalid");
    }

    #[test]
    fn save_invalid_config_returns_fieldless_validation() {
        // Document-wide rules (e.g. refresh_seconds out of range,
        // duplicate names after upsert) intentionally omit the
        // `field` token so the dialog uses the global error slot
        // rather than guessing a row.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file =
            crate::config_file::ConfigFile::load(Some(path), /*interactive*/ true).unwrap();
        let mut bad = file.config.clone();
        bad.refresh_seconds = 5;
        let err = file.save(bad).unwrap_err();
        assert!(err.is_validation(), "expected Validation, got {err:?}");
        assert_eq!(err.field(), None);
    }

    #[test]
    fn validate_name_does_not_echo_user_entered_value() {
        // The duplicate-name error must not echo the user-entered string;
        // a tampered draft (or a hostile front-end) cannot smuggle a
        // misleading payload into the surfaced message.
        let config = config_with(vec![env_account(
            "OpenAI",
            Provider::OpenAiApi,
            "OPENAI_ADMIN_KEY",
        )]);
        let err = validate_name("OpenAI", &config, None).unwrap_err();
        assert!(
            !err.text().contains("OpenAI"),
            "message echoed the name: {err}"
        );
        assert_eq!(err.text(), "Já existe uma conta com esse nome.");
        assert_eq!(err.id, "edit_error.duplicate_name");

        // Tabs and newlines must not slip past the generic message either.
        let err = validate_name("\tOpenAI ", &config, None).unwrap_err();
        assert!(
            !err.text().contains("OpenAI"),
            "message echoed the name: {err}"
        );
    }

    #[test]
    fn upsert_account_duplicate_error_does_not_echo_user_entered_value() {
        // A case-insensitive collision must not echo the user-supplied name.
        let config = config_with(vec![env_account(
            "OpenAI",
            Provider::OpenAiApi,
            "OPENAI_ADMIN_KEY",
        )]);
        let sneaky = Account {
            name: "oPeNaI".into(),
            provider: Provider::AnthropicApi,
            credential_env: Some("ANTHROPIC_ADMIN_KEY".into()),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        };
        let err = config.upsert_account(sneaky, None).unwrap_err();
        let msg = err.message();
        assert!(!msg.contains("oPeNaI"), "message echoed the name: {msg}");
        assert_eq!(msg, "Já existe uma conta com esse nome.");
    }

    // -------------------------------------------------------------------
    // RF3: `remove_account` + `purge_credential` behavioural tests.
    //
    // The tests cover the four documented outcomes:
    //   1. Normal purge: account removed from TOML, credential purged
    //      from the vault, no provider query fired.
    //   2. No-purge: account removed from TOML, vault untouched
    //      (`purge_credentials = false`).
    //   3. Shared-key guard: two accounts sharing the effective storage
    //      key; removing one keeps the credential because the survivor
    //      still depends on it.
    //   4. Vault failure: TOML save succeeds, vault delete fails,
    //      result is `PartialFailure` carrying the recovery primitive
    //      and the vault's error message.
    //   5. File-save failure: vault untouched, runtime state preserved.
    // -------------------------------------------------------------------

    use crate::config_file::ConfigFile;
    use crate::oauth::session::storage_key;
    use crate::oauth::{OAuthSession, OAuthToken, TokenStore};
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Memory-backed `TokenStore` that records every operation so the
    /// tests can assert the vault was touched the right number of times
    /// and with the right key.
    #[derive(Default)]
    struct MemoryStore {
        entries: Mutex<HashMap<String, OAuthToken>>,
        delete_calls: Mutex<Vec<String>>,
        fail_deletes: Mutex<u32>,
    }
    impl MemoryStore {
        fn fail_next_deletes(&self, n: u32) {
            *self.fail_deletes.lock() = n;
        }
    }
    impl TokenStore for MemoryStore {
        fn load(&self, id: &str) -> Result<Option<OAuthToken>, crate::oauth::OAuthError> {
            Ok(self.entries.lock().get(id).cloned())
        }
        fn save(&self, id: &str, token: &OAuthToken) -> Result<(), crate::oauth::OAuthError> {
            self.entries.lock().insert(id.into(), token.clone());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), crate::oauth::OAuthError> {
            self.delete_calls.lock().push(id.into());
            let mut remaining = self.fail_deletes.lock();
            if *remaining > 0 {
                *remaining -= 1;
                return Err(crate::oauth::OAuthError::Storage(
                    crate::text::Localized::fixed(
                        "oauth_error.test.keychain_unavailable",
                        "Keychain indisponível no teste",
                    ),
                ));
            }
            self.entries.lock().remove(id);
            Ok(())
        }
    }

    fn oauth_config_for(provider: Provider) -> OAuthConfig {
        let mut config = OAuthConfig::new(String::new());
        config.credential_id = "4be708d8-7073-4d01-85ba-3306cc3cf087".into();
        config.validate(provider).unwrap();
        config
    }

    fn oauth_account(name: &str, provider: Provider) -> Account {
        Account {
            name: name.into(),
            provider,
            credential_env: None,
            oauth: Some(oauth_config_for(provider)),
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    fn env_account_simple(name: &str, provider: Provider) -> Account {
        let mut account = env_account(name, provider, "FAKE_ENV");
        // env-var accounts have no OAuth config; nothing to purge.
        account.oauth = None;
        account
    }

    fn no_secrets() -> Arc<dyn crate::Credentials> {
        struct NoSecrets;
        impl crate::Credentials for NoSecrets {
            fn get(&self, _: &str) -> Option<String> {
                None
            }
        }
        Arc::new(NoSecrets)
    }

    async fn save_token(
        store: &MemoryStore,
        provider: Provider,
        oauth: &crate::oauth::OAuthConfig,
    ) -> String {
        let key = storage_key(provider, oauth).unwrap();
        // Insert directly into the memory store so the test does not need
        // to reach into the private fields of `AuthorizedLogin`.
        store.entries.lock().insert(
            key.clone(),
            OAuthToken {
                access_token: format!("access-for-{}", oauth.credential_id),
                refresh_token: None,
                expires_at: None,
                metadata: BTreeMap::new(),
            },
        );
        key
    }

    fn temp_config_file(config: Config) -> (tempfile::TempDir, ConfigFile) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();
        let mut file = ConfigFile::load(Some(path), false).unwrap();
        file.save(config).unwrap();
        (dir, file)
    }

    #[tokio::test]
    async fn remove_account_no_purge_keeps_credential_in_vault() {
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let config = config_with(vec![oauth_account("a", Provider::ChatGpt)]);
        let target_oauth = config.accounts[0].oauth.clone().unwrap();
        save_token(&store, Provider::ChatGpt, &target_oauth).await;
        let (_dir, mut file) = temp_config_file(config);
        let outcome = remove_account(&mut file, &session, 0, false).await.unwrap();
        match outcome {
            RemoveOutcome::CredentialKept { account, reason } => {
                assert_eq!(account.provider, Provider::ChatGpt);
                assert!(account.storage_key.is_some());
                assert_eq!(reason, KeptReason::Declined);
            }
            other => panic!("expected CredentialKept(Declined), got {other:?}"),
        }
        assert_eq!(file.config.accounts.len(), 0);
        assert!(
            store
                .entries
                .lock()
                .contains_key(&storage_key(Provider::ChatGpt, &target_oauth).unwrap()),
            "no-purge must leave the credential in the vault"
        );
        assert!(store.delete_calls.lock().is_empty());
    }

    #[tokio::test]
    async fn remove_account_normal_purge_returns_removed_from_vault() {
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let config = config_with(vec![oauth_account("only", Provider::ChatGpt)]);
        let target_oauth = config.accounts[0].oauth.clone().unwrap();
        let key = save_token(&store, Provider::ChatGpt, &target_oauth).await;
        let (_dir, mut file) = temp_config_file(config);
        let outcome = remove_account(&mut file, &session, 0, true).await.unwrap();
        match outcome {
            RemoveOutcome::RemovedFromVault(removed) => {
                assert_eq!(removed.provider, Provider::ChatGpt);
                assert_eq!(removed.storage_key.as_deref(), Some(key.as_str()));
            }
            other => panic!("expected RemovedFromVault, got {other:?}"),
        }
        assert_eq!(file.config.accounts.len(), 0);
        assert!(
            store.entries.lock().get(&key).is_none(),
            "purge must delete the vault entry"
        );
        assert_eq!(
            store.delete_calls.lock().as_slice(),
            std::slice::from_ref(&key)
        );
    }

    #[tokio::test]
    async fn remove_account_shared_key_returns_kept_shared_with_survivor() {
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        // Two accounts sharing the exact same OAuth config therefore
        // the same effective storage key.
        let config = config_with(vec![
            oauth_account("first", Provider::ChatGpt),
            oauth_account("second", Provider::ChatGpt),
        ]);
        let target_oauth = config.accounts[0].oauth.clone().unwrap();
        let key = save_token(&store, Provider::ChatGpt, &target_oauth).await;
        let (_dir, mut file) = temp_config_file(config);
        let outcome = remove_account(&mut file, &session, 0, true).await.unwrap();
        match outcome {
            RemoveOutcome::CredentialKept { account, reason } => {
                assert_eq!(account.provider, Provider::ChatGpt);
                assert_eq!(account.storage_key.as_deref(), Some(key.as_str()));
                assert_eq!(
                    reason,
                    KeptReason::SharedWith {
                        other_account: "second".into()
                    },
                );
            }
            other => panic!("expected CredentialKept(SharedWith), got {other:?}"),
        }
        assert_eq!(file.config.accounts.len(), 1);
        assert_eq!(file.config.accounts[0].name, "second");
        assert!(
            store.entries.lock().contains_key(&key),
            "shared-key guard must NOT delete the credential"
        );
        assert!(store.delete_calls.lock().is_empty());
    }

    #[tokio::test]
    async fn remove_account_vault_failure_returns_partial_failure() {
        let store = Arc::new(MemoryStore::default());
        store.fail_next_deletes(1);
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let config = config_with(vec![oauth_account("solo", Provider::ChatGpt)]);
        let target_oauth = config.accounts[0].oauth.clone().unwrap();
        let key = save_token(&store, Provider::ChatGpt, &target_oauth).await;
        let (_dir, mut file) = temp_config_file(config);
        let outcome = remove_account(&mut file, &session, 0, true).await.unwrap();
        match outcome {
            RemoveOutcome::PartialFailure {
                account,
                vault_error,
            } => {
                assert_eq!(account.provider, Provider::ChatGpt);
                assert_eq!(account.storage_key.as_deref(), Some(key.as_str()));
                assert!(
                    vault_error.text().contains("Keychain") || vault_error.text().contains("cofre"),
                    "vault_error should mention the failure: {vault_error}"
                );
            }
            other => panic!("expected PartialFailure, got {other:?}"),
        }
        assert_eq!(
            file.config.accounts.len(),
            0,
            "TOML must be saved before vault action"
        );
        assert!(
            store.entries.lock().contains_key(&key),
            "credential must remain in vault after PartialFailure"
        );
    }

    #[tokio::test]
    async fn remove_account_malformed_oauth_returns_invalid_oauth_pre_save() {
        // A non-empty OAuth config whose `credential_id` is not a
        // valid UUID makes the vault key un-derivable. The core must
        // refuse the operation up front (TOML NOT saved) rather than
        // silently claim `RemovedNoCredential` — the credential may
        // still exist in the vault under a key the core cannot
        // compute. We bypass `temp_config_file` (which would refuse to
        // load a config that fails `Config::validate`) by building a
        // valid `ConfigFile` from a clean default and then swapping in
        // the malformed account directly.
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();
        let mut file = ConfigFile::load(Some(path.clone()), false).unwrap();
        let mut account = oauth_account("broken", Provider::ChatGpt);
        account.oauth.as_mut().unwrap().credential_id = "not-a-uuid".into();
        // Replace the live config (bypassing validate by writing the
        // struct directly; this is exactly the shape a corrupted
        // TOML could produce at runtime).
        file.config = config_with(vec![account]);
        let result = remove_account(&mut file, &session, 0, true).await;
        match result {
            Err(RemoveError::InvalidOAuth(_)) => {}
            other => panic!("expected RemoveError::InvalidOAuth, got {other:?}"),
        }
        assert_eq!(
            file.config.accounts.len(),
            1,
            "TOML must be unchanged when OAuth is malformed"
        );
        assert_eq!(file.config.accounts[0].name, "broken");
        assert!(
            store.delete_calls.lock().is_empty(),
            "vault must not be touched on malformed OAuth"
        );
        // The on-disk file is also unchanged because the
        // pre-save check returned before `save` was called.
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("accounts = []"));
    }

    #[tokio::test]
    async fn remove_account_file_save_failure_keeps_vault_intact() {
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let config = config_with(vec![oauth_account("solo", Provider::ChatGpt)]);
        let target_oauth = config.accounts[0].oauth.clone().unwrap();
        let key = save_token(&store, Provider::ChatGpt, &target_oauth).await;
        // First load a real file so the `ConfigFile` is valid, then make
        // it un-writable by replacing the path with one under a
        // non-existent parent. `remove_account` will fail at the
        // `tempfile::NamedTempFile::new_in` step inside `save`.
        let (_dir, mut file) = temp_config_file(config);
        file.path = std::path::PathBuf::from("/nonexistent-parent/cuotai.toml");
        let result = remove_account(&mut file, &session, 0, true).await;
        assert!(matches!(result, Err(RemoveError::Save(_))));
        assert!(
            store.entries.lock().contains_key(&key),
            "file save failure must NOT touch the vault"
        );
        assert!(store.delete_calls.lock().is_empty());
    }

    #[tokio::test]
    async fn remove_account_invalid_index_returns_error_and_keeps_vault() {
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let config = config_with(vec![oauth_account("a", Provider::ChatGpt)]);
        let target_oauth = config.accounts[0].oauth.clone().unwrap();
        let key = save_token(&store, Provider::ChatGpt, &target_oauth).await;
        let (_dir, mut file) = temp_config_file(config);
        let result = remove_account(&mut file, &session, 99, true).await;
        assert!(matches!(result, Err(RemoveError::Index(99))));
        assert!(
            store.entries.lock().contains_key(&key),
            "invalid index must NOT touch the vault"
        );
        assert!(store.delete_calls.lock().is_empty());
    }

    #[tokio::test]
    async fn remove_account_env_only_returns_removed_no_credential() {
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let config = config_with(vec![env_account_simple("plain", Provider::OpenAiApi)]);
        let (_dir, mut file) = temp_config_file(config);
        let outcome = remove_account(&mut file, &session, 0, true).await.unwrap();
        assert!(matches!(outcome, RemoveOutcome::RemovedNoCredential(_)));
        assert_eq!(file.config.accounts.len(), 0);
        assert!(store.delete_calls.lock().is_empty());
    }

    #[tokio::test]
    async fn purge_credential_retries_after_partial_failure() {
        // Simulates the recovery flow described in the spec: a partial
        // failure leaves the credential in the vault; the caller uses
        // the `RemovedAccount` recovery primitive to retry the purge.
        let store = Arc::new(MemoryStore::default());
        store.fail_next_deletes(1);
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let config = config_with(vec![oauth_account("solo", Provider::ChatGpt)]);
        let target_oauth = config.accounts[0].oauth.clone().unwrap();
        let key = save_token(&store, Provider::ChatGpt, &target_oauth).await;
        let (_dir, mut file) = temp_config_file(config);
        let outcome = remove_account(&mut file, &session, 0, true).await.unwrap();
        let removed = match outcome {
            RemoveOutcome::PartialFailure { account, .. } => account,
            other => panic!("expected PartialFailure, got {other:?}"),
        };
        // Retry using the recovery primitive. The vault still holds the
        // entry and the second call should succeed (the live config
        // has zero accounts, so the shared-key guard lets the call
        // through).
        purge_credential(
            &session,
            &file.config,
            removed.provider,
            removed.oauth.as_ref().unwrap(),
        )
        .await
        .unwrap();
        assert!(store.entries.lock().get(&key).is_none());
    }

    #[tokio::test]
    async fn purge_credential_succeeds_after_shared_survivor_was_removed() {
        // First remove leaves a `CredentialKept` because a second
        // account shares the storage key. Drop the survivor; the next
        // `purge_credential` call against the now-unique config must
        // succeed and actually delete the vault entry.
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let config = config_with(vec![
            oauth_account("first", Provider::ChatGpt),
            oauth_account("second", Provider::ChatGpt),
        ]);
        let target_oauth = config.accounts[0].oauth.clone().unwrap();
        let key = save_token(&store, Provider::ChatGpt, &target_oauth).await;
        let (_dir, mut file) = temp_config_file(config);
        let outcome = remove_account(&mut file, &session, 0, true).await.unwrap();
        let removed = match outcome {
            RemoveOutcome::CredentialKept { account, reason } => {
                assert_eq!(
                    reason,
                    KeptReason::SharedWith {
                        other_account: "second".into()
                    },
                );
                account
            }
            other => panic!("expected CredentialKept(SharedWith), got {other:?}"),
        };
        assert!(
            store.entries.lock().contains_key(&key),
            "credential must remain after first remove"
        );
        // Simulate the user dropping the survivor (e.g. via another
        // `remove_account`). After that the live config no longer
        // shares the key, so the retry must succeed.
        let live = file.config.clone().without_account(0).unwrap();
        file.config = live.clone();
        purge_credential(
            &session,
            &live,
            removed.provider,
            removed.oauth.as_ref().unwrap(),
        )
        .await
        .unwrap();
        assert!(store.entries.lock().get(&key).is_none());
    }

    #[tokio::test]
    async fn purge_credential_refuses_when_new_shared_reference_appears() {
        // First remove leaves a `CredentialKept` because the survivor
        // shares the storage key. If a NEW account with the same
        // effective key appears (e.g. user re-imports), the retry must
        // still refuse — the guard runs against the live config.
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), no_secrets()).unwrap();
        let original_oauth = oauth_account("first", Provider::ChatGpt).oauth.unwrap();
        let config = config_with(vec![
            oauth_account("first", Provider::ChatGpt),
            oauth_account("second", Provider::ChatGpt),
        ]);
        let key = save_token(&store, Provider::ChatGpt, &original_oauth).await;
        let (_dir, mut file) = temp_config_file(config);
        let outcome = remove_account(&mut file, &session, 0, true).await.unwrap();
        let removed = match outcome {
            RemoveOutcome::CredentialKept { account, .. } => account,
            other => panic!("expected CredentialKept, got {other:?}"),
        };
        // The user removes "second" too, then adds a fresh account
        // that intentionally uses the same OAuth config (same
        // credential_id, client_id, options → same storage key).
        let live = file
            .config
            .clone()
            .without_account(0)
            .unwrap()
            .upsert_account(
                Account {
                    name: "replacement".into(),
                    provider: Provider::ChatGpt,
                    credential_env: None,
                    oauth: Some(original_oauth.clone()),
                    params: BTreeMap::new(),
                    refresh_seconds: None,
                },
                None,
            )
            .unwrap();
        file.config = live.clone();
        let result = purge_credential(
            &session,
            &live,
            removed.provider,
            removed.oauth.as_ref().unwrap(),
        )
        .await;
        let message = result.expect_err("retry must refuse while a new shared ref exists");
        assert_eq!(message.id, "remove_error.purge_shared");
        assert!(
            store.entries.lock().contains_key(&key),
            "credential must still be in the vault after refused retry"
        );
    }
    #[test]
    fn choice_applicability_contract_covers_every_provider_and_auth_mode() {
        let mut restricted = Vec::new();
        for provider in Provider::ALL {
            let mut auth_modes = vec![AUTH_MODE_CREDENTIAL.to_string()];
            auth_modes.extend(
                crate::oauth::login::login_modes(provider)
                    .modes
                    .into_iter()
                    .map(|mode| mode.id),
            );
            auth_modes.sort();
            auth_modes.dedup();

            for descriptor in field_descriptors(provider) {
                let FieldDescriptorKind::Choice(choice) = descriptor.kind else {
                    continue;
                };
                for (index, option) in choice.options.iter().enumerate() {
                    if option.auth_modes.is_some() {
                        restricted.push((provider, descriptor.key, option.value));
                    }
                    for auth_mode in &auth_modes {
                        let options = field_options(provider, descriptor.key, auth_mode)
                            .expect("every offered auth mode has a choice default");
                        let is_listed = options
                            .options
                            .iter()
                            .any(|candidate| candidate.index == index);
                        assert_eq!(
                            is_listed,
                            option.applies_to(auth_mode),
                            "{provider:?}.{}={} in {auth_mode}",
                            descriptor.key,
                            option.value
                        );
                        assert_eq!(
                            selected_choice_option(
                                provider,
                                descriptor,
                                &choice,
                                index,
                                auth_mode,
                            )
                            .is_ok(),
                            option.applies_to(auth_mode),
                            "draft validation diverged for {provider:?}.{}={} in {auth_mode}",
                            descriptor.key,
                            option.value
                        );

                        let mut draft_fields = Vec::new();
                        for candidate in field_descriptors(provider) {
                            match candidate.kind {
                                FieldDescriptorKind::Text if candidate.required => {
                                    let value = match candidate.key {
                                        "org" => "example",
                                        "team_id" => "team-1",
                                        _ => "value",
                                    };
                                    draft_fields.push(DraftField {
                                        key: candidate.key,
                                        value: DraftValue::Text(value.to_string()),
                                    });
                                }
                                FieldDescriptorKind::Choice(_) => {
                                    let selected = if candidate.key == descriptor.key {
                                        index
                                    } else {
                                        field_options(provider, candidate.key, auth_mode)
                                            .expect("choice default for offered auth mode")
                                            .default
                                            .index
                                    };
                                    draft_fields.push(DraftField {
                                        key: candidate.key,
                                        value: DraftValue::Choice(selected),
                                    });
                                }
                                FieldDescriptorKind::Text => {}
                            }
                        }
                        let oauth_mode = auth_mode == crate::oauth::login::MODE_OAUTH;
                        let draft = AccountDraft {
                            provider,
                            name: provider.label(),
                            credential: if oauth_mode {
                                ""
                            } else {
                                suggested_credential(provider)
                            },
                            auth_mode,
                            oauth_mode,
                            oauth_existing: None,
                            oauth_authorized: oauth_mode.then(|| OAuthConfig::new(String::new())),
                            oauth_login_params: BTreeMap::new(),
                            existing_params: None,
                            credential_params: None,
                            fields: &draft_fields,
                            interval_text: "",
                        };
                        let result = build_account(&draft, &config_with(Vec::new()), None);
                        if option.applies_to(auth_mode) {
                            result.unwrap_or_else(|error| {
                                panic!(
                                    "build_account rejected {provider:?}.{}={} in {auth_mode}: {error}",
                                    descriptor.key, option.value
                                )
                            });
                        } else {
                            let note = result
                                .expect_err("build_account must reject inapplicable option")
                                .l10n();
                            assert_eq!(
                                note.id, "edit_error.option_not_valid_for_auth_mode",
                                "{provider:?}.{}={} in {auth_mode}",
                                descriptor.key, option.value
                            );
                        }
                    }
                }
            }
        }
        assert!(
            restricted
                .iter()
                .all(|(provider, key, _)| { *provider == Provider::Kiro && *key == "auth_method" }),
            "only the adapter-evidenced Kiro auth_method restrictions are declared: {restricted:?}"
        );
        assert_eq!(restricted.len(), 7);
    }

    #[test]
    fn kiro_auth_method_contract_has_exact_mode_options_and_replacement_notice() {
        let credential = field_options(Provider::Kiro, "auth_method", AUTH_MODE_CREDENTIAL)
            .expect("Kiro environment choices");
        assert_eq!(
            credential
                .options
                .iter()
                .map(|option| option.value)
                .collect::<Vec<_>>(),
            ["google", "github", "import", "api_key", "external_idp"]
        );
        assert_eq!(credential.default.value, "google");

        let native = field_options(
            Provider::Kiro,
            "auth_method",
            crate::oauth::login::MODE_OAUTH,
        )
        .expect("Kiro native-login choices");
        assert_eq!(
            native
                .options
                .iter()
                .map(|option| option.value)
                .collect::<Vec<_>>(),
            ["builder-id", "idc"]
        );
        assert_eq!(native.default.value, "builder-id");

        let resolved = resolve_field_choice(
            Provider::Kiro,
            "auth_method",
            AUTH_MODE_CREDENTIAL,
            Some("builder-id"),
        )
        .expect("Kiro choice resolution");
        assert_eq!(resolved.option.value, "google");
        let notice = resolved
            .notice
            .expect("invalid prior choice emits a notice");
        assert_eq!(notice.id, "edit_notice.option_replaced_for_auth_mode");
        for key in ["field", "value", "replacement", "auth_mode", "reason"] {
            assert!(
                notice.params.contains_key(key),
                "missing notice param {key}"
            );
        }
    }

    #[test]
    fn kiro_environment_build_persists_google_and_rejects_native_only_choice() {
        let config = Config::default();
        let base = AccountDraft {
            provider: Provider::Kiro,
            name: "Kiro",
            credential: "KIRO_ACCESS_TOKEN",
            auth_mode: AUTH_MODE_CREDENTIAL,
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: BTreeMap::new(),
            existing_params: None,
            credential_params: None,
            fields: &[],
            interval_text: "",
        };
        let account = build_account(&base, &config, None).expect("new Kiro env account");
        assert_eq!(
            account.params.get("auth_method").map(String::as_str),
            Some("google")
        );

        let fields = [DraftField {
            key: "auth_method",
            value: DraftValue::Choice(0),
        }];
        let invalid = AccountDraft {
            fields: &fields,
            ..base
        };
        let error = build_account(&invalid, &config, None)
            .expect_err("builder-id must be rejected for an environment credential");
        assert_eq!(error.field(), Some("params"));
        let note = error.l10n();
        assert_eq!(note.id, "edit_error.option_not_valid_for_auth_mode");
        for key in ["field", "value", "auth_mode", "reason"] {
            assert!(note.params.contains_key(key), "missing error param {key}");
        }
    }
}
