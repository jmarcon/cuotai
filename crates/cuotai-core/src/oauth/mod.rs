//! Native OAuth authorization and secure, refreshable credentials. No UI dependencies.
mod bailian;
pub mod callback_page;
mod flows;
pub mod login;
pub(crate) mod session;
mod store;
pub mod vault_index;
pub use session::{AuthorizedLogin, OAuthLogin, OAuthSession};
pub mod aws;
pub mod cursor_import;
mod reference;
mod reference_antigravity;
mod reference_browser;
mod reference_device;
mod reference_native;
mod reference_qwen;
mod reference_xiaomi;

use crate::Provider;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use store::{SystemTokenStore, TokenStore};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_env: Option<String>,
    pub credential_id: String,
    /// Nonsecret flow options (region, auth_method, start_url). Tokens never belong here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("{0}")]
    /// RF8: every construction site passes a [`crate::text::Localized`]
    /// with a stable id (e.g. `oauth_error.session.client_secret_env`).
    /// `Display` renders the embedded Portuguese fallback;
    /// `l10n()` forwards the inner `Localized` unchanged so the
    /// catalog translates the message structure.
    Configuration(crate::text::Localized),
    #[error("Falha de rede durante autenticação OAuth.")]
    Network,
    #[error("Autorização recusada ou revogada. Inicie o login novamente.")]
    Denied,
    #[error("Autorização expirada. Inicie o login novamente.")]
    Expired,
    #[error("Resposta OAuth inválida; nenhuma credencial foi aceita.")]
    InvalidResponse,
    #[error("{0}")]
    Storage(crate::text::Localized),
}

/// Send an OAuth request after checking that its URL may carry secrets
/// ([`crate::transport::is_secure`]): HTTPS, or HTTP to a loopback host.
/// A refused URL fails closed before anything leaves the machine and is never
/// reported in the error.
pub(crate) async fn send_secure(
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, OAuthError> {
    let (client, request) = request.build_split();
    let request = request.map_err(|_| OAuthError::Network)?;
    if !crate::transport::is_secure(request.url()) {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.insecure_endpoint",
            "Endpoint OAuth recusado: segredos só trafegam por HTTPS. Nada foi enviado.",
        )));
    }
    client
        .execute(request)
        .await
        .map_err(|_| OAuthError::Network)
}

/// Backwards-compat: callers that still have a plain message build
/// `OAuthError::Configuration` from a `String`/`&str`. The message becomes
/// the `{detail}` parameter of the canonical `query.oauth.configuration`
/// template, so the catalog renders the wrapper sentence in its own
/// language and the detail (already sanitized: never a secret, URL or
/// body) flows through the placeholder. Sites needing a richer id should
/// construct a [`crate::text::Localized`] and use [`OAuthError::Configuration`].
impl From<String> for OAuthError {
    fn from(message: String) -> Self {
        let fallback = format!("Configuração OAuth: {message}");
        OAuthError::Configuration(crate::text::Localized::with_param(
            "query.oauth.configuration",
            "detail",
            message,
            fallback,
        ))
    }
}
impl From<&str> for OAuthError {
    fn from(message: &str) -> Self {
        OAuthError::from(message.to_string())
    }
}

// ---------------------------------------------------------------------------
// RF8: shared message text for errors emitted from multiple modules with an
// identical meaning. Each group shares ONE stable id (so the catalog carries
// a single translation) and ONE Portuguese fallback const (so the wording
// lives in exactly one place). The constructor helpers below keep call sites
// short and guarantee id/fallback stay in sync.
// ---------------------------------------------------------------------------
pub(crate) const MSG_OS_RNG_UNAVAILABLE: &str =
    "gerador de números aleatórios do sistema operacional indisponível";
pub(crate) const MSG_GOOGLE_CLIENT_SECRET_ENV_REQUIRED: &str =
    "Um client_id próprio do Google exige client_secret_env com o segredo correspondente.";
pub(crate) const MSG_GOOGLE_CLIENT_SECRET_ENV_UNSET: &str =
    "A variável de ambiente do client secret do Google não está definida.";
pub(crate) const MSG_GOOGLE_CLIENT_SECRET_ENV_EMPTY: &str =
    "A variável de ambiente do client secret do Google está vazia.";
pub(crate) const MSG_DEVICE_VERIFICATION_URL_UNTRUSTED: &str =
    "O provedor retornou um endereço de verificação não confiável.";
pub(crate) const MSG_BAILIAN_CONSOLE_SITE_BLANK: &str =
    "Site do console em branco; informe `international`.";
pub(crate) const MSG_NATIVE_INVALID_REGION: &str = "Região inválida; use \"intl\" ou \"cn\".";
pub(crate) const MSG_NATIVE_NO_INTEGRATED_FLOW: &str =
    "Este provedor não tem fluxo de login nativo integrado.";

