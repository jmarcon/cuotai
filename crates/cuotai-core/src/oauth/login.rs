//! RF2 — UI-independent login driver shared by every frontend.
//!
//! The core owns all login policy and orchestration: which flow each provider
//! uses (browser loopback, device code, MiMo manual code, Cursor import, AWS
//! SSO via CLI, Bailian console via `bl`), the event sequence, cancellation,
//! and the staged credential+TOML commit with typed partial failure.
//! Frontends only render [`LoginEvent`]s and forward user input
//! ([`LoginDriver::submit_code`]); they never branch on `Provider` to decide
//! how to authenticate (RF2.6).
//!
//! Secret hygiene (RF2.5): events, errors and serialized types never carry
//! tokens, callback URLs or authorization codes from the browser redirect.
//! The device-flow `user_code` is public by design (it is typed by the user
//! on the provider's page). The authorized token lives only in
//! [`CompletedLogin`], which is never serialized nor `Debug`.

use super::{
    aws, bailian, requires_client_registration, supports_login, AuthorizedLogin, OAuthConfig,
    OAuthError, OAuthSession,
};
use crate::account_edit;
use crate::config_file::ConfigFile;
use crate::{Config, Provider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

// ---------------------------------------------------------------------------
// RF2.1/RF2.5 — typed, serializable, secret-free events
// ---------------------------------------------------------------------------

/// Events emitted by a running [`LoginDriver`]. Serializable so GUI frontends
/// can forward them across IPC; every payload is nonsecret by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginEvent {
    /// The provider is ready for the user to authorize. `url` is the
    /// authorize/verification page (never a callback URL); `user_code` is the
    /// public device-flow code when the flow has one.
    Prompt {
        url: String,
        user_code: Option<String>,
        instructions: String,
    },
    /// Free-form progress line (sanitized; CLI output is filtered to known
    /// device-prompt lines before it becomes a `Progress` event).
    Progress { text: String },
    /// The flow accepts a manually pasted authorization code now (MiMo).
    WaitingCode,
    /// A submitted code was rejected; the user may paste another one.
    CodeRejected { message: String },
    /// RF2.9: the browser could not be opened; the frontend must display the
    /// URL (already shown in `Prompt`) for manual copy.
    BrowserUnavailable { url: String },
    /// Flow finished. `params` carries only public, whitelisted quota
    /// metadata ([`CompletedLogin::params`]); the token stays in the driver.
    Finished { params: BTreeMap<String, String> },
    /// Flow failed with a sanitized message (no input echo, no secrets).
    Failed { message: String },
}

// ---------------------------------------------------------------------------
// RF2.7 — per-provider login mode discovery
// ---------------------------------------------------------------------------

/// Stable mode identifiers sent back in [`LoginRequest::mode`].
pub const MODE_OAUTH: &str = "oauth";
pub const MODE_AWS_SSO: &str = "aws_sso";
pub const MODE_BAILIAN_CONSOLE: &str = "bailian_console";

/// Owned, serializable mirror of [`account_edit::NativeOptionDescriptor`]:
/// a nonsecret flow option (region, auth_method, start_url) or a CLI argument
/// (AWS profile, Bailian console site/profile).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginOption {
    pub key: String,
    pub label: String,
    pub help: String,
    /// Closed value list; `None` means free text.
    pub choices: Option<Vec<(String, String)>>,
}

impl LoginOption {
    fn text(key: &str, label: &str, help: &str) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            help: help.to_string(),
            choices: None,
        }
    }

    fn from_native(descriptor: &account_edit::NativeOptionDescriptor) -> Self {
        let choices = match descriptor.kind {
            account_edit::FieldDescriptorKind::Text => None,
            account_edit::FieldDescriptorKind::Choice(choice) => Some(
                choice
                    .options
                    .iter()
                    .map(|(value, label)| (value.to_string(), label.to_string()))
                    .collect(),
            ),
        };
        Self {
            key: descriptor.key.to_string(),
            label: descriptor.label.to_string(),
            help: descriptor.help.to_string(),
            choices,
        }
    }
}

/// One interactive login mode offered (or explicitly not offered) for a
/// provider. Frontends render the list verbatim and send the chosen `id`
/// back in [`LoginRequest::mode`] — no provider branching (RF2.6/RF2.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginMode {
    pub id: String,
    pub label: String,
    pub help: String,
    /// True when the flow needs a user-registered application (`client_id`,
    /// optional `tenant`/`client_secret_env`); those fields are inputs of
    /// [`LoginRequest`] itself and never appear in [`LoginMode::options`].
    pub requires_client_registration: bool,
    /// Nonsecret flow options accepted in [`LoginRequest::options`].
    pub options: Vec<LoginOption>,
    /// Set when the mode exists but cannot start now (reason shown as-is).
    pub unavailable_reason: Option<String>,
}

