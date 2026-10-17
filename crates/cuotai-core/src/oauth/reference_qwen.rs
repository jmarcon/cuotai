//! Qwen Code native OAuth login (device authorization grant, RFC 8628 + RFC 7636).
//!
//! Ported from the official `QwenLM/qwen-code` reference
//! (`packages/core/src/qwen/qwenOAuth2.ts`, Apache-2.0). The flow is the
//! standard device-code grant, NOT the DashScope API key path, and uses
//! the same fixed public client id the Qwen CLI ships with.
//!
//!   1. POST `https://chat.qwen.ai/api/v1/oauth2/device/code`
//!        - body: `client_id`, `scope=openid profile email model.completion`,
//!          `code_challenge` (S256), `code_challenge_method=S256`.
//!        - response: `{device_code, user_code, verification_uri,
//!          verification_uri_complete, expires_in}`.
//!   2. Open `verification_uri_complete` for the user; show `user_code`.
//!   3. POST `https://chat.qwen.ai/api/v1/oauth2/token`
//!        - body: `grant_type=urn:ietf:params:oauth:grant-type:device_code`,
//!          `client_id`, `device_code`, `code_verifier` (PKCE).
//!        - pending: `{error: "authorization_pending"}` (HTTP 400) or
//!          `{status: "pending", slowDown?}` (RFC 8628 §3.5 variant).
//!        - success: `{access_token, refresh_token, token_type, expires_in,
//!          scope?, resource_url?, endpoint?}`.
//!   4. Refresh on expiry: same endpoint, `grant_type=refresh_token`,
//!      `client_id`, `refresh_token`. The server rotates or preserves
//!      `refresh_token` per RFC 6749 §6.
//!
//! Safety properties:
//! - Only the fixed official hosts `chat.qwen.ai` are ever contacted
//!   (device authorization, token exchange). No URL from the response is
//!   ever used for follow-up requests.
//! - HTTP body reads are bounded (128 KiB); PKCE verifier / state come
//!   from the OS CSPRNG; token bodies are validated as non-empty bearer
//!   with a finite sane `expires_in` (RFC 6749 §5.1).
//! - No token material appears in any error text; upstream error codes
//!   collapse to `OAuthError::{Denied, Expired, InvalidResponse}`.
//! - `resource_url` returned by the server is preserved in metadata for
//!   the inference adapter (Qwen Code portal), but never used as a base
//!   for arbitrary quota queries here.

use super::{LoginPrompt, OAuthConfig, OAuthError, OAuthToken};
use crate::Provider;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration as ChronoDuration, Utc};
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::time::{sleep, timeout_at, Instant};

// ---------------------------------------------------------------------------
// Official endpoints (registry mirror of qwenOAuth2.ts). These are the ONLY
// hosts the public surface ever contacts; nothing is taken from response
// bodies. Internal helpers take URL parameters so tests can wiremock.
// ---------------------------------------------------------------------------

const QWEN_OAUTH_BASE_URL: &str = "https://chat.qwen.ai";
const QWEN_DEVICE_CODE_ENDPOINT: &str = "https://chat.qwen.ai/api/v1/oauth2/device/code";
const QWEN_TOKEN_ENDPOINT: &str = "https://chat.qwen.ai/api/v1/oauth2/token";

/// Public client identifier shipped with `QwenLM/qwen-code` (Qwen CLI). Fixed,
/// not user-registered; `OAuthConfig::client_id` may override for staging.
const QWEN_OAUTH_CLIENT_ID_DEFAULT: &str = "f0304373b74a44d2b584a3fb70ca9e56";
const QWEN_OAUTH_SCOPE: &str = "openid profile email model.completion";
const QWEN_DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const QWEN_REFRESH_GRANT_TYPE: &str = "refresh_token";

/// Whole-login deadline (10 minutes — well above RFC 8628 §3.5's 1-hour
/// default and below any reasonable user patience budget).
const LOGIN_DEADLINE: Duration = Duration::from_secs(600);
const DEFAULT_DEVICE_INTERVAL: Duration = Duration::from_secs(2);
/// RFC 8628 §3.5: each `slow_down` MUST add 5 seconds with no downward cap;
/// the new interval is sticky for the rest of the poll. The outer
/// `LOGIN_DEADLINE` is the only absolute bound.
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);

/// Upper bound on any provider response body we parse (matches
/// `reference_device.rs` hardening).
const MAX_RESPONSE_BODY: usize = 128 * 1024;
/// `expires_in` values above this are treated as malformed, not honored.
const MAX_EXPIRES_IN_SECS: i64 = 31_536_000; // one year

// ---------------------------------------------------------------------------
// Public contract used by reference.rs (parent dispatcher)
// ---------------------------------------------------------------------------

