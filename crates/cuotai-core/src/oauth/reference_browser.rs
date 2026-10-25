//! Browser-consent OAuth flows ported from 9router f01fb909 (`src/lib/oauth`).
//! Each provider uses the native public client identifier observed in the
//! reference implementation; no user-registered application is required.
//!
//! - `Claude`  → `providers/claude.js`  (authorization code + PKCE S256)
//! - `ChatGpt` → `providers/codex.js`   (authorization code + PKCE S256,
//!   fixed loopback port 1455 + `/auth/callback`)
//! - `GeminiAi`→ `providers/gemini-cli.js` (Google desktop client, standard
//!   authorization code, public non-confidential client secret)
//! - `Cline`   → `providers/cline.js`   (loopback callback whose `code` is a
//!   base64 token JSON payload, with a server exchange as fallback)
//!
//! The provider alone decides whether these native-client flows remain
//! usable; this is a compatibility client, not an official integration.
//! Token material and provider payloads never appear in error text or logs.

use super::send_secure;
use super::{LoginPrompt, OAuthConfig, OAuthError, OAuthToken};
use crate::Provider;
use base64::engine::general_purpose::{STANDARD as B64_STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::header::ACCEPT;
use reqwest::StatusCode;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, timeout_at, Instant};

// ---------------------------------------------------------------------------
// Native client constants (exact values from the 9router registry).
// ---------------------------------------------------------------------------

const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const CLAUDE_TOKEN_URL: &str = "https://api.anthropic.com/v1/oauth/token";
const CLAUDE_SCOPE: &str = "org:create_api_key user:profile user:inference";
const CLAUDE_AUTHORIZE_HOSTS: &[&str] = &["claude.ai"];

const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CODEX_SCOPE: &str = "openid profile email offline_access";
const CODEX_FIXED_PORT: u16 = 1455;
const CODEX_CALLBACK_PATH: &str = "/auth/callback";
const CODEX_AUTHORIZE_HOSTS: &[&str] = &["auth.openai.com"];

// Public Gemini CLI desktop client (google-gemini/gemini-cli, mirrored by the
// 9router registry). Installed-app clients cannot keep secrets; Google
// documents this pair as non-confidential.
const GEMINI_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GEMINI_CLIENT_SECRET: &str = "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";
const GEMINI_AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GEMINI_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GEMINI_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo";
const GEMINI_LOAD_CODE_ASSIST_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
const GEMINI_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform \
    https://www.googleapis.com/auth/userinfo.email \
    https://www.googleapis.com/auth/userinfo.profile";
const GEMINI_AUTHORIZE_HOSTS: &[&str] = &["accounts.google.com"];

const CLINE_AUTHORIZE_URL: &str = "https://api.cline.bot/api/v1/auth/authorize";
const CLINE_TOKEN_EXCHANGE_URL: &str = "https://api.cline.bot/api/v1/auth/token";
const CLINE_REFRESH_URL: &str = "https://api.cline.bot/api/v1/auth/refresh";
const CLINE_AUTHORIZE_HOSTS: &[&str] = &["api.cline.bot"];

// OpenRouter documents this flow for any app (no client registration):
// https://openrouter.ai/docs/guides/overview/auth/oauth. The exchange returns
// an ordinary inference API key; it has no expiry and no refresh token.
const OPENROUTER_AUTHORIZE_URL: &str = "https://openrouter.ai/auth";
const OPENROUTER_KEY_EXCHANGE_URL: &str = "https://openrouter.ai/api/v1/auth/keys";
const OPENROUTER_AUTHORIZE_HOSTS: &[&str] = &["openrouter.ai"];

// ---------------------------------------------------------------------------
// Shared bounds.
// ---------------------------------------------------------------------------

/// Whole-login deadline (consent + callback + exchange), capped at 10 min.
const LOGIN_TOTAL_WAIT: Duration = Duration::from_secs(600);
const CALLBACK_CONN_DEADLINE: Duration = Duration::from_secs(15);
const MAX_HEADER_BYTES: usize = 8192;
const MAX_RESPONSE_BODY: usize = 128 * 1024;
const RESPONSE_DEADLINE: Duration = Duration::from_secs(5);
const MAX_EXPIRES_IN_SECS: i64 = 31_536_000; // one year
/// Clock-skew leeway when sanity-checking a freshly issued JWT `exp`.
const JWT_EXP_SKEW_SECS: i64 = 300;
/// Cline's reference client defaults the lifetime to one hour when the
/// provider omits `expiresAt`.
const CLINE_DEFAULT_EXPIRES_SECS: i64 = 3600;

// ---------------------------------------------------------------------------
// Endpoint overrides (production uses the official constants only).
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Endpoints {
    authorize_url: String,
    authorize_hosts: Vec<String>,
    require_https: bool,
    token_url: String,
    /// Cline refresh endpoint (other providers refresh at `token_url`).
    refresh_url: Option<String>,
    /// Gemini CLI post-exchange lookups.
    userinfo_url: Option<String>,
    load_code_assist_url: Option<String>,
    fixed_port: Option<u16>,
    /// `None` (Cline) means the provider sends no state: the callback path
    /// itself is a random per-login capability.
    callback_path: Option<String>,
    /// Host part of the loopback redirect_uri. Codex's registered public
    /// client expects exactly `localhost`; everyone else accepts either.
    redirect_host: &'static str,
}

impl Endpoints {
    fn official(provider: Provider) -> Result<Self, OAuthError> {
        let endpoints = match provider {
            Provider::Claude => Self {
                authorize_url: CLAUDE_AUTHORIZE_URL.into(),
                authorize_hosts: hosts(CLAUDE_AUTHORIZE_HOSTS),
                require_https: true,
                token_url: CLAUDE_TOKEN_URL.into(),
                refresh_url: None,
                userinfo_url: None,
                load_code_assist_url: None,
                fixed_port: None,
                callback_path: Some("/callback".into()),
                redirect_host: "localhost",
            },
            Provider::ChatGpt => Self {
                authorize_url: CODEX_AUTHORIZE_URL.into(),
                authorize_hosts: hosts(CODEX_AUTHORIZE_HOSTS),
                require_https: true,
                token_url: CODEX_TOKEN_URL.into(),
                refresh_url: None,
                userinfo_url: None,
                load_code_assist_url: None,
                fixed_port: Some(CODEX_FIXED_PORT),
                callback_path: Some(CODEX_CALLBACK_PATH.into()),
                // Codex's public client is registered with `http://localhost`
                // as the redirect host; `127.0.0.1` is a different origin
                // and would be rejected with redirect_uri_mismatch.
                redirect_host: "localhost",
            },
            Provider::GeminiAi => Self {
                authorize_url: GEMINI_AUTHORIZE_URL.into(),
                authorize_hosts: hosts(GEMINI_AUTHORIZE_HOSTS),
                require_https: true,
                token_url: GEMINI_TOKEN_URL.into(),
                refresh_url: None,
                userinfo_url: Some(GEMINI_USERINFO_URL.into()),
                load_code_assist_url: Some(GEMINI_LOAD_CODE_ASSIST_URL.into()),
                fixed_port: None,
                callback_path: Some("/callback".into()),
                redirect_host: "localhost",
            },
            Provider::Cline => Self {
                authorize_url: CLINE_AUTHORIZE_URL.into(),
                authorize_hosts: hosts(CLINE_AUTHORIZE_HOSTS),
                require_https: true,
                token_url: CLINE_TOKEN_EXCHANGE_URL.into(),
                refresh_url: Some(CLINE_REFRESH_URL.into()),
                userinfo_url: None,
                load_code_assist_url: None,
                fixed_port: None,
                callback_path: None,
                redirect_host: "localhost",
            },
            Provider::OpenRouter => Self {
                authorize_url: OPENROUTER_AUTHORIZE_URL.into(),
                authorize_hosts: hosts(OPENROUTER_AUTHORIZE_HOSTS),
                require_https: true,
                token_url: OPENROUTER_KEY_EXCHANGE_URL.into(),
                refresh_url: None,
                userinfo_url: None,
                load_code_assist_url: None,
                fixed_port: None,
                // No state parameter exists: the random callback path is the
                // CSRF capability, as for Cline.
                callback_path: None,
                redirect_host: "localhost",
            },
            other => {
                return Err(OAuthError::Configuration(
                    crate::text::Localized::with_param(
                        "oauth_error.browser.provider_no_oauth",
                        "provider",
                        crate::text::provider_l10n(other),
                        "{provider} does not support browser OAuth login.",
                    ),
                ))
            }
        };
        Ok(endpoints)
    }
}

