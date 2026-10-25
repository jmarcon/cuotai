//! Real OAuth sign-in flows against official provider endpoints.
//!
//! Supported providers (mapped from [`Provider`], never arbitrary URLs):
//! - Google (`GeminiApi` / `VertexAi`): installed-app loopback flow with PKCE
//!   (RFC 7636) per Google's "OAuth 2.0 for Mobile & Desktop Apps" docs.
//! - Microsoft Entra (`AzureOpenAi`): device code flow (RFC 8628) against the
//!   v2.0 endpoints for a public client.
//! - GitHub (`Copilot`): disabled — usage endpoints are documented for
//!   classic PATs only, so no identity-only OAuth login is offered.
//!
//! All client registrations are user-supplied ([`OAuthConfig::client_id`]).
//! No borrowed client IDs, no browser-cookie or CLI-token extraction, and no
//! server error bodies or token material ever appear in error text.

use super::send_secure;
use super::session::ClientSecretLookup;
use super::{LoginPrompt, OAuthConfig, OAuthError, OAuthToken};
use crate::model::Provider;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::header::ACCEPT;
use serde_json::Value;
use sha2::{Digest, Sha256};

use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout, timeout_at, Instant};

// ---------------------------------------------------------------------------
// Official endpoints (the only hosts these flows will ever contact).
// ---------------------------------------------------------------------------

const GOOGLE_AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Usage-monitoring scope; grants the Cloud Monitoring read API used for
/// Gemini/Vertex usage, not merely identity.
const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/monitoring.read";
const GOOGLE_AUTHORIZE_HOSTS: &[&str] = &["accounts.google.com"];

const AZURE_SCOPE: &str = "https://management.azure.com/user_impersonation offline_access";
const AZURE_VERIFICATION_HOSTS: &[&str] = &["microsoft.com", "login.microsoftonline.com"];

/// Loopback callback constraints.
const CALLBACK_PATH: &str = "/oauth2/callback";
const CALLBACK_TOTAL_WAIT: Duration = Duration::from_secs(300);
const CALLBACK_CONN_DEADLINE: Duration = Duration::from_secs(15);
const MAX_HEADER_BYTES: usize = 8192;

const DEFAULT_DEVICE_INTERVAL: Duration = Duration::from_secs(5);
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);
const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Upper bound on any provider response body we parse.
const MAX_RESPONSE_BODY: usize = 64 * 1024;
/// Bound on writing the browser response after a loopback callback.
const RESPONSE_DEADLINE: Duration = Duration::from_secs(5);
/// `expires_in` values above this are treated as malformed, not honored.
const MAX_EXPIRES_IN_SECS: i64 = 31_536_000; // one year

// ---------------------------------------------------------------------------
// Public contract
// ---------------------------------------------------------------------------

/// An in-progress OAuth login. Owns the pending state (loopback listener or
/// device code). Dropping it before [`PendingLogin::finish`] completes stops
/// the listener / polling immediately.
pub struct PendingLogin {
    prompt: LoginPrompt,
    kind: PendingKind,
}

enum PendingKind {
    Loopback(LoopbackLogin),
    Device(DeviceLogin),
}

impl PendingLogin {
    /// UI-safe challenge: the authorization/verification URL and, for device
    /// flows, the user-facing code. Never contains device codes or secrets.
    pub fn prompt(&self) -> &LoginPrompt {
        &self.prompt
    }

    /// Drive the flow to completion. Cancelled cleanly by dropping the
    /// returned future (or this value beforehand).
    pub async fn finish(self) -> Result<OAuthToken, OAuthError> {
        match self.kind {
            PendingKind::Loopback(login) => login.finish().await,
            PendingKind::Device(login) => login.finish().await,
        }
    }
}

/// Begin an OAuth login for `provider`. For the Google loopback flow the
/// local listener is bound and listening before this returns; the caller opens
/// [`PendingLogin::prompt`]'s URL afterwards.
pub(super) async fn begin(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    secrets: ClientSecretLookup,
) -> Result<PendingLogin, OAuthError> {
    match provider {
        Provider::GeminiApi | Provider::VertexAi => {
            begin_google(
                http,
                config,
                secrets,
                GOOGLE_AUTHORIZE_URL,
                GOOGLE_TOKEN_URL,
                GOOGLE_AUTHORIZE_HOSTS,
                true,
            )
            .await
        }
        Provider::AzureOpenAi => begin_azure(http, config).await,
        other => Err(OAuthError::Configuration(
            crate::text::Localized::with_param(
                "oauth_error.flows.provider_no_oauth",
                "provider",
                crate::text::provider_l10n(other),
                "{provider} does not support OAuth login.",
            ),
        )),
    }
}

