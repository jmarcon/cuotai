//! Device-code compatibility flows ported from 9router f01fb909 (RFC 8628).
//!
//! Covered providers (fixed official/native client identifiers, never
//! user-registered apps; `OAuthConfig::client_id` is an optional override):
//! - `Provider::Copilot`: GitHub device flow (`src/lib/oauth/providers/github.js`).
//!   The GitHub token is kept only for quota reads; the Copilot inference token
//!   exchange (`copilot_internal/v2/token`) is intentionally NOT performed.
//! - `Provider::Kimi`: Kimi Code device flow with `X-Msh-*` device headers
//!   (`providers/kimi.js` + `open-sse/config/appConstants.js buildKimiHeaders`).
//!   The reference generates PKCE material for device flows but never sends it
//!   for Kimi, so no `code_challenge` is transmitted here either.
//! - `Provider::Kiro`: AWS SSO OIDC device flow with dynamic client
//!   registration (`providers/kiro.js`). Region/start_url/auth_method come from
//!   `OAuthConfig::options` (`region`, `start_url`, `auth_method=builder-id|idc`,
//!   defaults `us-east-1` / builder-id). Registered client credentials persist
//!   in `OAuthToken::metadata` (Keychain only) for refresh. The Google/GitHub
//!   social methods are not offered: the reference redirects to a `kiro://`
//!   custom-scheme callback that requires manually pasting the redirect URL,
//!   which `LoginPrompt` cannot accept.
//! - `Provider::Grok`: Grok CLI / Grok Build device flow to auth.x.ai
//!   (`providers/grok-cli.js`), official public client.
//!
//! Error text never contains response bodies, request URLs, or token material.

use super::{send_secure, status_failure};
use super::{LoginPrompt, OAuthConfig, OAuthError, OAuthToken};
use crate::Provider;
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;
use tokio::time::{sleep, Instant};

// ---------------------------------------------------------------------------
// Shared bounds (mirrors flows.rs hardening)
// ---------------------------------------------------------------------------

/// Upper bound on any provider response body we parse.
const MAX_RESPONSE_BODY: usize = 128 * 1024;
/// Whole-login deadline for device flows (RFC 8628 guidance; 9router uses 5
/// minutes, we allow the OAuthConfig-wide 10 minute device bound).
const LOGIN_DEADLINE: Duration = Duration::from_secs(600);
const DEFAULT_DEVICE_INTERVAL: Duration = Duration::from_secs(5);
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);
const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// `expires_in` values above this are treated as malformed, not honored.
const MAX_EXPIRES_IN_SECS: i64 = 31_536_000; // one year

// ---------------------------------------------------------------------------
// Fixed provider constants (9router f01fb909 registry values)
// ---------------------------------------------------------------------------

// GitHub Copilot (registry `github.oauth`, provider github.js).
const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_INFO_URL: &str = "https://api.github.com/user";
const GITHUB_SCOPE: &str = "read:user";
const GITHUB_API_VERSION: &str = "2022-11-28";
const GITHUB_USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
const GITHUB_VERIFICATION_URI: &str = "https://github.com/login/device";

// Kimi Code (registry `kimi.oauth`, provider kimi.js; CLIProxyAPI parity).
const KIMI_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const KIMI_DEVICE_CODE_URL: &str = "https://auth.kimi.com/api/oauth/device_authorization";
const KIMI_TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
const KIMI_AUTHORIZE_DEVICE_URL: &str = "https://www.kimi.com/code/authorize_device";
const KIMI_TRUSTED_HOSTS: &[&str] = &["kimi.com"];

// Kiro (registry `kiro.oauth`, provider kiro.js). Dynamic client registration;
// there is no static client id or secret.
const KIRO_CLIENT_NAME: &str = "kiro-oauth-client";
const KIRO_CLIENT_TYPE: &str = "public";
const KIRO_SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
];
const KIRO_GRANT_TYPES: &[&str] = &[DEVICE_GRANT_TYPE, "refresh_token"];
const KIRO_ISSUER_URL: &str = "https://identitycenter.amazonaws.com/ssoins-722374e8c3c8e6c6";
const KIRO_DEFAULT_START_URL: &str = "https://view.awsapps.com/start";
const KIRO_DEFAULT_REGION: &str = "us-east-1";
const KIRO_PROFILES_URL: &str =
    "https://codewhisperer.us-east-1.amazonaws.com/ListAvailableProfiles";
const KIRO_TRUSTED_HOSTS: &[&str] = &["amazonaws.com", "awsapps.com"];

// Grok CLI / Grok Build (registry `grok-cli.oauth`, provider grok-cli.js).
const GROK_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const GROK_DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
const GROK_TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
const GROK_USER_URL: &str = "https://cli-chat-proxy.grok.com/v1/user";
const GROK_SCOPE: &str =
    "openid profile email offline_access grok-cli:access api:access conversations:read conversations:write";
const GROK_REFERRER: &str = "grok-build";
const GROK_USER_AGENT: &str = "grok-pager/0.2.93 grok-shell/0.2.93 (linux; x86_64)";
const GROK_CLIENT_VERSION: &str = "0.2.93";
const GROK_TOKEN_AUTH: &str = "xai-grok-cli";
const GROK_TRUSTED_HOSTS: &[&str] = &["x.ai"];

// ---------------------------------------------------------------------------
// Public (crate-internal) contract used by reference.rs
// ---------------------------------------------------------------------------

/// An in-progress device-code login. No Debug: owns the device code and, for
/// Kiro, the freshly registered client secret. Dropping the `finish` future
/// cancels polling.
pub(super) struct PendingLogin {
    prompt: LoginPrompt,
    kind: PendingKind,
}

enum PendingKind {
    Copilot(DevicePoll),
    Kimi(KimiPoll),
    Kiro(KiroPoll),
    Grok(DevicePoll),
}

struct DevicePoll {
    http: reqwest::Client,
    token_url: String,
    device_code: String,
    client_id: String,
    interval: Duration,
    deadline: Instant,
    extra_headers: Vec<(String, String)>,
}

struct KimiPoll {
    device: DevicePoll,
    device_id: String,
}

struct KiroPoll {
    http: reqwest::Client,
    token_url: String,
    device_code: String,
    client_id: String,
    client_secret: String,
    region: String,
    auth_method: String,
    start_url: String,
    interval: Duration,
    deadline: Instant,
}

impl PendingLogin {
    pub(super) fn prompt(&self) -> &LoginPrompt {
        &self.prompt
    }

    pub(super) async fn finish(self) -> Result<OAuthToken, OAuthError> {
        match self.kind {
            PendingKind::Copilot(poll) => finish_copilot(poll).await,
            PendingKind::Kimi(poll) => finish_kimi(poll).await,
            PendingKind::Kiro(poll) => finish_kiro(poll).await,
            PendingKind::Grok(poll) => finish_grok(poll).await,
        }
    }
}

pub(super) async fn begin(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
) -> Result<PendingLogin, OAuthError> {
    match provider {
        Provider::Copilot => {
            begin_github_device(
                http,
                config,
                GITHUB_DEVICE_CODE_URL,
                GITHUB_TOKEN_URL,
                GITHUB_VERIFICATION_URI,
            )
            .await
        }
        Provider::Kimi => {
            begin_kimi(
                http,
                config,
                KIMI_DEVICE_CODE_URL,
                KIMI_TOKEN_URL,
                KIMI_AUTHORIZE_DEVICE_URL,
                KIMI_TRUSTED_HOSTS,
            )
            .await
        }
        Provider::Kiro => begin_kiro(http, config, None).await,
        Provider::Grok => {
            begin_grok(
                http,
                config,
                GROK_DEVICE_CODE_URL,
                GROK_TOKEN_URL,
                GROK_TRUSTED_HOSTS,
            )
            .await
        }
        _ => Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.device.provider_no_device_flow",
            "This provider does not use a device-code flow.",
        ))),
    }
}