/// Result of [`login_modes`]: the interactive modes a frontend may offer for
/// `provider`, plus the reason when no login exists at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderLogin {
    pub modes: Vec<LoginMode>,
    pub unsupported_reason: Option<String>,
}

/// RF2.7: every interactive login mode for `provider`, with descriptors.
/// The flow choice behind [`MODE_OAUTH`] (browser loopback, device code,
/// MiMo manual code, Cursor import) stays in the core.
pub fn login_modes(provider: Provider) -> ProviderLogin {
    let mut modes = Vec::new();
    match provider {
        Provider::AwsBedrock => modes.push(LoginMode {
            id: MODE_AWS_SSO.to_string(),
            label: "AWS SSO (AWS CLI)".to_string(),
            help: "Executa `aws sso login --use-device-code` no perfil informado; o token fica no cache da própria AWS CLI, fora do cofre do Cuotai.".to_string(),
            requires_client_registration: false,
            options: vec![LoginOption::text(
                "profile",
                "Perfil AWS",
                "Perfil SSO configurado previamente via `aws configure sso`.",
            )],
            unavailable_reason: None,
        }),
        Provider::AlibabaTokenPlan => modes.push(LoginMode {
            id: MODE_BAILIAN_CONSOLE.to_string(),
            label: "Console Bailian (`bl`)".to_string(),
            help: "Executa `bl auth login --console` em um perfil dedicado do `bl`; o login fica no cofre local do `bl`, fora do cofre do Cuotai.".to_string(),
            requires_client_registration: false,
            options: vec![
                LoginOption {
                    key: "console_site".to_string(),
                    label: "Site do console".to_string(),
                    help: "Site do Token Plan atendido pelo `bl --console-site`.".to_string(),
                    choices: Some(vec![(
                        "international".to_string(),
                        "Internacional".to_string(),
                    )]),
                },
                LoginOption::text(
                    "profile",
                    "Perfil do `bl`",
                    "Perfil dedicado (`--config`) onde o `bl` grava o login.",
                ),
            ],
            unavailable_reason: None,
        }),
        _ if supports_login(provider) => {
            let registration = requires_client_registration(provider);
            let help = if registration {
                account_edit::oauth_registration_help(provider).to_string()
            } else {
                "Login nativo no navegador do sistema; nenhum registro de aplicativo é necessário.".to_string()
            };
            modes.push(LoginMode {
                id: MODE_OAUTH.to_string(),
                label: "Login no navegador".to_string(),
                help,
                requires_client_registration: registration,
                options: account_edit::native_option_descriptors(provider)
                    .iter()
                    .map(LoginOption::from_native)
                    .collect(),
                unavailable_reason: None,
            });
        }
        _ => {}
    }
    ProviderLogin {
        modes,
        unsupported_reason: account_edit::oauth_unsupported_reason(provider).map(|l| l.to_string()),
    }
}

// ---------------------------------------------------------------------------
// RF2.1/RF2.6 — login driver
// ---------------------------------------------------------------------------

/// What the user asked to start. `mode` must be one of the ids returned by
/// [`login_modes`] for the provider; anything else is rejected up front.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginRequest {
    pub mode: String,
    /// Nonsecret flow options: native OAuth options (region/auth_method/
    /// start_url), `profile` for AWS SSO, `console_site`+`profile` for the
    /// Bailian console. Secrets never belong here.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret_env: Option<String>,
    #[serde(default)]
    pub tenant: Option<String>,
    /// RF2.8: `Some(existing_id)` reautentica reutilizando a mesma chave do
    /// cofre (sem credencial órfã); `None` gera um identificador novo.
    #[serde(default)]
    pub credential_id: Option<String>,
}

impl LoginRequest {
    /// Build an OAuth request from an existing [`OAuthConfig`] (used by
    /// frontends whose forms already produce one).
    pub fn oauth(config: &OAuthConfig) -> Self {
        Self {
            mode: MODE_OAUTH.to_string(),
            options: config.options.clone(),
            client_id: Some(config.client_id.clone()),
            client_secret_env: config.client_secret_env.clone(),
            tenant: config.tenant.clone(),
            credential_id: Some(config.credential_id.clone()),
        }
    }

    /// AWS SSO device login for a configured CLI profile.
    pub fn aws_sso(profile: String) -> Self {
        let mut options = BTreeMap::new();
        options.insert("profile".to_string(), profile);
        Self {
            mode: MODE_AWS_SSO.to_string(),
            options,
            ..Self::default()
        }
    }