fn hosts(fixed: &[&str]) -> Vec<String> {
    fixed.iter().map(|h| (*h).to_owned()).collect()
}

// ---------------------------------------------------------------------------
// Public contract (dispatched by `reference.rs`).
// ---------------------------------------------------------------------------

/// An in-progress browser-consent login. Dropping it (or the `finish`
/// future) closes the loopback listener immediately.
pub(super) struct PendingLogin {
    prompt: LoginPrompt,
    login: LoopbackLogin,
}

impl PendingLogin {
    pub(super) fn prompt(&self) -> &LoginPrompt {
        &self.prompt
    }

    pub(super) async fn finish(self) -> Result<OAuthToken, OAuthError> {
        self.login.finish().await
    }
}

pub(super) async fn begin(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
) -> Result<PendingLogin, OAuthError> {
    begin_with(http, provider, config, Endpoints::official(provider)?).await
}

pub(super) async fn refresh(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    token: &OAuthToken,
) -> Result<OAuthToken, OAuthError> {
    let endpoints = Endpoints::official(provider)?;
    refresh_with(http, provider, config, token, &endpoints).await
}

// ---------------------------------------------------------------------------
// begin: bind the loopback listener, then build the consent URL.
// ---------------------------------------------------------------------------

struct LoopbackLogin {
    http: reqwest::Client,
    listener: TcpListener,
    redirect_uri: String,
    callback_path: String,
    /// Token/exchange endpoint (production: the official constant).
    token_url: String,
    /// Gemini CLI post-exchange lookups; absent disables them.
    userinfo_url: Option<String>,
    load_code_assist_url: Option<String>,
    /// `None` for Cline: no state parameter exists, so the random path is
    /// the only callback capability.
    expected_state: Option<String>,
    deadline: Instant,
    exchange: Exchange,
}

enum Exchange {
    Claude {
        client_id: String,
        code_verifier: String,
    },
    Codex {
        client_id: String,
        code_verifier: String,
    },
    Gemini {
        client_id: String,
        // The pending login keeps the configuration (the variable's name), never
        // the client secret: the value is read again in `finish`, only while the
        // exchange form is built.
        config: Box<OAuthConfig>,
        code_verifier: String,
    },
    Cline,
    /// PKCE code exchanged for an inference API key.
    OpenRouter {
        code_verifier: String,
    },
}

async fn begin_with(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    endpoints: Endpoints,
) -> Result<PendingLogin, OAuthError> {
    // Codex registers exactly one redirect URI (fixed port + path); a busy
    // port means another client is mid-login.
    let bind_port = endpoints.fixed_port.unwrap_or(0);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, bind_port))
        .await
        .map_err(|_| match endpoints.fixed_port {
            Some(port) => OAuthError::Configuration(crate::text::Localized::with_param(
                "oauth_error.browser.local_port_in_use",
                "port",
                port,
                "The local port {port} required by this login is in use; wait for the login in progress to finish.",
            )),
            None => OAuthError::Network,
        })?;
    let port = listener
        .local_addr()
        .map_err(|_| OAuthError::Network)?
        .port();
    let callback_path = match &endpoints.callback_path {
        Some(path) => path.clone(),
        None => format!("/{}", random_url_safe_32()?),
    };
    let redirect_uri = format!("http://{}:{port}{callback_path}", endpoints.redirect_host);
    let deadline = Instant::now() + LOGIN_TOTAL_WAIT;

    let (url, exchange, expected_state) = match provider {
        Provider::Claude => {
            let client_id = native_client_id(config, CLAUDE_CLIENT_ID);
            let verifier = random_url_safe_32()?;
            let state = random_url_safe_32()?;
            let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            // providers/claude.js buildAuthUrl (note the literal code=true).
            let query = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("code", "true"),
                    ("client_id", client_id.as_str()),
                    ("response_type", "code"),
                    ("redirect_uri", redirect_uri.as_str()),
                    ("scope", CLAUDE_SCOPE),
                    ("code_challenge", challenge.as_str()),
                    ("code_challenge_method", "S256"),
                    ("state", state.as_str()),
                ])
                .finish();
            (
                format!("{}?{query}", endpoints.authorize_url),
                Exchange::Claude {
                    client_id,
                    code_verifier: verifier,
                },
                Some(state),
            )
        }
        Provider::ChatGpt => {
            let client_id = native_client_id(config, CODEX_CLIENT_ID);
            let verifier = random_url_safe_32()?;
            let state = random_url_safe_32()?;
            let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            // providers/codex.js buildAuthUrl encodes with encodeURIComponent
            // (spaces as %20); form_urlencoded emits '+', so normalize.
            let query = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("response_type", "code"),
                    ("client_id", client_id.as_str()),
                    ("redirect_uri", redirect_uri.as_str()),
                    ("scope", CODEX_SCOPE),
                    ("code_challenge", challenge.as_str()),
                    ("code_challenge_method", "S256"),
                    ("id_token_add_organizations", "true"),
                    ("codex_cli_simplified_flow", "true"),
                    ("originator", "codex_cli_rs"),
                    ("state", state.as_str()),
                ])
                .finish()
                .replace('+', "%20");
            (
                format!("{}?{query}", endpoints.authorize_url),
                Exchange::Codex {
                    client_id,
                    code_verifier: verifier,
                },
                Some(state),
            )
        }
        Provider::GeminiAi => {
            let client_id = gemini_client_id(config);
            // Validate only: the value is dropped here and resolved again at
            // `finish`.
            ensure_gemini_client_ready(config)?;
            let state = random_url_safe_32()?;
            // PKCE S256 is optional in the reference gemini-cli flow but
            // Google supports it on every installed-app client and it
            // raises the bar against code interception; no downside.
            let verifier = random_url_safe_32()?;
            let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            let query = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("client_id", client_id.as_str()),
                    ("response_type", "code"),
                    ("redirect_uri", redirect_uri.as_str()),
                    ("scope", GEMINI_SCOPE),
                    ("state", state.as_str()),
                    ("access_type", "offline"),
                    ("prompt", "consent"),
                    ("code_challenge", challenge.as_str()),
                    ("code_challenge_method", "S256"),
                ])
                .finish();
            (
                format!("{}?{query}", endpoints.authorize_url),
                Exchange::Gemini {
                    client_id,
                    config: Box::new(config.clone()),
                    code_verifier: verifier,
                },
                Some(state),
            )
        }
        Provider::Cline => {
            // providers/cline.js buildAuthUrl: no state support; the random
            // callback path above is the CSRF capability.
            let query = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("client_type", "extension"),
                    ("callback_url", redirect_uri.as_str()),
                    ("redirect_uri", redirect_uri.as_str()),
                ])
                .finish();
            (
                format!("{}?{query}", endpoints.authorize_url),
                Exchange::Cline,
                None,
            )
        }
        Provider::OpenRouter => {
            let verifier = random_url_safe_32()?;
            let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            let query = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("callback_url", redirect_uri.as_str()),
                    ("code_challenge", challenge.as_str()),
                    ("code_challenge_method", "S256"),
                ])
                .finish();
            (
                format!("{}?{query}", endpoints.authorize_url),
                Exchange::OpenRouter {
                    code_verifier: verifier,
                },
                None,
            )
        }
        other => {
            return Err(OAuthError::Configuration(
                crate::text::Localized::with_param(
                    "oauth_error.browser.provider_no_oauth",
                    "provider",
                    crate::text::provider_l10n(other),
                    "{provider} does not support browser OAuth login.",
                ),
            ))
        }
    };
    validate_url_host(&url, &endpoints.authorize_hosts, endpoints.require_https)?;

    Ok(PendingLogin {
        prompt: LoginPrompt {
            url,
            user_code: None,
        },
        login: LoopbackLogin {
            http: http.clone(),
            listener,
            redirect_uri,
            callback_path,
            token_url: endpoints.token_url,
            userinfo_url: endpoints.userinfo_url,
            load_code_assist_url: endpoints.load_code_assist_url,
            expected_state,
            deadline,
            exchange,
        },
    })
}

