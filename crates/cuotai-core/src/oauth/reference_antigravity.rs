//! Antigravity's Google browser-consent flow, compatible with the public
//! client and endpoints used by 9router f01fb909.
//!
//! The local callback, token exchange, and post-login project lookup are
//! deliberately kept here rather than sharing the broader browser adapter:
//! production endpoints stay fixed, token payloads never reach diagnostics,
//! and the project lookup cannot accidentally trigger Code Assist onboarding.

use super::send_secure;
use super::{LoginPrompt, OAuthConfig, OAuthError, OAuthToken};
use crate::Provider;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::{header::ACCEPT, StatusCode};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{timeout, timeout_at, Instant},
};

// Public installed-app client mirrored by 9router's
// open-sse/providers/shared.js. Although Google calls this field a secret,
// installed applications cannot keep it confidential.
const CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "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";

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const LOAD_CODE_ASSIST_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
const AUTHORIZE_HOST: &str = "accounts.google.com";
const TOKEN_HOST: &str = "oauth2.googleapis.com";
const LOAD_CODE_ASSIST_HOST: &str = "cloudcode-pa.googleapis.com";

// Exact Antigravity registry scopes. Keep their order stable: it makes the
// consent URL auditable against the reference client.
const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform \
https://www.googleapis.com/auth/userinfo.email \
https://www.googleapis.com/auth/userinfo.profile \
https://www.googleapis.com/auth/cclog \
https://www.googleapis.com/auth/experimentsandconfigs";

const LOAD_CODE_ASSIST_USER_AGENT: &str = "antigravity/ide/2.11.0 darwin/arm64";
const CALLBACK_PATH: &str = "/callback";
const LOGIN_DEADLINE: Duration = Duration::from_secs(300);
const NETWORK_DEADLINE: Duration = Duration::from_secs(30);
const CALLBACK_CONNECTION_DEADLINE: Duration = Duration::from_secs(15);
const CALLBACK_RESPONSE_DEADLINE: Duration = Duration::from_secs(5);
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_RESPONSE_BYTES: usize = 128 * 1024;
const MAX_EXPIRES_IN_SECONDS: i64 = 31_536_000;
const MAX_PROJECT_ID_BYTES: usize = 2048;

#[derive(Clone)]
struct Endpoints {
    authorize_url: String,
    authorize_host: String,
    token_url: String,
    token_host: String,
    load_code_assist_url: String,
    load_code_assist_host: String,
    require_https: bool,
    redirect_host: &'static str,
}

impl Endpoints {
    fn official() -> Self {
        Self {
            authorize_url: AUTHORIZE_URL.into(),
            authorize_host: AUTHORIZE_HOST.into(),
            token_url: TOKEN_URL.into(),
            token_host: TOKEN_HOST.into(),
            load_code_assist_url: LOAD_CODE_ASSIST_URL.into(),
            load_code_assist_host: LOAD_CODE_ASSIST_HOST.into(),
            require_https: true,
            // This is the redirect host registered by the public client. The
            // socket itself binds only the IPv4 loopback interface.
            redirect_host: "localhost",
        }
    }

    fn validate(&self) -> Result<(), OAuthError> {
        validate_endpoint(
            &self.authorize_url,
            &self.authorize_host,
            self.require_https,
        )?;
        validate_endpoint(&self.token_url, &self.token_host, self.require_https)?;
        validate_endpoint(
            &self.load_code_assist_url,
            &self.load_code_assist_host,
            self.require_https,
        )
    }
}

/// Browser authorization that is waiting for Google's loopback redirect.
/// Dropping this value (or its `finish` future) closes the callback listener.
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
    ensure_provider(provider)?;
    begin_with(http, config, Endpoints::official()).await
}

pub(super) async fn refresh(
    http: &reqwest::Client,
    provider: Provider,
    config: &OAuthConfig,
    token: &OAuthToken,
) -> Result<OAuthToken, OAuthError> {
    ensure_provider(provider)?;
    refresh_with(http, config, token, &Endpoints::official()).await
}

struct LoopbackLogin {
    http: reqwest::Client,
    listener: TcpListener,
    redirect_uri: String,
    expected_state: String,
    code_verifier: String,
    client_id: String,
    client_secret: String,
    endpoints: Endpoints,
    deadline: Instant,
}