    /// Bailian console login in a dedicated `bl` profile.
    pub fn bailian_console(console_site: String, profile: String) -> Self {
        let mut options = BTreeMap::new();
        options.insert("console_site".to_string(), console_site);
        options.insert("profile".to_string(), profile);
        Self {
            mode: MODE_BAILIAN_CONSOLE.to_string(),
            options,
            ..Self::default()
        }
    }
}

/// Why a pasted code was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitCodeError {
    /// This flow has no manual-code step.
    Unsupported,
    /// A previous submission is still being validated; the code is returned
    /// so the frontend can keep it in the input field.
    Full(String),
    /// The flow already finished or was cancelled.
    Closed,
}

enum FlowDone {
    Authorized(CompletedLogin),
    AwsSso,
    BailianConsole,
}

/// A running login flow. Owns the background task; dropping or
/// [`LoginDriver::cancel`]ling it stops the flow immediately. Cancellation
/// before [`LoginEvent::Finished`] never touches the vault (RF2.4).
pub struct LoginDriver {
    events: mpsc::Receiver<LoginEvent>,
    code_tx: Option<mpsc::Sender<String>>,
    job: Option<JoinHandle<Result<FlowDone, OAuthError>>>,
}

impl Drop for LoginDriver {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.abort();
        }
    }
}

/// The terminal result of a login flow, after [`LoginDriver::finish`].
pub enum LoginResult {
    /// Tokens authorized and held in memory; commit via [`commit_login`] or
    /// [`begin_credential_commit`]. Nothing is in the vault yet.
    Authorized(CompletedLogin),
    /// `aws sso login` finished; the token lives in the AWS CLI cache.
    AwsSso,
    /// `bl auth login --console` finished; the login lives in the `bl` vault.
    BailianConsole,
}

impl LoginDriver {
    /// Wait for the next event; `None` once the flow task ended.
    pub async fn next_event(&mut self) -> Option<LoginEvent> {
        self.events.recv().await
    }

    /// Nonblocking event poll (event-loop frontends).
    pub fn try_next_event(&mut self) -> Option<LoginEvent> {
        self.events.try_recv().ok()
    }

    /// True while the flow accepts a pasted code (MiMo manual step).
    pub fn accepts_code(&self) -> bool {
        self.code_tx.is_some()
    }

    /// Forward a pasted authorization code to the flow.
    pub fn submit_code(&self, code: String) -> Result<(), SubmitCodeError> {
        let Some(sender) = &self.code_tx else {
            return Err(SubmitCodeError::Unsupported);
        };
        match sender.try_send(code) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(code)) => Err(SubmitCodeError::Full(code)),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(SubmitCodeError::Closed),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.job.as_ref().is_some_and(JoinHandle::is_finished)
    }

    /// Await the terminal result. Call after [`LoginEvent::Finished`] or
    /// [`LoginEvent::Failed`] (or poll [`LoginDriver::is_finished`]); the
    /// future resolves immediately once the flow task has ended.
    pub async fn finish(&mut self) -> Result<LoginResult, OAuthError> {
        let job = self.job.take().ok_or(OAuthError::InvalidResponse)?;
        let done = job.await.map_err(|_| OAuthError::Network)??;
        Ok(match done {
            FlowDone::Authorized(login) => LoginResult::Authorized(login),
            FlowDone::AwsSso => LoginResult::AwsSso,
            FlowDone::BailianConsole => LoginResult::BailianConsole,
        })
    }

    /// Abort the flow. Equivalent to dropping the driver; nothing is written
    /// to the vault because commit is always a separate, explicit step.
    pub fn cancel(mut self) {
        if let Some(job) = self.job.take() {
            job.abort();
        }
    }
}

/// Start a login flow. Fails synchronously only when `request.mode` is not
/// offered for `provider`; all flow failures are delivered as
/// [`LoginEvent::Failed`] plus the [`LoginDriver::finish`] error.
pub fn start_login(
    session: Arc<OAuthSession>,
    provider: Provider,
    request: LoginRequest,
) -> Result<LoginDriver, OAuthError> {
    let offered = login_modes(provider);
    let mode = offered
        .modes
        .iter()
        .find(|mode| mode.id == request.mode)
        .ok_or_else(|| {
            OAuthError::Configuration("Modo de login não oferecido para este provedor.".into())
        })?;
    if let Some(reason) = &mode.unavailable_reason {
        return Err(OAuthError::Configuration(reason.clone()));
    }
    match mode.id.as_str() {
        MODE_OAUTH => Ok(start_oauth(session, provider, request)),
        MODE_AWS_SSO => Ok(start_aws_sso(request.options.get("profile").cloned())),
        MODE_BAILIAN_CONSOLE => Ok(start_bailian_console(
            request.options.get("console_site").cloned(),
            request.options.get("profile").cloned(),
        )),
        _ => Err(OAuthError::Configuration(
            "Modo de login desconhecido.".into(),
        )),
    }
}