impl LoopbackLogin {
    async fn finish(self) -> Result<OAuthToken, OAuthError> {
        let endpoints_token_url = self.token_url.as_str();
        let callback = self.wait_for_callback().await?;
        let now = Utc::now();
        match &self.exchange {
            Exchange::Claude {
                client_id,
                code_verifier,
            } => {
                // providers/claude.js exchangeToken: the code may carry the
                // state after '#' (manual copy/paste of `code#state`).
                let (auth_code, code_state) = split_code_state(&callback.code);
                let state = code_state
                    .or(callback.state)
                    .or_else(|| self.expected_state.clone())
                    .ok_or(OAuthError::InvalidResponse)?;
                let body = serde_json::json!({
                    "code": auth_code,
                    "state": state,
                    "grant_type": "authorization_code",
                    "client_id": client_id,
                    "redirect_uri": self.redirect_uri,
                    "code_verifier": code_verifier,
                });
                let (status, body) = timeout_at(
                    self.deadline,
                    post_json(&self.http, endpoints_token_url, &body),
                )
                .await
                .map_err(|_| OAuthError::Expired)??;
                check_oauth_error(&body, status)?;
                parse_bearer_body(&body, None, now)
            }
            Exchange::Codex {
                client_id,
                code_verifier,
            } => {
                let form = [
                    ("grant_type", "authorization_code".to_owned()),
                    ("client_id", client_id.clone()),
                    ("code", callback.code),
                    ("redirect_uri", self.redirect_uri.clone()),
                    ("code_verifier", code_verifier.clone()),
                ];
                let (status, body) = timeout_at(
                    self.deadline,
                    post_form(&self.http, endpoints_token_url, &form),
                )
                .await
                .map_err(|_| OAuthError::Expired)??;
                check_oauth_error(&body, status)?;
                let mut metadata = BTreeMap::new();
                if let Some(id_token) = body.get("id_token").and_then(Value::as_str) {
                    codex_id_token_metadata(id_token, &mut metadata);
                }
                parse_bearer_body(&body, None, now).map(|mut token| {
                    token.metadata = metadata;
                    token
                })
            }
            Exchange::Gemini {
                client_id,
                config,
                code_verifier,
            } => {
                let client_secret = resolve_gemini_client_proof(config)?;
                let form = [
                    ("grant_type", "authorization_code".to_owned()),
                    ("client_id", client_id.clone()),
                    ("client_secret", client_secret),
                    ("code", callback.code),
                    ("redirect_uri", self.redirect_uri.clone()),
                    ("code_verifier", code_verifier.clone()),
                ];
                let (status, body) = timeout_at(
                    self.deadline,
                    post_form(&self.http, endpoints_token_url, &form),
                )
                .await
                .map_err(|_| OAuthError::Expired)??;
                check_oauth_error(&body, status)?;
                let mut token = parse_bearer_body(&body, None, now)?;
                // providers/gemini-cli.js postExchange: best-effort identity
                // + project lookup; failures leave the metadata empty and the
                // quota adapter resolves the project itself.
                self.gemini_post_exchange(&mut token).await;
                Ok(token)
            }
            Exchange::Cline => {
                // providers/cline.js exchangeToken: the callback code is
                // usually the token payload itself, base64-encoded; only
                // undecodable codes hit the server exchange endpoint.
                if let Some(token) = decode_cline_code(&callback.code, now) {
                    return Ok(token);
                }
                let body = serde_json::json!({
                    "grant_type": "authorization_code",
                    "code": callback.code,
                    "client_type": "extension",
                    "redirect_uri": self.redirect_uri,
                });
                let (status, body) = timeout_at(
                    self.deadline,
                    post_json(&self.http, endpoints_token_url, &body),
                )
                .await
                .map_err(|_| OAuthError::Expired)??;
                check_oauth_error(&body, status)?;
                parse_cline_token_envelope(&body, None, now)
            }
            Exchange::OpenRouter { code_verifier } => {
                let body = serde_json::json!({
                    "code": callback.code,
                    "code_verifier": code_verifier,
                    "code_challenge_method": "S256",
                });
                let (status, body) = timeout_at(
                    self.deadline,
                    post_json(&self.http, endpoints_token_url, &body),
                )
                .await
                .map_err(|_| OAuthError::Expired)??;
                check_oauth_error(&body, status)?;
                // Only the key is kept: `user_id` is never stored.
                let key = body
                    .get("key")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|key| !key.is_empty())
                    .ok_or(OAuthError::InvalidResponse)?;
                Ok(OAuthToken {
                    access_token: key.to_owned(),
                    refresh_token: None,
                    expires_at: None,
                    metadata: BTreeMap::new(),
                })
            }
        }
    }

    /// gemini-cli.js postExchange: userinfo (email) + loadCodeAssist
    /// (project id). Strictly best-effort and bounded by the login deadline.
    async fn gemini_post_exchange(&self, token: &mut OAuthToken) {
        let request = async {
            if let Some(url) = self.userinfo_url.as_deref() {
                if let Some(email) = gemini_fetch_email(&self.http, url, &token.access_token).await
                {
                    token.metadata.insert("email".into(), email);
                }
            }
            if let Some(url) = self.load_code_assist_url.as_deref() {
                if let Some(project) =
                    gemini_fetch_project_id(&self.http, url, &token.access_token).await
                {
                    token.metadata.insert("project_id".into(), project);
                }
            }
        };
        let _ = timeout_at(self.deadline, request).await;
    }

    /// Accept connections until a valid callback arrives or the deadline
    /// passes. Unrelated requests never trigger a token exchange.
    async fn wait_for_callback(&self) -> Result<Callback, OAuthError> {
        loop {
            let (mut stream, _) = timeout_at(self.deadline, self.listener.accept())
                .await
                .map_err(|_| OAuthError::Expired)?
                .map_err(|_| OAuthError::Network)?;
            let conn_deadline =
                std::cmp::min(self.deadline, Instant::now() + CALLBACK_CONN_DEADLINE);
            let outcome = match timeout_at(
                conn_deadline,
                read_request(
                    &mut stream,
                    &self.callback_path,
                    self.expected_state.as_deref(),
                ),
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) => {
                    if Instant::now() >= self.deadline {
                        return Err(OAuthError::Expired);
                    }
                    LoopbackOutcome::Ignored
                }
            };
            match outcome {
                LoopbackOutcome::Code(callback) => {
                    respond(
                        &mut stream,
                        "200 OK",
                        &super::callback_page::render(super::callback_page::CallbackPage::Success),
                    )
                    .await;
                    return Ok(callback);
                }
                LoopbackOutcome::Denied => {
                    respond(
                        &mut stream,
                        "200 OK",
                        &super::callback_page::render(super::callback_page::CallbackPage::Denied),
                    )
                    .await;
                    return Err(OAuthError::Denied);
                }
                LoopbackOutcome::Failed(error) => {
                    respond(
                        &mut stream,
                        "400 Bad Request",
                        &super::callback_page::render(super::callback_page::CallbackPage::Failed),
                    )
                    .await;
                    return Err(error);
                }
                LoopbackOutcome::Ignored => {
                    // No details: a stray request learns nothing from us.
                    respond(&mut stream, "404 Not Found", "").await;
                }
            }
        }
    }
}