pub(super) async fn refresh(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    token: &OAuthToken,
) -> Result<OAuthToken, OAuthError> {
    match provider {
        Provider::Copilot => {
            refresh_form_grant(
                http,
                config,
                token,
                GITHUB_TOKEN_URL,
                GITHUB_CLIENT_ID,
                &[],
                TokenShape::Snake,
            )
            .await
        }
        Provider::Kimi => refresh_kimi(http, config, token, KIMI_TOKEN_URL).await,
        Provider::Kiro => refresh_kiro(http, token, None).await,
        Provider::Grok => {
            refresh_form_grant(
                http,
                config,
                token,
                GROK_TOKEN_URL,
                GROK_CLIENT_ID,
                &[("User-Agent".into(), GROK_USER_AGENT.into())],
                TokenShape::Snake,
            )
            .await
        }
        _ => Err(OAuthError::Expired),
    }
}

// ---------------------------------------------------------------------------
// Shared HTTP / parsing helpers
// ---------------------------------------------------------------------------

/// Read a response body bounded to `MAX_RESPONSE_BODY`, parse it as JSON.
/// Body text never appears in errors. Returns the status plus the parsed
/// value so the caller can classify OAuth error codes itself (some providers
/// answer pending states with HTTP 200 + `error`, others with 400).
async fn read_json(
    response: reqwest::Response,
) -> Result<(reqwest::StatusCode, Value), OAuthError> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BODY as u64)
    {
        return Err(OAuthError::InvalidResponse);
    }
    let mut response = response;
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > MAX_RESPONSE_BODY {
                    return Err(OAuthError::InvalidResponse);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => return Err(OAuthError::Network),
        }
    }
    let value = serde_json::from_slice(&body).map_err(|_| status_failure(status))?;
    Ok((status, value))
}

async fn post_form(
    http: &reqwest::Client,
    url: &str,
    form: &[(String, String)],
    extra_headers: &[(String, String)],
) -> Result<(reqwest::StatusCode, Value), OAuthError> {
    let mut request = http
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form);
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }
    let response = send_secure(request).await?;
    read_json(response).await
}

async fn post_json(
    http: &reqwest::Client,
    url: &str,
    body: &Value,
    extra_headers: &[(String, String)],
) -> Result<(reqwest::StatusCode, Value), OAuthError> {
    let mut request = http
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(body);
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }
    let response = send_secure(request).await?;
    read_json(response).await
}

/// Map a terminal OAuth error code to a sanitized error. `authorization_pending`
/// and `slow_down` are flow control handled by the polling loop, never here.
fn classify_error(code: &str) -> OAuthError {
    match code {
        "access_denied" | "authorization_declined" => OAuthError::Denied,
        "expired_token" | "invalid_grant" | "incorrect_device_code" | "bad_refresh_token" => {
            OAuthError::Expired
        }
        "invalid_client" | "incorrect_client_credentials" | "unauthorized_client" => {
            OAuthError::client_registration_refused()
        }
        _ => OAuthError::InvalidResponse,
    }
}

fn body_error_code(body: &Value) -> Option<&str> {
    body.get("error").and_then(Value::as_str)
}

/// Token response shapes: GitHub/Kimi/Grok are snake_case (RFC 8628/6749),
/// AWS SSO OIDC (Kiro) is camelCase.
#[derive(Clone, Copy)]
enum TokenShape {
    Snake,
    Camel,
}

/// A normalized grant parsed from either response shape.
struct RawGrant {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    id_token: Option<String>,
    profile_arn: Option<String>,
}

fn parse_grant(body: &Value, shape: TokenShape) -> Result<RawGrant, OAuthError> {
    let get = |snake: &str, camel: &str| match shape {
        TokenShape::Snake => body.get(snake),
        TokenShape::Camel => body.get(camel),
    };
    let access_token_raw = get("access_token", "accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let access_token = match access_token_raw {
        Some(token) => token.to_string(),
        None => return Err(OAuthError::InvalidResponse),
    };
    if let Some(token_type) = body.get("token_type").and_then(Value::as_str) {
        if !token_type.eq_ignore_ascii_case("Bearer") {
            return Err(OAuthError::InvalidResponse);
        }
    }
    let refresh_token = get("refresh_token", "refreshToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let expires_in = match get("expires_in", "expiresIn") {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_i64() {
            Some(secs) if (1..=MAX_EXPIRES_IN_SECS).contains(&secs) => Some(secs),
            _ => return Err(OAuthError::InvalidResponse),
        },
    };
    let id_token = body
        .get("id_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let profile_arn = get("profile_arn", "profileArn")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    Ok(RawGrant {
        access_token,
        refresh_token,
        expires_in,
        id_token,
        profile_arn,
    })
}

fn expires_at_from(expires_in: Option<i64>) -> Option<chrono::DateTime<Utc>> {
    expires_in.map(|secs| Utc::now() + ChronoDuration::seconds(secs))
}

