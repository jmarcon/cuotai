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
use crate::text::Localized;
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
        /// English text; `instructions_l10n` carries its id (PRD 0034).
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions_l10n: Option<Localized>,
    },
    /// Free-form progress line (sanitized; CLI output is filtered to known
    /// device-prompt lines before it becomes a `Progress` event). `text` is
    /// English; `l10n` is set for Cuotai's own lines (PRD 0034), absent for
    /// lines relayed from a provider CLI.
    Progress {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        l10n: Option<Localized>,
    },
    /// The flow accepts a manually pasted authorization code now (MiMo).
    WaitingCode,
    /// A submitted code was rejected; the user may paste another one.
    CodeRejected {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        l10n: Option<Localized>,
    },
    /// RF2.9: the browser could not be opened; the frontend must display the
    /// URL (already shown in `Prompt`) for manual copy.
    BrowserUnavailable { url: String },
    /// Flow finished. `params` carries only public, whitelisted quota
    /// metadata ([`CompletedLogin::params`]); the token stays in the driver.
    Finished { params: BTreeMap<String, String> },
    /// Flow failed with a sanitized message (no input echo, no secrets).
    Failed {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        l10n: Option<Localized>,
    },
}

impl LoginEvent {
    /// A progress line written by Cuotai (PRD 0034).
    pub fn progress(note: Localized) -> Self {
        Self::Progress {
            text: note.text().to_string(),
            l10n: Some(note),
        }
    }

    /// The failure of a flow, from its error.
    pub fn failed(error: &OAuthError) -> Self {
        Self::Failed {
            message: error.to_string(),
            l10n: Some(error.l10n()),
        }
    }
}

// ---------------------------------------------------------------------------
// RF2.7 — per-provider login mode discovery
// ---------------------------------------------------------------------------

/// Stable mode identifiers sent back in [`LoginRequest::mode`].
pub const MODE_OAUTH: &str = "oauth";
pub const MODE_AWS_SSO: &str = "aws_sso";
pub const MODE_BAILIAN_CONSOLE: &str = "bailian_console";

/// Validation owned by the core for a nonsecret login input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginOptionValidation {
    #[default]
    None,
    ClientId,
    EnvName,
    Tenant,
    NoWhitespace,
    Path,
    CliProfile,
}

/// A text or closed-choice input accepted by a login mode. Registration and
/// native-flow inputs share this descriptor so frontends never hardcode keys.
///
/// RF8: `label` and `help` travel as [`Localized`] with stable ids
/// (`login.<provider>.<mode>.registration.<key>.label` for registration
/// options, `native.<provider>.<key>.label` for native options). Frontends
/// resolve them against their catalog and use the embedded Portuguese
/// fallback when the id is unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginOption {
    pub key: String,
    pub label: crate::text::Localized,
    pub help: crate::text::Localized,
    /// Closed value list; `None` means free text. Each entry carries the
    /// choice's localizable label so the UI can render translated text.
    pub choices: Option<Vec<LoginChoice>>,
    pub required: bool,
    pub validation: LoginOptionValidation,
}

/// One closed-choice entry of a [`LoginOption`]. `value` is the stable
/// machine identifier (validated by the core); `label` is the user-visible
/// text. Applicability metadata is additive for API consumers: `None` means
/// all auth modes, matching [`account_edit::ChoiceOptionDescriptor`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginChoice {
    pub value: String,
    pub label: crate::text::Localized,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_modes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<crate::text::Localized>,
}

impl LoginOption {
    fn text(id_prefix: &str, key: &str, label: &str, help: &str) -> Self {
        Self {
            key: key.to_string(),
            label: localized_at(id_prefix, "label", label),
            help: localized_at(id_prefix, "help", help),
            choices: None,
            required: false,
            validation: LoginOptionValidation::None,
        }
    }

    fn required(mut self) -> Self {
        self.required = true;
        self
    }

    fn validation(mut self, validation: LoginOptionValidation) -> Self {
        self.validation = validation;
        self
    }

    fn from_native(
        provider: crate::Provider,
        auth_mode: &str,
        descriptor: &account_edit::FieldDescriptor,
    ) -> Self {
        let key = descriptor.key;
        let choices = match descriptor.kind {
            account_edit::FieldDescriptorKind::Text => None,
            account_edit::FieldDescriptorKind::Choice(choice) => Some(
                choice
                    .options
                    .iter()
                    .filter(|option| option.applies_to(auth_mode))
                    .map(|option| LoginChoice {
                        value: option.value.to_string(),
                        label: crate::text::Localized::fixed(
                            account_edit::native_option_option_id(provider, key, option.value),
                            option.label.to_string(),
                        ),
                        auth_modes: option
                            .auth_modes
                            .map(|modes| modes.iter().map(|mode| (*mode).to_string()).collect()),
                        unavailable_reason: account_edit::field_option_unavailability(
                            provider,
                            key,
                            option.value,
                        ),
                    })
                    .collect(),
            ),
        };
        let validation = if key == "database_path" {
            LoginOptionValidation::Path
        } else {
            LoginOptionValidation::NoWhitespace
        };
        Self {
            key: key.to_string(),
            label: crate::text::Localized::fixed(
                account_edit::native_option_label_id(provider, key),
                descriptor.label.to_string(),
            ),
            help: crate::text::Localized::fixed(
                account_edit::native_option_help_id(provider, key),
                descriptor.help.to_string(),
            ),
            choices,
            required: false,
            validation,
        }
    }

    /// Validate one value against this trusted descriptor. The input itself is
    /// never echoed in the error.
    pub fn validate(&self, value: &str) -> Result<(), OAuthError> {
        let value = value.trim();
        // RF8: the offending field's label travels as a nested `Localized`
        // param (`{field}`) so it translates with the message instead of
        // baking the English label into a translated sentence.
        let field = crate::text::ParamValue::from(self.label.clone());
        if value.is_empty() {
            if self.required {
                return Err(OAuthError::Configuration(
                    crate::text::Localized::with_param(
                        "oauth_error.login.field_required",
                        "field",
                        field,
                        "{field} is required.",
                    ),
                ));
            }
            return Ok(());
        }
        if value.len() > 2048 || value.chars().any(char::is_control) {
            return Err(OAuthError::Configuration(
                crate::text::Localized::with_param(
                    "oauth_error.login.field_invalid_chars",
                    "field",
                    field,
                    "{field} contains invalid characters or is too long.",
                ),
            ));
        }
        if let Some(choices) = &self.choices {
            if !choices.iter().any(|choice| choice.value == value) {
                return Err(OAuthError::Configuration(
                    crate::text::Localized::with_param(
                        "oauth_error.login.field_unknown_choice",
                        "field",
                        field,
                        "{field} does not contain a recognised option.",
                    ),
                ));
            }
        }
        let valid = match self.validation {
            LoginOptionValidation::None | LoginOptionValidation::Path => true,
            LoginOptionValidation::ClientId => value.len() <= 256,
            LoginOptionValidation::EnvName => account_edit::valid_env_name(value),
            LoginOptionValidation::Tenant => value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.')),
            LoginOptionValidation::NoWhitespace => !value.chars().any(char::is_whitespace),
            LoginOptionValidation::CliProfile => account_edit::is_valid_cli_profile(value),
        };
        if valid {
            Ok(())
        } else {
            Err(OAuthError::Configuration(
                crate::text::Localized::with_param(
                    "oauth_error.login.field_invalid_value",
                    "field",
                    field,
                    "{field} has an invalid value.",
                ),
            ))
        }
    }
}