/// Refresh an existing token. `client_secret` is the already-resolved value of
/// [`OAuthConfig::client_secret_env`], when configured.
pub(super) async fn refresh(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    client_secret: Option<&str>,
    token: &OAuthToken,
) -> Result<OAuthToken, OAuthError> {
    match provider {
        Provider::GeminiApi | Provider::VertexAi => {
            let refresh_token = token.refresh_token.as_deref().ok_or(OAuthError::Expired)?;
            refresh_grant(
                http,
                config,
                client_secret,
                refresh_token,
                token,
                GOOGLE_TOKEN_URL,
                None,
            )
            .await
        }
        Provider::AzureOpenAi => {
            let tenant = resolve_tenant(config)?;
            let refresh_token = token.refresh_token.as_deref().ok_or(OAuthError::Expired)?;
            refresh_grant(
                http,
                config,
                client_secret,
                refresh_token,
                token,
                &format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"),
                Some(AZURE_SCOPE),
            )
            .await
        }
        other => Err(OAuthError::Configuration(
            crate::text::Localized::with_param(
                "oauth_error.flows.provider_no_oauth",
                "provider",
                crate::text::provider_l10n(other),
                "{provider} does not support OAuth login.",
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn require_client_id(config: &OAuthConfig) -> Result<(), OAuthError> {
    if config.client_id.trim().is_empty() {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.flows.client_id_required",
            "OAuth client_id is required; register your own OAuth app with the provider.",
        )));
    }
    Ok(())
}

/// Azure tenant: `organizations` by default; a configured tenant is validated
/// before it is embedded in a request path.
fn resolve_tenant(config: &OAuthConfig) -> Result<String, OAuthError> {
    let tenant = config
        .tenant
        .as_deref()
        .map(str::trim)
        .unwrap_or("organizations");
    let valid = !tenant.is_empty()
        && tenant.len() <= 128
        && !tenant.contains("..")
        && tenant
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    if !valid {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.flows.invalid_azure_tenant",
            "Invalid Azure tenant; use a tenant GUID, a domain, or `organizations`.",
        )));
    }
    Ok(tenant.to_owned())
}

fn random_url_safe_32() -> Result<String, OAuthError> {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut buf)
        .map_err(|_| OAuthError::os_rng_unavailable())?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Validate that a URL we are about to hand to the UI points at a fixed
/// trusted host (authorize / verification endpoints only). The hardened
/// production path additionally requires plain HTTPS: no credentials in the
/// URL and no non-default port.
fn validate_url_host(
    raw: &str,
    allowed_hosts: &[&str],
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

/// POST a form, parse the JSON body, and classify OAuth error codes into
/// sanitized errors. The response body never appears in error text.
async fn post_form(
    http: &reqwest::Client,
    url: &str,
    form: &[(String, String)],
) -> Result<(reqwest::StatusCode, Value), OAuthError> {
    let response =
        send_secure(http.post(url).header(ACCEPT, "application/json").form(form)).await?;
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

/// Map a provider error code to a sanitized error. `authorization_pending`
/// and `slow_down` are flow-control codes handled by the caller, not here.
fn classify_error(code: &str) -> OAuthError {
    match code {
        "access_denied" => OAuthError::Denied,
        // `invalid_grant` on refresh means the grant was revoked/expired:
        // the user must sign in again.
        "expired_token" | "invalid_grant" => OAuthError::Expired,
        _ => OAuthError::InvalidResponse,
    }
}

/// Strict token response parsing: non-empty `access_token`, bearer
/// `token_type`, finite sane `expires_in`. A missing `refresh_token` preserves
/// the previous one (rotation is optional per RFC 6749 §6).
fn parse_token_body(
    body: &Value,
    previous_refresh: Option<String>,
) -> Result<OAuthToken, OAuthError> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::InvalidResponse)?;
    let token_type = body
        .get("token_type")
        .and_then(Value::as_str)
        .ok_or(OAuthError::InvalidResponse)?;
    if !token_type.eq_ignore_ascii_case("bearer") {
        return Err(OAuthError::InvalidResponse);
    }
    let expires_at = match body.get("expires_in") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let secs = value.as_i64().ok_or(OAuthError::InvalidResponse)?;
            if secs <= 0 || secs > MAX_EXPIRES_IN_SECS {
                return Err(OAuthError::InvalidResponse);
            }
            Some(
                Utc::now()
                    .checked_add_signed(ChronoDuration::seconds(secs))
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
        metadata: Default::default(),
    })
}