async fn begin_with(
    http: &reqwest::Client,
    config: &OAuthConfig,
    endpoints: Endpoints,
) -> Result<PendingLogin, OAuthError> {
    endpoints.validate()?;
    let (client_id, client_secret) = client_credentials(config)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| OAuthError::Network)?;
    let port = listener
        .local_addr()
        .map_err(|_| OAuthError::Network)?
        .port();
    let redirect_uri = format!("http://{}:{port}{CALLBACK_PATH}", endpoints.redirect_host);

    let state = random_url_safe_32()?;
    let verifier = random_url_safe_32()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri.as_str()),
            ("scope", SCOPES),
            ("state", state.as_str()),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ])
        .finish();
    let authorize_url = format!("{}?{query}", endpoints.authorize_url);
    validate_endpoint(
        &authorize_url,
        &endpoints.authorize_host,
        endpoints.require_https,
    )?;

    Ok(PendingLogin {
        prompt: LoginPrompt {
            url: authorize_url,
            user_code: None,
        },
        login: LoopbackLogin {
            http: http.clone(),
            listener,
            redirect_uri,
            expected_state: state,
            code_verifier: verifier,
            client_id,
            client_secret,
            endpoints,
            deadline: Instant::now() + LOGIN_DEADLINE,
        },
    })
}

impl LoopbackLogin {
    async fn finish(self) -> Result<OAuthToken, OAuthError> {
        let callback = self.wait_for_callback().await?;
        let form = [
            ("grant_type", "authorization_code".to_owned()),
            ("client_id", self.client_id.clone()),
            ("client_secret", self.client_secret.clone()),
            ("code", callback.code),
            ("redirect_uri", self.redirect_uri.clone()),
            ("code_verifier", self.code_verifier.clone()),
        ];
        let (status, body) = timeout_at(
            self.deadline,
            post_form(&self.http, &self.endpoints.token_url, &form),
        )
        .await
        .map_err(|_| OAuthError::Expired)??;
        check_oauth_error(&body, status)?;
        let mut token = parse_token(&body, None, Utc::now())?;

        // Antigravity's project is account context needed by quota requests.
        // This lookup is intentionally best-effort: the quota adapter can
        // repeat it later, and login must never provision or onboard a user.
        if let Some(project_id) = fetch_project_id(
            &self.http,
            &self.endpoints.load_code_assist_url,
            &token.access_token,
            self.deadline,
        )
        .await
        {
            token.metadata.insert("project_id".into(), project_id);
        }
        Ok(token)
    }

    async fn wait_for_callback(&self) -> Result<Callback, OAuthError> {
        loop {
            let (mut stream, _) = timeout_at(self.deadline, self.listener.accept())
                .await
                .map_err(|_| OAuthError::Expired)?
                .map_err(|_| OAuthError::Network)?;
            let connection_deadline =
                std::cmp::min(self.deadline, Instant::now() + CALLBACK_CONNECTION_DEADLINE);
            let outcome = match timeout_at(
                connection_deadline,
                read_callback(&mut stream, CALLBACK_PATH, &self.expected_state),
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) if Instant::now() >= self.deadline => return Err(OAuthError::Expired),
                Err(_) => CallbackOutcome::Ignored,
            };
            match outcome {
                CallbackOutcome::Code(callback) => {
                    respond(
                        &mut stream,
                        "200 OK",
                        "<!doctype html><html><body><p>Autorização recebida. \
                         Você pode fechar esta janela e voltar ao Cuotai.</p></body></html>",
                    )
                    .await;
                    return Ok(callback);
                }
                CallbackOutcome::Denied => {
                    respond(
                        &mut stream,
                        "200 OK",
                        "<!doctype html><html><body><p>A autorização foi negada. \
                         Você pode fechar esta janela.</p></body></html>",
                    )
                    .await;
                    return Err(OAuthError::Denied);
                }
                CallbackOutcome::Failed(error) => {
                    respond(&mut stream, "400 Bad Request", "").await;
                    return Err(error);
                }
                CallbackOutcome::Ignored => {
                    // Wrong state/path and unrelated browser requests reveal
                    // nothing and cannot end the active login.
                    respond(&mut stream, "404 Not Found", "").await;
                }
            }
        }
    }
}