/// Build a [`Localized`] under `id_prefix.<suffix>` (e.g. for an option
/// descriptor). Used by registration/native option constructors to attach
/// stable ids while keeping the construction call sites readable.
fn localized_at(id_prefix: &str, suffix: &str, fallback: &str) -> crate::text::Localized {
    crate::text::Localized::fixed(format!("{id_prefix}.{suffix}"), fallback.to_string())
}

/// Where a successful login stores its credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginCredentialStore {
    /// Cuotai owns the token and commits it to the OS vault.
    CuotaiVault,
    /// An external CLI owns its credential; Cuotai only records public params.
    External,
}

/// Generic binding from public login values to the account draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginAccountBinding {
    None,
    /// The named option/result value is the account's credential text (AWS
    /// profile, which `build_account` maps to `params.profile`).
    Credential {
        option: String,
    },
    /// The named option/result value is the isolated `bl --config` profile.
    CliProfile {
        option: String,
    },
}

/// One interactive login mode offered for a provider. All labels, help,
/// registration fields, outcome text and account bindings come from the core.
///
/// RF8: `label`, `help`, `completion_notice` and `unavailable_reason` are
/// [`crate::text::Localized`] with stable ids derived from the provider
/// slug and the mode id (`login.<provider>.<mode>.{label,help,...}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginMode {
    pub id: String,
    pub label: crate::text::Localized,
    pub help: crate::text::Localized,
    pub completion_notice: crate::text::Localized,
    pub credential_store: LoginCredentialStore,
    pub account_binding: LoginAccountBinding,
    /// User-registered application inputs accepted in
    /// [`LoginRequest::registration`]. Empty for native/external flows.
    pub registration: Vec<LoginOption>,
    /// Nonsecret flow options accepted in [`LoginRequest::options`].
    pub options: Vec<LoginOption>,
    pub unavailable_reason: Option<crate::text::Localized>,
}

/// Result of [`login_modes`]: interactive modes and the reason when no login
/// exists at all. `unsupported_reason` is a [`Localized`] when the
/// provider has no built-in OAuth (RF8 id `oauth.unsupported_reason.<slug>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderLogin {
    pub modes: Vec<LoginMode>,
    pub unsupported_reason: Option<crate::text::Localized>,
}

fn registration_options(provider: Provider) -> Vec<LoginOption> {
    if !requires_client_registration(provider) {
        return Vec::new();
    }
    let slug = provider.slug();
    let client_id_prefix = format!("login.{slug}.oauth.registration.client_id");
    let client_id = LoginOption::text(
        &client_id_prefix,
        "client_id",
        "OAuth client ID",
        "From YOUR registered app; Cuotai has no app of its own.",
    )
    .required()
    .validation(LoginOptionValidation::ClientId);
    if provider == Provider::AzureOpenAi {
        let tenant_prefix = format!("login.{slug}.oauth.registration.tenant");
        vec![
            client_id,
            LoginOption::text(
                &tenant_prefix,
                "tenant",
                "Tenant (optional)",
                "Empty uses \"organizations\" (accounts in any organisation).",
            )
            .validation(LoginOptionValidation::Tenant),
        ]
    } else {
        let secret_prefix = format!("login.{slug}.oauth.registration.client_secret_env");
        vec![
            client_id,
            LoginOption::text(
                &secret_prefix,
                "client_secret_env",
                "Client secret variable (optional)",
                "Only the NAME of the exported variable holding the secret; never the value.",
            )
            .validation(LoginOptionValidation::EnvName),
        ]
    }
}

fn oauth_help(provider: Provider) -> String {
    if requires_client_registration(provider) {
        return account_edit::oauth_registration_help(provider).to_string();
    }
    match provider {
        Provider::Cursor => "Imports the personal Cursor session from this computer's local install. Does not start OAuth: log into the Cursor app first. After confirmation, accessToken and machine ID are read from the local SQLite in read-only mode. Tokens are never displayed; the copy is saved to the vault only at the final review.".into(),
        Provider::Qwen => "EXPERIMENTAL flow based on the native client. Qwen Code: free OAuth discontinued on 15/04/2026; the attempt may be rejected and login does not guarantee an available quota. No password or token is typed into Cuotai.".into(),
        Provider::Claude => "EXPERIMENTAL flow based on the native client. The provider may restrict this flow to its own clients; if refused, use the environment variable. No password or token is typed into Cuotai.".into(),
        Provider::OpenRouter => "OpenRouter's documented OAuth PKCE flow. Consent happens in the browser and OpenRouter returns an inference key kept in the system vault; it shows that key's numbers, not the account balance (use a management key for that). No password is typed into Cuotai.".into(),
        Provider::XiaomiMimo => "The code authorises an inference key, not the console session. Balance and Token Plan use platform_session_env; the weekly Desktop quota uses session_env. No password is typed into Cuotai.".into(),
        _ => "EXPERIMENTAL flow based on the provider's native client (internal interface, without official support; may fail or be restricted at any time). Consent happens in the browser/device-code; no password or token is typed into Cuotai.".into(),
    }
}

fn oauth_completion_notice(provider: Provider) -> String {
    match provider {
        Provider::Cursor => "Cursor session imported in memory. Review and save to persist in the vault.".into(),
        Provider::OpenRouter => "Login authorised; the OpenRouter inference key was saved in the system vault.".into(),
        Provider::XiaomiMimo => "MiMo key authorised. To query the balance/plan, set platform_session_env with the web session; do not re-enter the code.".into(),
        _ => "Login authorised; account parameters filled in automatically.".into(),
    }
}