/// Shared refresh-token grant for all providers. Preserves the previous
/// refresh token when the response omits one.
async fn refresh_grant(
    http: &reqwest::Client,
    config: &OAuthConfig,
    client_secret: Option<&str>,
    refresh_token: &str,
    previous: &OAuthToken,
    token_url: &str,
    scope: Option<&str>,
) -> Result<OAuthToken, OAuthError> {
    require_client_id(config)?;
    let mut form = vec![
        ("grant_type".to_owned(), "refresh_token".to_owned()),
        ("client_id".to_owned(), config.client_id.clone()),
        ("refresh_token".to_owned(), refresh_token.to_owned()),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret".to_owned(), secret.to_owned()));
    }
    if let Some(scope) = scope {
        form.push(("scope".to_owned(), scope.to_owned()));
    }
    let (status, body) = post_form(http, token_url, &form).await?;
    if let Some(code) = body.get("error").and_then(Value::as_str) {
        return Err(classify_error(code));
    }
    if !status.is_success() {
        return Err(OAuthError::InvalidResponse);
    }
    parse_token_body(&body, previous.refresh_token.clone())
}

// ---------------------------------------------------------------------------
// Google: installed-app loopback flow with PKCE
// ---------------------------------------------------------------------------

struct LoopbackLogin {
    http: reqwest::Client,
    listener: TcpListener,
    token_url: String,
    redirect_uri: String,
    client_id: String,
    secrets: ClientSecretLookup,
    code_verifier: String,
    state: String,
    deadline: Instant,
}

async fn begin_google(
    http: &reqwest::Client,
    config: &OAuthConfig,
    secrets: ClientSecretLookup,
    authorize_url: &str,
    token_url: &str,
    authorize_hosts: &[&str],
    require_https: bool,
) -> Result<PendingLogin, OAuthError> {
    require_client_id(config)?;
    // Bind before returning so the browser can be opened immediately after.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| OAuthError::Network)?;
    let port = listener
        .local_addr()
        .map_err(|_| OAuthError::Network)?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");

    // OS CSPRNG for both the PKCE verifier and the CSRF state.
    let code_verifier = random_url_safe_32()?;
    let state = random_url_safe_32()?;
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));

    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("response_type", "code"),
            ("client_id", config.client_id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("scope", GOOGLE_SCOPE),
            ("state", state.as_str()),
            ("code_challenge", code_challenge.as_str()),
            ("code_challenge_method", "S256"),
            // Desktop docs: offline access + consent ensures a refresh token.
            ("access_type", "offline"),
            ("prompt", "consent"),
        ])
        .finish();
    let url = format!("{authorize_url}?{query}");
    validate_url_host(&url, authorize_hosts, require_https)?;

    Ok(PendingLogin {
        prompt: LoginPrompt {
            url,
            user_code: None,
        },
        kind: PendingKind::Loopback(LoopbackLogin {
            http: http.clone(),
            listener,
            token_url: token_url.to_owned(),
            redirect_uri,
            client_id: config.client_id.clone(),
            secrets,
            code_verifier,
            state,
            deadline: Instant::now() + CALLBACK_TOTAL_WAIT,
        }),
    })
}

/// Outcome of parsing a single loopback HTTP request.
enum LoopbackOutcome {
    Code(String),
    Denied,
    /// Wrong path, wrong state, duplicate/malformed parameters, favicon, ...
    /// The connection is closed without any token exchange.
    Ignored,
    Failed(OAuthError),
}