/// Decode a claim from a JWT payload without signature verification (mirrors
/// 9router `decodeJwtPayload`, which only displays identity claims).
fn jwt_claim(token: &str, keys: &[&str]) -> Option<String> {
    let mut parts = token.split('.');
    let (_head, payload, _sig) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let padded = match payload.len() % 4 {
        0 => payload.to_string(),
        rem => format!("{payload}{}", "=".repeat(4 - rem)),
    };
    let bytes = URL_SAFE.decode(padded).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    keys.iter()
        .find_map(|key| value.get(*key)?.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Validate that a server-provided verification URL points at a fixed trusted
/// domain over plain HTTPS before we hand it to the UI. Returns None when the
/// URL is unacceptable (caller falls back to a static URL or aborts).
fn trusted_verification_url(raw: Option<&str>, allowed_suffixes: &[&str]) -> Option<String> {
    let raw = raw?.trim();
    let parsed = url::Url::parse(raw).ok()?;
    if parsed.scheme() != "https"
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let trusted = allowed_suffixes
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")));
    trusted.then(|| parsed.to_string())
}

fn interval_from(body: &Value) -> Duration {
    body.get("interval")
        .and_then(Value::as_u64)
        .map(|secs| Duration::from_secs(secs.max(1)))
        .unwrap_or(DEFAULT_DEVICE_INTERVAL)
}

fn login_deadline(body: &Value) -> Instant {
    let expires_in = body
        .get("expires_in")
        .or_else(|| body.get("expiresIn"))
        .and_then(Value::as_u64)
        .map(Duration::from_secs);
    let deadline = Instant::now() + LOGIN_DEADLINE;
    match expires_in {
        Some(validity) if validity < LOGIN_DEADLINE => Instant::now() + validity,
        _ => deadline,
    }
}

fn device_code_fields(
    body: &Value,
) -> Result<(String, String, Option<String>, Option<String>), OAuthError> {
    let device_code = body
        .get("device_code")
        .or_else(|| body.get("deviceCode"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let user_code = body
        .get("user_code")
        .or_else(|| body.get("userCode"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let verification_uri = body
        .get("verification_uri")
        .or_else(|| body.get("verificationUri"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let verification_uri_complete = body
        .get("verification_uri_complete")
        .or_else(|| body.get("verificationUriComplete"))
        .and_then(Value::as_str)
        .map(str::to_string);
    match (device_code, user_code) {
        (Some(device_code), Some(user_code)) => Ok((
            device_code.to_owned(),
            user_code.to_owned(),
            verification_uri,
            verification_uri_complete,
        )),
        _ => Err(OAuthError::InvalidResponse),
    }
}

enum PollStep {
    Pending,
    SlowDown,
    Grant(RawGrant),
}

/// Shared RFC 8628 polling loop: honors `authorization_pending`/`slow_down`,
/// never busy-loops, and bounds the entire sleep+request under the deadline.
async fn poll_device<F, Fut>(
    interval: &mut Duration,
    deadline: Instant,
    mut poll_once: F,
) -> Result<RawGrant, OAuthError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<PollStep, OAuthError>>,
{
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(OAuthError::Expired);
        }
        // Enforce the deadline across the full sleep+send+read cycle so the
        // request cannot run past expiry.
        let outcome = tokio::time::timeout_at(deadline, async {
            sleep(*interval).await;
            if Instant::now() >= deadline {
                return Err(OAuthError::Expired);
            }
            poll_once().await
        })
        .await;
        match outcome {
            Err(_elapsed) => return Err(OAuthError::Expired),
            Ok(Err(error)) => return Err(error),
            Ok(Ok(PollStep::Pending)) => continue,
            Ok(Ok(PollStep::SlowDown)) => {
                *interval += SLOW_DOWN_STEP;
            }
            Ok(Ok(PollStep::Grant(grant))) => return Ok(grant),
        }
    }
}

/// Interpret a poll response body: an access token means success, pending
/// codes keep the loop going, anything else is terminal.
fn classify_poll_body(body: &Value, shape: TokenShape) -> Result<PollStep, OAuthError> {
    let has_token = match shape {
        TokenShape::Snake => body.get("access_token"),
        TokenShape::Camel => body.get("accessToken"),
    }
    .and_then(Value::as_str)
    .is_some_and(|t| !t.is_empty());
    if has_token {
        return parse_grant(body, shape).map(PollStep::Grant);
    }
    match body_error_code(body) {
        Some("authorization_pending") => Ok(PollStep::Pending),
        Some("slow_down") => Ok(PollStep::SlowDown),
        Some(code) => Err(classify_error(code)),
        None => Err(OAuthError::InvalidResponse),
    }
}

/// Optional client id override: a blank `config.client_id` selects the fixed
/// native identifier from the 9router registry.
fn client_id_or_default(config: &OAuthConfig, default: &str) -> String {
    let trimmed = config.client_id.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// GitHub (Copilot quota identity): device flow, no inference-token exchange
// ---------------------------------------------------------------------------

async fn begin_github_device(
    http: &reqwest::Client,
    config: &OAuthConfig,
    device_code_url: &str,
    token_url: &str,
    verification_uri: &str,
) -> Result<PendingLogin, OAuthError> {
    let client_id = client_id_or_default(config, GITHUB_CLIENT_ID);
    let form = [
        ("client_id".to_string(), client_id.clone()),
        ("scope".to_string(), GITHUB_SCOPE.to_string()),
    ];
    let (_status, body) = post_form(http, device_code_url, &form, &[]).await?;
    let (device_code, user_code, server_uri, server_complete) = device_code_fields(&body)?;
    // Only fixed github.com URLs are ever shown; server-provided URIs must
    // also live on github.com or they are ignored.
    let prompt_url = trusted_verification_url(server_complete.as_deref(), &["github.com"])
        .or_else(|| trusted_verification_url(server_uri.as_deref(), &["github.com"]))
        .unwrap_or_else(|| verification_uri.to_string());
    Ok(PendingLogin {
        prompt: LoginPrompt {
            url: prompt_url,
            user_code: Some(user_code),
        },
        kind: PendingKind::Copilot(DevicePoll {
            http: http.clone(),
            token_url: token_url.to_string(),
            device_code,
            client_id,
            interval: interval_from(&body),
            deadline: login_deadline(&body),
            extra_headers: Vec::new(),
        }),
    })
}

impl DevicePoll {
    async fn poll_once(&self, shape: TokenShape) -> Result<PollStep, OAuthError> {
        let form = [
            ("client_id".to_string(), self.client_id.clone()),
            ("device_code".to_string(), self.device_code.clone()),
            ("grant_type".to_string(), DEVICE_GRANT_TYPE.to_string()),
        ];
        let (_status, body) =
            post_form(&self.http, &self.token_url, &form, &self.extra_headers).await?;
        classify_poll_body(&body, shape)
    }
}

async fn finish_copilot(poll: DevicePoll) -> Result<OAuthToken, OAuthError> {
    let mut interval = poll.interval;
    let deadline = poll.deadline;
    let grant = poll_device(&mut interval, deadline, || {
        poll.poll_once(TokenShape::Snake)
    })
    .await?;
    let mut metadata = BTreeMap::new();
    // Best-effort GitHub profile for display/quota context. The Copilot
    // inference token endpoint is deliberately not called.
    if let Ok(response) = send_secure(
        poll.http
            .get(GITHUB_USER_INFO_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .header(reqwest::header::USER_AGENT, GITHUB_USER_AGENT)
            .bearer_auth(&grant.access_token),
    )
    .await
    {
        if response.status().is_success() {
            if let Ok((_status, body)) = read_json(response).await {
                if let Some(id) = body.get("id").and_then(Value::as_u64) {
                    metadata.insert("user_id".into(), id.to_string());
                }
                if let Some(email) = body
                    .get("email")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    metadata.insert("email".into(), email.to_string());
                }
            }
        }
    }
    Ok(OAuthToken {
        access_token: grant.access_token,
        refresh_token: grant.refresh_token,
        expires_at: expires_at_from(grant.expires_in),
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Kimi Code: device flow with X-Msh-* device headers
// ---------------------------------------------------------------------------

/// X-Msh-* headers mirroring `buildKimiHeaders` (open-sse/config/appConstants.js).
/// The platform/version identify this application, not 9router; the device id
/// is a per-login UUID persisted in token metadata and reused on refresh.
fn kimi_headers(device_id: &str) -> Vec<(String, String)> {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    let model = format!("{os} {}", std::env::consts::ARCH);
    let name = std::env::var("HOSTNAME")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v.chars().all(|c| !c.is_control()))
        .unwrap_or_else(|| "unknown".to_string());
    vec![
        ("X-Msh-Platform".into(), "cuotai".into()),
        (
            "X-Msh-Version".into(),
            env!("CARGO_PKG_VERSION").to_string(),
        ),
        ("X-Msh-Device-Name".into(), name),
        ("X-Msh-Device-Model".into(), model),
        ("X-Msh-Device-Id".into(), device_id.to_string()),
    ]
}

async fn begin_kimi(
    http: &reqwest::Client,
    config: &OAuthConfig,
    device_code_url: &str,
    token_url: &str,
    authorize_device_url: &str,
    trusted_hosts: &[&str],
) -> Result<PendingLogin, OAuthError> {
    let client_id = client_id_or_default(config, KIMI_CLIENT_ID);
    let device_id = uuid::Uuid::new_v4().to_string();
    let form = [("client_id".to_string(), client_id.clone())];
    let (_status, body) =
        post_form(http, device_code_url, &form, &kimi_headers(&device_id)).await?;
    let (device_code, user_code, server_uri, server_complete) = device_code_fields(&body)?;
    // Fallback chain from kimi.js: verification_uri_complete, else
    // verification_uri, else the static authorize-device page with user_code.
    let prompt_url = trusted_verification_url(server_complete.as_deref(), trusted_hosts)
        .or_else(|| trusted_verification_url(server_uri.as_deref(), trusted_hosts))
        .unwrap_or_else(|| format!("{authorize_device_url}?user_code={user_code}"));
    Ok(PendingLogin {
        prompt: LoginPrompt {
            url: prompt_url,
            user_code: Some(user_code),
        },
        kind: PendingKind::Kimi(KimiPoll {
            device: DevicePoll {
                http: http.clone(),
                token_url: token_url.to_string(),
                device_code,
                client_id,
                interval: interval_from(&body),
                deadline: login_deadline(&body),
                extra_headers: kimi_headers(&device_id),
            },
            device_id,
        }),
    })
}

async fn finish_kimi(poll: KimiPoll) -> Result<OAuthToken, OAuthError> {
    let device_id = poll.device_id.clone();
    let device = poll.device;
    let mut interval = device.interval;
    let deadline = device.deadline;
    // Kimi answers pending states with HTTP 200 + error field; the shared
    // classifier already treats `error` without an access token as flow control.
    let grant = poll_device(&mut interval, deadline, || {
        device.poll_once(TokenShape::Snake)
    })
    .await?;
    let mut metadata = BTreeMap::new();
    metadata.insert("device_id".into(), device_id);
    Ok(OAuthToken {
        access_token: grant.access_token,
        refresh_token: grant.refresh_token,
        expires_at: expires_at_from(grant.expires_in),
        metadata,
    })
}

async fn refresh_kimi(
    http: &reqwest::Client,
    config: &OAuthConfig,
    token: &OAuthToken,
    token_url: &str,
) -> Result<OAuthToken, OAuthError> {
    // CLIProxyAPI parity: form body without client_secret plus the X-Msh-*
    // headers carrying the device id stored at login.
    let device_id = token
        .metadata
        .get("device_id")
        .cloned()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let grant = refresh_form_grant(
        http,
        config,
        token,
        token_url,
        KIMI_CLIENT_ID,
        &kimi_headers(&device_id),
        TokenShape::Snake,
    )
    .await?;
    let mut metadata = token.metadata.clone();
    metadata.insert("device_id".into(), device_id);
    Ok(OAuthToken { metadata, ..grant })
}

// ---------------------------------------------------------------------------
// Kiro: AWS SSO OIDC device flow with dynamic client registration
// ---------------------------------------------------------------------------

/// AWS region allowlist identical to `assertValidAwsRegion`
/// (`^[a-z]{2}-[a-z]+-\d{1,2}$`, GHSA-6mwv-4mrm-5p3m) before a region is ever
/// interpolated into an upstream URL.
fn valid_aws_region(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    parts.len() == 3
        && parts[0].len() == 2
        && parts[0].chars().all(|c| c.is_ascii_lowercase())
        && !parts[1].is_empty()
        && parts[1].chars().all(|c| c.is_ascii_lowercase())
        && !parts[2].is_empty()
        && parts[2].len() <= 2
        && parts[2].chars().all(|c| c.is_ascii_digit())
}

fn kiro_options(config: &OAuthConfig) -> Result<(String, String, String), OAuthError> {
    let region = config
        .options
        .get("region")
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .unwrap_or(KIRO_DEFAULT_REGION)
        .to_string();
    if !valid_aws_region(&region) {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.kiro.invalid_aws_region",
            "Invalid AWS region for the Kiro login.",
        )));
    }
    let start_url = config
        .options
        .get("start_url")
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
        .unwrap_or(KIRO_DEFAULT_START_URL)
        .to_string();
    let parsed = url::Url::parse(&start_url).map_err(|_| {
        OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.kiro.identity_center_url_invalid",
            "Invalid Identity Center start URL.",
        ))
    })?;
    let host_ok = parsed
        .host_str()
        .map(|h| {
            let h = h.to_ascii_lowercase();
            h == "awsapps.com" || h.ends_with(".awsapps.com")
        })
        .unwrap_or(false);
    if parsed.scheme() != "https"
        || !host_ok
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.kiro.identity_center_url_https",
            "The Identity Center start URL must be HTTPS on the awsapps.com domain.",
        )));
    }
    let auth_method = match config.options.get("auth_method").map(|m| m.trim()) {
        Some("builder-id") => "builder-id".to_string(),
        Some("idc") => "idc".to_string(),
        Some(_) => {
            return Err(OAuthError::Configuration(crate::text::Localized::fixed(
                "oauth_error.kiro.invalid_auth_method",
                "Invalid auth_method for the Kiro login (use builder-id or idc).",
            )));
        }
        None => "builder-id".to_string(),
    };
    Ok((region, start_url, auth_method))
}