/// RF2.7: every interactive login mode for `provider`, with all descriptors.
pub fn login_modes(provider: Provider) -> ProviderLogin {
    let mut modes = Vec::new();
    match provider {
        Provider::AwsBedrock => {
            let id = MODE_AWS_SSO.to_string();
            let slug = provider.slug();
            let mode_prefix = format!("login.{slug}.{id}");
            let option_prefix = format!("{mode_prefix}.option.profile");
            modes.push(LoginMode {
                id,
                label: crate::text::Localized::fixed(
                    format!("{mode_prefix}.label"),
                    "AWS SSO (AWS CLI)".to_string(),
                ),
                help: crate::text::Localized::fixed(
                    format!("{mode_prefix}.help"),
                    "Runs \"aws sso login --use-device-code\" using the supplied profile; the token is cached by the AWS CLI itself, outside the Cuotai vault. Configure the profile beforehand with \"aws configure sso\"; Cost Explorer charges per query.".to_string(),
                ),
                completion_notice: crate::text::Localized::fixed(
                    format!("{mode_prefix}.completion_notice"),
                    "AWS CLI SSO login completed.".to_string(),
                ),
                credential_store: LoginCredentialStore::External,
                account_binding: LoginAccountBinding::Credential {
                    option: "profile".to_string(),
                },
                registration: Vec::new(),
                options: vec![LoginOption::text(
                    &option_prefix,
                    "profile",
                    "AWS profile",
                    "Profile configured previously via \"aws configure sso\".",
                )
                .required()],
                unavailable_reason: None,
            });
        }
        Provider::AlibabaTokenPlan => {
            let id = MODE_BAILIAN_CONSOLE.to_string();
            let slug = provider.slug();
            let mode_prefix = format!("login.{slug}.{id}");
            let site_prefix = format!("{mode_prefix}.option.console_site");
            let profile_prefix = format!("{mode_prefix}.option.profile");
            modes.push(LoginMode {
                id,
                label: crate::text::Localized::fixed(
                    format!("{mode_prefix}.label"),
                    "Bailian console (bl)".to_string(),
                ),
                help: crate::text::Localized::fixed(
                    format!("{mode_prefix}.help"),
                    "Runs \"bl auth login --console\" on a dedicated bl profile; the login lives in bl's local store, outside the Cuotai vault. Cancelling does not undo what bl recorded. params.session_env remains an optional Cookie override; QWEN_API_KEY only authenticates inference and is not sent to the quota query.".to_string(),
                ),
                completion_notice: crate::text::Localized::fixed(
                    format!("{mode_prefix}.completion_notice"),
                    "bl auth login --console completed on the dedicated profile.".to_string(),
                ),
                credential_store: LoginCredentialStore::External,
                account_binding: LoginAccountBinding::CliProfile {
                    option: "profile".to_string(),
                },
                registration: Vec::new(),
                options: vec![
                    LoginOption {
                        key: "console_site".to_string(),
                        label: crate::text::Localized::fixed(
                            format!("{site_prefix}.label"),
                            "Console site".to_string(),
                        ),
                        help: crate::text::Localized::fixed(
                            format!("{site_prefix}.help"),
                            "Token Plan site served by \"bl --console-site\".".to_string(),
                        ),
                        choices: Some(vec![LoginChoice {
                            value: "international".to_string(),
                            label: crate::text::Localized::fixed(
                                format!("{site_prefix}.option.international"),
                                "International".to_string(),
                            ),
                            auth_modes: None,
                            unavailable_reason: None,
                        }]),
                        required: true,
                        validation: LoginOptionValidation::None,
                    },
                    LoginOption::text(
                        &profile_prefix,
                        "profile",
                        "Dedicated bl profile (optional)",
                        "Profile (--config) where bl stores the login; empty generates an isolated profile.",
                    )
                    .validation(LoginOptionValidation::CliProfile),
                ],
                unavailable_reason: None,
            });
        }
        _ if supports_login(provider) => {
            let id = MODE_OAUTH.to_string();
            let slug = provider.slug();
            let mode_prefix = format!("login.{slug}.{id}");
            let registration = registration_options(provider);
            let label_text = if provider == Provider::Cursor {
                "Import Cursor session".to_string()
            } else if provider == Provider::OpenRouter {
                "Sign in with OpenRouter".to_string()
            } else {
                "Native browser login".to_string()
            };
            modes.push(LoginMode {
                id,
                label: crate::text::Localized::fixed(format!("{mode_prefix}.label"), label_text),
                help: oauth_help_localized(provider, &mode_prefix),
                completion_notice: oauth_completion_notice_localized(provider, &mode_prefix),
                credential_store: LoginCredentialStore::CuotaiVault,
                account_binding: LoginAccountBinding::None,
                registration,
                options: account_edit::native_option_descriptors(provider)
                    .map(|descriptor| LoginOption::from_native(provider, MODE_OAUTH, descriptor))
                    .collect(),
                unavailable_reason: None,
            });
        }
        _ => {}
    }
    ProviderLogin {
        modes,
        unsupported_reason: account_edit::oauth_unsupported_reason(provider),
    }
}

/// Localized help text for the OAuth mode (per-provider variants for
/// Cursor/Qwen/Claude/XiaomiMiMo + a generic fallback). `id_prefix` is
/// `login.<slug>.oauth`; the function emits `<id_prefix>.help`.
fn oauth_help_localized(provider: Provider, id_prefix: &str) -> crate::text::Localized {
    crate::text::Localized::fixed(
        format!("{id_prefix}.help"),
        oauth_help(provider).to_string(),
    )
}

/// Localized completion notice for the OAuth mode (Cursor/XiaomiMiMo
/// variants + a generic fallback). `id_prefix` is `login.<slug>.oauth`;
/// the function emits `<id_prefix>.completion_notice`.
fn oauth_completion_notice_localized(
    provider: Provider,
    id_prefix: &str,
) -> crate::text::Localized {
    crate::text::Localized::fixed(
        format!("{id_prefix}.completion_notice"),
        oauth_completion_notice(provider).to_string(),
    )
}

/// Resolve one trusted mode descriptor by its stable id.
pub fn login_mode(provider: Provider, mode_id: &str) -> Option<LoginMode> {
    login_modes(provider)
        .modes
        .into_iter()
        .find(|mode| mode.id == mode_id)
}

/// What the user asked to start. Use [`LoginRequest::for_account`] for both
/// new logins and reauthentication: the core, not a frontend, decides whether
/// an existing `credential_id` is reused and which prior key may be replaced.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginRequest {
    pub mode: String,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub registration: BTreeMap<String, String>,
    #[serde(default)]
    credential_id: Option<String>,
    /// Existing account being replaced; never accepted from serialized IPC.
    #[serde(skip)]
    replaced_account: Option<crate::Account>,
}