struct Callback {
    code: String,
    state: Option<String>,
}

enum LoopbackOutcome {
    Code(Callback),
    Denied,
    /// Wrong path, wrong state, duplicate/malformed parameters, favicon, ...
    Ignored,
    Failed(OAuthError),
}

/// Read one HTTP request head (bounded) and extract the callback outcome.
/// `expected_state` is the issued CSRF token; `None` means the provider has
/// no state support and the path itself is the capability (Cline).
async fn read_request(
    stream: &mut TcpStream,
    expected_path: &str,
    expected_state: Option<&str>,
) -> LoopbackOutcome {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if head.len() >= MAX_HEADER_BYTES {
            return LoopbackOutcome::Ignored;
        }
        let read = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return LoopbackOutcome::Ignored,
            Ok(n) => n,
        };
        head.extend_from_slice(&chunk[..read]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&head);
    let request_line = head.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target), Some(_version)) = (parts.next(), parts.next(), parts.next())
    else {
        return LoopbackOutcome::Ignored;
    };
    if method != "GET" {
        return LoopbackOutcome::Ignored;
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    };
    if path != expected_path {
        return LoopbackOutcome::Ignored;
    }
    parse_callback_query(query.unwrap_or_default(), expected_state)
}

fn parse_callback_query(query: &str, expected_state: Option<&str>) -> LoopbackOutcome {
    if !valid_percent_encoding(query) {
        return LoopbackOutcome::Ignored;
    }
    let mut states = Vec::new();
    let mut codes = Vec::new();
    let mut errors = Vec::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "state" => states.push(value.into_owned()),
            "code" => codes.push(value.into_owned()),
            "error" => errors.push(value.into_owned()),
            _ => {}
        }
    }
    let state = match expected_state {
        // CSRF check: exactly one state and it must match the issued value.
        Some(expected) => {
            if states.len() != 1 || states[0] != expected {
                return LoopbackOutcome::Ignored;
            }
            Some(states.pop().expect("one state"))
        }
        // No state support: any state parameter is ignored (never trusted).
        None => None,
    };
    // Exactly one non-empty code XOR exactly one error; anything else
    // (duplicates, empty code, both present) is rejected without exchange.
    match (codes.as_slice(), errors.as_slice()) {
        ([code], []) if !code.is_empty() => LoopbackOutcome::Code(Callback {
            code: code.clone(),
            state,
        }),
        ([], [error]) if error == "access_denied" => LoopbackOutcome::Denied,
        ([], [_]) => LoopbackOutcome::Failed(OAuthError::InvalidResponse),
        _ => LoopbackOutcome::Ignored,
    }
}