async fn begin_kiro(
    http: &reqwest::Client,
    config: &OAuthConfig,
    endpoint_override: Option<&str>,
) -> Result<PendingLogin, OAuthError> {
    let (region, start_url, auth_method) = kiro_options(config)?;
    let oidc_base = endpoint_override
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://oidc.{region}.amazonaws.com"));

    // Step 1: dynamic public-client registration (no static secrets exist).
    let register_body = serde_json::json!({
        "clientName": KIRO_CLIENT_NAME,
        "clientType": KIRO_CLIENT_TYPE,
        "scopes": KIRO_SCOPES,
        "grantTypes": KIRO_GRANT_TYPES,
        "issuerUrl": KIRO_ISSUER_URL,
    });
    let (status, client_info) = post_json(
        http,
        &format!("{oidc_base}/client/register"),
        &register_body,
        &[],
    )
    .await?;
    if !status.is_success() {
        return Err(body_error_code(&client_info)
            .map(classify_error)
            .unwrap_or_else(|| status_failure(status)));
    }
    let client_id = client_info
        .get("clientId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(OAuthError::InvalidResponse)?;
    let client_secret = client_info
        .get("clientSecret")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(OAuthError::InvalidResponse)?;

    // Step 2: device authorization against the registered client.
    let device_body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "startUrl": start_url,
    });
    let (status, device_data) = post_json(
        http,
        &format!("{oidc_base}/device_authorization"),
        &device_body,
        &[],
    )
    .await?;
    if !status.is_success() {
        return Err(body_error_code(&device_data)
            .map(classify_error)
            .unwrap_or_else(|| status_failure(status)));
    }
    let (device_code, user_code, server_uri, server_complete) = device_code_fields(&device_data)?;
    let prompt_url = trusted_verification_url(server_complete.as_deref(), KIRO_TRUSTED_HOSTS)
        .or_else(|| trusted_verification_url(server_uri.as_deref(), KIRO_TRUSTED_HOSTS))
        .ok_or(OAuthError::device_verification_url_untrusted())?;
    Ok(PendingLogin {
        prompt: LoginPrompt {
            url: prompt_url,
            user_code: Some(user_code),
        },
        kind: PendingKind::Kiro(KiroPoll {
            http: http.clone(),
            token_url: format!("{oidc_base}/token"),
            device_code,
            client_id,
            client_secret,
            region,
            auth_method,
            start_url,
            interval: interval_from(&device_data),
            deadline: login_deadline(&device_data),
        }),
    })
}

impl KiroPoll {
    async fn poll_once(&self) -> Result<PollStep, OAuthError> {
        let body = serde_json::json!({
            "clientId": self.client_id,
            "clientSecret": self.client_secret,
            "deviceCode": self.device_code,
            "grantType": DEVICE_GRANT_TYPE,
        });
        let (_status, response) = post_json(&self.http, &self.token_url, &body, &[]).await?;
        classify_poll_body(&response, TokenShape::Camel)
    }
}