impl LoginRequest {
    pub fn new(mode: impl Into<String>) -> Self {
        Self {
            mode: mode.into(),
            ..Self::default()
        }
    }

    /// Build a request bound to the account being edited. Existing public
    /// options/registration are prefilled; a same-provider OAuth reauth keeps
    /// its `credential_id`. The replaced account is retained privately so the
    /// commit can clean up a changed vault key after the TOML save succeeds.
    pub fn for_account(
        provider: Provider,
        mode: impl Into<String>,
        existing: Option<&crate::Account>,
    ) -> Self {
        let mut request = Self::new(mode);
        let Some(account) = existing else {
            return request;
        };
        request.replaced_account = Some(account.clone());
        if account.provider != provider {
            return request;
        }
        if let Some(oauth) = &account.oauth {
            if login_mode(provider, &request.mode)
                .is_some_and(|mode| mode.credential_store == LoginCredentialStore::CuotaiVault)
            {
                request.credential_id = Some(oauth.credential_id.clone());
                request.options = oauth.options.clone();
                if !oauth.client_id.is_empty() {
                    request
                        .registration
                        .insert("client_id".to_string(), oauth.client_id.clone());
                }
                if let Some(tenant) = &oauth.tenant {
                    request
                        .registration
                        .insert("tenant".to_string(), tenant.clone());
                }
                if let Some(secret) = &oauth.client_secret_env {
                    request
                        .registration
                        .insert("client_secret_env".to_string(), secret.clone());
                }
            }
        }
        if let Some(mode) = login_mode(provider, &request.mode) {
            match mode.account_binding {
                LoginAccountBinding::Credential { option } => {
                    let value = account_edit::stored_credential(account);
                    if !value.is_empty() {
                        request.options.insert(option, value);
                    }
                }
                LoginAccountBinding::CliProfile { option } => {
                    if let Some(value) = account.params.get("cli_profile") {
                        request.options.insert(option, value.clone());
                    }
                }
                LoginAccountBinding::None => {}
            }
        }
        request
    }

    /// Public read-only view used by frontend contract tests and diagnostics.
    pub fn credential_id(&self) -> Option<&str> {
        self.credential_id.as_deref()
    }
}

/// Resolve the credential text and dedicated CLI params for the selected mode.
/// Frontends pass the same public option/result maps to this helper; only the
/// core knows the AWS profile and `bl cli_profile` persistence semantics.
pub fn account_login_inputs(
    provider: Provider,
    mode_id: &str,
    credential: &str,
    options: &BTreeMap<String, String>,
    result_params: &BTreeMap<String, String>,
    login_completed: bool,
) -> Result<(String, Option<BTreeMap<String, String>>), OAuthError> {
    let mode = login_mode(provider, mode_id).ok_or_else(|| {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.login.mode_not_offered",
            "Login mode not offered for this provider.",
        ))
    })?;
    let value = |option: &str| {
        result_params
            .get(option)
            .or_else(|| options.get(option))
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };
    match mode.account_binding {
        LoginAccountBinding::None => Ok((credential.to_string(), None)),
        LoginAccountBinding::Credential { option } => {
            let credential = value(&option);
            if credential.is_empty() {
                return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                    "oauth_error.login.profile_required_before_save",
                    "Supply the configured profile before saving.",
                )));
            }
            Ok((credential, None))
        }
        LoginAccountBinding::CliProfile { option } => {
            let mut params = BTreeMap::new();
            // The profile the completed login actually used (`cli_profile`, including one the core
            // generated when the optional field was blank) wins over the typed option.
            let reported = result_params
                .get("cli_profile")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let profile = reported.unwrap_or_else(|| value(&option));
            let profile = if profile.is_empty() {
                if login_completed {
                    return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                        "oauth_error.login.dedicated_profile_missing",
                        "The completed login did not report the dedicated profile.",
                    )));
                }
                account_edit::ensure_cli_profile(&mut params)
            } else {
                if !account_edit::is_valid_cli_profile(&profile) {
                    return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                        "oauth_error.login.invalid_bl_profile",
                        "Invalid `bl` profile; use letters, digits, '-' or '_', up to 32 characters.",
                    )));
                }
                profile
            };
            params.insert("cli_profile".to_string(), profile);
            Ok((credential.to_string(), Some(params)))
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
    External(ExternalLogin),
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

/// Public result of a login whose credential is owned by an external CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalLogin {
    pub params: BTreeMap<String, String>,
}

/// The terminal result of a login flow, after [`LoginDriver::finish`].
pub enum LoginResult {
    /// Tokens authorized and held in memory; commit via [`commit_login`] or
    /// [`begin_credential_commit`]. Nothing is in the vault yet.
    Authorized(CompletedLogin),
    /// An external CLI completed; only nonsecret account params are returned.
    External(ExternalLogin),
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
            FlowDone::External(login) => LoginResult::External(login),
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

/// Start a login flow. Mode discovery, input validation and flow selection
/// stay in the core; frontends only forward a [`LoginRequest`].
pub fn start_login(
    session: Arc<OAuthSession>,
    provider: Provider,
    request: LoginRequest,
) -> Result<LoginDriver, OAuthError> {
    let mode = login_mode(provider, &request.mode).ok_or_else(|| {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.login.mode_not_offered",
            "Login mode not offered for this provider.",
        ))
    })?;
    if let Some(reason) = &mode.unavailable_reason {
        return Err(OAuthError::Configuration(
            crate::text::Localized::with_param(
                "oauth_error.login.mode_unavailable",
                "reason",
                reason.clone(),
                "Login mode unavailable: {reason}",
            ),
        ));
    }
    validate_request(&mode, &request)?;
    match mode.id.as_str() {
        MODE_OAUTH => Ok(start_oauth(session, provider, request)),
        MODE_AWS_SSO => Ok(start_aws_sso(request.options.get("profile").cloned())),
        MODE_BAILIAN_CONSOLE => Ok(start_bailian_console(
            request.options.get("console_site").cloned(),
            request.options.get("profile").cloned(),
        )),
        _ => Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.login.unknown_mode",
            "Unknown login mode.",
        ))),
    }
}