fn oauth_config_from(request: LoginRequest) -> OAuthConfig {
    OAuthConfig {
        client_id: request.client_id.unwrap_or_default(),
        tenant: request.tenant,
        client_secret_env: request.client_secret_env,
        credential_id: request
            .credential_id
            .filter(|id| uuid::Uuid::parse_str(id).is_ok())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        options: request.options,
    }
}

/// Best-effort event delivery; a closed channel means the frontend went away
/// and the flow result no longer matters.
async fn send(events: &mpsc::Sender<LoginEvent>, event: LoginEvent) {
    let _ = events.send(event).await;
}

fn start_oauth(
    session: Arc<OAuthSession>,
    provider: Provider,
    request: LoginRequest,
) -> LoginDriver {
    let config = oauth_config_from(request);
    // The manual-code step exists only for the MiMo native flow; every other
    // provider completes via loopback callback or device polling (RF2.6: the
    // decision lives here, not in the frontends).
    let manual_code = provider == Provider::XiaomiMimo;
    let (event_tx, events) = mpsc::channel(16);
    let (code_tx, codes) = mpsc::channel::<String>(1);
    let job = tokio::spawn(run_oauth(
        session,
        provider,
        config,
        manual_code.then_some(codes),
        event_tx,
    ));
    LoginDriver {
        events,
        code_tx: manual_code.then_some(code_tx),
        job: Some(job),
    }
}

async fn run_oauth(
    session: Arc<OAuthSession>,
    provider: Provider,
    config: OAuthConfig,
    codes: Option<mpsc::Receiver<String>>,
    events: mpsc::Sender<LoginEvent>,
) -> Result<FlowDone, OAuthError> {
    match run_oauth_inner(&session, provider, &config, codes, &events).await {
        Ok(login) => {
            let params = login.params.clone();
            send(&events, LoginEvent::Finished { params }).await;
            Ok(FlowDone::Authorized(login))
        }
        Err(error) => {
            send(
                &events,
                LoginEvent::Failed {
                    message: error.to_string(),
                },
            )
            .await;
            Err(error)
        }
    }
}

async fn run_oauth_inner(
    session: &OAuthSession,
    provider: Provider,
    config: &OAuthConfig,
    codes: Option<mpsc::Receiver<String>>,
    events: &mpsc::Sender<LoginEvent>,
) -> Result<CompletedLogin, OAuthError> {
    if provider == Provider::Cursor {
        // Explicit user-authorized import of the local Cursor session; the
        // database is opened read-only and no browser is involved.
        send(
            events,
            LoginEvent::Progress {
                text: "Lendo sessão Cursor autorizada, em modo somente leitura...".into(),
            },
        )
        .await;
        let login = session.import_cursor(config).await?;
        return Ok(CompletedLogin::new(provider, config.clone(), login));
    }
    send(
        events,
        LoginEvent::Progress {
            text: "Preparando autorização no provedor...".into(),
        },
    )
    .await;
    let pending = session.begin(provider, config).await?;
    let prompt = pending.prompt().clone();
    let url = prompt.url.clone();
    send(
        events,
        LoginEvent::Prompt {
            url: prompt.url,
            user_code: prompt.user_code,
            instructions: "Autorize no navegador do sistema. Aguardando sua autorização; nenhuma senha deve ser digitada no Cuotai.".into(),
        },
    )
    .await;
    if !open_browser(&url).await {
        send(events, LoginEvent::BrowserUnavailable { url }).await;
    }
    let login = match codes {
        Some(codes) => {
            send(events, LoginEvent::WaitingCode).await;
            let (error_tx, mut error_rx) = mpsc::channel(4);
            let finish = pending.finish_with_codes(codes, error_tx);
            tokio::pin!(finish);
            loop {
                tokio::select! {
                    result = &mut finish => break result?,
                    Some(message) = error_rx.recv() => {
                        send(events, LoginEvent::CodeRejected { message }).await;
                    }
                }
            }
        }
        None => pending.finish().await?,
    };
    Ok(CompletedLogin::new(provider, config.clone(), login))
}