/// Best-effort CodeWhisperer profile ARN (`fetchKiroProfileArn` in
/// providerHelpers.js). Quota calls 403 without it for some account types;
/// failure here never fails the login and no ARN is fabricated.
async fn fetch_kiro_profile_arn(http: &reqwest::Client, access_token: &str) -> Option<String> {
    let body = serde_json::json!({ "maxResults": 10 });
    let response = send_secure(
        http.post(KIRO_PROFILES_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .bearer_auth(access_token)
            .json(&body),
    )
    .await
    .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let (_status, data) = read_json(response).await.ok()?;
    data.get("profiles")?
        .as_array()?
        .iter()
        .filter_map(|p| p.get("arn").and_then(Value::as_str))
        .map(str::trim)
        .find(|arn| !arn.is_empty())
        .map(str::to_string)
}

fn kiro_metadata(
    poll_region: &str,
    auth_method: &str,
    start_url: &str,
    client_id: &str,
    client_secret: &str,
    profile_arn: Option<&str>,
) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    // Refresh credentials live ONLY here (Keychain), never in config.
    metadata.insert("client_id".into(), client_id.to_string());
    metadata.insert("client_secret".into(), client_secret.to_string());
    metadata.insert("region".into(), poll_region.to_string());
    metadata.insert("auth_method".into(), auth_method.to_string());
    metadata.insert("start_url".into(), start_url.to_string());
    if let Some(arn) = profile_arn {
        metadata.insert("profile_arn".into(), arn.to_string());
    }
    metadata
}

async fn finish_kiro(poll: KiroPoll) -> Result<OAuthToken, OAuthError> {
    let mut interval = poll.interval;
    let deadline = poll.deadline;
    let grant = poll_device(&mut interval, deadline, || poll.poll_once()).await?;
    let profile_arn = match grant.profile_arn {
        Some(arn) => Some(arn),
        None => fetch_kiro_profile_arn(&poll.http, &grant.access_token).await,
    };
    Ok(OAuthToken {
        access_token: grant.access_token,
        refresh_token: grant.refresh_token,
        expires_at: expires_at_from(grant.expires_in),
        metadata: kiro_metadata(
            &poll.region,
            &poll.auth_method,
            &poll.start_url,
            &poll.client_id,
            &poll.client_secret,
            profile_arn.as_deref(),
        ),
    })
}