struct Callback {
    code: String,
}

enum CallbackOutcome {
    Code(Callback),
    Denied,
    Failed(OAuthError),
    Ignored,
}

async fn read_callback(
    stream: &mut TcpStream,
    expected_path: &str,
    expected_state: &str,
) -> CallbackOutcome {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if head.len() >= MAX_HEADER_BYTES {
            return CallbackOutcome::Ignored;
        }
        let read = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return CallbackOutcome::Ignored,
            Ok(read) => read,
        };
        if head.len() + read > MAX_HEADER_BYTES {
            return CallbackOutcome::Ignored;
        }
        head.extend_from_slice(&chunk[..read]);
        if head.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let head = String::from_utf8_lossy(&head);
    let mut request = head.lines().next().unwrap_or_default().split_whitespace();
    let (Some(method), Some(target), Some(_version)) =
        (request.next(), request.next(), request.next())
    else {
        return CallbackOutcome::Ignored;
    };
    if method != "GET" {
        return CallbackOutcome::Ignored;
    }
    let (path, query) = target
        .split_once('?')
        .map_or((target, ""), |(path, query)| (path, query));
    if path != expected_path {
        return CallbackOutcome::Ignored;
    }
    parse_callback_query(query, expected_state)
}

fn parse_callback_query(query: &str, expected_state: &str) -> CallbackOutcome {
    if !valid_percent_encoding(query) {
        return CallbackOutcome::Ignored;
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
    if states.len() != 1 || states[0] != expected_state {
        return CallbackOutcome::Ignored;
    }
    match (codes.as_slice(), errors.as_slice()) {
        ([code], []) if !code.is_empty() => CallbackOutcome::Code(Callback { code: code.clone() }),
        ([], [error]) if error == "access_denied" => CallbackOutcome::Denied,
        ([], [_]) => CallbackOutcome::Failed(OAuthError::InvalidResponse),
        _ => CallbackOutcome::Ignored,
    }
}

fn valid_percent_encoding(input: &str) -> bool {
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
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
    let _ = timeout(CALLBACK_RESPONSE_DEADLINE, write).await;
}

async fn refresh_with(
    http: &reqwest::Client,
    config: &OAuthConfig,
    token: &OAuthToken,
    endpoints: &Endpoints,
) -> Result<OAuthToken, OAuthError> {
    endpoints.validate()?;
    let refresh_token = token
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(OAuthError::Expired)?;
    let (client_id, client_secret) = client_credentials(config)?;
    let form = [
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh_token.to_owned()),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];
    let (status, body) = timeout(
        NETWORK_DEADLINE,
        post_form(http, &endpoints.token_url, &form),
    )
    .await
    .map_err(|_| OAuthError::Network)??;
    check_oauth_error(&body, status)?;
    let mut refreshed = parse_token(&body, token.refresh_token.clone(), Utc::now())?;
    // Project context is stable and refresh responses do not repeat it.
    refreshed.metadata = token.metadata.clone();
    Ok(refreshed)
}

/// Best-effort read-only `loadCodeAssist`. In particular, this module never
/// calls `onboardUser`: authenticating a quota reader must not mutate a user's
/// Google Cloud account.
async fn fetch_project_id(
    http: &reqwest::Client,
    url: &str,
    access_token: &str,
    deadline: Instant,
) -> Option<String> {
    let body = serde_json::json!({
        "metadata": {
            "ideType": 9,
            "platform": google_platform_enum(),
            "pluginType": 2,
        }
    });
    let request = async {
        let response = send_secure(
            http.post(url)
                .header(ACCEPT, "application/json")
                .header("User-Agent", LOAD_CODE_ASSIST_USER_AGENT)
                .header("x-request-source", "local")
                .bearer_auth(access_token)
                .json(&body),
        )
        .await?;
        read_bounded(response).await
    };
    let (status, response) = timeout_at(deadline, request).await.ok()?.ok()?;
    if !status.is_success() {
        return None;
    }
    extract_project_id(&response)
}