impl OAuthError {
    /// OS random-number generator unavailable (PKCE/state generation).
    pub(crate) fn os_rng_unavailable() -> Self {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.os.rng_unavailable",
            MSG_OS_RNG_UNAVAILABLE,
        ))
    }
    /// A custom Google client_id requires `client_secret_env`.
    pub(crate) fn google_client_secret_env_required() -> Self {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.google.client_secret_env_required",
            MSG_GOOGLE_CLIENT_SECRET_ENV_REQUIRED,
        ))
    }
    /// The Google client-secret env var is not set.
    pub(crate) fn google_client_secret_env_unset() -> Self {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.google.client_secret_env_unset",
            MSG_GOOGLE_CLIENT_SECRET_ENV_UNSET,
        ))
    }
    /// The Google client-secret env var is set but empty.
    pub(crate) fn google_client_secret_env_empty() -> Self {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.google.client_secret_env_empty",
            MSG_GOOGLE_CLIENT_SECRET_ENV_EMPTY,
        ))
    }
    /// Device-flow verification URL returned by the provider is untrusted.
    pub(crate) fn device_verification_url_untrusted() -> Self {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.device.verification_url_untrusted",
            MSG_DEVICE_VERIFICATION_URL_UNTRUSTED,
        ))
    }
    /// Bailian console site left blank.
    pub(crate) fn bailian_console_site_blank() -> Self {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.bailian.console_site_blank",
            MSG_BAILIAN_CONSOLE_SITE_BLANK,
        ))
    }
    /// Native-flow region is neither `intl` nor `cn`.
    pub(crate) fn native_invalid_region() -> Self {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.native.invalid_region",
            MSG_NATIVE_INVALID_REGION,
        ))
    }
    /// Provider has no integrated native login flow.
    pub(crate) fn native_no_integrated_flow() -> Self {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.native.no_integrated_flow",
            MSG_NATIVE_NO_INTEGRATED_FLOW,
        ))
    }
}

impl OAuthError {
    /// RF8: stable localized form of the error. Frontends resolve the id
    /// against their catalog; the embedded Portuguese text acts as
    /// fallback. `Configuration` and `Storage` carry the offending detail
    /// in the `{detail}` parameter so translation drops the value through
    /// `{detail}` rather than concatenating Portuguese into English.
    pub fn l10n(&self) -> crate::text::Localized {
        use crate::text::Localized;
        match self {
            // RF8: Configuration/Storage carry a `Localized` whose stable
            // id the catalog already resolves; we forward it as-is so the
            // catalog translates the message structure (no Portuguese
            // fragment inside English text).
            OAuthError::Configuration(note) | OAuthError::Storage(note) => note.clone(),
            OAuthError::Network => Localized::fixed(
                "query.oauth.network",
                "Falha de rede durante autenticação OAuth.",
            ),
            OAuthError::Denied => Localized::fixed(
                "query.oauth.denied",
                "Autorização recusada ou revogada. Inicie o login novamente.",
            ),
            OAuthError::Expired => Localized::fixed(
                "query.oauth.expired",
                "Autorização expirada. Inicie o login novamente.",
            ),
            OAuthError::InvalidResponse => Localized::fixed(
                "query.oauth.invalid_response",
                "Resposta OAuth inválida; nenhuma credencial foi aceita.",
            ),
        }
    }

    /// Specialized form for the `purge_credential` path: rewrites the
    /// inner Configuration/Storage id into `remove_error.purge_vault.*`
    /// so the desktop doesn't render an OAuth-flavoured id in a "purge"
    /// context. The inner params (e.g. `{detail}`) are preserved.
    pub fn l10n_for_purge(&self) -> crate::text::Localized {
        use crate::text::Localized;
        match self {
            OAuthError::Configuration(note) => Localized::with_params(
                "remove_error.purge_vault.configuration",
                note.params.clone(),
                note.fallback.clone().into_owned(),
            ),
            OAuthError::Storage(note) => Localized::with_params(
                "remove_error.purge_vault.storage",
                note.params.clone(),
                note.fallback.clone().into_owned(),
            ),
            OAuthError::Network => Localized::fixed(
                "remove_error.purge_vault.network",
                "Falha de rede ao apagar o cofre.",
            ),
            OAuthError::Denied => Localized::fixed(
                "remove_error.purge_vault.denied",
                "Operação recusada pelo cofre do sistema.",
            ),
            OAuthError::Expired => Localized::fixed(
                "remove_error.purge_vault.expired",
                "Sessão expirada; tente novamente.",
            ),
            OAuthError::InvalidResponse => Localized::fixed(
                "remove_error.purge_vault.invalid_response",
                "Resposta inválida do cofre do sistema.",
            ),
        }
    }
}

/// Secret payload: intentionally no Debug implementation. Never write to TOML/logs.
#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    /// Refresh credentials and provider context, stored ONLY in the secure store.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct LoginPrompt {
    pub url: String,
    pub user_code: Option<String>,
}

pub fn supports_login(provider: Provider) -> bool {
    matches!(
        provider,
        Provider::GeminiApi
            | Provider::VertexAi
            | Provider::AzureOpenAi
            | Provider::Claude
            | Provider::ChatGpt
            | Provider::Copilot
            | Provider::Kimi
            | Provider::Kiro
            | Provider::Grok
            | Provider::GeminiAi
            | Provider::Cline
            | Provider::Zed
            | Provider::Qoder
            | Provider::CodeBuddy
            | Provider::XiaomiMimo
            | Provider::Cursor
            | Provider::Qwen
            | Provider::Antigravity
            | Provider::OpenRouter
    )
}