/// `url::form_urlencoded::parse` is lossy on bad `%` escapes; precheck so a
/// malformed callback is ignored instead of being silently rewritten.
fn valid_percent_encoding(input: &str) -> bool {
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit()
            {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    true
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let write = async {
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
    };
    let _ = timeout(RESPONSE_DEADLINE, write).await;
}

// ---------------------------------------------------------------------------
// Refresh grants (tokenRefresh/providers.js + executors/default.js).
// ---------------------------------------------------------------------------

async fn refresh_with(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    token: &OAuthToken,
    endpoints: &Endpoints,
) -> Result<OAuthToken, OAuthError> {
    let refresh_token = token
        .refresh_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::Expired)?;
    let now = Utc::now();
    let mut metadata = token.metadata.clone();
    let previous = token.refresh_token.clone();
    match provider {
        Provider::Claude => {
            // tokenRefresh REFRESH_PROFILES.claude: JSON body, client_id only.
            let body = serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": native_client_id(config, CLAUDE_CLIENT_ID),
            });
            let (status, body) = post_json(http, &endpoints.token_url, &body).await?;
            check_oauth_error(&body, status)?;
            parse_bearer_body(&body, previous, now).map(|mut t| {
                t.metadata = metadata;
                t
            })
        }
        Provider::ChatGpt => {
            // Registry oauth.refresh for codex: form encoding + the login scope.
            let form = [
                ("grant_type", "refresh_token".to_owned()),
                ("refresh_token", refresh_token.to_owned()),
                ("client_id", native_client_id(config, CODEX_CLIENT_ID)),
                ("scope", CODEX_SCOPE.to_owned()),
            ];
            let (status, body) = post_form(http, &endpoints.token_url, &form).await?;
            check_oauth_error(&body, status)?;
            if let Some(id_token) = body.get("id_token").and_then(Value::as_str) {
                codex_id_token_metadata(id_token, &mut metadata);
            }
            parse_bearer_body(&body, previous, now).map(|mut t| {
                t.metadata = metadata;
                t
            })
        }
        Provider::GeminiAi => {
            let client_id = gemini_client_id(config);
            let client_secret = resolve_gemini_client_proof(config)?;
            let form = [
                ("grant_type", "refresh_token".to_owned()),
                ("refresh_token", refresh_token.to_owned()),
                ("client_id", client_id),
                ("client_secret", client_secret),
            ];
            let (status, body) = post_form(http, &endpoints.token_url, &form).await?;
            check_oauth_error(&body, status)?;
            parse_bearer_body(&body, previous, now).map(|mut t| {
                t.metadata = metadata;
                t
            })
        }
        Provider::Cline => {
            // tokenRefresh/providers.js refreshClineToken: JSON camelCase
            // envelope against the dedicated refresh endpoint.
            let refresh_url = endpoints
                .refresh_url
                .as_deref()
                .ok_or(OAuthError::InvalidResponse)?;
            let body = serde_json::json!({
                "refreshToken": refresh_token,
                "grantType": "refresh_token",
                "clientType": "extension",
            });
            let (status, body) = post_json(http, refresh_url, &body).await?;
            check_oauth_error(&body, status)?;
            parse_cline_token_envelope(&body, previous, now).map(|mut t| {
                t.metadata = metadata;
                t
            })
        }
        other => Err(OAuthError::Configuration(
            crate::text::Localized::with_param(
                "oauth_error.browser.provider_no_oauth",
                "provider",
                crate::text::provider_l10n(other),
                "{provider} does not support browser OAuth login.",
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Provider-specific payload handling.
// ---------------------------------------------------------------------------

/// codex.js mapTokens + providerHelpers.extractCodexAccountInfo: identity
/// claims from the id_token JWT payload (decoded, never signature-trusted
/// for authorization — only recorded as account metadata).
fn codex_id_token_metadata(id_token: &str, metadata: &mut BTreeMap<String, String>) {
    let Some(payload) = decode_jwt_payload(id_token) else {
        return;
    };
    let account_id = payload
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("account_id").and_then(Value::as_str))
        .filter(|s| !s.is_empty());
    if let Some(account_id) = account_id {
        metadata.insert("account_id".into(), account_id.to_owned());
    }
    if let Some(email) = payload
        .get("email")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        metadata.insert("email".into(), email.to_owned());
    }
}

/// Decode a JWT payload segment without verifying the signature. Used only
/// for identity metadata and expiry sanity checks, never to grant access.
fn decode_jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    parts.next()?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// claude.js exchangeToken: a pasted code may be `code#state`.
fn split_code_state(code: &str) -> (String, Option<String>) {
    match code.split_once('#') {
        Some((auth_code, state)) => (
            auth_code.to_owned(),
            (!state.is_empty()).then(|| state.to_owned()),
        ),
        None => (code.to_owned(), None),
    }
}

/// cline.js exchangeToken (decode branch): the callback `code` is base64
/// over a JSON token payload, possibly with trailing bytes after the final
/// `}`. Returns `None` when the code is not decodable, in which case the
/// caller falls back to the server exchange endpoint.
fn decode_cline_code(code: &str, now: DateTime<Utc>) -> Option<OAuthToken> {
    let mut padded = code.to_owned();
    let remainder = padded.len() % 4;
    if remainder != 0 {
        padded.push_str(&"=".repeat(4 - remainder));
    }
    let bytes = B64_STANDARD
        .decode(&padded)
        .or_else(|_| URL_SAFE.decode(&padded))
        .ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let end = text.rfind('}')?;
    let payload: Value = serde_json::from_str(&text[..=end]).ok()?;
    let access_token = payload
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_owned();
    // Identity sanity only: a structurally valid JWT must not already be
    // expired on arrival. The unverified payload is never used to authorize.
    if !jwt_expiry_sane(&access_token, now) {
        return None;
    }
    let refresh_token = payload
        .get("refreshToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let mut metadata = BTreeMap::new();
    if let Some(email) = payload
        .get("email")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        metadata.insert("email".into(), email.to_owned());
    }
    let expires_at = match payload.get("expiresAt") {
        Some(Value::String(_)) => payload
            .get("expiresAt")
            .and_then(Value::as_str)
            .and_then(parse_iso8601)?,
        Some(_) => return None,
        None => now.checked_add_signed(ChronoDuration::seconds(CLINE_DEFAULT_EXPIRES_SECS))?,
    };
    Some(OAuthToken {
        access_token,
        refresh_token,
        expires_at: Some(expires_at),
        metadata,
    })
}

/// When the token is a JWT carrying `exp`, it must not be expired beyond
/// clock-skew leeway at the moment we receive it. Non-JWT tokens pass.
fn jwt_expiry_sane(token: &str, now: DateTime<Utc>) -> bool {
    let Some(payload) = decode_jwt_payload(token) else {
        return true;
    };
    match payload.get("exp").and_then(Value::as_i64) {
        Some(exp) => exp + JWT_EXP_SKEW_SECS > now.timestamp(),
        None => true,
    }
}

/// cline.js exchangeToken (fallback branch) + refreshClineToken: tokens may
/// arrive bare or wrapped in a `{ "data": { ... } }` envelope.
fn parse_cline_token_envelope(
    body: &Value,
    previous_refresh: Option<String>,
    now: DateTime<Utc>,
) -> Result<OAuthToken, OAuthError> {
    let data = body.get("data").filter(|d| d.is_object()).unwrap_or(body);
    let access_token = data
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::InvalidResponse)?
        .to_owned();
    if !jwt_expiry_sane(&access_token, now) {
        return Err(OAuthError::InvalidResponse);
    }
    let refresh_token = data
        .get("refreshToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or(previous_refresh);
    let expires_at = match data.get("expiresAt").or_else(|| body.get("expiresAt")) {
        Some(Value::String(_)) => data
            .get("expiresAt")
            .or_else(|| body.get("expiresAt"))
            .and_then(Value::as_str)
            .and_then(parse_iso8601)
            .ok_or(OAuthError::InvalidResponse)?,
        Some(_) => return Err(OAuthError::InvalidResponse),
        None => {
            let secs = data
                .get("expiresIn")
                .or_else(|| data.get("expires_in"))
                .and_then(Value::as_i64)
                .filter(|&s| s > 0 && s <= MAX_EXPIRES_IN_SECS)
                .unwrap_or(CLINE_DEFAULT_EXPIRES_SECS);
            now.checked_add_signed(ChronoDuration::seconds(secs))
                .ok_or(OAuthError::InvalidResponse)?
        }
    };
    let mut metadata = BTreeMap::new();
    if let Some(email) = data
        .get("userInfo")
        .and_then(|u| u.get("email"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        metadata.insert("email".into(), email.to_owned());
    }
    Ok(OAuthToken {
        access_token,
        refresh_token,
        expires_at: Some(expires_at),
        metadata,
    })
}

fn parse_iso8601(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// gemini-cli.js postExchange (userInfo): email only, best-effort.
async fn gemini_fetch_email(
    http: &reqwest::Client,
    url: &str,
    access_token: &str,
) -> Option<String> {
    let body = get_json(
        http.get(url)
            .query(&[("alt", "json")])
            .bearer_auth(access_token),
    )
    .await?;
    body.get("email")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// gemini-cli.js postExchange (loadCodeAssist): the Code Assist project id,
/// best-effort. This is the same provisioning lookup the official CLI makes;
/// no generation, quota, or reset calls are involved.
async fn gemini_fetch_project_id(
    http: &reqwest::Client,
    url: &str,
    access_token: &str,
) -> Option<String> {
    let body = serde_json::json!({
        "metadata": {
            "ideType": 9,
            "platform": google_platform_enum(),
            "pluginType": 2,
        },
        "mode": 1,
    });
    let response = send_secure(
        http.post(url)
            .header(ACCEPT, "application/json")
            .bearer_auth(access_token)
            .json(&body),
    )
    .await
    .ok()?;
    let (status, body) = read_bounded(response).await.ok()?;
    if !status.is_success() {
        return None;
    }
    let project = body.get("cloudaicompanionProject")?;
    let id = match project {
        Value::String(s) => Some(s.as_str()),
        Value::Object(_) => project.get("id").and_then(Value::as_str),
        _ => None,
    };
    id.filter(|s| !s.is_empty()).map(str::to_owned)
}

/// Matches the reference's ClientMetadata.Platform numeric enum.
fn google_platform_enum() -> u32 {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => 2,
        ("macos", _) => 1,
        ("linux", "aarch64") => 4,
        ("linux", _) => 3,
        ("windows", _) => 5,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Shared helpers.
// ---------------------------------------------------------------------------

/// Reference flows need no registered client; a blank `client_id` uses the
/// native public identifier, and a non-empty one is honored as an override.
fn native_client_id(config: &OAuthConfig, fixed: &str) -> String {
    let configured = config.client_id.trim();
    if configured.is_empty() {
        fixed.to_owned()
    } else {
        configured.to_owned()
    }
}

/// The Gemini CLI client id: the public desktop client by default, or the
/// configured override.
fn gemini_client_id(config: &OAuthConfig) -> String {
    let configured = config.client_id.trim();
    if configured.is_empty() || configured == GEMINI_CLIENT_ID {
        GEMINI_CLIENT_ID.to_owned()
    } else {
        configured.to_owned()
    }
}

/// Validates that a custom Gemini client is fully configured, i.e. its
/// variable is present and not empty, and keeps nothing.
fn ensure_gemini_client_ready(config: &OAuthConfig) -> Result<(), OAuthError> {
    resolve_gemini_client_proof(config).map(|_| ())
}

/// Resolves the client's exchange value (what Google calls the client secret):
/// the public desktop client's built-in value, or the environment variable an
/// overridden client id names via `client_secret_env`. The caller must use the
/// result only for the request it is building and never store it.
fn resolve_gemini_client_proof(config: &OAuthConfig) -> Result<String, OAuthError> {
    let configured = config.client_id.trim();
    if configured.is_empty() || configured == GEMINI_CLIENT_ID {
        return Ok(GEMINI_CLIENT_SECRET.to_owned());
    }
    let env_name = config
        .client_secret_env
        .as_deref()
        .ok_or_else(OAuthError::google_client_secret_env_required)?;
    let secret =
        std::env::var(env_name).map_err(|_| OAuthError::google_client_secret_env_unset())?;
    if secret.is_empty() {
        return Err(OAuthError::google_client_secret_env_empty());
    }
    Ok(secret)
}

fn random_url_safe_32() -> Result<String, OAuthError> {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut buf)
        .map_err(|_| OAuthError::os_rng_unavailable())?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Validate that the consent URL we hand to the UI points at a fixed trusted
/// host. The production path additionally requires plain HTTPS: no
/// credentials in the URL and no non-default port.
fn validate_url_host(
    raw: &str,
    allowed_hosts: &[String],
    require_https: bool,
) -> Result<(), OAuthError> {
    let parsed = url::Url::parse(raw).map_err(|_| OAuthError::InvalidResponse)?;
    let host = parsed.host_str().ok_or(OAuthError::InvalidResponse)?;
    if !allowed_hosts.iter().any(|h| host.eq_ignore_ascii_case(h)) {
        return Err(OAuthError::InvalidResponse);
    }
    if require_https
        && (parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some())
    {
        return Err(OAuthError::InvalidResponse);
    }
    Ok(())
}

async fn read_bounded(response: reqwest::Response) -> Result<(StatusCode, Value), OAuthError> {
    let status = response.status();
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| OAuthError::Network)? {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BODY {
            return Err(OAuthError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| OAuthError::InvalidResponse)?;
    Ok((status, body))
}

/// POST a form and parse the JSON body. The body never appears in errors.
async fn post_form(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, String)],
) -> Result<(StatusCode, Value), OAuthError> {
    let response =
        send_secure(http.post(url).header(ACCEPT, "application/json").form(form)).await?;
    read_bounded(response).await
}

/// POST JSON and parse the JSON body. The body never appears in errors.
async fn post_json(
    http: &reqwest::Client,
    url: &str,
    body: &Value,
) -> Result<(StatusCode, Value), OAuthError> {
    let response =
        send_secure(http.post(url).header(ACCEPT, "application/json").json(body)).await?;
    read_bounded(response).await
}

/// Bounded GET returning parsed JSON (identity lookups only).
async fn get_json(request: reqwest::RequestBuilder) -> Option<Value> {
    let response = send_secure(request.header(ACCEPT, "application/json"))
        .await
        .ok()?;
    let (status, body) = read_bounded(response).await.ok()?;
    status.is_success().then_some(body)
}

/// Classify a provider OAuth error code, then fall back to status. The
/// response body never appears in error text.
fn check_oauth_error(body: &Value, status: StatusCode) -> Result<(), OAuthError> {
    if let Some(code) = body.get("error").and_then(Value::as_str) {
        return Err(match code {
            "access_denied" => OAuthError::Denied,
            // `invalid_grant` on a grant means it was revoked/expired: the
            // user must sign in again.
            "expired_token" | "invalid_grant" => OAuthError::Expired,
            _ => OAuthError::InvalidResponse,
        });
    }
    match status.as_u16() {
        _ if status.is_success() => Ok(()),
        401 | 403 => Err(OAuthError::Expired),
        _ => Err(OAuthError::InvalidResponse),
    }
}

/// Token parsing for the reference flows: non-empty `access_token`, sane
/// optional `expires_in`. A missing `refresh_token` preserves the previous
/// one (rotation is optional per RFC 6749 §6). Unlike the cloud flows, no
/// `token_type` assertion: the reference providers omit or vary it.
fn parse_bearer_body(
    body: &Value,
    previous_refresh: Option<String>,
    now: DateTime<Utc>,
) -> Result<OAuthToken, OAuthError> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::InvalidResponse)?;
    let expires_at = match body.get("expires_in") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let secs = value.as_i64().ok_or(OAuthError::InvalidResponse)?;
            if secs <= 0 || secs > MAX_EXPIRES_IN_SECS {
                return Err(OAuthError::InvalidResponse);
            }
            Some(
                now.checked_add_signed(ChronoDuration::seconds(secs))
                    .ok_or(OAuthError::InvalidResponse)?,
            )
        }
    };
    let refresh_token = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or(previous_refresh);
    Ok(OAuthToken {
        access_token: access_token.to_owned(),
        refresh_token,
        expires_at,
        metadata: BTreeMap::new(),
    })
}