fn extract_project_id(body: &Value) -> Option<String> {
    let project = body.get("cloudaicompanionProject")?;
    let raw = match project {
        Value::String(value) => Some(value.as_str()),
        Value::Object(_) => project.get("id").and_then(Value::as_str),
        _ => None,
    }?;
    let project_id = raw.trim();
    if project_id.is_empty()
        || project_id.len() > MAX_PROJECT_ID_BYTES
        || project_id.chars().any(char::is_control)
    {
        return None;
    }
    Some(project_id.to_owned())
}

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

fn ensure_provider(provider: Provider) -> Result<(), OAuthError> {
    if provider == Provider::Antigravity {
        Ok(())
    } else {
        Err(OAuthError::Configuration(crate::text::Localized::fixed(
            "oauth_error.antigravity.exclusive_flow",
            "Este fluxo OAuth é exclusivo do Antigravity.",
        )))
    }
}

fn client_credentials(config: &OAuthConfig) -> Result<(String, String), OAuthError> {
    let configured = config.client_id.trim();
    if configured.is_empty() || configured == CLIENT_ID {
        return Ok((CLIENT_ID.to_owned(), CLIENT_SECRET.to_owned()));
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
    Ok((configured.to_owned(), secret))
}

fn random_url_safe_32() -> Result<String, OAuthError> {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| OAuthError::os_rng_unavailable())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn validate_endpoint(raw: &str, allowed_host: &str, require_https: bool) -> Result<(), OAuthError> {
    let parsed = url::Url::parse(raw).map_err(|_| OAuthError::InvalidResponse)?;
    if !parsed
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case(allowed_host))
    {
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

async fn post_form(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, String)],
) -> Result<(StatusCode, Value), OAuthError> {
    let response =
        send_secure(http.post(url).header(ACCEPT, "application/json").form(form)).await?;
    read_bounded(response).await
}

