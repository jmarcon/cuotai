//! Compatibility authorization flows ported from 9router f01fb909.
//! The provider controls whether its native-client flow is available to the user.
use super::reference_antigravity;
use super::reference_qwen;
use super::reference_xiaomi;
use super::{
    reference_browser, reference_device, reference_native, LoginPrompt, OAuthConfig, OAuthError,
    OAuthToken,
};
use crate::Provider;

pub(super) enum PendingLogin {
    Device(reference_device::PendingLogin),
    Browser(reference_browser::PendingLogin),
    Native(reference_native::PendingLogin),
    Xiaomi(reference_xiaomi::PendingLogin),
    Qwen(reference_qwen::PendingLogin),
    Antigravity(reference_antigravity::PendingLogin),
}
impl PendingLogin {
    pub(super) fn prompt(&self) -> &LoginPrompt {
        match self {
            Self::Device(p) => p.prompt(),
            Self::Browser(p) => p.prompt(),
            Self::Native(p) => p.prompt(),
            Self::Xiaomi(p) => p.prompt(),
            Self::Qwen(p) => p.prompt(),
            Self::Antigravity(p) => p.prompt(),
        }
    }
    pub(super) async fn finish(self) -> Result<OAuthToken, OAuthError> {
        match self {
            Self::Device(p) => p.finish().await,
            Self::Browser(p) => p.finish().await,
            Self::Native(p) => p.finish().await,
            Self::Xiaomi(p) => p.finish().await,
            Self::Qwen(p) => p.finish().await,
            Self::Antigravity(p) => p.finish().await,
        }
    }
    pub(super) async fn finish_with_codes(
        self,
        codes: tokio::sync::mpsc::Receiver<String>,
        errors: tokio::sync::mpsc::Sender<String>,
    ) -> Result<OAuthToken, OAuthError> {
        match self {
            Self::Xiaomi(pending) => pending.finish_with_codes(codes, errors).await,
            other => other.finish().await,
        }
    }
}

pub(super) async fn begin(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
) -> Result<PendingLogin, OAuthError> {
    match provider {
        Provider::Antigravity => reference_antigravity::begin(http, provider, config)
            .await
            .map(PendingLogin::Antigravity),
        Provider::Qwen => reference_qwen::begin(http, provider, config)
            .await
            .map(PendingLogin::Qwen),
        Provider::Copilot | Provider::Kimi | Provider::Kiro | Provider::Grok => {
            reference_device::begin(http, provider, config)
                .await
                .map(PendingLogin::Device)
        }
        Provider::Claude | Provider::ChatGpt | Provider::GeminiAi | Provider::Cline => {
            reference_browser::begin(http, provider, config)
                .await
                .map(PendingLogin::Browser)
        }
        Provider::Qoder | Provider::CodeBuddy | Provider::Zed => {
            reference_native::begin(http, provider, config)
                .await
                .map(PendingLogin::Native)
        }
        Provider::XiaomiMimo => reference_xiaomi::begin(http, provider, config)
            .await
            .map(PendingLogin::Xiaomi),
        _ => Err(OAuthError::native_no_integrated_flow()),
    }
}

pub(super) async fn refresh(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    token: &OAuthToken,
) -> Result<OAuthToken, OAuthError> {
    match provider {
        Provider::Antigravity => {
            reference_antigravity::refresh(http, provider, config, token).await
        }
        Provider::Qwen => reference_qwen::refresh(http, provider, config, token).await,
        Provider::Copilot | Provider::Kimi | Provider::Kiro | Provider::Grok => {
            reference_device::refresh(http, provider, config, token).await
        }
        Provider::Claude | Provider::ChatGpt | Provider::GeminiAi | Provider::Cline => {
            reference_browser::refresh(http, provider, config, token).await
        }
        Provider::Qoder | Provider::CodeBuddy | Provider::Zed => {
            reference_native::refresh(http, provider, config, token).await
        }
        Provider::XiaomiMimo => reference_xiaomi::refresh(http, provider, config, token).await,
        _ => Err(OAuthError::Expired),
    }
}