/// In-progress Qwen device-code login. Owns the device code + PKCE verifier;
/// dropping the `finish` future cancels polling.
pub(super) struct PendingLogin {
    prompt: LoginPrompt,
    http: reqwest::Client,
    token_url: String,
    client_id: String,
    device_code: String,
    code_verifier: String,
    interval: Duration,
    deadline: Instant,
}

impl PendingLogin {
    pub(super) fn prompt(&self) -> &LoginPrompt {
        &self.prompt
    }

    pub(super) async fn finish(self) -> Result<OAuthToken, OAuthError> {
        let mut interval = self.interval;
        let deadline = self.deadline;
        let grant = poll_device_token(
            &self.http,
            &self.token_url,
            &self.client_id,
            &self.device_code,
            &self.code_verifier,
            &mut interval,
            deadline,
        )
        .await?;
        Ok(token_from_grant(grant, None))
    }
}

pub(super) async fn begin(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
) -> Result<PendingLogin, OAuthError> {
    if provider != Provider::Qwen {
        return Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.qwen.provider_no_native_flow",
            "Este provedor não usa o fluxo nativo Qwen Code.",
        )));
    }
    let client_id = client_id_or_default(config);
    begin_qwen(
        http,
        config,
        &client_id,
        QWEN_DEVICE_CODE_ENDPOINT,
        QWEN_TOKEN_ENDPOINT,
    )
    .await
}

pub(super) async fn refresh(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    token: &OAuthToken,
) -> Result<OAuthToken, OAuthError> {
    if provider != Provider::Qwen {
        return Err(OAuthError::Expired);
    }
    let refresh_token = token
        .refresh_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::Expired)?;
    let client_id = client_id_or_default(config);
    let previous_refresh = Some(refresh_token.to_owned());
    refresh_qwen(
        http,
        &client_id,
        refresh_token,
        QWEN_TOKEN_ENDPOINT,
        previous_refresh,
    )
    .await
}

// ---------------------------------------------------------------------------
// Internal entry points (URLs parameterized for wiremock tests)
// ---------------------------------------------------------------------------

async fn begin_qwen(
    http: &reqwest::Client,
    _config: &OAuthConfig,
    client_id: &str,
    device_code_url: &str,
    token_url: &str,
) -> Result<PendingLogin, OAuthError> {
    // PKCE: 32 random bytes -> base64url (43 chars; RFC 7636 §4.1 minimum).
    let code_verifier = random_url_safe_43()?;
    let code_challenge = pkce_challenge(&code_verifier);

    // Device authorization request — application/x-www-form-urlencoded, no
    // client secret (public client, Qwen CLI parity).
    let form = [
        ("client_id".to_owned(), client_id.to_owned()),
        ("scope".to_owned(), QWEN_OAUTH_SCOPE.to_owned()),
        ("code_challenge".to_owned(), code_challenge),
        ("code_challenge_method".to_owned(), "S256".to_owned()),
    ];
    let (status, body) = post_form(http, device_code_url, &form).await?;
    if !status.is_success() {
        return Err(body
            .get("error")
            .and_then(Value::as_str)
            .map(classify_poll_error)
            .unwrap_or(OAuthError::InvalidResponse));
    }
    let issued = parse_device_authorization(&body)?;

    // The verification URL is server-provided; only `chat.qwen.ai` is
    // trusted. Anything else is ignored (we still fall back to the bare
    // `verification_uri` static host below).
    let prompt_url = issued
        .verification_uri_complete
        .as_deref()
        .filter(|u| is_trusted_qwen_url(u))
        .map(str::to_owned)
        .or_else(|| {
            issued
                .verification_uri
                .as_deref()
                .filter(|u| is_trusted_qwen_url(u))
                .map(str::to_owned)
        })
        // The Qwen CLI always points users at the `/code` endpoint;
        // fall back to that exact path on `chat.qwen.ai` if the server
        // omitted (or returned an untrusted) URL.
        .unwrap_or_else(|| format!("{QWEN_OAUTH_BASE_URL}/code"));

    Ok(PendingLogin {
        prompt: LoginPrompt {
            url: prompt_url,
            user_code: Some(issued.user_code),
        },
        http: http.clone(),
        token_url: token_url.to_owned(),
        client_id: client_id.to_owned(),
        device_code: issued.device_code,
        code_verifier,
        interval: issued.interval,
        deadline: issued.deadline,
    })
}