// ---------------------------------------------------------------------------
// Tests: consumer-visible boundaries only. Endpoint overrides stay private
// to this module; public entry points always use the official constants.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config() -> OAuthConfig {
        OAuthConfig {
            client_id: String::new(),
            tenant: None,
            client_secret_env: None,
            credential_id: "test-cred".into(),
            options: Default::default(),
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    /// Loopback-test endpoints: random port, fixed callback path, hosts
    /// relaxed to 127.0.0.1 so the consent URL can point at the mock.
    fn loopback_endpoints(server: &MockServer, provider: Provider) -> Endpoints {
        let base = server.uri();
        let (refresh_url, userinfo_url, load_code_assist_url) = match provider {
            Provider::Cline => (Some(format!("{base}/refresh")), None, None),
            Provider::OpenRouter => (None, None, None),
            Provider::GeminiAi => (None, None, None),
            _ => (None, None, None),
        };
        Endpoints {
            authorize_url: format!("{base}/authorize"),
            authorize_hosts: vec!["127.0.0.1".into()],
            require_https: false,
            token_url: format!("{base}/token"),
            refresh_url,
            userinfo_url,
            load_code_assist_url,
            fixed_port: None,
            callback_path: match provider {
                Provider::Cline | Provider::OpenRouter => None,
                _ => Some("/callback".into()),
            },
            redirect_host: "127.0.0.1",
        }
    }

    fn prompt_state_and_port(pending: &PendingLogin) -> (Option<String>, u16, String) {
        let url = url::Url::parse(&pending.prompt().url).expect("prompt url");
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.into_owned());
        let redirect = url
            .query_pairs()
            .find(|(k, _)| k == "redirect_uri")
            .map(|(_, v)| v.into_owned())
            .expect("redirect_uri");
        let parsed = url::Url::parse(&redirect).expect("redirect uri");
        (
            state,
            parsed.port().expect("port"),
            parsed.path().to_owned(),
        )
    }

    async fn send_callback(port: u16, path: &str, query: &str) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .expect("connect");
        stream
            .write_all(format!("GET {path}?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        String::from_utf8_lossy(&response).into_owned()
    }

    // -- state matching / malformed callbacks --------------------------------

    #[tokio::test]
    async fn callback_ignores_wrong_state_and_malformed_then_accepts_valid() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "sk-ant-test",
                "refresh_token": "rt-1",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;
        let pending = begin_with(
            &client(),
            Provider::Claude,
            &test_config(),
            loopback_endpoints(&server, Provider::Claude),
        )
        .await
        .expect("begin");
        let (state, port, path) = prompt_state_and_port(&pending);
        let state = state.expect("claude issues state");
        let task = tokio::spawn(pending.finish());

        // Wrong state: 404, no token exchange, flow keeps waiting.
        let response = send_callback(port, &path, "state=forged&code=stolen").await;
        assert!(response.starts_with("HTTP/1.1 404"), "got: {response}");
        assert!(!task.is_finished(), "wrong state must not resolve the flow");

        // Malformed percent-encoding: ignored, flow keeps waiting.
        let response = send_callback(port, &path, &format!("state={state}&code=%zz")).await;
        assert!(response.starts_with("HTTP/1.1 404"), "got: {response}");
        assert!(
            !task.is_finished(),
            "malformed query must not resolve the flow"
        );

        // Duplicated code: ignored.
        let response = send_callback(port, &path, &format!("state={state}&code=a&code=b")).await;
        assert!(response.starts_with("HTTP/1.1 404"), "got: {response}");
        assert!(
            !task.is_finished(),
            "duplicate code must not resolve the flow"
        );

        // Correct state + exactly one code completes the flow.
        let response = send_callback(port, &path, &format!("state={state}&code=authcode")).await;
        assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
        let token = timeout(Duration::from_secs(5), task)
            .await
            .expect("timed out")
            .expect("join")
            .expect("token");
        assert_eq!(token.access_token, "sk-ant-test");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-1"));
    }

    #[tokio::test]
    async fn callback_denial_surfaces_denied() {
        let server = MockServer::start().await;
        let pending = begin_with(
            &client(),
            Provider::ChatGpt,
            &test_config(),
            loopback_endpoints(&server, Provider::ChatGpt),
        )
        .await
        .expect("begin");
        let (state, port, path) = prompt_state_and_port(&pending);
        let state = state.expect("codex issues state");
        let task = tokio::spawn(pending.finish());
        let response =
            send_callback(port, &path, &format!("state={state}&error=access_denied")).await;
        assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
        let result = timeout(Duration::from_secs(5), task)
            .await
            .expect("timed out")
            .expect("join");
        if !matches!(result, Err(OAuthError::Denied)) {
            panic!("expected Denied error");
        }
    }

    #[test]
    fn codex_authorize_url_uses_percent_20_and_fixed_callback() {
        // Pure query-construction boundary: scope spaces must be %20 and the
        // redirect must use the fixed port + /auth/callback path.
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([("scope", CODEX_SCOPE)])
            .finish()
            .replace('+', "%20");
        assert_eq!(query, "scope=openid%20profile%20email%20offline_access");
        assert_eq!(CODEX_FIXED_PORT, 1455);
        assert_eq!(CODEX_CALLBACK_PATH, "/auth/callback");
    }

    // -- token rotation on refresh -------------------------------------------

    fn stored_token() -> OAuthToken {
        OAuthToken {
            access_token: "old-access".into(),
            refresh_token: Some("rt-old".into()),
            expires_at: None,
            metadata: BTreeMap::from([("account_id".to_owned(), "acct-1".to_owned())]),
        }
    }

    #[tokio::test]
    async fn claude_refresh_rotates_refresh_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("refresh_token"))
            .and(body_string_contains("rt-old"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "new-access",
                "refresh_token": "rt-new",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;
        let endpoints = loopback_endpoints(&server, Provider::Claude);
        let token = refresh_with(
            &client(),
            Provider::Claude,
            &test_config(),
            &stored_token(),
            &endpoints,
        )
        .await
        .expect("refresh");
        assert_eq!(token.access_token, "new-access");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-new"));
        // Prior metadata is retained across rotation.
        assert_eq!(
            token.metadata.get("account_id").map(String::as_str),
            Some("acct-1")
        );
    }

    #[tokio::test]
    async fn codex_refresh_preserves_refresh_token_when_omitted() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "new-access",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;
        let endpoints = loopback_endpoints(&server, Provider::ChatGpt);
        let token = refresh_with(
            &client(),
            Provider::ChatGpt,
            &test_config(),
            &stored_token(),
            &endpoints,
        )
        .await
        .expect("refresh");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-old"));
    }

    #[tokio::test]
    async fn refresh_without_refresh_token_is_expired() {
        let server = MockServer::start().await;
        let endpoints = loopback_endpoints(&server, Provider::Claude);
        let mut token = stored_token();
        token.refresh_token = None;
        let result = refresh_with(
            &client(),
            Provider::Claude,
            &test_config(),
            &token,
            &endpoints,
        )
        .await;
        if !matches!(result, Err(OAuthError::Expired)) {
            panic!("refresh must require re-login when refresh token is absent");
        }
    }

    #[tokio::test]
    async fn cline_refresh_reads_data_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/refresh"))
            .and(body_string_contains("\"refreshToken\":\"rt-old\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "data": {
                    "accessToken": "cline-new",
                    "refreshToken": "rt-rotated",
                    "expiresIn": 1800
                }
            })))
            .mount(&server)
            .await;
        let endpoints = loopback_endpoints(&server, Provider::Cline);
        let token = refresh_with(
            &client(),
            Provider::Cline,
            &test_config(),
            &stored_token(),
            &endpoints,
        )
        .await
        .expect("refresh");
        assert_eq!(token.access_token, "cline-new");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-rotated"));
        assert!(token.expires_at.is_some());
    }

    // -- Cline callback payload: decode vs exchange ---------------------------

    fn fake_jwt(payload: &Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).expect("payload"));
        format!("{header}.{body}.sig")
    }

    #[test]
    fn cline_code_decodes_base64_token_json_with_trailing_garbage() {
        let jwt = fake_jwt(&json!({"exp": now().timestamp() + 3600}));
        let payload = json!({
            "accessToken": jwt,
            "refreshToken": "rt-cline",
            "email": "user@example.com",
            "expiresAt": "2099-01-01T00:00:00Z"
        });
        let encoded = B64_STANDARD.encode(format!("{}garbage-after-json", payload));
        let token = decode_cline_code(&encoded, now()).expect("decodable");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-cline"));
        assert_eq!(
            token.metadata.get("email").map(String::as_str),
            Some("user@example.com")
        );
        assert!(token.expires_at.is_some());
    }

    #[test]
    fn cline_code_needs_no_padding_to_decode() {
        // Length not divisible by 4: the reference pads before decoding.
        let payload = json!({"accessToken": "opaque-token", "refreshToken": "rt"});
        let encoded = B64_STANDARD.encode(payload.to_string());
        let unpadded = encoded.trim_end_matches('=').to_owned();
        let token = decode_cline_code(&unpadded, now()).expect("decodable");
        assert_eq!(token.access_token, "opaque-token");
    }

    #[test]
    fn cline_code_rejects_malformed_payloads() {
        // Not base64 at all.
        assert!(decode_cline_code("!!! not base64 !!!", now()).is_none());
        // Base64 of non-JSON.
        assert!(decode_cline_code(&B64_STANDARD.encode("hello world"), now()).is_none());
        // Base64 of JSON without accessToken.
        let no_token = B64_STANDARD.encode(json!({"refreshToken": "rt"}).to_string());
        assert!(decode_cline_code(&no_token, now()).is_none());
        // Expired JWT must not be accepted from the unverified payload.
        let expired = fake_jwt(&json!({"exp": now().timestamp() - 7200}));
        let payload = B64_STANDARD.encode(json!({"accessToken": expired}).to_string());
        assert!(decode_cline_code(&payload, now()).is_none());
    }

    #[tokio::test]
    async fn openrouter_pkce_exchanges_the_code_for_an_inference_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("or-callback-code"))
            .and(body_string_contains("\"code_challenge_method\":\"S256\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "key": "sk-or-v1-issued",
                "user_id": "user_must_not_be_kept"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let pending = begin_with(
            &client(),
            Provider::OpenRouter,
            &test_config(),
            loopback_endpoints(&server, Provider::OpenRouter),
        )
        .await
        .expect("begin");
        let url = url::Url::parse(&pending.prompt().url).expect("prompt url");
        let pairs: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert!(pairs.contains_key("code_challenge"));
        assert!(
            !pairs.contains_key("state"),
            "OpenRouter has no state parameter"
        );
        let callback = url::Url::parse(&pairs["callback_url"]).expect("callback");
        assert_eq!(callback.host_str(), Some("127.0.0.1"));
        assert!(
            callback.path().len() > 20,
            "the random path is the CSRF capability"
        );
        let port = callback.port().expect("port");
        let path_and_query = format!("{}?code=or-callback-code", callback.path());
        let hit = tokio::spawn(async move {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                .await
                .unwrap();
            stream
                .write_all(format!("GET {path_and_query} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = Vec::new();
            let _ = stream.read_to_end(&mut out).await;
        });
        let token = pending.finish().await.expect("token");
        hit.await.unwrap();
        assert_eq!(token.access_token, "sk-or-v1-issued");
        assert!(token.refresh_token.is_none() && token.expires_at.is_none());
        assert!(token.metadata.is_empty(), "user_id is not stored");
    }

    #[tokio::test]
    async fn cline_undecodable_code_falls_back_to_server_exchange() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("opaque-callback-code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "accessToken": "exchanged-access",
                    "refreshToken": "rt-exchanged",
                    "userInfo": {"email": "ex@example.com"},
                    "expiresAt": "2099-01-01T00:00:00Z"
                }
            })))
            .mount(&server)
            .await;
        let pending = begin_with(
            &client(),
            Provider::Cline,
            &test_config(),
            loopback_endpoints(&server, Provider::Cline),
        )
        .await
        .expect("begin");
        let (state, port, path) = prompt_state_and_port(&pending);
        assert!(state.is_none(), "cline has no state parameter");
        let task = tokio::spawn(pending.finish());
        // Stray request on the wrong path must be ignored (path capability).
        let response = send_callback(port, "/callback", "code=opaque-callback-code").await;
        assert!(response.starts_with("HTTP/1.1 404"), "got: {response}");
        assert!(!task.is_finished(), "wrong path must not resolve the flow");
        let response = send_callback(port, &path, "code=opaque-callback-code").await;
        assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
        let token = timeout(Duration::from_secs(5), task)
            .await
            .expect("timed out")
            .expect("join")
            .expect("token");
        assert_eq!(token.access_token, "exchanged-access");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-exchanged"));
        assert_eq!(
            token.metadata.get("email").map(String::as_str),
            Some("ex@example.com")
        );
    }

    // -- Gemini custom client: the variable is validated at begin, read at finish

    /// Environment variable that is removed again when the guard drops. Each
    /// test uses its own variable name because the environment is process-wide.
    struct EnvGuard(&'static str);
    impl EnvGuard {
        fn set(name: &'static str, value: &str) -> Self {
            std::env::set_var(name, value);
            Self(name)
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    fn custom_gemini_config(env_name: Option<&str>) -> OAuthConfig {
        OAuthConfig {
            client_id: "custom-client.apps.test".into(),
            client_secret_env: env_name.map(str::to_owned),
            ..test_config()
        }
    }

    fn configuration_id(error: &OAuthError) -> &str {
        match error {
            OAuthError::Configuration(localized) => localized.id.as_ref(),
            _ => "not-a-configuration-error",
        }
    }

    #[tokio::test]
    async fn gemini_begin_requires_the_variable_for_a_custom_client() {
        let server = MockServer::start().await;
        let cases = [
            (
                custom_gemini_config(None),
                "oauth_error.google.client_secret_env_required",
            ),
            (
                custom_gemini_config(Some("CUOTAI_TEST_GEMINI_BROWSER_UNSET")),
                "oauth_error.google.client_secret_env_unset",
            ),
        ];
        for (config, expected) in cases {
            let error = begin_with(
                &client(),
                Provider::GeminiAi,
                &config,
                loopback_endpoints(&server, Provider::GeminiAi),
            )
            .await
            .err()
            .expect("begin must fail");
            assert_eq!(configuration_id(&error), expected);
        }

        let _empty = EnvGuard::set("CUOTAI_TEST_GEMINI_BROWSER_EMPTY", "");
        let error = begin_with(
            &client(),
            Provider::GeminiAi,
            &custom_gemini_config(Some("CUOTAI_TEST_GEMINI_BROWSER_EMPTY")),
            loopback_endpoints(&server, Provider::GeminiAi),
        )
        .await
        .err()
        .expect("empty value must fail");
        assert_eq!(
            configuration_id(&error),
            "oauth_error.google.client_secret_env_empty"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn gemini_exchange_reads_the_variable_when_the_login_finishes() {
        const NAME: &str = "CUOTAI_TEST_GEMINI_BROWSER_FINISH";
        let server = MockServer::start().await;
        // Only the finish-time value is accepted: a request carrying the
        // begin-time value would not match and the exchange would fail.
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("client_secret=fake-after"))
            .and(body_string_contains("client_id=custom-client.apps.test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "access-new",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        let _guard = EnvGuard::set(NAME, "fake-before");
        let pending = begin_with(
            &client(),
            Provider::GeminiAi,
            &custom_gemini_config(Some(NAME)),
            loopback_endpoints(&server, Provider::GeminiAi),
        )
        .await
        .expect("begin");
        std::env::set_var(NAME, "fake-after");
        let (state, port, path) = prompt_state_and_port(&pending);
        let state = state.expect("gemini issues state");
        let task = tokio::spawn(pending.finish());
        let response = send_callback(port, &path, &format!("state={state}&code=approved")).await;
        assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
        let token = timeout(Duration::from_secs(5), task)
            .await
            .expect("login timeout")
            .expect("login task")
            .expect("token");
        assert_eq!(token.access_token, "access-new");
    }

    #[tokio::test]
    async fn gemini_exchange_fails_without_a_request_when_the_variable_disappears() {
        const NAME: &str = "CUOTAI_TEST_GEMINI_BROWSER_GONE";
        // No mock is mounted: any token request would be answered with 404.
        let server = MockServer::start().await;
        let guard = EnvGuard::set(NAME, "fake-before");
        let pending = begin_with(
            &client(),
            Provider::GeminiAi,
            &custom_gemini_config(Some(NAME)),
            loopback_endpoints(&server, Provider::GeminiAi),
        )
        .await
        .expect("begin");
        drop(guard);
        let (state, port, path) = prompt_state_and_port(&pending);
        let state = state.expect("gemini issues state");
        let task = tokio::spawn(pending.finish());
        send_callback(port, &path, &format!("state={state}&code=approved")).await;
        let error = timeout(Duration::from_secs(5), task)
            .await
            .expect("login timeout")
            .expect("login task")
            .err()
            .expect("exchange must fail");
        assert_eq!(
            configuration_id(&error),
            "oauth_error.google.client_secret_env_unset"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn gemini_public_client_exchange_needs_no_variable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains(format!(
                "client_id={GEMINI_CLIENT_ID}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "access-public",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        let pending = begin_with(
            &client(),
            Provider::GeminiAi,
            &test_config(),
            loopback_endpoints(&server, Provider::GeminiAi),
        )
        .await
        .expect("begin");
        let (state, port, path) = prompt_state_and_port(&pending);
        let state = state.expect("gemini issues state");
        let task = tokio::spawn(pending.finish());
        let response = send_callback(port, &path, &format!("state={state}&code=approved")).await;
        assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
        let token = timeout(Duration::from_secs(5), task)
            .await
            .expect("login timeout")
            .expect("login task")
            .expect("token");
        assert_eq!(token.access_token, "access-public");
    }

    #[tokio::test]
    async fn gemini_refresh_requires_the_variable_for_a_custom_client() {
        let server = MockServer::start().await;
        let error = refresh_with(
            &client(),
            Provider::GeminiAi,
            &custom_gemini_config(Some("CUOTAI_TEST_GEMINI_BROWSER_REFRESH_UNSET")),
            &stored_token(),
            &loopback_endpoints(&server, Provider::GeminiAi),
        )
        .await
        .err()
        .expect("refresh must fail");
        assert_eq!(
            configuration_id(&error),
            "oauth_error.google.client_secret_env_unset"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    // -- Codex id_token identity ----------------------------------------------

    #[test]
    fn codex_id_token_extracts_account_id_and_email() {
        let id_token = fake_jwt(&json!({
            "email": "dev@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct-xyz",
                "chatgpt_plan_type": "plus"
            }
        }));
        let mut metadata = BTreeMap::new();
        codex_id_token_metadata(&id_token, &mut metadata);
        assert_eq!(
            metadata.get("account_id").map(String::as_str),
            Some("acct-xyz")
        );
        assert_eq!(
            metadata.get("email").map(String::as_str),
            Some("dev@example.com")
        );
    }

    #[test]
    fn codex_id_token_falls_back_to_top_level_account_id() {
        let id_token = fake_jwt(&json!({"account_id": "acct-top"}));
        let mut metadata = BTreeMap::new();
        codex_id_token_metadata(&id_token, &mut metadata);
        assert_eq!(
            metadata.get("account_id").map(String::as_str),
            Some("acct-top")
        );
        // Garbage never produces identity.
        let mut metadata = BTreeMap::new();
        codex_id_token_metadata("not-a-jwt", &mut metadata);
        assert!(metadata.is_empty());
    }

    // -- Claude code#state split -----------------------------------------------

    #[test]
    fn claude_code_state_split_matches_reference() {
        let (code, state) = split_code_state("authcode#statevalue");
        assert_eq!(code, "authcode");
        assert_eq!(state.as_deref(), Some("statevalue"));
        let (code, state) = split_code_state("plaincode");
        assert_eq!(code, "plaincode");
        assert_eq!(state, None);
        let (code, state) = split_code_state("authcode#");
        assert_eq!(code, "authcode");
        assert_eq!(state, None);
    }
}