fn start_aws_sso(profile: Option<String>) -> LoginDriver {
    let (event_tx, events) = mpsc::channel(16);
    let job = tokio::spawn(async move {
        let result = run_aws_sso(profile, &event_tx).await;
        match result {
            Ok(()) => {
                let _ = event_tx
                    .send(LoginEvent::Finished {
                        params: BTreeMap::new(),
                    })
                    .await;
                Ok(FlowDone::AwsSso)
            }
            Err(error) => {
                let _ = event_tx
                    .send(LoginEvent::Failed {
                        message: error.to_string(),
                    })
                    .await;
                Err(error)
            }
        }
    });
    LoginDriver {
        events,
        code_tx: None,
        job: Some(job),
    }
}

async fn run_aws_sso(
    profile: Option<String>,
    events: &mpsc::Sender<LoginEvent>,
) -> Result<(), OAuthError> {
    let profile = profile.filter(|p| !p.trim().is_empty()).ok_or_else(|| {
        OAuthError::Configuration(
            "Informe o perfil AWS configurado via `aws configure sso`.".into(),
        )
    })?;
    let _ = events
        .send(LoginEvent::Progress {
            text: "AWS SSO: abra a URL e use o código indicados pela AWS CLI.".into(),
        })
        .await;
    let (lines_tx, mut lines_rx) = mpsc::channel(16);
    let login = aws::login(&profile, lines_tx);
    tokio::pin!(login);
    loop {
        tokio::select! {
            result = &mut login => return result,
            Some(line) = lines_rx.recv() => {
                let _ = events.send(LoginEvent::Progress { text: line }).await;
            }
        }
    }
}

fn start_bailian_console(console_site: Option<String>, profile: Option<String>) -> LoginDriver {
    let (event_tx, events) = mpsc::channel(8);
    let job = tokio::spawn(async move {
        let result = run_bailian_console(console_site, profile, &event_tx).await;
        match result {
            Ok(()) => {
                let _ = event_tx
                    .send(LoginEvent::Finished {
                        params: BTreeMap::new(),
                    })
                    .await;
                Ok(FlowDone::BailianConsole)
            }
            Err(error) => {
                let _ = event_tx
                    .send(LoginEvent::Failed {
                        message: error.to_string(),
                    })
                    .await;
                Err(error)
            }
        }
    });
    LoginDriver {
        events,
        code_tx: None,
        job: Some(job),
    }
}

async fn run_bailian_console(
    console_site: Option<String>,
    profile: Option<String>,
    events: &mpsc::Sender<LoginEvent>,
) -> Result<(), OAuthError> {
    let site = console_site
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            OAuthError::Configuration("Site do console em branco; informe `international`.".into())
        })?;
    let profile = profile.filter(|p| !p.trim().is_empty()).ok_or_else(|| {
        OAuthError::Configuration(
            "Perfil do `bl` em branco; impossível iniciar o login isolado.".into(),
        )
    })?;
    // Mensagens estáticas e genéricas — nunca conteúdo do stdout do CLI.
    let _ = events
        .send(LoginEvent::Progress {
            text: "Iniciando `bl auth login --console`. O navegador pode abrir automaticamente."
                .into(),
        })
        .await;
    bailian::console_login(&site, &profile).await?;
    let _ = events
        .send(LoginEvent::Progress {
            text: "Login do `bl` concluído no perfil dedicado. Cuotai NÃO guarda nada disto."
                .into(),
        })
        .await;
    let _ = events.send(LoginEvent::Progress { text: "A consulta Token Plan usará esse perfil via `bl --config <name>`. `params.session_env` permanece um override opcional (Cookie manual) caso você prefira a rota antiga.".into() }).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// RF2.3/RF2.9 — browser opening (moved from the TUI; https only)
// ---------------------------------------------------------------------------

/// Open `url` in the system browser. Only plain `https://` URLs are opened
/// (RF2.9); anything else reports failure so the caller can surface the URL
/// for manual copy. Same platform commands and 5 s budget the TUI used.
pub async fn open_browser(url: &str) -> bool {
    if !url.starts_with("https://") {
        return false;
    }
    #[cfg(target_os = "macos")]
    let mut cmd = tokio::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = tokio::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = tokio::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return false;
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        cmd.arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        matches!(tokio::time::timeout(std::time::Duration::from_secs(5), cmd.status()).await, Ok(Ok(status)) if status.success())
    }
}

// ---------------------------------------------------------------------------
// RF2.4/RF2.8 — staged credential + TOML commit
// ---------------------------------------------------------------------------

/// The authorized result of an OAuth login, safe to hold in memory until the
/// account form is saved. Cheap to clone (the token handle is shared); never
/// serialized nor `Debug`. Dropping it without committing writes nothing.
#[derive(Clone)]
pub struct CompletedLogin {
    pub provider: Provider,
    /// OAuth configuration to embed in the account (carries the
    /// `credential_id`; reauth reuses the existing one, RF2.8).
    pub config: OAuthConfig,
    /// Whitelisted public quota metadata from the login (`account_params`).
    pub params: BTreeMap<String, String>,
    login: Arc<AuthorizedLogin>,
}