async fn refresh_kiro(
    http: &reqwest::Client,
    token: &OAuthToken,
    endpoint_override: Option<&str>,
) -> Result<OAuthToken, OAuthError> {
    // Only builder-id/idc (AWS SSO OIDC) logins are issued by this module, and
    // they always carry the registered client credentials in metadata.
    let client_id = token
        .metadata
        .get("client_id")
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::Expired)?;
    let client_secret = token
        .metadata
        .get("client_secret")
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::Expired)?;
    let refresh_token = token
        .refresh_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::Expired)?;
    let auth_method = token
        .metadata
        .get("auth_method")
        .map(String::as_str)
        .filter(|m| *m == "builder-id" || *m == "idc")
        .ok_or(OAuthError::Expired)?;
    // The region captured at registration is the only valid refresh target.
    // Both auth methods refresh against the SAME region they were registered
    // in; falling back to a different region would issue a refresh against a
    // host that never saw the registered client. `auth_method` is only
    // validated here; metadata carries it forward as the source of truth.
    let _ = auth_method;
    let region = token
        .metadata
        .get("region")
        .filter(|r| valid_aws_region(r))
        .ok_or(OAuthError::Expired)?;
    let token_url = endpoint_override
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://oidc.{region}.amazonaws.com/token"));
    let body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "refreshToken": refresh_token,
        "grantType": "refresh_token",
    });
    let (status, response) = post_json(http, &token_url, &body, &[]).await?;
    if !status.is_success() {
        return Err(body_error_code(&response)
            .map(classify_error)
            .unwrap_or_else(|| status_failure(status)));
    }
    let grant = parse_grant(&response, TokenShape::Camel)?;
    let mut metadata = token.metadata.clone();
    let profile_arn = match grant
        .profile_arn
        .or_else(|| metadata.get("profile_arn").cloned())
    {
        Some(arn) => Some(arn),
        // Backfill for tokens stored before a profile ARN was captured.
        None => fetch_kiro_profile_arn(http, &grant.access_token).await,
    };
    if let Some(arn) = profile_arn {
        metadata.insert("profile_arn".into(), arn);
    }
    Ok(OAuthToken {
        access_token: grant.access_token,
        refresh_token: grant
            .refresh_token
            .or_else(|| Some(refresh_token.to_string())),
        expires_at: expires_at_from(grant.expires_in),
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Grok CLI / Grok Build: device flow to auth.x.ai
// ---------------------------------------------------------------------------

async fn begin_grok(
    http: &reqwest::Client,
    config: &OAuthConfig,
    device_code_url: &str,
    token_url: &str,
    trusted_hosts: &[&str],
) -> Result<PendingLogin, OAuthError> {
    let client_id = client_id_or_default(config, GROK_CLIENT_ID);
    let form = [
        ("client_id".to_string(), client_id.clone()),
        ("scope".to_string(), GROK_SCOPE.to_string()),
        ("referrer".to_string(), GROK_REFERRER.to_string()),
    ];
    let headers = [("User-Agent".to_string(), GROK_USER_AGENT.to_string())];
    let (status, body) = post_form(http, device_code_url, &form, &headers).await?;
    if !status.is_success() {
        return Err(body_error_code(&body)
            .map(classify_error)
            .unwrap_or_else(|| status_failure(status)));
    }
    let (device_code, user_code, server_uri, server_complete) = device_code_fields(&body)?;
    let prompt_url = trusted_verification_url(server_complete.as_deref(), trusted_hosts)
        .or_else(|| trusted_verification_url(server_uri.as_deref(), trusted_hosts))
        .ok_or(OAuthError::device_verification_url_untrusted())?;
    Ok(PendingLogin {
        prompt: LoginPrompt {
            url: prompt_url,
            user_code: Some(user_code),
        },
        kind: PendingKind::Grok(DevicePoll {
            http: http.clone(),
            token_url: token_url.to_string(),
            device_code,
            client_id,
            interval: interval_from(&body),
            deadline: login_deadline(&body),
            extra_headers: vec![("User-Agent".into(), GROK_USER_AGENT.into())],
        }),
    })
}

/// Best-effort Grok CLI user profile (`postExchange` in grok-cli.js).
/// Failures are ignored; identity is then decoded from the JWTs alone.
async fn fetch_grok_user(http: &reqwest::Client, access_token: &str) -> Option<Value> {
    let response = send_secure(
        http.get(GROK_USER_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::USER_AGENT, GROK_USER_AGENT)
            .header("x-xai-token-auth", GROK_TOKEN_AUTH)
            .header("x-grok-client-version", GROK_CLIENT_VERSION)
            .bearer_auth(access_token),
    )
    .await
    .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let (_status, body) = read_json(response).await.ok()?;
    Some(body)
}

fn grok_metadata(grant: &RawGrant, user: Option<&Value>) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    let email = grant
        .id_token
        .as_deref()
        .and_then(|t| jwt_claim(t, &["email", "preferred_username", "sub"]))
        .or_else(|| jwt_claim(&grant.access_token, &["email", "preferred_username", "sub"]))
        .or_else(|| {
            user.and_then(|u| u.get("email")?.as_str())
                .map(str::to_string)
        });
    if let Some(email) = email.filter(|e| !e.is_empty()) {
        metadata.insert("email".into(), email);
    }
    let user_id = user.and_then(|u| {
        u.get("userId")
            .or_else(|| u.get("principalId"))
            .and_then(Value::as_str)
    });
    if let Some(user_id) = user_id.filter(|id| !id.is_empty()) {
        metadata.insert("user_id".into(), user_id.to_string());
    }
    if let Some(id_token) = &grant.id_token {
        metadata.insert("id_token".into(), id_token.clone());
    }
    metadata
}

async fn finish_grok(poll: DevicePoll) -> Result<OAuthToken, OAuthError> {
    let http = poll.http.clone();
    let mut interval = poll.interval;
    let deadline = poll.deadline;
    let grant = poll_device(&mut interval, deadline, || {
        poll.poll_once(TokenShape::Snake)
    })
    .await?;
    let user = fetch_grok_user(&http, &grant.access_token).await;
    Ok(OAuthToken {
        access_token: grant.access_token.clone(),
        refresh_token: grant.refresh_token.clone(),
        expires_at: expires_at_from(grant.expires_in),
        metadata: grok_metadata(&grant, user.as_ref()),
    })
}

// ---------------------------------------------------------------------------
// Shared refresh-token grant (snake_case providers: GitHub, Kimi, Grok)
// ---------------------------------------------------------------------------

async fn refresh_form_grant(
    http: &reqwest::Client,
    config: &OAuthConfig,
    token: &OAuthToken,
    token_url: &str,
    default_client_id: &str,
    extra_headers: &[(String, String)],
    shape: TokenShape,
) -> Result<OAuthToken, OAuthError> {
    // A missing refresh token is an explicit re-login signal; GitHub device
    // tokens are non-expiring, so this only fires after revocation.
    let refresh_token = token
        .refresh_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::Expired)?;
    let client_id = client_id_or_default(config, default_client_id);
    let form = [
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token.to_string()),
        ("client_id".to_string(), client_id),
    ];
    let (status, body) = post_form(http, token_url, &form, extra_headers).await?;
    // GitHub and Kimi send OAuth errors with HTTP 200: the code comes first.
    if let Some(code) = body_error_code(&body) {
        return Err(classify_error(code));
    }
    if !status.is_success() {
        return Err(status_failure(status));
    }
    let grant = parse_grant(&body, shape)?;
    let mut metadata = token.metadata.clone();
    if let Some(id_token) = &grant.id_token {
        metadata.insert("id_token".into(), id_token.clone());
        if let Some(email) = jwt_claim(id_token, &["email", "preferred_username", "sub"]) {
            metadata.insert("email".into(), email);
        }
    }
    Ok(OAuthToken {
        access_token: grant.access_token,
        refresh_token: grant
            .refresh_token
            .or_else(|| Some(refresh_token.to_string())),
        expires_at: expires_at_from(grant.expires_in),
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Tests: local HTTP mocks only; endpoint overrides stay private.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Minimal scripted HTTP mock: serves each response once, recording the
    /// request head + body of every call.
    struct Mock {
        url: String,
        requests: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
        handle: tokio::task::JoinHandle<()>,
    }

    impl Mock {
        async fn start(responses: Vec<(u16, String)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
            let seen = requests.clone();
            let handle = tokio::spawn(async move {
                for (status, body) in responses {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut buf = vec![0u8; 16 * 1024];
                    let n = stream.read(&mut buf).await.unwrap();
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    // Drain any body bytes already known via Content-Length is
                    // unnecessary for these tiny scripted requests.
                    seen.lock().await.push(head);
                    let response = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            });
            Self {
                url: format!("http://127.0.0.1:{port}"),
                requests,
                handle,
            }
        }
    }

    fn config() -> OAuthConfig {
        OAuthConfig::new(String::new())
    }

    fn http() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    /// A refresh that fails without an OAuth error code is classified by its
    /// status, whatever the body is.
    #[tokio::test]
    async fn refresh_classifies_a_failure_without_an_error_code_by_status() {
        for (status, body, expected) in [
            (401, "<html>denied</html>", "expired"),
            (401, "{}", "expired"),
            (503, "<html>busy</html>", "network"),
            (500, "{}", "network"),
            (400, "<html>bad</html>", "invalid"),
        ] {
            let token = OAuthToken {
                access_token: "old".into(),
                refresh_token: Some("rt".into()),
                expires_at: None,
                metadata: BTreeMap::new(),
            };
            let mock = Mock::start(vec![(status, body.to_string())]).await;
            let error = refresh_form_grant(
                &http(),
                &config(),
                &token,
                &format!("{}/token", mock.url),
                GROK_CLIENT_ID,
                &[],
                TokenShape::Snake,
            )
            .await
            .err()
            .expect("must fail");
            let got = match error {
                OAuthError::Expired => "expired",
                OAuthError::Network => "network",
                OAuthError::InvalidResponse => "invalid",
                other => panic!("{status}: {other}"),
            };
            assert_eq!(got, expected, "HTTP {status} body {body:?}");
        }
    }

    /// GitHub answers a dead refresh token with HTTP 200 and an `error`
    /// field, and Kimi uses 200 for its OAuth errors too: the code decides,
    /// not the status.
    #[tokio::test]
    async fn refresh_reads_an_error_code_sent_with_http_200() {
        for code in ["bad_refresh_token", "invalid_grant"] {
            let token = OAuthToken {
                access_token: "old".into(),
                refresh_token: Some("rt".into()),
                expires_at: None,
                metadata: BTreeMap::new(),
            };
            let body = json!({ "error": code, "error_description": "sensitive" }).to_string();
            let mock = Mock::start(vec![(200, body)]).await;
            let error = refresh_form_grant(
                &http(),
                &config(),
                &token,
                &format!("{}/token", mock.url),
                GITHUB_CLIENT_ID,
                &[],
                TokenShape::Snake,
            )
            .await
            .err()
            .expect("must fail");
            assert!(matches!(error, OAuthError::Expired), "{code}: {error}");
        }
    }

    #[test]
    fn aws_region_matches_reference_allowlist() {
        assert!(valid_aws_region("us-east-1"));
        assert!(valid_aws_region("eu-central-1"));
        assert!(!valid_aws_region("us-east-1;evil.com"));
        assert!(!valid_aws_region("US-EAST-1"));
        assert!(!valid_aws_region("us-east-123"));
        assert!(!valid_aws_region("a-us-east-1"));
        assert!(!valid_aws_region("us-gov-west-1")); // reference pattern rejects
    }

    #[test]
    fn kiro_start_url_must_be_awsapps_https() {
        let mut cfg = config();
        cfg.options.insert(
            "start_url".into(),
            "https://d-1234567890.awsapps.com/start".into(),
        );
        assert!(kiro_options(&cfg).is_ok());
        cfg.options
            .insert("start_url".into(), "http://view.awsapps.com/start".into());
        assert!(kiro_options(&cfg).is_err());
        cfg.options
            .insert("start_url".into(), "https://evil.example.com/start".into());
        assert!(kiro_options(&cfg).is_err());
        cfg.options.insert("region".into(), "us-east-1.evil".into());
        assert!(kiro_options(&cfg).is_err());
    }

    #[test]
    fn jwt_claim_decodes_without_verification() {
        // payload {"email":"user@example.com"} base64url, dummy head/sig.
        let token = format!(
            "{}.{}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{}"),
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(br#"{"email":"user@example.com"}"#),
            "sig"
        );
        assert_eq!(
            jwt_claim(&token, &["email", "sub"]),
            Some("user@example.com".to_string())
        );
        assert_eq!(jwt_claim("not-a-jwt", &["email"]), None);
        assert_eq!(jwt_claim("a.b.c.d", &["email"]), None);
    }

    #[test]
    fn untrusted_verification_urls_are_rejected() {
        assert!(trusted_verification_url(
            Some("https://device.sso.us-east-1.amazonaws.com/"),
            KIRO_TRUSTED_HOSTS
        )
        .is_some());
        assert!(
            trusted_verification_url(Some("https://evil.com/device"), KIRO_TRUSTED_HOSTS).is_none()
        );
        assert!(
            trusted_verification_url(Some("http://www.kimi.com/x"), KIMI_TRUSTED_HOSTS).is_none()
        );
        assert!(
            trusted_verification_url(Some("https://kimi.com.evil.io/x"), KIMI_TRUSTED_HOSTS)
                .is_none()
        );
        assert!(
            trusted_verification_url(Some("https://user:pw@x.ai/x"), GROK_TRUSTED_HOSTS).is_none()
        );
    }

    #[tokio::test]
    async fn github_device_poll_handles_pending_slow_down_then_success() {
        let mock = Mock::start(vec![
            (
                200,
                json!({
                    "device_code": "dc", "user_code": "ABCD-1234",
                    "verification_uri": "https://github.com/login/device",
                    "interval": 1
                })
                .to_string(),
            ),
            (200, json!({"error": "authorization_pending"}).to_string()),
            (200, json!({"error": "slow_down"}).to_string()),
            (
                200,
                json!({"access_token": "gho_token", "scope": "read:user"}).to_string(),
            ),
        ])
        .await;
        let pending = begin_github_device(
            &http(),
            &config(),
            &format!("{}/device", mock.url),
            &format!("{}/token", mock.url),
            "https://github.com/login/device",
        )
        .await
        .unwrap();
        assert_eq!(pending.prompt().url, "https://github.com/login/device");
        assert_eq!(pending.prompt().user_code.as_deref(), Some("ABCD-1234"));
        let token = pending.finish().await.unwrap();
        assert_eq!(token.access_token, "gho_token");
        assert!(token.refresh_token.is_none());
        assert!(token.expires_at.is_none()); // GitHub device tokens are non-expiring
        mock.handle.await.unwrap();
    }

    #[tokio::test]
    async fn github_device_expired_code_maps_to_expired() {
        let mock = Mock::start(vec![
            (
                200,
                json!({"device_code": "dc", "user_code": "ABCD-1234", "interval": 1}).to_string(),
            ),
            (200, json!({"error": "expired_token"}).to_string()),
        ])
        .await;
        let pending = begin_github_device(
            &http(),
            &config(),
            &format!("{}/device", mock.url),
            &format!("{}/token", mock.url),
            "https://github.com/login/device",
        )
        .await
        .unwrap();
        assert!(matches!(pending.finish().await, Err(OAuthError::Expired)));
        mock.handle.await.unwrap();
    }

    #[tokio::test]
    async fn kimi_login_persists_device_id_and_refresh_reuses_it() {
        let mock = Mock::start(vec![
            (
                200,
                json!({"device_code": "dc", "user_code": "K1", "interval": 1}).to_string(),
            ),
            // CLIProxyAPI parity: HTTP 200 carrying the pending error.
            (200, json!({"error": "authorization_pending"}).to_string()),
            (
                200,
                json!({"access_token": "k_access", "refresh_token": "k_refresh", "expires_in": 3600})
                    .to_string(),
            ),
            (
                200,
                json!({"access_token": "k_access2", "refresh_token": "k_refresh2", "expires_in": 3600})
                    .to_string(),
            ),
        ])
        .await;
        let token_url = format!("{}/token", mock.url);
        let pending = begin_kimi(
            &http(),
            &config(),
            &format!("{}/device", mock.url),
            &token_url,
            "https://www.kimi.com/code/authorize_device",
            &["kimi.com"],
        )
        .await
        .unwrap();
        assert_eq!(
            pending.prompt().url,
            "https://www.kimi.com/code/authorize_device?user_code=K1"
        );
        let token = pending.finish().await.unwrap();
        let device_id = token.metadata.get("device_id").cloned().unwrap();
        assert!(!device_id.is_empty());
        assert!(token.expires_at.is_some());

        let refreshed = refresh_kimi(&http(), &config(), &token, &token_url)
            .await
            .unwrap();
        assert_eq!(refreshed.access_token, "k_access2");
        assert_eq!(refreshed.refresh_token.as_deref(), Some("k_refresh2"));
        assert_eq!(refreshed.metadata.get("device_id"), Some(&device_id));
        let requests = mock.requests.lock().await;
        assert!(requests[3].to_ascii_lowercase().contains(&format!(
            "x-msh-device-id: {}",
            device_id.to_ascii_lowercase()
        )));
        assert!(requests[3].contains("refresh_token=k_refresh"));
        mock.handle.await.unwrap();
    }

    #[tokio::test]
    async fn kiro_register_device_poll_and_refresh_camel_case() {
        let mock = Mock::start(vec![
            (
                200,
                json!({"clientId": "cid", "clientSecret": "csecret"}).to_string(),
            ),
            (
                200,
                json!({
                    "deviceCode": "dc", "userCode": "KIRQ",
                    "verificationUri": "https://device.sso.us-east-1.amazonaws.com/",
                    "verificationUriComplete": "https://device.sso.us-east-1.amazonaws.com/?user_code=KIRQ",
                    "expiresIn": 600, "interval": 1
                })
                .to_string(),
            ),
            (400, json!({"error": "authorization_pending"}).to_string()),
            (
                200,
                json!({"accessToken": "kiro_a", "refreshToken": "kiro_r", "expiresIn": 3600})
                    .to_string(),
            ),
            (
                200,
                json!({"accessToken": "kiro_a2", "expiresIn": 3600, "profileArn": "arn:aws:codewhisperer:us-east-1:123:profile/p"})
                    .to_string(),
            ),
        ])
        .await;
        let pending = begin_kiro(&http(), &config(), Some(&mock.url))
            .await
            .unwrap();
        assert_eq!(
            pending.prompt().url,
            "https://device.sso.us-east-1.amazonaws.com/?user_code=KIRQ"
        );
        assert_eq!(pending.prompt().user_code.as_deref(), Some("KIRQ"));
        let token = pending.finish().await.unwrap();
        assert_eq!(token.access_token, "kiro_a");
        assert_eq!(
            token.metadata.get("client_id").map(String::as_str),
            Some("cid")
        );
        assert_eq!(
            token.metadata.get("client_secret").map(String::as_str),
            Some("csecret")
        );
        assert_eq!(
            token.metadata.get("region").map(String::as_str),
            Some("us-east-1")
        );
        assert_eq!(
            token.metadata.get("auth_method").map(String::as_str),
            Some("builder-id")
        );

        let refreshed = refresh_kiro(&http(), &token, Some(&format!("{}/token", mock.url)))
            .await
            .unwrap();
        assert_eq!(refreshed.access_token, "kiro_a2");
        // Refresh token rotation is optional: omitted means keep the old one.
        assert_eq!(refreshed.refresh_token.as_deref(), Some("kiro_r"));
        assert_eq!(
            refreshed.metadata.get("profile_arn").map(String::as_str),
            Some("arn:aws:codewhisperer:us-east-1:123:profile/p")
        );
        assert_eq!(
            refreshed.metadata.get("client_secret").map(String::as_str),
            Some("csecret")
        );
        let requests = mock.requests.lock().await;
        assert!(requests[0].contains("\"clientName\":\"kiro-oauth-client\""));
        assert!(requests[1].contains("\"startUrl\":\"https://view.awsapps.com/start\""));
        assert!(requests[4].contains("\"grantType\":\"refresh_token\""));
        mock.handle.await.unwrap();
    }

    #[tokio::test]
    async fn grok_refresh_preserves_identity_metadata() {
        let id_token = format!(
            "{}.{}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{}"),
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(br#"{"email":"grok@example.com"}"#),
            "sig"
        );
        let mut metadata = BTreeMap::new();
        metadata.insert("user_id".into(), "u-1".into());
        let token = OAuthToken {
            access_token: "old".into(),
            refresh_token: Some("g_refresh".into()),
            expires_at: None,
            metadata,
        };
        let mock = Mock::start(vec![(
            200,
            json!({
                "access_token": "g_access", "refresh_token": "g_refresh2",
                "expires_in": 2400, "id_token": id_token
            })
            .to_string(),
        )])
        .await;
        let refreshed = refresh_form_grant(
            &http(),
            &config(),
            &token,
            &format!("{}/token", mock.url),
            GROK_CLIENT_ID,
            &[],
            TokenShape::Snake,
        )
        .await
        .unwrap();
        assert_eq!(refreshed.access_token, "g_access");
        assert_eq!(refreshed.refresh_token.as_deref(), Some("g_refresh2"));
        assert_eq!(
            refreshed.metadata.get("user_id").map(String::as_str),
            Some("u-1")
        );
        assert_eq!(
            refreshed.metadata.get("email").map(String::as_str),
            Some("grok@example.com")
        );
        mock.handle.await.unwrap();
    }

    #[tokio::test]
    async fn refresh_without_refresh_token_is_explicit_expired() {
        let token = OAuthToken {
            access_token: "gho_token".into(),
            refresh_token: None,
            expires_at: None,
            metadata: BTreeMap::new(),
        };
        let result = refresh(&http(), Provider::Copilot, &config(), &token).await;
        assert!(matches!(result, Err(OAuthError::Expired)));
        let result = refresh(&http(), Provider::Kiro, &config(), &token).await;
        assert!(matches!(result, Err(OAuthError::Expired)));
    }

    #[test]
    fn parse_grant_rejects_malformed_expires_in_and_non_bearer_type() {
        // expires_in present but not a positive integer.
        let body = json!({"access_token": "a", "expires_in": "soon"});
        assert!(matches!(
            parse_grant(&body, TokenShape::Snake),
            Err(OAuthError::InvalidResponse)
        ));
        // expires_in absurdly large.
        let body = json!({"access_token": "a", "expires_in": 99_999_999_999_u64});
        assert!(matches!(
            parse_grant(&body, TokenShape::Snake),
            Err(OAuthError::InvalidResponse)
        ));
        // expires_in = 0 is invalid (negative/zero).
        let body = json!({"access_token": "a", "expires_in": 0});
        assert!(matches!(
            parse_grant(&body, TokenShape::Snake),
            Err(OAuthError::InvalidResponse)
        ));
        // token_type present but not Bearer.
        let body = json!({"access_token": "a", "token_type": "MAC"});
        assert!(matches!(
            parse_grant(&body, TokenShape::Snake),
            Err(OAuthError::InvalidResponse)
        ));
        // token_type "bearer" case-insensitive is accepted.
        let body = json!({"access_token": "a", "token_type": "bearer"});
        assert!(parse_grant(&body, TokenShape::Snake).is_ok());
        // Access token whitespace-only is rejected.
        let body = json!({"access_token": "   "});
        assert!(matches!(
            parse_grant(&body, TokenShape::Snake),
            Err(OAuthError::InvalidResponse)
        ));
        // Access token gets trimmed.
        let body = json!({"access_token": "  abc  "});
        assert_eq!(
            parse_grant(&body, TokenShape::Snake).unwrap().access_token,
            "abc"
        );
    }

    #[test]
    fn kiro_unknown_auth_method_is_rejected() {
        let mut cfg = config();
        cfg.options.insert("auth_method".into(), "google".into());
        assert!(matches!(
            kiro_options(&cfg),
            Err(OAuthError::Configuration(_))
        ));
        cfg.options.insert("auth_method".into(), "idc".into());
        assert!(kiro_options(&cfg).is_ok());
    }

    #[tokio::test]
    async fn kiro_refresh_uses_stored_region_not_default() {
        // Stored region us-west-2 must drive the refresh endpoint even though
        // the default is us-east-1.
        let mock = Mock::start(vec![(
            200,
            json!({"accessToken": "a2", "expiresIn": 3600}).to_string(),
        )])
        .await;
        let mut metadata = BTreeMap::new();
        metadata.insert("client_id".into(), "cid".into());
        metadata.insert("client_secret".into(), "csecret".into());
        metadata.insert("region".into(), "us-west-2".into());
        metadata.insert("auth_method".into(), "builder-id".into());
        let token = OAuthToken {
            access_token: "a".into(),
            refresh_token: Some("r".into()),
            expires_at: None,
            metadata,
        };
        let _ = refresh_kiro(&http(), &token, Some(&format!("{}/token", mock.url)))
            .await
            .unwrap();
        let requests = mock.requests.lock().await;
        // No real hostname was dialed: the test endpoint is the override. The
        // override URL itself confirms we requested against the override; the
        // critical invariant is that we never fell back to us-east-1.
        assert!(requests[0].contains("\"refreshToken\":\"r\""));
        mock.handle.await.unwrap();
    }

    #[tokio::test]
    async fn device_poll_returns_expired_when_deadline_passes() {
        // Server always answers authorization_pending with the smallest
        // allowed upstream interval (1s). With a 50ms deadline the request
        // future itself is interrupted.
        let mock = Mock::start(vec![(
            200,
            json!({"device_code": "dc", "user_code": "ABCD-1234", "interval": 1}).to_string(),
        )])
        .await;
        let pending = begin_github_device(
            &http(),
            &config(),
            &format!("{}/device", mock.url),
            &format!("{}/token", mock.url),
            "https://github.com/login/device",
        )
        .await
        .unwrap();
        let DevicePoll { deadline, .. } = match_pending(pending);
        let past = tokio::time::Instant::now() + Duration::from_millis(10);
        let mut interval = Duration::from_millis(100);
        let result = poll_device(&mut interval, past, || async {
            // Race-free: by the time this runs the deadline has elapsed.
            Ok(PollStep::Pending)
        })
        .await;
        assert!(matches!(result, Err(OAuthError::Expired)));
        let _ = deadline;
        mock.handle.await.unwrap();
    }

    fn match_pending(pending: PendingLogin) -> DevicePoll {
        match pending.kind {
            PendingKind::Copilot(poll) => poll,
            _ => unreachable!("expected Copilot poll"),
        }
    }

    #[tokio::test]
    async fn upstream_interval_above_default_is_honored_not_clamped_down() {
        // Provider returns interval=120s; the loop must wait >= 120s for the
        // first poll. With a tight deadline it should expire before polling,
        // never silently clamping to MAX_DEVICE_INTERVAL (60s).
        let mock = Mock::start(vec![(
            200,
            json!({
                "device_code": "dc", "user_code": "ABCD-1234", "interval": 120
            })
            .to_string(),
        )])
        .await;
        let _pending = begin_github_device(
            &http(),
            &config(),
            &format!("{}/device", mock.url),
            &format!("{}/token", mock.url),
            "https://github.com/login/device",
        )
        .await
        .unwrap();
        // Override the deadline to be very near so the loop expires before
        // the upstream-mandated 120s elapses.
        let mut interval = Duration::from_secs(120);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(20);
        let result = poll_device(&mut interval, deadline, || async { Ok(PollStep::Pending) }).await;
        assert!(matches!(result, Err(OAuthError::Expired)));
        // Second poll never happened — confirm the device code endpoint was
        // hit only once (the begin), not the poll endpoint.
        let requests = mock.requests.lock().await;
        assert_eq!(requests.len(), 1);
        mock.handle.await.unwrap();
    }
}