fn validate_request(mode: &LoginMode, request: &LoginRequest) -> Result<(), OAuthError> {
    for (key, value) in &request.options {
        let option = mode
            .options
            .iter()
            .find(|option| &option.key == key)
            .ok_or_else(|| {
                OAuthError::Configuration(crate::text::Localized::fixed(
                    "oauth_error.login.unknown_option",
                    "Unknown login option for this mode.",
                ))
            })?;
        option.validate(value)?;
    }
    for option in &mode.options {
        if option.required && !request.options.contains_key(&option.key) {
            option.validate("")?;
        }
    }
    for (key, value) in &request.registration {
        let option = mode
            .registration
            .iter()
            .find(|option| &option.key == key)
            .ok_or_else(|| {
                OAuthError::Configuration(crate::text::Localized::fixed(
                    "oauth_error.login.unknown_registration_field",
                    "Unknown registration field for this mode.",
                ))
            })?;
        option.validate(value)?;
    }
    for option in &mode.registration {
        if option.required && !request.registration.contains_key(&option.key) {
            option.validate("")?;
        }
    }
    Ok(())
}

fn oauth_config_from(request: LoginRequest) -> OAuthConfig {
    let value = |key: &str| request.registration.get(key).cloned();
    OAuthConfig {
        client_id: value("client_id").unwrap_or_default(),
        tenant: value("tenant").filter(|value| !value.trim().is_empty()),
        client_secret_env: value("client_secret_env").filter(|value| !value.trim().is_empty()),
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
    let replaced_key = request
        .replaced_account
        .as_ref()
        .and_then(account_edit::account_storage_key);
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
        replaced_key,
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
    replaced_key: Option<String>,
    codes: Option<mpsc::Receiver<String>>,
    events: mpsc::Sender<LoginEvent>,
) -> Result<FlowDone, OAuthError> {
    match run_oauth_inner(&session, provider, &config, replaced_key, codes, &events).await {
        Ok(login) => {
            let params = login.params.clone();
            send(&events, LoginEvent::Finished { params }).await;
            Ok(FlowDone::Authorized(login))
        }
        Err(error) => {
            send(&events, LoginEvent::failed(&error)).await;
            Err(error)
        }
    }
}

async fn run_oauth_inner(
    session: &OAuthSession,
    provider: Provider,
    config: &OAuthConfig,
    replaced_key: Option<String>,
    codes: Option<mpsc::Receiver<String>>,
    events: &mpsc::Sender<LoginEvent>,
) -> Result<CompletedLogin, OAuthError> {
    if provider == Provider::Cursor {
        // Explicit user-authorized import of the local Cursor session; the
        // database is opened read-only and no browser is involved.
        send(
            events,
            LoginEvent::progress(Localized::fixed(
                "login.progress.cursor_import",
                "Reading the authorized Cursor session, read-only...",
            )),
        )
        .await;
        let login = session.import_cursor(config).await?;
        return Ok(CompletedLogin::new(
            provider,
            config.clone(),
            login,
            replaced_key,
        ));
    }
    send(
        events,
        LoginEvent::progress(Localized::fixed(
            "login.progress.preparing",
            "Preparing the authorization with the provider...",
        )),
    )
    .await;
    let pending = session.begin(provider, config).await?;
    let prompt = pending.prompt().clone();
    let url = prompt.url.clone();
    let instructions = Localized::fixed(
        "login.prompt.instructions",
        "Authorize in the system browser. Waiting for your authorization; no password is ever typed into Cuotai.",
    );
    send(
        events,
        LoginEvent::Prompt {
            url: prompt.url,
            user_code: prompt.user_code,
            instructions: instructions.text().to_string(),
            instructions_l10n: Some(instructions),
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
                    Some(note) = error_rx.recv() => {
                        let note: Localized = note;
                        send(
                            events,
                            LoginEvent::CodeRejected {
                                message: note.text().to_string(),
                                l10n: Some(note),
                            },
                        )
                        .await;
                    }
                }
            }
        }
        None => pending.finish().await?,
    };
    Ok(CompletedLogin::new(
        provider,
        config.clone(),
        login,
        replaced_key,
    ))
}