impl CompletedLogin {
    fn new(provider: Provider, config: OAuthConfig, login: AuthorizedLogin) -> Self {
        let params = login.account_params(provider, &config);
        Self {
            provider,
            config,
            params,
            login: Arc::new(login),
        }
    }
}

/// Stage that failed inside a commit (vault = OS secure store, config = TOML).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitStage {
    Vault,
    Config,
}

/// What the commit did to the vault after a failed config save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compensation {
    /// Nothing to undo (the vault write itself failed).
    NotNeeded,
    /// A previous token had been replaced (reauth) and was restored.
    PreviousRestored,
    /// A brand-new credential was removed again.
    NewRemoved,
    /// Compensation itself failed; the vault holds the NEW token while the
    /// TOML was not saved. For reauth the same key stays usable; for a new
    /// account the unreferenced token is overwritten by the next commit.
    Failed,
}

/// RF2.4 typed partial failure: the vault and the TOML are independent
/// stores and cannot commit atomically; this describes the exact state.
/// `message` is sanitized (no secrets, no user input echo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialFailure {
    pub stage: CommitStage,
    pub config_saved: bool,
    pub credential_saved: bool,
    pub compensation: Compensation,
    pub message: String,
}

/// Outcome of [`commit_login`]. On [`LoginCommitOutcome::PartialFailure`] the
/// [`CompletedLogin`] is untouched and the commit may simply be retried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginCommitOutcome {
    Saved,
    PartialFailure(PartialFailure),
}

/// Commit an authorized login: write the credential to the vault, then save
/// the new configuration to the TOML. Ordering and compensation are owned by
/// the core (RF2.4):
///
/// 1. The prior token under the same key is captured (reauth case).
/// 2. The new token is written; a failure here writes nothing anywhere.
/// 3. `file.save(config)` persists the TOML; on failure the vault is
///    compensated — restore the prior token, or remove the brand-new one.
///    A replaced token is never deleted without a successful restore
///    ([`Compensation::Failed`] reports that state instead).
///
/// Retrying with the same [`CompletedLogin`] and `config` is safe and
/// idempotent. AWS SSO and Bailian console logins write outside the Cuotai
/// vault and are not part of this commit.
pub async fn commit_login(
    file: &mut ConfigFile,
    session: &OAuthSession,
    login: &CompletedLogin,
    config: Config,
) -> LoginCommitOutcome {
    let commit = match begin_credential_commit(session, login).await {
        Ok(commit) => commit,
        Err(error) => {
            return LoginCommitOutcome::PartialFailure(PartialFailure {
                stage: CommitStage::Vault,
                config_saved: false,
                credential_saved: false,
                compensation: Compensation::NotNeeded,
                message: error.to_string(),
            });
        }
    };
    match file.save(config) {
        Ok(()) => commit.confirm(),
        Err(error) => {
            LoginCommitOutcome::PartialFailure(commit.rollback(session, error.message()).await)
        }
    }
}

/// Stage 1 of the commit, for event-loop frontends that cannot hold a
/// `&mut ConfigFile` across an await: capture the prior token and write the
/// new credential to the vault. Pair with [`CredentialCommit::confirm`] after
/// a successful TOML save or [`CredentialCommit::rollback`] after a failed
/// one — never drop it undecided.
pub async fn begin_credential_commit(
    session: &OAuthSession,
    login: &CompletedLogin,
) -> Result<CredentialCommit, OAuthError> {
    let key = login.login.key().to_string();
    let prior = session.load_by_key(&key).await?;
    session.save_login(&login.login).await?;
    Ok(CredentialCommit { key, prior })
}

/// In-flight credential commit awaiting the TOML save verdict. Holds the
/// prior token for compensation; intentionally not `Debug`.
pub struct CredentialCommit {
    key: String,
    prior: Option<super::OAuthToken>,
}

impl CredentialCommit {
    /// The TOML save succeeded; the commit is complete.
    pub fn confirm(self) -> LoginCommitOutcome {
        LoginCommitOutcome::Saved
    }