async fn refresh_qwen(
    http: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
    token_url: &str,
    previous_refresh: Option<String>,
) -> Result<OAuthToken, OAuthError> {
    let form = [
        ("grant_type".to_owned(), QWEN_REFRESH_GRANT_TYPE.to_owned()),
        ("client_id".to_owned(), client_id.to_owned()),
        ("refresh_token".to_owned(), refresh_token.to_owned()),
    ];
    let (status, body) = post_form(http, token_url, &form).await?;
    if let Some(code) = body.get("error").and_then(Value::as_str) {
        return Err(classify_poll_error(code));
    }
    if !status.is_success() {
        return Err(OAuthError::InvalidResponse);
    }
    let grant = parse_grant_body(&body, body.get("access_token").is_some())?;
    Ok(token_from_grant(grant, previous_refresh))
}

// ---------------------------------------------------------------------------
// Shared HTTP / parsing helpers
// ---------------------------------------------------------------------------

struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    interval: Duration,
    deadline: Instant,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
}

#[derive(Clone)]
struct RawGrant {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    resource_url: Option<String>,
    endpoint: Option<String>,
    scope: Option<String>,
}

async fn post_form(
    http: &reqwest::Client,
    url: &str,
    form: &[(String, String)],
) -> Result<(reqwest::StatusCode, Value), OAuthError> {
    let response = http
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|_| OAuthError::Network)?;
    read_json(response).await
}

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
    let value = serde_json::from_slice(&body).map_err(|_| OAuthError::InvalidResponse)?;
    Ok((status, value))
}

fn classify_poll_error(code: &str) -> OAuthError {
    match code {
        "access_denied" | "authorization_declined" => OAuthError::Denied,
        // RFC 8628 §3.5: `expired_token` means the device code is dead;
        // the user must restart the flow.
        "expired_token" | "invalid_grant" | "incorrect_device_code" => OAuthError::Expired,
        // Slow-down and pending are flow-control signals; the caller
        // already handles them before reaching this function. Any other
        // code from a poll that did succeed means a malformed exchange.
        "authorization_pending" | "slow_down" => OAuthError::InvalidResponse,
        _ => OAuthError::InvalidResponse,
    }
}