/// Registered-client configuration is needed only for the existing Cloud APIs.
/// Native-compatible logins use the provider flow observed in 9router.
pub fn requires_client_registration(provider: Provider) -> bool {
    matches!(
        provider,
        Provider::GeminiApi | Provider::VertexAi | Provider::AzureOpenAi
    )
}

impl OAuthConfig {
    pub fn new(client_id: String) -> Self {
        Self {
            client_id,
            tenant: None,
            client_secret_env: None,
            credential_id: uuid::Uuid::new_v4().to_string(),
            options: BTreeMap::new(),
        }
    }

    pub fn validate(&self, provider: Provider) -> Result<(), OAuthError> {
        if !supports_login(provider) {
            return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                "oauth_error.config.provider_no_integrated_oauth",
                "Este provedor não oferece login OAuth integrado para esta consulta.",
            )));
        }
        if (requires_client_registration(provider) && self.client_id.trim().is_empty())
            || self.client_id.len() > 256
            || self.client_id.chars().any(char::is_control)
        {
            return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                "oauth_error.config.client_id_required",
                "Informe o client_id do seu aplicativo registrado.",
            )));
        }
        if uuid::Uuid::parse_str(&self.credential_id).is_err() {
            return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                "oauth_error.config.invalid_local_credential_id",
                "Identificador local de credencial inválido; refaça o login.",
            )));
        }
        if let Some(tenant) = &self.tenant {
            if tenant.is_empty()
                || !tenant
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
            {
                return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                    "oauth_error.config.invalid_microsoft_tenant",
                    "Tenant Microsoft inválido.",
                )));
            }
        }
        for (key, value) in &self.options {
            if !(matches!(key.as_str(), "region" | "auth_method" | "start_url")
                || (provider == Provider::Cursor && key == "database_path"))
                || value.len() > 2048
                || value.chars().any(char::is_control)
            {
                return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                    "oauth_error.config.credentials_not_in_options",
                    "Opção de login inválida; credenciais não devem ser armazenadas nas opções.",
                )));
            }
        }
        if let Some(name) = &self.client_secret_env {
            let mut chars = name.chars();
            if !chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                    "oauth_error.config.client_secret_name_only",
                    "Informe apenas o nome da variável do client secret, nunca seu valor.",
                )));
            }
        }
        Ok(())
    }
}

/// Public/nonsecret metadata needed by quota adapters; NEVER project token secrets.
pub fn account_params(
    provider: Provider,
    config: &OAuthConfig,
    metadata: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let allowed: &[&str] = match provider {
        Provider::ChatGpt => &["account_id"],
        Provider::Kiro => &["auth_method", "profile_arn", "region"],
        Provider::CodeBuddy | Provider::Qoder => &["region"],
        Provider::Zed => &["user_id", "system_id"],
        Provider::Cursor => &["device_id", "user_id"],
        Provider::Kimi => &["device_id"],
        Provider::GeminiAi | Provider::Antigravity => &["project_id"],
        Provider::Grok => &["email", "user_id"],
        Provider::XiaomiMimo => &["region", "auth_method"],
        _ => &[],
    };
    for key in allowed {
        if let Some(value) = metadata.get(*key).or_else(|| config.options.get(*key)) {
            if !value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control) {
                out.insert((*key).into(), value.clone());
            }
        }
    }
    match provider {
        Provider::Qwen => {
            out.insert("product".into(), "coding".into());
        }
        Provider::Cursor => {
            out.insert("query_mode".into(), "personal".into());
        }
        Provider::Copilot => {
            out.insert("query_mode".into(), "quota".into());
        }
        Provider::Kimi => {
            out.insert("product".into(), "coding".into());
        }
        Provider::GeminiAi | Provider::Grok => {
            out.insert("product".into(), "cli".into());
        }
        Provider::Qoder => {
            out.insert("credential_type".into(), "bearer".into());
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod send_secure_tests {
    use super::*;

    #[tokio::test]
    async fn refuses_plain_http_to_a_remote_host_before_sending() {
        let client = reqwest::Client::new();
        for (scheme, host) in [("http", "oauth.example.invalid"), ("http", "192.168.1.10")] {
            let url = format!("{scheme}://{host}/token");
            let request = client.post(&url).form(&[
                ("client_secret", "never-sent"),
                ("grant_type", "refresh_token"),
            ]);
            match send_secure(request).await {
                Err(OAuthError::Configuration(text)) => {
                    assert_eq!(&*text.id, "oauth_error.insecure_endpoint", "{url}");
                }
                other => panic!("expected the insecure-endpoint refusal for {url}, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn allows_plain_http_to_loopback_servers() {
        use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let response = send_secure(reqwest::Client::new().post(server.uri()))
            .await
            .unwrap();
        assert!(response.status().is_success());
    }
}