    /// The TOML save failed; compensate the vault write. `message` is the
    /// sanitized save error shown to the user.
    pub async fn rollback(self, session: &OAuthSession, message: String) -> PartialFailure {
        match self.prior {
            Some(prior) => match session.restore_by_key(self.key, prior).await {
                Ok(()) => PartialFailure {
                    stage: CommitStage::Config,
                    config_saved: false,
                    credential_saved: true,
                    compensation: Compensation::PreviousRestored,
                    message,
                },
                Err(_) => PartialFailure {
                    stage: CommitStage::Config,
                    config_saved: false,
                    credential_saved: true,
                    compensation: Compensation::Failed,
                    message,
                },
            },
            None => match session.delete_by_key(self.key).await {
                Ok(()) => PartialFailure {
                    stage: CommitStage::Config,
                    config_saved: false,
                    credential_saved: false,
                    compensation: Compensation::NewRemoved,
                    message,
                },
                Err(_) => PartialFailure {
                    stage: CommitStage::Config,
                    config_saved: false,
                    credential_saved: true,
                    compensation: Compensation::Failed,
                    message,
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::ConfigFile;
    use crate::oauth::session::storage_key;
    use crate::oauth::{OAuthToken, TokenStore};
    use parking_lot::Mutex;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemoryStore(Mutex<HashMap<String, OAuthToken>>);
    impl TokenStore for MemoryStore {
        fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
            Ok(self.0.lock().get(id).cloned())
        }
        fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
            self.0.lock().insert(id.into(), token.clone());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), OAuthError> {
            self.0.lock().remove(id);
            Ok(())
        }
    }
    struct NoSecrets;
    impl crate::Credentials for NoSecrets {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    fn session() -> (OAuthSession, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::default());
        let session = OAuthSession::new(store.clone(), Arc::new(NoSecrets)).unwrap();
        (session, store)
    }

    fn token(access: &str) -> OAuthToken {
        OAuthToken {
            access_token: access.into(),
            refresh_token: Some("refresh".into()),
            expires_at: None,
            metadata: Default::default(),
        }
    }

    fn completed(provider: Provider) -> CompletedLogin {
        let config = OAuthConfig::new("test-client".into());
        let key = storage_key(provider, &config).unwrap();
        CompletedLogin::new(
            provider,
            config,
            crate::oauth::AuthorizedLogin::for_test(key, token("access-new")),
        )
    }

    fn empty_config() -> Config {
        Config {
            refresh_seconds: 60,
            concurrency: 2,
            timeout_seconds: 10,
            accounts: vec![],
        }
    }

    // RF2.7: every provider resolves to the expected interactive modes.
    #[test]
    fn login_modes_cover_every_existing_flow() {
        let aws = login_modes(Provider::AwsBedrock);
        assert_eq!(aws.modes.len(), 1);
        assert_eq!(aws.modes[0].id, MODE_AWS_SSO);
        assert!(aws.modes[0].options.iter().any(|o| o.key == "profile"));

        let bailian = login_modes(Provider::AlibabaTokenPlan);
        assert_eq!(bailian.modes.len(), 1);
        assert_eq!(bailian.modes[0].id, MODE_BAILIAN_CONSOLE);
        assert!(bailian.modes[0]
            .options
            .iter()
            .any(|o| o.key == "console_site"));

        for provider in [Provider::Kiro, Provider::Cursor, Provider::XiaomiMimo] {
            let modes = login_modes(provider);
            assert_eq!(modes.modes.len(), 1, "{provider:?}");
            assert_eq!(modes.modes[0].id, MODE_OAUTH);
            assert!(!modes.modes[0].requires_client_registration);
            assert!(modes.unsupported_reason.is_none());
        }

        let google = login_modes(Provider::GeminiApi);
        assert_eq!(google.modes.len(), 1);
        assert!(google.modes[0].requires_client_registration);

        let openai = login_modes(Provider::OpenAiApi);
        assert!(openai.modes.is_empty());
        assert!(openai.unsupported_reason.is_some());
    }

    // RF2.6: adding a provider flow must never require frontend branching;
    // the offered mode options stay within the declared descriptor keys.
    #[test]
    fn oauth_mode_options_match_native_descriptors() {
        for provider in [Provider::Kiro, Provider::Qwen, Provider::Cursor] {
            let modes = login_modes(provider);
            let declared: Vec<&str> = account_edit::native_option_descriptors(provider)
                .iter()
                .map(|d| d.key)
                .collect();
            for option in &modes.modes[0].options {
                assert!(declared.contains(&option.key.as_str()));
            }
        }
    }

    // RF2.5: serialized events carry no token material and use stable tags.
    #[test]
    fn events_serialize_without_secrets() {
        let event = LoginEvent::Prompt {
            url: "https://example.com/authorize".into(),
            user_code: Some("ABCD-EFGH".into()),
            instructions: "instrucoes".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"prompt\""));
        assert!(!json.contains("access_token"));
        assert!(!json.contains("refresh_token"));
        let back: LoginEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);

        let outcome = LoginCommitOutcome::PartialFailure(PartialFailure {
            stage: CommitStage::Config,
            config_saved: false,
            credential_saved: true,
            compensation: Compensation::PreviousRestored,
            message: "conflito".into(),
        });
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(json.contains("\"type\":\"partial_failure\""));
        assert!(json.contains("\"compensation\":\"previous_restored\""));
    }