async fn read_bounded(response: reqwest::Response) -> Result<(StatusCode, Value), OAuthError> {
    let status = response.status();
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| OAuthError::Network)? {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(OAuthError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    // Never include these bytes in an error: OAuth servers commonly echo
    // authorization codes, refresh tokens, and client details.
    let body = serde_json::from_slice(&bytes).map_err(|_| OAuthError::InvalidResponse)?;
    Ok((status, body))
}

fn check_oauth_error(body: &Value, status: StatusCode) -> Result<(), OAuthError> {
    if let Some(code) = body.get("error").and_then(Value::as_str) {
        return Err(match code {
            "access_denied" => OAuthError::Denied,
            "expired_token" | "invalid_grant" | "invalid_token" => OAuthError::Expired,
            _ => OAuthError::InvalidResponse,
        });
    }
    match status.as_u16() {
        _ if status.is_success() => Ok(()),
        401 | 403 => Err(OAuthError::Expired),
        _ => Err(OAuthError::InvalidResponse),
    }
}

fn parse_token(
    body: &Value,
    previous_refresh: Option<String>,
    now: DateTime<Utc>,
) -> Result<OAuthToken, OAuthError> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(OAuthError::InvalidResponse)?;
    let expires_at = match body.get("expires_in") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let seconds = value.as_i64().ok_or(OAuthError::InvalidResponse)?;
            if seconds <= 0 || seconds > MAX_EXPIRES_IN_SECONDS {
                return Err(OAuthError::InvalidResponse);
            }
            Some(
                now.checked_add_signed(ChronoDuration::seconds(seconds))
                    .ok_or(OAuthError::InvalidResponse)?,
            )
        }
    };
    let refresh_token = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or(previous_refresh);
    Ok(OAuthToken {
        access_token: access_token.to_owned(),
        refresh_token,
        expires_at,
        metadata: BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn config() -> OAuthConfig {
        OAuthConfig {
            client_id: String::new(),
            tenant: None,
            client_secret_env: None,
            credential_id: "test-credential".into(),
            options: BTreeMap::new(),
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .expect("test client")
    }

    fn endpoints(server: &MockServer) -> Endpoints {
        let base = server.uri();
        Endpoints {
            authorize_url: format!("{base}/authorize"),
            authorize_host: "127.0.0.1".into(),
            token_url: format!("{base}/token"),
            token_host: "127.0.0.1".into(),
            load_code_assist_url: format!("{base}/load"),
            load_code_assist_host: "127.0.0.1".into(),
            require_https: false,
            redirect_host: "127.0.0.1",
        }
    }

    fn callback_details(pending: &PendingLogin) -> (String, u16, String) {
        let prompt = url::Url::parse(&pending.prompt().url).expect("prompt URL");
        let state = prompt
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .expect("state");
        let redirect = prompt
            .query_pairs()
            .find(|(key, _)| key == "redirect_uri")
            .map(|(_, value)| value.into_owned())
            .expect("redirect URI");
        let redirect = url::Url::parse(&redirect).expect("parsed redirect");
        (
            state,
            redirect.port().expect("callback port"),
            redirect.path().to_owned(),
        )
    }

    async fn send_callback(port: u16, path: &str, query: &str) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .expect("connect callback");
        stream
            .write_all(format!("GET {path}?{query} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .expect("write callback");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("read callback response");
        String::from_utf8_lossy(&response).into_owned()
    }

    #[tokio::test]
    async fn callback_state_mismatch_is_ignored_before_exchange() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("code_verifier="))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "access-new",
                "refresh_token": "refresh-new",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;
        let pending = begin_with(&client(), &config(), endpoints(&server))
            .await
            .expect("begin");
        let (state, port, path) = callback_details(&pending);
        let task = tokio::spawn(pending.finish());

        let response = send_callback(port, &path, "state=forged&code=stolen").await;
        assert!(response.starts_with("HTTP/1.1 404"));
        assert!(!task.is_finished(), "mismatched state ended the login");

        let response = send_callback(port, &path, &format!("state={state}&code=approved")).await;
        assert!(response.starts_with("HTTP/1.1 200"));
        let token = timeout(Duration::from_secs(5), task)
            .await
            .expect("login timeout")
            .expect("login task")
            .expect("token");
        assert_eq!(token.access_token, "access-new");
    }

    #[tokio::test]
    async fn refresh_uses_rotated_refresh_token_and_retains_project() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=refresh-old"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "access-new",
                "refresh_token": "refresh-rotated",
                "expires_in": 1800
            })))
            .mount(&server)
            .await;
        let stored = OAuthToken {
            access_token: "access-old".into(),
            refresh_token: Some("refresh-old".into()),
            expires_at: None,
            metadata: BTreeMap::from([("project_id".into(), "project-one".into())]),
        };
        let token = refresh_with(&client(), &config(), &stored, &endpoints(&server))
            .await
            .expect("refresh");
        assert_eq!(token.access_token, "access-new");
        assert_eq!(token.refresh_token.as_deref(), Some("refresh-rotated"));
        assert_eq!(
            token.metadata.get("project_id").map(String::as_str),
            Some("project-one")
        );
    }

    #[tokio::test]
    async fn load_code_assist_accepts_object_and_string_project_shapes() {
        for (body, expected) in [
            (
                json!({"cloudaicompanionProject": {"id": " object-project "}}),
                "object-project",
            ),
            (
                json!({"cloudaicompanionProject": "string-project"}),
                "string-project",
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/load"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            let project = fetch_project_id(
                &client(),
                &format!("{}/load", server.uri()),
                "access-secret",
                Instant::now() + Duration::from_secs(5),
            )
            .await;
            assert_eq!(project.as_deref(), Some(expected));
        }
    }

    #[tokio::test]
    async fn oauth_response_body_never_appears_in_errors() {
        let server = MockServer::start().await;
        const PRIVATE_BODY: &str = "authorization_code=do-not-leak&refresh_token=secret";
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(400).set_body_raw(PRIVATE_BODY, "text/plain; charset=utf-8"),
            )
            .mount(&server)
            .await;
        let stored = OAuthToken {
            access_token: "access-old".into(),
            refresh_token: Some("refresh-old".into()),
            expires_at: None,
            metadata: BTreeMap::new(),
        };
        let error = match refresh_with(&client(), &config(), &stored, &endpoints(&server)).await {
            Ok(_) => panic!("malformed OAuth response unexpectedly succeeded"),
            Err(error) => error,
        };
        let rendered = format!("{error}");
        assert!(!rendered.contains(PRIVATE_BODY));
        assert!(!rendered.contains("do-not-leak"));
        assert!(!rendered.contains("secret"));
    }
}