impl LoopbackLogin {
    async fn finish(self) -> Result<OAuthToken, OAuthError> {
        let code = self.wait_for_code().await?;
        let mut form = vec![
            ("grant_type".to_owned(), "authorization_code".to_owned()),
            ("client_id".to_owned(), self.client_id.clone()),
            ("redirect_uri".to_owned(), self.redirect_uri.clone()),
            ("code".to_owned(), code),
            ("code_verifier".to_owned(), self.code_verifier.clone()),
        ];
        // Resolved here, not at `begin`: the pending login never owns the value.
        if let Some(secret) = self.secrets.resolve()? {
            form.push(("client_secret".to_owned(), secret));
        }
        // The exchange must finish within the remaining login deadline.
        let (status, body) =
            timeout_at(self.deadline, post_form(&self.http, &self.token_url, &form))
                .await
                .map_err(|_| OAuthError::Expired)??;
        if let Some(code) = body.get("error").and_then(Value::as_str) {
            return Err(classify_error(code));
        }
        if !status.is_success() {
            return Err(OAuthError::InvalidResponse);
        }
        parse_token_body(&body, None)
    }

    /// Accept connections until a valid callback arrives or the deadline
    /// passes. Unrelated requests never trigger a token exchange.
    async fn wait_for_code(&self) -> Result<String, OAuthError> {
        loop {
            let accepted = timeout_at(self.deadline, self.listener.accept())
                .await
                .map_err(|_| OAuthError::Expired)?
                .map_err(|_| OAuthError::Network)?;
            let (mut stream, _) = accepted;
            // Per-connection read bounded by both its own deadline and the
            // remaining total login deadline.
            let conn_deadline =
                std::cmp::min(self.deadline, Instant::now() + CALLBACK_CONN_DEADLINE);
            let outcome =
                match timeout_at(conn_deadline, read_request(&mut stream, &self.state)).await {
                    Ok(outcome) => outcome,
                    Err(_) => {
                        if Instant::now() >= self.deadline {
                            return Err(OAuthError::Expired);
                        }
                        LoopbackOutcome::Ignored
                    }
                };
            match outcome {
                LoopbackOutcome::Code(code) => {
                    respond(
                        &mut stream,
                        "200 OK",
                        &super::callback_page::render(super::callback_page::CallbackPage::Success),
                    )
                    .await;
                    return Ok(code);
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

/// Read one HTTP request head (bounded) and extract the callback outcome.
/// `expected_state` is the CSRF token issued in the authorize URL; a callback
/// without exactly one matching state is ignored without any token exchange.
async fn read_request(stream: &mut TcpStream, expected_state: &str) -> LoopbackOutcome {
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
    if path != CALLBACK_PATH {
        return LoopbackOutcome::Ignored;
    }
    parse_callback_query(query.unwrap_or_default(), expected_state)
}

fn parse_callback_query(query: &str, expected_state: &str) -> LoopbackOutcome {
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
    // CSRF check: exactly one state and it must match the issued value.
    if states.len() != 1 || states[0] != expected_state {
        return LoopbackOutcome::Ignored;
    }
    // Exactly one non-empty code XOR exactly one error; anything else
    // (duplicates, empty code, both present) is rejected without exchange.
    match (codes.as_slice(), errors.as_slice()) {
        ([code], []) if !code.is_empty() => LoopbackOutcome::Code(code.clone()),
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
// Device code flow (Microsoft Entra v2.0), RFC 8628
// ---------------------------------------------------------------------------

struct DeviceLogin {
    http: reqwest::Client,
    token_url: String,
    client_id: String,
    device_code: String,
    interval: Duration,
    deadline: Instant,
}

async fn begin_azure(
    http: &reqwest::Client,
    config: &OAuthConfig,
) -> Result<PendingLogin, OAuthError> {
    let tenant = resolve_tenant(config)?;
    begin_device(
        http,
        config,
        &format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/devicecode"),
        &format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"),
        AZURE_SCOPE,
        AZURE_VERIFICATION_HOSTS,
        true,
    )
    .await
}

async fn begin_device(
    http: &reqwest::Client,
    config: &OAuthConfig,
    device_url: &str,
    token_url: &str,
    scope: &str,
    verification_hosts: &[&str],
    require_https: bool,
) -> Result<PendingLogin, OAuthError> {
    require_client_id(config)?;
    let form = [
        ("client_id".to_owned(), config.client_id.clone()),
        ("scope".to_owned(), scope.to_owned()),
    ];
    let (status, body) = post_form(http, device_url, &form).await?;
    if let Some(code) = body.get("error").and_then(Value::as_str) {
        return Err(classify_error(code));
    }
    if !status.is_success() {
        return Err(OAuthError::InvalidResponse);
    }
    let device_code = body
        .get("device_code")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::InvalidResponse)?
        .to_owned();
    let user_code = body
        .get("user_code")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::InvalidResponse)?
        .to_owned();
    let verification_uri = body
        .get("verification_uri")
        .and_then(Value::as_str)
        .ok_or(OAuthError::InvalidResponse)?;
    validate_url_host(verification_uri, verification_hosts, require_https)?;

    // RFC 8628 requires `expires_in`; reject missing/invalid values instead
    // of silently choosing a lifetime.
    let lifetime = match body.get("expires_in") {
        Some(value) => {
            let secs = value
                .as_u64()
                .filter(|&s| s > 0 && s <= 86_400)
                .ok_or(OAuthError::InvalidResponse)?;
            Duration::from_secs(secs)
        }
        None => return Err(OAuthError::InvalidResponse),
    };
    // `interval` is optional (default 5s) but must be positive when present:
    // a zero interval would busy-loop the token endpoint.
    let interval = match body.get("interval") {
        None | Some(Value::Null) => DEFAULT_DEVICE_INTERVAL,
        Some(value) => {
            let secs = value
                .as_u64()
                .filter(|&s| s > 0 && s <= 3_600)
                .ok_or(OAuthError::InvalidResponse)?;
            Duration::from_secs(secs)
        }
    };

    Ok(PendingLogin {
        prompt: LoginPrompt {
            url: verification_uri.to_owned(),
            user_code: Some(user_code),
        },
        kind: PendingKind::Device(DeviceLogin {
            http: http.clone(),
            token_url: token_url.to_owned(),
            client_id: config.client_id.clone(),
            device_code,
            interval,
            deadline: Instant::now() + lifetime,
        }),
    })
}

impl DeviceLogin {
    /// Poll the token endpoint, honoring `interval` before every request,
    /// cumulative `slow_down` backoff, and the overall expiration deadline.
    /// Dropping this future cancels polling.
    async fn finish(self) -> Result<OAuthToken, OAuthError> {
        let mut extra_wait = Duration::ZERO;
        loop {
            let wait = self.interval + extra_wait;
            if Instant::now() + wait >= self.deadline {
                return Err(OAuthError::Expired);
            }
            sleep(wait).await;
            let form = [
                ("grant_type".to_owned(), DEVICE_GRANT_TYPE.to_owned()),
                ("client_id".to_owned(), self.client_id.clone()),
                ("device_code".to_owned(), self.device_code.clone()),
            ];
            // The whole poll (request + bounded body read) must finish within
            // the device-code lifetime; no request is issued at/past it.
            let (status, body) =
                timeout_at(self.deadline, post_form(&self.http, &self.token_url, &form))
                    .await
                    .map_err(|_| OAuthError::Expired)??;
            match body.get("error").and_then(Value::as_str) {
                Some("authorization_pending") => continue,
                Some("slow_down") => {
                    extra_wait += SLOW_DOWN_STEP;
                    continue;
                }
                Some(code) => return Err(classify_error(code)),
                None => {}
            }
            if !status.is_success() {
                return Err(OAuthError::InvalidResponse);
            }
            if Instant::now() >= self.deadline {
                return Err(OAuthError::Expired);
            }
            return parse_token_body(&body, None);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests: consumer-visible boundaries only. Endpoint overrides stay private to
// this module; public entry points always use the official constants.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config() -> OAuthConfig {
        OAuthConfig {
            client_id: "test-client".into(),
            tenant: None,
            client_secret_env: None,
            credential_id: "test-cred".into(),
            options: Default::default(),
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    /// Credential source whose single variable can change between steps.
    struct SwitchableSecret(parking_lot::Mutex<Option<String>>);
    impl SwitchableSecret {
        fn set(&self, value: Option<&str>) {
            *self.0.lock() = value.map(str::to_owned);
        }
    }
    impl crate::Credentials for SwitchableSecret {
        fn get(&self, _: &str) -> Option<String> {
            self.0.lock().clone()
        }
    }

    fn no_secrets() -> ClientSecretLookup {
        ClientSecretLookup::new(
            Arc::new(SwitchableSecret(Default::default())),
            &test_config(),
        )
    }

    fn token_json(access: &str) -> Value {
        json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "rt-new"
        })
    }

    // -- loopback flow -------------------------------------------------------

    async fn google_pending(server: &MockServer) -> PendingLogin {
        begin_google(
            &client(),
            &test_config(),
            no_secrets(),
            &format!("{}/auth", server.uri()),
            &format!("{}/token", server.uri()),
            &["127.0.0.1"],
            false,
        )
        .await
        .expect("begin_google")
    }

    fn prompt_state_and_port(pending: &PendingLogin) -> (String, u16) {
        let url = url::Url::parse(&pending.prompt().url).expect("prompt url");
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.into_owned())
            .expect("state");
        let redirect = url
            .query_pairs()
            .find(|(k, _)| k == "redirect_uri")
            .map(|(_, v)| v.into_owned())
            .expect("redirect_uri");
        let port = url::Url::parse(&redirect)
            .expect("redirect uri")
            .port()
            .expect("port");
        (state, port)
    }

    async fn send_callback(port: u16, query: &str) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .expect("connect");
        stream
            .write_all(
                format!("GET {CALLBACK_PATH}?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        String::from_utf8_lossy(&response).into_owned()
    }

    #[tokio::test]
    async fn loopback_ignores_wrong_state_then_accepts_valid_callback() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("ya29.test")))
            .mount(&server)
            .await;
        let pending = google_pending(&server).await;
        let (state, port) = prompt_state_and_port(&pending);
        let task = tokio::spawn(pending.finish());

        // Wrong state: 404, no token exchange, flow keeps waiting.
        let response = send_callback(port, "state=forged&code=stolen").await;
        assert!(response.starts_with("HTTP/1.1 404"), "got: {response}");
        assert!(!task.is_finished(), "wrong state must not resolve the flow");

        // Correct state + code completes the flow.
        let response = send_callback(port, &format!("state={state}&code=authcode")).await;
        assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
        let token = timeout(Duration::from_secs(5), task)
            .await
            .expect("timed out")
            .expect("join")
            .expect("token");
        assert_eq!(token.access_token, "ya29.test");
    }

    #[tokio::test]
    async fn loopback_exchange_resolves_the_client_secret_when_it_finishes() {
        use wiremock::matchers::body_string_contains;
        let mut config = test_config();
        config.client_secret_env = Some("FAKE_CLIENT_SECRET".into());
        let credentials = Arc::new(SwitchableSecret(Default::default()));
        credentials.set(Some("fake-before"));
        let secrets = ClientSecretLookup::new(credentials.clone(), &config);

        let server = MockServer::start().await;
        // Only the finish-time value is accepted: a request that carried the
        // begin-time value would not match and the exchange would fail.
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("client_secret=fake-after"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("ya29.secret")))
            .expect(1)
            .mount(&server)
            .await;
        let pending = begin_google(
            &client(),
            &config,
            secrets,
            &format!("{}/auth", server.uri()),
            &format!("{}/token", server.uri()),
            &["127.0.0.1"],
            false,
        )
        .await
        .expect("begin_google");
        credentials.set(Some("fake-after"));
        let (state, port) = prompt_state_and_port(&pending);
        let task = tokio::spawn(pending.finish());
        send_callback(port, &format!("state={state}&code=authcode")).await;
        let token = timeout(Duration::from_secs(5), task)
            .await
            .expect("timed out")
            .expect("join")
            .expect("token");
        assert_eq!(token.access_token, "ya29.secret");
    }

    #[tokio::test]
    async fn loopback_exchange_fails_when_the_client_secret_disappears() {
        let mut config = test_config();
        config.client_secret_env = Some("FAKE_CLIENT_SECRET".into());
        let credentials = Arc::new(SwitchableSecret(Default::default()));
        credentials.set(Some("fake-before"));
        let secrets = ClientSecretLookup::new(credentials.clone(), &config);

        // No mock is mounted: any token request would answer 404.
        let server = MockServer::start().await;
        let pending = begin_google(
            &client(),
            &config,
            secrets,
            &format!("{}/auth", server.uri()),
            &format!("{}/token", server.uri()),
            &["127.0.0.1"],
            false,
        )
        .await
        .expect("begin_google");
        credentials.set(None);
        let (state, port) = prompt_state_and_port(&pending);
        let task = tokio::spawn(pending.finish());
        send_callback(port, &format!("state={state}&code=authcode")).await;
        let error = timeout(Duration::from_secs(5), task)
            .await
            .expect("timed out")
            .expect("join")
            .err()
            .expect("must fail");
        assert!(
            matches!(&error, OAuthError::Configuration(l)
                if l.id == "oauth_error.session.client_secret_env_missing"),
            "got: {error}"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn loopback_denial_surfaces_denied() {
        let server = MockServer::start().await;
        let pending = google_pending(&server).await;
        let (state, port) = prompt_state_and_port(&pending);
        let task = tokio::spawn(pending.finish());
        send_callback(port, &format!("state={state}&error=access_denied")).await;
        let error = timeout(Duration::from_secs(5), task)
            .await
            .expect("timed out")
            .expect("join")
            .err()
            .expect("must fail");
        assert!(matches!(error, OAuthError::Denied), "got: {error}");
    }

    #[tokio::test]
    async fn loopback_duplicate_code_is_ignored() {
        let server = MockServer::start().await;
        let pending = google_pending(&server).await;
        let (state, port) = prompt_state_and_port(&pending);
        let task = tokio::spawn(pending.finish());
        let response = send_callback(port, &format!("state={state}&code=one&code=two")).await;
        assert!(response.starts_with("HTTP/1.1 404"), "got: {response}");
        assert!(!task.is_finished());
        task.abort();
    }

    // -- device flow ---------------------------------------------------------

    fn device_body(server: &MockServer, interval: u64, expires_in: u64) -> Value {
        json!({
            "device_code": "dc-secret",
            "user_code": "ABCD-EFGH",
            "verification_uri": format!("{}/device", server.uri()),
            "interval": interval,
            "expires_in": expires_in
        })
    }

    async fn device_pending(server: &MockServer, interval: u64, expires_in: u64) -> PendingLogin {
        Mock::given(method("POST"))
            .and(path("/device"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(device_body(server, interval, expires_in)),
            )
            .mount(server)
            .await;
        begin_device(
            &client(),
            &test_config(),
            &format!("{}/device", server.uri()),
            &format!("{}/token", server.uri()),
            "test-scope",
            &["127.0.0.1"],
            false,
        )
        .await
        .expect("begin_device")
    }

    #[tokio::test]
    async fn device_polling_honors_interval_and_cumulative_slow_down() {
        let server = MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(move |_req: &wiremock::Request| {
                match counter.fetch_add(1, Ordering::SeqCst) {
                    0 => ResponseTemplate::new(400)
                        .set_body_json(json!({"error": "authorization_pending"})),
                    1 => ResponseTemplate::new(400).set_body_json(json!({"error": "slow_down"})),
                    _ => ResponseTemplate::new(200).set_body_json(token_json("device-token")),
                }
            })
            .mount(&server)
            .await;
        let pending = device_pending(&server, 1, 120).await;
        assert_eq!(pending.prompt().user_code.as_deref(), Some("ABCD-EFGH"));

        let start = std::time::Instant::now();
        let token = pending.finish().await.expect("token");
        let elapsed = start.elapsed();
        assert_eq!(token.access_token, "device-token");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        // 1s pending + 1s + 5s slow_down step before the third request.
        assert!(
            elapsed >= Duration::from_secs(7),
            "interval/slow_down not honored: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn device_polling_stops_at_expiration_deadline() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(400).set_body_json(json!({"error": "authorization_pending"})),
            )
            .mount(&server)
            .await;
        let pending = device_pending(&server, 1, 2).await;
        let start = std::time::Instant::now();
        let error = pending.finish().await.err().expect("must expire");
        assert!(matches!(error, OAuthError::Expired), "got: {error}");
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn device_polling_denied_and_cancellation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(400).set_body_json(json!({"error": "access_denied"})),
            )
            .mount(&server)
            .await;
        let pending = device_pending(&server, 1, 60).await;
        let error = pending.finish().await.err().expect("must be denied");
        assert!(matches!(error, OAuthError::Denied), "got: {error}");

        // Dropping the future cancels polling: it must not resolve on its own.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(400).set_body_json(json!({"error": "authorization_pending"})),
            )
            .mount(&server)
            .await;
        let pending = device_pending(&server, 1, 60).await;
        let cancelled = timeout(Duration::from_secs(3), pending.finish()).await;
        assert!(
            cancelled.is_err(),
            "finish resolved instead of being cancelled"
        );
    }

    // -- refresh -------------------------------------------------------------

    #[tokio::test]
    async fn refresh_preserves_previous_refresh_token_when_omitted() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "access-2",
                "token_type": "Bearer",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;
        let previous = OAuthToken {
            access_token: "access-1".into(),
            refresh_token: Some("rt-1".into()),
            expires_at: Some(Utc::now()),
            metadata: Default::default(),
        };
        let out = refresh_grant(
            &client(),
            &test_config(),
            None,
            "rt-1",
            &previous,
            &format!("{}/token", server.uri()),
            None,
        )
        .await
        .expect("refresh");
        assert_eq!(out.access_token, "access-2");
        assert_eq!(out.refresh_token.as_deref(), Some("rt-1"));
    }

    #[tokio::test]
    async fn refresh_applies_rotated_refresh_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("access-2")))
            .mount(&server)
            .await;
        let previous = OAuthToken {
            access_token: "access-1".into(),
            refresh_token: Some("rt-1".into()),
            expires_at: None,
            metadata: Default::default(),
        };
        let out = refresh_grant(
            &client(),
            &test_config(),
            None,
            "rt-1",
            &previous,
            &format!("{}/token", server.uri()),
            None,
        )
        .await
        .expect("refresh");
        assert_eq!(out.refresh_token.as_deref(), Some("rt-new"));
    }

    #[tokio::test]
    async fn token_parsing_rejects_malformed_bodies() {
        for body in [
            json!({"access_token": "", "token_type": "Bearer"}),
            json!({"access_token": "x", "token_type": "MAC"}),
            json!({"access_token": "x", "token_type": "Bearer", "expires_in": -5}),
            json!({"access_token": "x", "token_type": "Bearer", "expires_in": "soon"}),
        ] {
            assert!(
                parse_token_body(&body, None).is_err(),
                "accepted bad body: {body}"
            );
        }
        // Non-expiring token (GitHub): no expires_in is fine.
        let ok = parse_token_body(&json!({"access_token": "x", "token_type": "bearer"}), None)
            .expect("non-expiring token");
        assert!(ok.expires_at.is_none());
    }

    #[tokio::test]
    async fn refresh_error_body_is_classified_not_leaked() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": "invalid_grant",
                "error_description": "sensitive server detail here"
            })))
            .mount(&server)
            .await;
        let previous = OAuthToken {
            access_token: "a".into(),
            refresh_token: Some("rt".into()),
            expires_at: None,
            metadata: Default::default(),
        };
        let error = refresh_grant(
            &client(),
            &test_config(),
            None,
            "rt",
            &previous,
            &format!("{}/token", server.uri()),
            None,
        )
        .await
        .err()
        .expect("must fail");
        assert!(matches!(error, OAuthError::Expired), "got: {error}");
        assert!(
            !format!("{error}").contains("sensitive"),
            "leaked body: {error}"
        );
    }

    // -- public dispatch ------------------------------------------------------

    #[tokio::test]
    async fn unsupported_provider_and_missing_client_id_are_configuration_errors() {
        let error = begin(&client(), Provider::OpenAiApi, &test_config(), no_secrets())
            .await
            .err()
            .expect("unsupported");
        assert!(
            matches!(error, OAuthError::Configuration(_)),
            "got: {error}"
        );

        let mut config = test_config();
        config.client_id = "   ".into();
        let error = begin(&client(), Provider::GeminiApi, &config, no_secrets())
            .await
            .err()
            .expect("empty client id");
        assert!(
            matches!(error, OAuthError::Configuration(_)),
            "got: {error}"
        );
    }

    #[test]
    fn azure_tenant_validation() {
        let mut config = test_config();
        assert_eq!(resolve_tenant(&config).unwrap(), "organizations");
        config.tenant = Some("contoso.onmicrosoft.com".into());
        assert_eq!(resolve_tenant(&config).unwrap(), "contoso.onmicrosoft.com");
        for bad in ["../evil", "a/b", "org?x", "org#x", "a..b", ""] {
            config.tenant = Some(bad.into());
            assert!(resolve_tenant(&config).is_err(), "accepted tenant {bad:?}");
        }
    }
}
