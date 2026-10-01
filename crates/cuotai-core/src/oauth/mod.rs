//! Native OAuth authorization and secure, refreshable credentials. No UI dependencies.
mod flows;
mod session;
mod store;
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
    #[error("Configuração OAuth: {0}")]
    Configuration(String),
    #[error("Falha de rede durante autenticação OAuth.")]
    Network,
    #[error("Autorização recusada ou revogada. Inicie o login novamente.")]
    Denied,
    #[error("Autorização expirada. Inicie o login novamente.")]
    Expired,
    #[error("Resposta OAuth inválida; nenhuma credencial foi aceita.")]
    InvalidResponse,
    #[error("Armazenamento seguro: {0}")]
    Storage(String),
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
            return Err(OAuthError::Configuration(
                "Este provedor não oferece login OAuth integrado para esta consulta.".into(),
            ));
        }
        if (requires_client_registration(provider) && self.client_id.trim().is_empty())
            || self.client_id.len() > 256
            || self.client_id.chars().any(char::is_control)
        {
            return Err(OAuthError::Configuration(
                "Informe o client_id do seu aplicativo registrado.".into(),
            ));
        }
        if uuid::Uuid::parse_str(&self.credential_id).is_err() {
            return Err(OAuthError::Configuration(
                "Identificador local de credencial inválido; refaça o login.".into(),
            ));
        }
        if let Some(tenant) = &self.tenant {
            if tenant.is_empty()
                || !tenant
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
            {
                return Err(OAuthError::Configuration(
                    "Tenant Microsoft inválido.".into(),
                ));
            }
        }
        for (key, value) in &self.options {
            if !(matches!(key.as_str(), "region" | "auth_method" | "start_url")
                || (provider == Provider::Cursor && key == "database_path"))
                || value.len() > 2048
                || value.chars().any(char::is_control)
            {
                return Err(OAuthError::Configuration(
                    "Opção de login inválida; credenciais não devem ser armazenadas nas opções."
                        .into(),
                ));
            }
        }
        if let Some(name) = &self.client_secret_env {
            let mut chars = name.chars();
            if !chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(OAuthError::Configuration(
                    "Informe apenas o nome da variável do client secret, nunca seu valor.".into(),
                ));
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