fn start_aws_sso(profile: Option<String>) -> LoginDriver {
    let (event_tx, events) = mpsc::channel(16);
    let job = tokio::spawn(async move {
        let result = run_aws_sso(profile, &event_tx).await;
        match result {
            Ok(login) => {
                let _ = event_tx
                    .send(LoginEvent::Finished {
                        params: login.params.clone(),
                    })
                    .await;
                Ok(FlowDone::External(login))
            }
            Err(error) => {
                let _ = event_tx.send(LoginEvent::failed(&error)).await;
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
) -> Result<ExternalLogin, OAuthError> {
    let profile = profile.filter(|p| !p.trim().is_empty()).ok_or_else(|| {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.login.aws_profile_required",
            "Supply the AWS profile configured via `aws configure sso`.",
        ))
    })?;
    let _ = events
        .send(LoginEvent::progress(Localized::fixed(
            "login.progress.aws_sso",
            "AWS SSO: open the URL and use the code the AWS CLI shows.",
        )))
        .await;
    let (lines_tx, mut lines_rx) = mpsc::channel(16);
    let result = {
        let login = aws::login(&profile, lines_tx);
        tokio::pin!(login);
        loop {
            tokio::select! {
                result = &mut login => break result,
                Some(line) = lines_rx.recv() => {
                    let _ = events
                        .send(LoginEvent::Progress {
                            text: line,
                            l10n: None,
                        })
                        .await;
                }
            }
        }
    };
    result?;
    Ok(ExternalLogin {
        params: BTreeMap::from([("profile".to_string(), profile)]),
    })
}

fn start_bailian_console(console_site: Option<String>, profile: Option<String>) -> LoginDriver {
    let profile = profile
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| account_edit::ensure_cli_profile(&mut BTreeMap::new()));
    let (event_tx, events) = mpsc::channel(8);
    let job = tokio::spawn(async move {
        let result = run_bailian_console(console_site, profile, &event_tx).await;
        match result {
            Ok(login) => {
                let _ = event_tx
                    .send(LoginEvent::Finished {
                        params: login.params.clone(),
                    })
                    .await;
                Ok(FlowDone::External(login))
            }
            Err(error) => {
                let _ = event_tx.send(LoginEvent::failed(&error)).await;
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
    profile: String,
    events: &mpsc::Sender<LoginEvent>,
) -> Result<ExternalLogin, OAuthError> {
    let site = console_site
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(OAuthError::bailian_console_site_blank)?;
    // Static, generic messages — never the CLI's stdout content.
    let _ = events
        .send(LoginEvent::progress(Localized::fixed(
            "login.progress.bailian_start",
            "Starting `bl auth login --console`. The browser may open automatically.",
        )))
        .await;
    bailian::console_login(&site, &profile).await?;
    let _ = events
        .send(LoginEvent::progress(Localized::fixed(
            "login.progress.bailian_done",
            "The `bl` login finished in the dedicated profile. Cuotai stores NONE of it.",
        )))
        .await;
    let _ = events
        .send(LoginEvent::progress(Localized::fixed(
            "login.progress.bailian_profile",
            "The Token Plan query will use this profile via `bl --config <name>`. `params.session_env` stays an optional override (manual Cookie) if you prefer the old route.",
        )))
        .await;
    Ok(ExternalLogin {
        params: BTreeMap::from([("cli_profile".to_string(), profile)]),
    })
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
    /// Key referenced by the account before this login, if any. It is only
    /// eligible for cleanup after the replacement TOML was saved.
    replaced_key: Option<String>,
    login: Arc<AuthorizedLogin>,
}

impl CompletedLogin {
    fn new(
        provider: Provider,
        config: OAuthConfig,
        login: AuthorizedLogin,
        replaced_key: Option<String>,
    ) -> Self {
        let params = login.account_params(provider, &config);
        Self {
            provider,
            config,
            params,
            replaced_key,
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

/// Non-fatal post-save cleanup failure: the TOML and NEW credential are
/// saved, while the replaced credential remains in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupFailure {
    pub message: String,
}

/// Outcome of [`commit_login`]. On [`LoginCommitOutcome::PartialFailure`] the
/// [`CompletedLogin`] is untouched and the commit may simply be retried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginCommitOutcome {
    Saved,
    SavedWithCleanupFailure(CleanupFailure),
    PartialFailure(PartialFailure),
}

/// Commit an authorized login: write the credential to the vault, then save
/// the new configuration to the TOML. Ordering and compensation are owned by
/// the core (RF2.4/RF2.8):
///
/// 1. The prior token under the same key is captured (reauth case).
/// 2. The new token is written; a failure here writes nothing anywhere.
/// 3. `file.save(config)` persists the TOML; on failure the vault is
///    compensated — restore the prior token, or remove the brand-new one.
/// 4. Only after a successful save, a different key used by the replaced
///    account is deleted unless the saved config still shares it. A deletion
///    failure is a typed, non-fatal outcome; both credentials remain saved.
///
/// A replaced token is never deleted without a successful restore
/// ([`Compensation::Failed`] reports that state instead). Retrying with the
/// same [`CompletedLogin`] and `config` is safe and idempotent. AWS SSO and
/// Bailian console logins write outside the Cuotai vault and are not part of
/// this commit.
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
        Ok(()) => commit.confirm(session, &file.config).await,
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
    let replaced_key = login.replaced_key.clone().filter(|old| old != &key);
    Ok(CredentialCommit {
        key,
        prior,
        replaced_key,
    })
}

/// In-flight credential commit awaiting the TOML save verdict. Holds the
/// prior token for compensation; intentionally not `Debug`.
pub struct CredentialCommit {
    key: String,
    prior: Option<super::OAuthToken>,
    replaced_key: Option<String>,
}

impl CredentialCommit {
    /// The TOML save succeeded. Only now may a replaced, unshared key be
    /// removed; failure keeps the old token and returns a non-fatal outcome.
    pub async fn confirm(
        self,
        session: &OAuthSession,
        saved_config: &Config,
    ) -> LoginCommitOutcome {
        let Some(previous) = self.replaced_key else {
            return LoginCommitOutcome::Saved;
        };
        let shared = saved_config
            .accounts
            .iter()
            .filter_map(account_edit::account_storage_key)
            .any(|key| key == previous);
        if shared {
            return LoginCommitOutcome::Saved;
        }
        match session.delete_by_key(previous).await {
            Ok(()) => LoginCommitOutcome::Saved,
            Err(_) => LoginCommitOutcome::SavedWithCleanupFailure(CleanupFailure {
                message: "The configuration and the new credential were saved; the replaced credential remains in the vault because it could not be removed.".into(),
            }),
        }
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

    /// The Bailian flow reports the profile it logged in as `cli_profile`. When the optional
    /// "dedicated profile" field was left blank (the core generated one), saving must keep that
    /// generated profile instead of failing with "did not report the dedicated profile".
    #[test]
    fn bailian_save_uses_the_profile_reported_by_the_completed_login() {
        let options = BTreeMap::from([
            ("console_site".to_string(), "international".to_string()),
            ("profile".to_string(), String::new()),
        ]);
        let reported = BTreeMap::from([("cli_profile".to_string(), "cuotai-abc123".to_string())]);
        let (credential, params) = account_login_inputs(
            Provider::AlibabaTokenPlan,
            MODE_BAILIAN_CONSOLE,
            "",
            &options,
            &reported,
            true,
        )
        .expect("generated profile is kept");
        assert_eq!(credential, "");
        assert_eq!(
            params.unwrap().get("cli_profile").map(String::as_str),
            Some("cuotai-abc123")
        );

        // A typed profile still works when the result uses the option key.
        let typed = BTreeMap::from([("profile".to_string(), "qwen-personal".to_string())]);
        let (_, params) = account_login_inputs(
            Provider::AlibabaTokenPlan,
            MODE_BAILIAN_CONSOLE,
            "",
            &options,
            &typed,
            true,
        )
        .unwrap();
        assert_eq!(
            params.unwrap().get("cli_profile").map(String::as_str),
            Some("qwen-personal")
        );

        // A completed login that reports nothing, with nothing typed, is still an error.
        let error = account_login_inputs(
            Provider::AlibabaTokenPlan,
            MODE_BAILIAN_CONSOLE,
            "",
            &options,
            &BTreeMap::new(),
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("dedicated profile"));
    }

    use crate::config_file::ConfigFile;
    use crate::oauth::session::storage_key;
    use crate::oauth::{OAuthToken, TokenStore};
    use crate::Account;
    use parking_lot::Mutex;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct MemoryStore {
        tokens: Mutex<HashMap<String, OAuthToken>>,
        fail_deletes: Mutex<HashSet<String>>,
    }
    impl MemoryStore {
        fn len(&self) -> usize {
            self.tokens.lock().len()
        }
    }
    impl TokenStore for MemoryStore {
        fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
            Ok(self.tokens.lock().get(id).cloned())
        }
        fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
            self.tokens.lock().insert(id.into(), token.clone());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), OAuthError> {
            if self.fail_deletes.lock().contains(id) {
                return Err(OAuthError::Storage(crate::text::Localized::fixed(
                    "oauth_error.login.storage_failure",
                    "falha",
                )));
            }
            self.tokens.lock().remove(id);
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

    fn empty_config() -> Config {
        Config {
            refresh_seconds: 60,
            concurrency: 2,
            timeout_seconds: 10,
            accounts: vec![],
            groups: Vec::new(),
        }
    }

    fn config_with(accounts: Vec<Account>) -> Config {
        Config {
            accounts,
            ..empty_config()
        }
    }

    fn oauth_account(name: &str, provider: Provider, config: OAuthConfig) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider,
            credential_env: None,
            session: None,
            oauth: Some(config),
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn request_for(provider: Provider, existing: Option<&Account>) -> LoginRequest {
        let mut request = LoginRequest::for_account(provider, MODE_OAUTH, existing);
        if requires_client_registration(provider) && !request.registration.contains_key("client_id")
        {
            request
                .registration
                .insert("client_id".into(), "test-client".into());
        }
        request
    }

    fn completed_from(request: LoginRequest, provider: Provider) -> CompletedLogin {
        let replaced_key = request
            .replaced_account
            .as_ref()
            .and_then(account_edit::account_storage_key);
        let config = oauth_config_from(request);
        let key = storage_key(provider, &config).unwrap();
        CompletedLogin::new(
            provider,
            config,
            crate::oauth::AuthorizedLogin::for_test(key, token("access-new")),
            replaced_key,
        )
    }

    fn completed(provider: Provider) -> CompletedLogin {
        completed_from(request_for(provider, None), provider)
    }

    #[test]
    fn login_modes_describe_every_flow_and_registration_field() {
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
            assert!(modes.modes[0].registration.is_empty());
            assert!(modes.unsupported_reason.is_none());
        }

        let google = login_modes(Provider::GeminiApi);
        let keys: Vec<&str> = google.modes[0]
            .registration
            .iter()
            .map(|option| option.key.as_str())
            .collect();
        assert_eq!(keys, ["client_id", "client_secret_env"]);
        let azure = login_modes(Provider::AzureOpenAi);
        let keys: Vec<&str> = azure.modes[0]
            .registration
            .iter()
            .map(|option| option.key.as_str())
            .collect();
        assert_eq!(keys, ["client_id", "tenant"]);

        let openai = login_modes(Provider::OpenAiApi);
        assert!(openai.modes.is_empty());
        assert!(openai.unsupported_reason.is_some());
    }

    #[test]
    fn oauth_mode_options_match_native_descriptors() {
        for provider in [Provider::Kiro, Provider::Qwen, Provider::Cursor] {
            let modes = login_modes(provider);
            let declared: Vec<&str> = account_edit::native_option_descriptors(provider)
                .map(|d| d.key)
                .collect();
            for option in &modes.modes[0].options {
                assert!(declared.contains(&option.key.as_str()));
            }
        }
    }

    #[test]
    fn kiro_native_login_exposes_exactly_builder_id_and_idc() {
        let modes = login_modes(Provider::Kiro);
        let oauth = modes
            .modes
            .iter()
            .find(|mode| mode.id == MODE_OAUTH)
            .expect("Kiro OAuth mode");
        let auth_method = oauth
            .options
            .iter()
            .find(|option| option.key == "auth_method")
            .expect("shared auth_method option");
        assert_eq!(
            auth_method
                .choices
                .as_ref()
                .expect("auth_method choices")
                .iter()
                .map(|choice| choice.value.as_str())
                .collect::<Vec<_>>(),
            ["builder-id", "idc"]
        );
    }

    #[test]
    fn events_serialize_without_secrets() {
        let event = LoginEvent::Prompt {
            url: "https://example.com/authorize".into(),
            user_code: Some("ABCD-EFGH".into()),
            instructions: "instrucoes".into(),
            instructions_l10n: None,
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

    #[test]
    fn start_login_rejects_mode_not_offered() {
        let (session, _) = session();
        let result = start_login(
            Arc::new(session),
            Provider::Kiro,
            LoginRequest::new(MODE_AWS_SSO),
        );
        assert!(matches!(result, Err(OAuthError::Configuration(_))));
    }

    #[tokio::test]
    async fn commit_vault_failure_leaves_toml_untouched() {
        struct FailingStore;
        impl TokenStore for FailingStore {
            fn load(&self, _: &str) -> Result<Option<OAuthToken>, OAuthError> {
                Ok(None)
            }
            fn save(&self, _: &str, _: &OAuthToken) -> Result<(), OAuthError> {
                Err(OAuthError::Storage(crate::text::Localized::fixed(
                    "oauth_error.login.storage_failure",
                    "falha",
                )))
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

    #[tokio::test]
    async fn reauth_same_identity_overwrites_one_vault_entry() {
        let (session, store) = session();
        let old = oauth_account(
            "Kiro",
            Provider::Kiro,
            oauth_config_from(request_for(Provider::Kiro, None)),
        );
        let old_key = account_edit::account_storage_key(&old).unwrap();
        store.save(&old_key, &token("access-old")).unwrap();
        let request = request_for(Provider::Kiro, Some(&old));
        assert_eq!(
            request.credential_id(),
            Some(old.oauth.as_ref().unwrap().credential_id.as_str())
        );
        let completed = completed_from(request, Provider::Kiro);
        let new_key = storage_key(Provider::Kiro, &completed.config).unwrap();
        assert_eq!(new_key, old_key);

        let dir = tempfile::tempdir().unwrap();
        let mut file = ConfigFile::load(Some(dir.path().join("cuotai.toml")), true).unwrap();
        file.save(config_with(vec![old])).unwrap();
        let replacement = oauth_account("Kiro", Provider::Kiro, completed.config.clone());
        let outcome = commit_login(
            &mut file,
            &session,
            &completed,
            config_with(vec![replacement]),
        )
        .await;
        assert_eq!(outcome, LoginCommitOutcome::Saved);
        assert_eq!(store.len(), 1);
        assert_eq!(
            store.load(&old_key).unwrap().unwrap().access_token,
            "access-new"
        );
    }

    #[tokio::test]
    async fn staged_identity_change_deletes_replaced_key_only_after_save() {
        let (session, store) = session();
        let mut old_oauth = oauth_config_from(request_for(Provider::Kiro, None));
        old_oauth
            .options
            .insert("region".into(), "us-east-1".into());
        let old = oauth_account("Kiro", Provider::Kiro, old_oauth);
        let old_key = account_edit::account_storage_key(&old).unwrap();
        store.save(&old_key, &token("access-old")).unwrap();
        let mut request = request_for(Provider::Kiro, Some(&old));
        request.options.insert("region".into(), "us-west-2".into());
        let completed = completed_from(request, Provider::Kiro);
        let new_key = storage_key(Provider::Kiro, &completed.config).unwrap();
        assert_ne!(new_key, old_key);

        let dir = tempfile::tempdir().unwrap();
        let mut file = ConfigFile::load(Some(dir.path().join("cuotai.toml")), true).unwrap();
        file.save(config_with(vec![old])).unwrap();
        let commit = begin_credential_commit(&session, &completed).await.unwrap();
        assert!(store.load(&old_key).unwrap().is_some());
        let replacement = oauth_account("Kiro", Provider::Kiro, completed.config.clone());
        file.save(config_with(vec![replacement])).unwrap();
        assert_eq!(
            commit.confirm(&session, &file.config).await,
            LoginCommitOutcome::Saved
        );
        assert!(store.load(&old_key).unwrap().is_none());
        assert_eq!(
            store.load(&new_key).unwrap().unwrap().access_token,
            "access-new"
        );
        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn identity_change_keeps_replaced_key_shared_by_survivor() {
        let (session, store) = session();
        let old_oauth = oauth_config_from(request_for(Provider::Kiro, None));
        let first = oauth_account("First", Provider::Kiro, old_oauth.clone());
        let shared = oauth_account("Shared", Provider::Kiro, old_oauth);
        let old_key = account_edit::account_storage_key(&first).unwrap();
        store.save(&old_key, &token("access-old")).unwrap();
        let mut request = request_for(Provider::Kiro, Some(&first));
        request.options.insert("region".into(), "us-west-2".into());
        let completed = completed_from(request, Provider::Kiro);
        let new_key = storage_key(Provider::Kiro, &completed.config).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let mut file = ConfigFile::load(Some(dir.path().join("cuotai.toml")), true).unwrap();
        file.save(config_with(vec![first, shared.clone()])).unwrap();
        let replacement = oauth_account("First", Provider::Kiro, completed.config.clone());
        let outcome = commit_login(
            &mut file,
            &session,
            &completed,
            config_with(vec![replacement, shared]),
        )
        .await;
        assert_eq!(outcome, LoginCommitOutcome::Saved);
        assert!(store.load(&old_key).unwrap().is_some());
        assert!(store.load(&new_key).unwrap().is_some());
        assert_eq!(store.len(), 2);
    }

    #[tokio::test]
    async fn cleanup_failure_is_non_fatal_and_keeps_both_tokens() {
        let (session, store) = session();
        let old = oauth_account(
            "Kiro",
            Provider::Kiro,
            oauth_config_from(request_for(Provider::Kiro, None)),
        );
        let old_key = account_edit::account_storage_key(&old).unwrap();
        store.save(&old_key, &token("access-old")).unwrap();
        store.fail_deletes.lock().insert(old_key.clone());
        let mut request = request_for(Provider::Kiro, Some(&old));
        request.options.insert("region".into(), "us-west-2".into());
        let completed = completed_from(request, Provider::Kiro);
        let new_key = storage_key(Provider::Kiro, &completed.config).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let mut file = ConfigFile::load(Some(dir.path().join("cuotai.toml")), true).unwrap();
        file.save(config_with(vec![old])).unwrap();
        let replacement = oauth_account("Kiro", Provider::Kiro, completed.config.clone());
        let outcome = commit_login(
            &mut file,
            &session,
            &completed,
            config_with(vec![replacement]),
        )
        .await;
        assert!(matches!(
            outcome,
            LoginCommitOutcome::SavedWithCleanupFailure(_)
        ));
        assert!(store.load(&old_key).unwrap().is_some());
        assert!(store.load(&new_key).unwrap().is_some());
        assert_eq!(file.config.accounts[0].oauth, Some(completed.config));
    }

    #[tokio::test]
    async fn save_failure_never_deletes_replaced_key() {
        let (session, store) = session();
        let old = oauth_account(
            "Kiro",
            Provider::Kiro,
            oauth_config_from(request_for(Provider::Kiro, None)),
        );
        let old_key = account_edit::account_storage_key(&old).unwrap();
        store.save(&old_key, &token("access-old")).unwrap();
        let mut request = request_for(Provider::Kiro, Some(&old));
        request.options.insert("region".into(), "us-west-2".into());
        let completed = completed_from(request, Provider::Kiro);
        let new_key = storage_key(Provider::Kiro, &completed.config).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = ConfigFile::load(Some(path.clone()), true).unwrap();
        file.save(config_with(vec![old])).unwrap();
        std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();
        let replacement = oauth_account("Kiro", Provider::Kiro, completed.config.clone());
        let outcome = commit_login(
            &mut file,
            &session,
            &completed,
            config_with(vec![replacement]),
        )
        .await;
        let LoginCommitOutcome::PartialFailure(failure) = outcome else {
            panic!("esperava falha parcial");
        };
        assert_eq!(failure.compensation, Compensation::NewRemoved);
        assert!(store.load(&old_key).unwrap().is_some());
        assert!(store.load(&new_key).unwrap().is_none());
    }

    #[tokio::test]
    async fn commit_config_conflict_restores_prior_token() {
        let (session, store) = session();
        let completed = completed(Provider::GeminiApi);
        let key = storage_key(Provider::GeminiApi, &completed.config).unwrap();
        store.save(&key, &token("access-old")).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = ConfigFile::load(Some(path.clone()), true).unwrap();
        file.save(empty_config()).unwrap();
        std::fs::write(&path, "refresh_seconds = 300\naccounts = []\n").unwrap();

        let outcome = commit_login(&mut file, &session, &completed, empty_config()).await;
        let LoginCommitOutcome::PartialFailure(failure) = outcome else {
            panic!("esperava falha parcial");
        };
        assert_eq!(failure.stage, CommitStage::Config);
        assert!(!failure.config_saved);
        assert!(failure.credential_saved);
        assert_eq!(failure.compensation, Compensation::PreviousRestored);
        assert_eq!(
            store.load(&key).unwrap().unwrap().access_token,
            "access-old"
        );

        file.reload().unwrap();
        let outcome = commit_login(&mut file, &session, &completed, empty_config()).await;
        assert_eq!(outcome, LoginCommitOutcome::Saved);
        assert_eq!(
            store.load(&key).unwrap().unwrap().access_token,
            "access-new"
        );
    }

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

    #[tokio::test]
    async fn dropped_completed_login_writes_nothing() {
        let (_, store) = session();
        let completed = completed(Provider::Kiro);
        let key = storage_key(Provider::Kiro, &completed.config).unwrap();
        drop(completed);
        assert!(store.load(&key).unwrap().is_none());
    }
}