    // RF2.6: a mode id that was not offered for the provider is rejected
    // before any flow starts.
    #[test]
    fn start_login_rejects_mode_not_offered() {
        let (session, _) = session();
        let result = start_login(
            Arc::new(session),
            Provider::Kiro,
            LoginRequest::aws_sso("default".into()),
        );
        assert!(matches!(result, Err(OAuthError::Configuration(_))));
    }

    // RF2.4: vault failure writes nothing anywhere.
    #[tokio::test]
    async fn commit_vault_failure_leaves_toml_untouched() {
        struct FailingStore;
        impl TokenStore for FailingStore {
            fn load(&self, _: &str) -> Result<Option<OAuthToken>, OAuthError> {
                Ok(None)
            }
            fn save(&self, _: &str, _: &OAuthToken) -> Result<(), OAuthError> {
                Err(OAuthError::Storage("falha".into()))
            }
            fn delete(&self, _: &str) -> Result<(), OAuthError> {
                Ok(())
            }
        }
        let session = OAuthSession::new(Arc::new(FailingStore), Arc::new(NoSecrets)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = ConfigFile::load(Some(path.clone()), true).unwrap();
        let outcome = commit_login(
            &mut file,
            &session,
            &completed(Provider::GeminiApi),
            empty_config(),
        )
        .await;
        let LoginCommitOutcome::PartialFailure(failure) = outcome else {
            panic!("esperava falha parcial");
        };
        assert_eq!(failure.stage, CommitStage::Vault);
        assert!(!failure.config_saved);
        assert!(!failure.credential_saved);
        assert_eq!(failure.compensation, Compensation::NotNeeded);
        assert!(!path.exists());
    }

    // RF2.4/RF2.8: reauth commit whose TOML save conflicts restores the prior
    // token — never leaves the new token behind, never loses the old one.
    #[tokio::test]
    async fn commit_config_conflict_restores_prior_token() {
        let (session, store) = session();
        let completed = completed(Provider::GeminiApi);
        let key = storage_key(Provider::GeminiApi, &completed.config).unwrap();
        store
            .save(&key, &token("access-old"))
            .expect("preload prior token");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = ConfigFile::load(Some(path.clone()), true).unwrap();
        file.save(empty_config()).unwrap();
        // External tamper -> ConfigFile::save must report Conflict.
        std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();

        let outcome = commit_login(&mut file, &session, &completed, empty_config()).await;
        let LoginCommitOutcome::PartialFailure(failure) = outcome else {
            panic!("esperava falha parcial");
        };
        assert_eq!(failure.stage, CommitStage::Config);
        assert!(!failure.config_saved);
        assert!(failure.credential_saved);
        assert_eq!(failure.compensation, Compensation::PreviousRestored);
        let stored = store.load(&key).unwrap().unwrap();
        assert_eq!(stored.access_token, "access-old");

        // Retry after resolving the conflict succeeds and writes the new token.
        file.reload().unwrap();
        let outcome = commit_login(&mut file, &session, &completed, empty_config()).await;
        assert_eq!(outcome, LoginCommitOutcome::Saved);
        let stored = store.load(&key).unwrap().unwrap();
        assert_eq!(stored.access_token, "access-new");
    }

    // RF2.4: a brand-new credential is removed again when the TOML save fails.
    #[tokio::test]
    async fn commit_config_conflict_removes_fresh_token() {
        let (session, store) = session();
        let completed = completed(Provider::GeminiApi);
        let key = storage_key(Provider::GeminiApi, &completed.config).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = ConfigFile::load(Some(path.clone()), true).unwrap();
        file.save(empty_config()).unwrap();
        std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();

        let outcome = commit_login(&mut file, &session, &completed, empty_config()).await;
        let LoginCommitOutcome::PartialFailure(failure) = outcome else {
            panic!("esperava falha parcial");
        };
        assert_eq!(failure.compensation, Compensation::NewRemoved);
        assert!(!failure.credential_saved);
        assert!(store.load(&key).unwrap().is_none());
    }

    // RF2.4: cancel before commit never touches the vault — a completed login
    // that is simply dropped leaves no trace.
    #[tokio::test]
    async fn dropped_completed_login_writes_nothing() {
        let (_, store) = session();
        let completed = completed(Provider::Kiro);
        let key = storage_key(Provider::Kiro, &completed.config).unwrap();
        drop(completed);
        assert!(store.load(&key).unwrap().is_none());
    }
}