fn parse_device_authorization(body: &Value) -> Result<DeviceAuthorization, OAuthError> {
    let device_code = body
        .get("device_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::InvalidResponse)?;
    let user_code = body
        .get("user_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::InvalidResponse)?;
    let verification_uri = body
        .get("verification_uri")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let verification_uri_complete = body
        .get("verification_uri_complete")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    // `expires_in` bounds the whole device-code lifetime; clamp to our
    // outer deadline so a malicious / buggy server can't extend past the
    // outer safety bound.
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(600);
    let outer = Duration::from_secs(LOGIN_DEADLINE.as_secs().min(expires_in));
    let deadline = Instant::now() + outer;

    let interval = body
        .get("interval")
        .and_then(Value::as_u64)
        .map(|secs| Duration::from_secs(secs.max(1)))
        .unwrap_or(DEFAULT_DEVICE_INTERVAL);

    Ok(DeviceAuthorization {
        device_code: device_code.to_owned(),
        user_code: user_code.to_owned(),
        interval,
        deadline,
        verification_uri,
        verification_uri_complete,
    })
}

fn parse_grant_body(body: &Value, is_success: bool) -> Result<RawGrant, OAuthError> {
    if !is_success {
        return Err(OAuthError::InvalidResponse);
    }
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(OAuthError::InvalidResponse)?
        .to_owned();
    if let Some(token_type) = body.get("token_type").and_then(Value::as_str) {
        if !token_type.eq_ignore_ascii_case("Bearer") {
            return Err(OAuthError::InvalidResponse);
        }
    }
    let refresh_token = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let expires_in = match body.get("expires_in") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let secs = value.as_i64().ok_or(OAuthError::InvalidResponse)?;
            if secs <= 0 || secs > MAX_EXPIRES_IN_SECS {
                return Err(OAuthError::InvalidResponse);
            }
            Some(secs)
        }
    };
    let resource_url = body
        .get("resource_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let endpoint = body
        .get("endpoint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let scope = body
        .get("scope")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    Ok(RawGrant {
        access_token,
        refresh_token,
        expires_in,
        resource_url,
        endpoint,
        scope,
    })
}

fn expires_at_from(expires_in: Option<i64>) -> Option<chrono::DateTime<Utc>> {
    expires_in.map(|secs| Utc::now() + ChronoDuration::seconds(secs))
}

fn token_from_grant(grant: RawGrant, previous_refresh: Option<String>) -> OAuthToken {
    let expires_at = expires_at_from(grant.expires_in);
    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    metadata.insert("auth_method".to_owned(), "oauth_device_pkce".to_owned());
    if let Some(resource) = grant.resource_url {
        // Bound to a sane size; reject control characters and oversize
        // server responses (the value travels through token metadata only).
        if resource.len() <= 2048 && !resource.chars().any(char::is_control) {
            metadata.insert("resource_url".to_owned(), resource);
        }
    }
    if let Some(endpoint) = grant.endpoint {
        if endpoint.len() <= 2048 && !endpoint.chars().any(char::is_control) {
            metadata.insert("endpoint".to_owned(), endpoint);
        }
    }
    if let Some(scope) = grant.scope {
        if scope.len() <= 2048 && !scope.chars().any(char::is_control) {
            metadata.insert("scope".to_owned(), scope);
        }
    }
    OAuthToken {
        access_token: grant.access_token,
        refresh_token: grant.refresh_token.or(previous_refresh),
        expires_at,
        metadata,
    }
}

/// RFC 8628 §3.5 polling: honor `authorization_pending` / `slow_down`,
/// never busy-loop, and bound the whole sleep+send+read cycle under the
/// deadline.
async fn poll_device_token(
    http: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    device_code: &str,
    code_verifier: &str,
    interval: &mut Duration,
    deadline: Instant,
) -> Result<RawGrant, OAuthError> {
    loop {
        if Instant::now() >= deadline {
            return Err(OAuthError::Expired);
        }
        let outcome = timeout_at(deadline, async {
            sleep(*interval).await;
            if Instant::now() >= deadline {
                return Err(OAuthError::Expired);
            }
            let form = [
                ("grant_type".to_owned(), QWEN_DEVICE_GRANT_TYPE.to_owned()),
                ("client_id".to_owned(), client_id.to_owned()),
                ("device_code".to_owned(), device_code.to_owned()),
                ("code_verifier".to_owned(), code_verifier.to_owned()),
            ];
            let (status, body) = post_form(http, token_url, &form).await?;
            let result = classify_poll_response(&body)?;
            if matches!(result, PollStep::Grant(_)) && !status.is_success() {
                return Err(OAuthError::InvalidResponse);
            }
            Ok(result)
        })
        .await;
        match outcome {
            Err(_elapsed) => return Err(OAuthError::Expired),
            Ok(Err(error)) => return Err(error),
            Ok(Ok(PollStep::Pending)) => continue,
            Ok(Ok(PollStep::SlowDown)) => {
                // RFC 8628 §3.5: increase interval by 5s with NO downward
                // cap; the new interval is sticky until grant. The outer
                // `LOGIN_DEADLINE` is the only absolute bound (no request
                // is sent after the deadline because of `timeout_at` plus
                // the pre-poll guard above).
                *interval += SLOW_DOWN_STEP;
            }
            Ok(Ok(PollStep::Grant(grant))) => return Ok(grant),
        }
    }
}

enum PollStep {
    Pending,
    SlowDown,
    Grant(RawGrant),
}

/// Interpret a poll response body:
///   - `access_token` present + non-empty  -> success.
///   - `slowDown` flag (Qwen variant) -> bump interval.
///   - `error: "authorization_pending"` -> keep polling.
///   - `error: "slow_down"` -> bump interval.
///   - any other error code -> terminal (Denied / Expired / InvalidResponse).
fn classify_poll_response(body: &Value) -> Result<PollStep, OAuthError> {
    if body
        .get("access_token")
        .and_then(Value::as_str)
        .is_some_and(|t| !t.is_empty())
    {
        return parse_grant_body(body, true).map(PollStep::Grant);
    }
    if let Some(status) = body.get("status").and_then(Value::as_str) {
        if status.eq_ignore_ascii_case("pending") {
            if body
                .get("slowDown")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Ok(PollStep::SlowDown);
            }
            return Ok(PollStep::Pending);
        }
    }
    match body.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => Ok(PollStep::Pending),
        Some("slow_down") => Ok(PollStep::SlowDown),
        Some(code) => Err(classify_poll_error(code)),
        None => Err(OAuthError::InvalidResponse),
    }
}

// ---------------------------------------------------------------------------
// PKCE (RFC 7636) + CSPRNG helpers
// ---------------------------------------------------------------------------

fn random_url_safe_43() -> Result<String, OAuthError> {
    let mut buf = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut buf)
        .map_err(|_| OAuthError::os_rng_unavailable())?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// SHA-256(verifier) -> base64url, no padding. RFC 7636 §4.2.
fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// Optional client id override: a blank `config.client_id` selects the fixed
/// Qwen Code public client. The reference ships this exact value as a
/// non-secret identifier.
fn client_id_or_default(config: &OAuthConfig) -> String {
    let trimmed = config.client_id.trim();
    if trimmed.is_empty() {
        QWEN_OAUTH_CLIENT_ID_DEFAULT.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Only `chat.qwen.ai` (with optional subdomains and `qwen.ai` parents)
/// is ever shown to the user. Strips non-default ports + embedded creds.
fn is_trusted_qwen_url(raw: &str) -> bool {
    let parsed = match url::Url::parse(raw) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if parsed.scheme() != "https"
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return false;
    }
    let host = match parsed.host_str() {
        Some(h) => h.to_ascii_lowercase(),
        None => return false,
    };
    host == "chat.qwen.ai"
        || host == "qwen.ai"
        || host.ends_with(".chat.qwen.ai")
        || host.ends_with(".qwen.ai")
}

// ---------------------------------------------------------------------------
// Tests (consumer-visible boundaries; endpoint overrides stay private)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config() -> OAuthConfig {
        OAuthConfig {
            client_id: String::new(),
            tenant: None,
            client_secret_env: None,
            credential_id: uuid::Uuid::new_v4().to_string(),
            options: Default::default(),
        }
    }

    // -- PKCE / parsing helpers (pure unit tests) ---------------------------

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        // RFC 7636 Appendix B: verifier "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        // -> challenge "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn pkce_challenge_round_trips_through_url_safe() {
        let verifier = random_url_safe_43().expect("verifier");
        let challenge = pkce_challenge(&verifier);
        // 32-byte SHA-256 digest -> 43 chars base64url-no-pad.
        assert_eq!(challenge.len(), 43);
        assert!(challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }

    #[test]
    fn random_verifier_is_43_chars_url_safe_no_pad() {
        let verifier = random_url_safe_43().expect("verifier");
        assert_eq!(verifier.len(), 43);
        assert!(verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }

    #[test]
    fn client_id_blank_picks_default_non_empty_overrides() {
        assert_eq!(
            client_id_or_default(&test_config()),
            QWEN_OAUTH_CLIENT_ID_DEFAULT
        );
        let mut cfg = test_config();
        cfg.client_id = "f0304373b74a44d2b584a3fb70ca9e56".into();
        assert_eq!(
            client_id_or_default(&cfg),
            "f0304373b74a44d2b584a3fb70ca9e56"
        );
        let mut cfg = test_config();
        cfg.client_id = "   ".into();
        assert_eq!(client_id_or_default(&cfg), QWEN_OAUTH_CLIENT_ID_DEFAULT);
    }

    #[test]
    fn trusted_url_accepts_qwen_hosts_rejects_others() {
        assert!(is_trusted_qwen_url("https://chat.qwen.ai/code"));
        assert!(is_trusted_qwen_url("https://qwen.ai/oauth"));
        assert!(is_trusted_qwen_url("https://api.chat.qwen.ai/device"));
        assert!(!is_trusted_qwen_url("http://chat.qwen.ai/code"));
        assert!(!is_trusted_qwen_url("https://attacker.example/code"));
        assert!(!is_trusted_qwen_url("https://chat.qwen.ai:8443/code"));
        assert!(!is_trusted_qwen_url("https://user:pass@chat.qwen.ai/"));
        assert!(!is_trusted_qwen_url("not a url"));
    }

    #[test]
    fn device_authorization_parsing_requires_fields() {
        let body = json!({});
        assert!(matches!(
            parse_device_authorization(&body),
            Err(OAuthError::InvalidResponse)
        ));
        let body = json!({"device_code": "  ", "user_code": "ABCD-1234"});
        assert!(matches!(
            parse_device_authorization(&body),
            Err(OAuthError::InvalidResponse)
        ));
        let body = json!({
            "device_code": "dc", "user_code": "ABCD-1234",
            "verification_uri_complete": "https://chat.qwen.ai/code?u=ABCD-1234",
            "expires_in": 600, "interval": 2
        });
        let parsed = parse_device_authorization(&body).expect("parsed");
        assert_eq!(parsed.device_code, "dc");
        assert_eq!(parsed.user_code, "ABCD-1234");
        assert_eq!(
            parsed.verification_uri_complete.as_deref(),
            Some("https://chat.qwen.ai/code?u=ABCD-1234")
        );
    }

    #[test]
    fn device_authorization_falls_back_to_static_host_when_server_url_untrusted() {
        let body = json!({
            "device_code": "dc", "user_code": "ABCD-1234",
            "verification_uri": "https://attacker.example/steal",
            "expires_in": 600, "interval": 2
        });
        let parsed = parse_device_authorization(&body).expect("parsed");
        assert!(parsed.verification_uri.is_some());
        assert!(parsed.verification_uri_complete.is_none());
        // begin_qwen must filter the untrusted URL and fall back to the
        // hardcoded chat.qwen.ai/code path. We exercise the filter inline:
        let prompt_url = parsed
            .verification_uri_complete
            .as_deref()
            .filter(|u| is_trusted_qwen_url(u))
            .map(str::to_owned)
            .or_else(|| {
                parsed
                    .verification_uri
                    .as_deref()
                    .filter(|u| is_trusted_qwen_url(u))
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("{QWEN_OAUTH_BASE_URL}/code"));
        assert_eq!(prompt_url, "https://chat.qwen.ai/code");
    }

    #[test]
    fn poll_response_classifies_pending_slowdown_and_grant() {
        let pending = json!({"error": "authorization_pending"});
        assert!(matches!(
            classify_poll_response(&pending),
            Ok(PollStep::Pending)
        ));
        let slow = json!({"error": "slow_down"});
        assert!(matches!(
            classify_poll_response(&slow),
            Ok(PollStep::SlowDown)
        ));
        let alt_slow = json!({"status": "pending", "slowDown": true});
        assert!(matches!(
            classify_poll_response(&alt_slow),
            Ok(PollStep::SlowDown)
        ));
        let alt_pending = json!({"status": "pending"});
        assert!(matches!(
            classify_poll_response(&alt_pending),
            Ok(PollStep::Pending)
        ));
        let grant = json!({
            "access_token": "at",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "rt"
        });
        let step = classify_poll_response(&grant).expect("grant");
        match step {
            PollStep::Grant(g) => {
                assert_eq!(g.access_token, "at");
                assert_eq!(g.refresh_token.as_deref(), Some("rt"));
                assert_eq!(g.expires_in, Some(3600));
            }
            _ => panic!("expected Grant"),
        }
    }

    #[test]
    fn grant_parsing_rejects_non_bearer_and_bad_expires_in() {
        let body = json!({"access_token": "at", "token_type": "MAC"});
        assert!(matches!(
            parse_grant_body(&body, true),
            Err(OAuthError::InvalidResponse)
        ));
        let body = json!({"access_token": "at", "expires_in": 0});
        assert!(matches!(
            parse_grant_body(&body, true),
            Err(OAuthError::InvalidResponse)
        ));
        let body = json!({"access_token": "at", "expires_in": MAX_EXPIRES_IN_SECS + 1});
        assert!(matches!(
            parse_grant_body(&body, true),
            Err(OAuthError::InvalidResponse)
        ));
        let body = json!({"access_token": ""});
        assert!(matches!(
            parse_grant_body(&body, true),
            Err(OAuthError::InvalidResponse)
        ));
    }

    #[test]
    fn grant_parsing_preserves_resource_url_and_endpoint_metadata() {
        let body = json!({
            "access_token": "at-very-long-secret-string",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "rt-also-very-long",
            "resource_url": "https://portal.qwen.ai/v1",
            "endpoint": "https://portal.qwen.ai/v1/chat/completions",
            "scope": "openid profile email model.completion"
        });
        let grant = parse_grant_body(&body, true).expect("grant");
        assert_eq!(
            grant.resource_url.as_deref(),
            Some("https://portal.qwen.ai/v1")
        );
        assert_eq!(
            grant.endpoint.as_deref(),
            Some("https://portal.qwen.ai/v1/chat/completions")
        );
        assert_eq!(
            grant.scope.as_deref(),
            Some("openid profile email model.completion")
        );
    }

    #[test]
    fn token_from_grant_preserves_previous_refresh() {
        let grant = RawGrant {
            access_token: "at".into(),
            refresh_token: None,
            expires_in: Some(60),
            resource_url: None,
            endpoint: None,
            scope: None,
        };
        let token = token_from_grant(grant, Some("rt-prev".into()));
        assert_eq!(token.refresh_token.as_deref(), Some("rt-prev"));
        assert_eq!(token.access_token, "at");
        assert!(token.expires_at.is_some());
        assert_eq!(
            token.metadata.get("auth_method").map(String::as_str),
            Some("oauth_device_pkce")
        );
    }

    #[test]
    fn token_from_grant_rejects_oversized_or_control_metadata() {
        let oversize = "x".repeat(2049);
        let grant = RawGrant {
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            expires_in: Some(60),
            resource_url: Some(oversize.clone()),
            endpoint: Some(oversize.clone()),
            scope: Some(oversize.clone()),
        };
        let token = token_from_grant(grant, None);
        assert!(!token.metadata.contains_key("resource_url"));
        assert!(!token.metadata.contains_key("endpoint"));
        assert!(!token.metadata.contains_key("scope"));
    }

    #[test]
    fn poll_error_codes_map_to_sanitized_variants() {
        assert!(matches!(
            classify_poll_error("access_denied"),
            OAuthError::Denied
        ));
        assert!(matches!(
            classify_poll_error("expired_token"),
            OAuthError::Expired
        ));
        assert!(matches!(
            classify_poll_error("invalid_grant"),
            OAuthError::Expired
        ));
        assert!(matches!(
            classify_poll_error("authorization_pending"),
            OAuthError::InvalidResponse
        ));
        assert!(matches!(
            classify_poll_error("something else"),
            OAuthError::InvalidResponse
        ));
    }

    // -- Wiremock-driven flows (against private helper entry points) --------

    #[tokio::test]
    async fn begin_qwen_issues_prompt_and_polling_stops_on_grant() {
        let server = MockServer::start().await;
        let device_url = format!("{}/api/v1/oauth2/device/code", server.uri());
        let token_url = format!("{}/api/v1/oauth2/token", server.uri());

        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/device/code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "device_code": "dc-test",
                "user_code": "WXYZ-9876",
                "verification_uri": "https://chat.qwen.ai/code",
                "verification_uri_complete": "https://chat.qwen.ai/code?u=WXYZ-9876",
                "expires_in": 600,
                "interval": 1
            })))
            .mount(&server)
            .await;

        // Token endpoint: first poll = pending, second = success.
        let calls = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/token"))
            .respond_with(move |_req: &wiremock::Request| {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    ResponseTemplate::new(400).set_body_json(json!({
                        "error": "authorization_pending"
                    }))
                } else {
                    ResponseTemplate::new(200).set_body_json(json!({
                        "access_token": "at-secret-string",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "refresh_token": "rt-secret-string",
                        "resource_url": "https://portal.qwen.ai/v1",
                        "endpoint": "https://portal.qwen.ai/v1/chat/completions",
                        "scope": "openid profile email model.completion"
                    }))
                }
            })
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let pending = begin_qwen(
            &http,
            &test_config(),
            QWEN_OAUTH_CLIENT_ID_DEFAULT,
            &device_url,
            &token_url,
        )
        .await
        .expect("begin");

        let prompt = pending.prompt().clone();
        assert_eq!(prompt.url, "https://chat.qwen.ai/code?u=WXYZ-9876");
        assert_eq!(prompt.user_code.as_deref(), Some("WXYZ-9876"));

        let token = pending.finish().await.expect("token");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-secret-string"));
        assert!(token.expires_at.is_some());
        assert_eq!(
            token.metadata.get("auth_method").map(String::as_str),
            Some("oauth_device_pkce")
        );
        assert_eq!(
            token.metadata.get("resource_url").map(String::as_str),
            Some("https://portal.qwen.ai/v1")
        );
    }

    #[tokio::test]
    async fn begin_qwen_handles_slow_down_then_grant() {
        let server = MockServer::start().await;
        let device_url = format!("{}/api/v1/oauth2/device/code", server.uri());
        let token_url = format!("{}/api/v1/oauth2/token", server.uri());

        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/device/code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "device_code": "dc-slow",
                "user_code": "AAAA-1111",
                "verification_uri_complete": "https://chat.qwen.ai/code?u=AAAA-1111",
                "expires_in": 600,
                "interval": 1
            })))
            .mount(&server)
            .await;

        let calls = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/token"))
            .respond_with(move |_req: &wiremock::Request| {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                match n {
                    0 => ResponseTemplate::new(429).set_body_json(json!({
                        "error": "slow_down"
                    })),
                    _ => ResponseTemplate::new(200).set_body_json(json!({
                        "access_token": "at-after-slowdown",
                        "token_type": "Bearer",
                        "expires_in": 1800
                    })),
                }
            })
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let pending = begin_qwen(
            &http,
            &test_config(),
            QWEN_OAUTH_CLIENT_ID_DEFAULT,
            &device_url,
            &token_url,
        )
        .await
        .expect("begin");
        let token = pending.finish().await.expect("token");
        assert_eq!(token.access_token, "at-after-slowdown");
    }

    #[tokio::test]
    async fn begin_qwen_maps_access_denied_to_denied_variant() {
        let server = MockServer::start().await;
        let device_url = format!("{}/api/v1/oauth2/device/code", server.uri());
        let token_url = format!("{}/api/v1/oauth2/token", server.uri());

        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/device/code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "device_code": "dc-deny",
                "user_code": "DENY-0001",
                "verification_uri_complete": "https://chat.qwen.ai/code?u=DENY-0001",
                "expires_in": 600,
                "interval": 1
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": "access_denied"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let pending = begin_qwen(
            &http,
            &test_config(),
            QWEN_OAUTH_CLIENT_ID_DEFAULT,
            &device_url,
            &token_url,
        )
        .await
        .expect("begin");
        let err = match pending.finish().await {
            Ok(_) => panic!("expected Denied"),
            Err(e) => e,
        };
        assert!(matches!(err, OAuthError::Denied));
    }

    #[tokio::test]
    async fn refresh_qwen_preserves_refresh_token_when_omitted() {
        let server = MockServer::start().await;
        let token_url = format!("{}/api/v1/oauth2/token", server.uri());

        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "at-rotated",
                "token_type": "Bearer",
                "expires_in": 7200
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let token = refresh_qwen(
            &http,
            QWEN_OAUTH_CLIENT_ID_DEFAULT,
            "rt-existing",
            &token_url,
            Some("rt-existing".to_owned()),
        )
        .await
        .expect("refresh");
        assert_eq!(token.access_token, "at-rotated");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-existing"));
        assert!(token.expires_at.is_some());
    }

    #[tokio::test]
    async fn refresh_qwen_rotates_when_server_returns_new_refresh_token() {
        let server = MockServer::start().await;
        let token_url = format!("{}/api/v1/oauth2/token", server.uri());

        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "at-rotated",
                "token_type": "Bearer",
                "expires_in": 7200,
                "refresh_token": "rt-rotated"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let token = refresh_qwen(
            &http,
            QWEN_OAUTH_CLIENT_ID_DEFAULT,
            "rt-existing",
            &token_url,
            Some("rt-existing".to_owned()),
        )
        .await
        .expect("refresh");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-rotated"));
    }

    #[tokio::test]
    async fn refresh_qwen_maps_invalid_grant_to_expired() {
        let server = MockServer::start().await;
        let token_url = format!("{}/api/v1/oauth2/token", server.uri());

        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": "invalid_grant",
                "error_description": "refresh token revoked"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = match refresh_qwen(
            &http,
            QWEN_OAUTH_CLIENT_ID_DEFAULT,
            "rt-revoked",
            &token_url,
            Some("rt-revoked".to_owned()),
        )
        .await
        {
            Ok(_) => panic!("expected Expired"),
            Err(e) => e,
        };
        assert!(matches!(err, OAuthError::Expired));
    }

    #[tokio::test]
    async fn public_refresh_rejects_wrong_provider_and_missing_refresh_token() {
        let token = OAuthToken {
            access_token: "at".into(),
            refresh_token: None,
            expires_at: None,
            metadata: Default::default(),
        };
        let err = match refresh(
            &reqwest::Client::new(),
            Provider::Kimi,
            &test_config(),
            &token,
        )
        .await
        {
            Ok(_) => panic!("expected Expired"),
            Err(e) => e,
        };
        assert!(matches!(err, OAuthError::Expired));
        let err = match refresh(
            &reqwest::Client::new(),
            Provider::Qwen,
            &test_config(),
            &token,
        )
        .await
        {
            Ok(_) => panic!("expected Expired"),
            Err(e) => e,
        };
        assert!(matches!(err, OAuthError::Expired));
    }

    #[tokio::test]
    async fn public_begin_rejects_wrong_provider() {
        let err = match begin(&reqwest::Client::new(), Provider::Kimi, &test_config()).await {
            Ok(_) => panic!("expected Configuration"),
            Err(e) => e,
        };
        assert!(matches!(err, OAuthError::Configuration(_)));
    }

    #[tokio::test]
    async fn device_code_request_includes_pkce_fields() {
        let server = MockServer::start().await;
        let device_url = format!("{}/api/v1/oauth2/device/code", server.uri());
        let token_url = format!("{}/api/v1/oauth2/token", server.uri());

        let captured = Arc::new(Mutex::new(String::new()));
        let captured_for_handler = captured.clone();
        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/device/code"))
            .respond_with(move |req: &wiremock::Request| {
                let body = String::from_utf8_lossy(&req.body).to_string();
                *captured_for_handler.lock() = body;
                ResponseTemplate::new(200).set_body_json(json!({
                    "device_code": "dc-pkce",
                    "user_code": "PKCE-0001",
                    "verification_uri_complete": "https://chat.qwen.ai/code?u=PKCE-0001",
                    "expires_in": 600,
                    "interval": 5
                }))
            })
            .mount(&server)
            .await;
        // Token endpoint never polled in this test; register a default
        // 500 so an accidental poll surfaces as InvalidResponse rather
        // than hanging.
        Mock::given(method("POST"))
            .and(path("/api/v1/oauth2/token"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let pending = begin_qwen(
            &http,
            &test_config(),
            QWEN_OAUTH_CLIENT_ID_DEFAULT,
            &device_url,
            &token_url,
        )
        .await
        .expect("begin");

        let body = captured.lock().clone();
        assert!(body.contains("client_id=f0304373b74a44d2b584a3fb70ca9e56"));
        assert!(body.contains("scope=openid+profile+email+model.completion"));
        assert!(body.contains("code_challenge_method=S256"));
        assert!(body.contains("code_challenge="));
        assert_eq!(pending.prompt().user_code.as_deref(), Some("PKCE-0001"));
    }
}
