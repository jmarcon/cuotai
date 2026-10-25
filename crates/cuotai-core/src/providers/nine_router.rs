//! A user's own 9router instance, read through its management routes
//! (PRD 0036, aggregator class).
//!
//! This is **not** the `router_*` family: those modules reimplement the
//! upstream quota queries 9router's source performs, talking to the upstream
//! providers with the user's own credential. This adapter talks only to the
//! instance and shows the quotas of the connections the user already added
//! inside 9router. No 9router code is incorporated: the routes below were
//! observed at revision `f39d8f07481ae8518e93175f0ecaaafa61573c6c`
//! (<https://github.com/decolua/9router>, MIT) and the response shapes are
//! re-described here as typed, allow-listed parsers.
//!
//! Safety rules (PRD 0036 RF3.6, NFR):
//!
//! - [`Route`] is a closed enum: `GET /api/health`, `GET /api/version`,
//!   `POST /api/auth/login`, `GET /api/providers/client` and
//!   `GET /api/usage/{id}`. Nothing else can be requested. `/api/providers`
//!   (it keeps unsanitized `providerSpecificData`), the reset routes,
//!   settings, keys and `/v1/*` are never built.
//! - A refresh never sends `force=1`.
//! - The connection list is parsed into an allow-list of fields; everything
//!   else (including `lastError`, proxy URLs and any token-like field) is
//!   dropped by the deserializer and never reaches a report.
//! - The dashboard JWT lives only in memory, is sent only to the base URL
//!   and a failed login is never retried inside the instance's lockout
//!   window.
//! - Plain HTTP is accepted for loopback hosts, and for private-network
//!   hosts only with `allow_private_http`; everything else needs HTTPS.

use super::{Context, Data, QueryError};
use crate::model::{Metric, MetricKind, MetricWindow, Provider, Status, SubReport};
use crate::providers::nine_router_tls as tls;
use crate::text::{params, Localized, Notes, ParamValue};
use crate::Account;
use base64::Engine as _;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::{Method, RequestBuilder, Response};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;
use url::{Host, Url};

/// Public file that defines the usage handlers, at the revision the routes
/// and shapes were observed at (`f39d8f07481ae8518e93175f0ecaaafa61573c6c`,
/// PRD 0036 RF3.8).
pub(crate) const VIA: &str = "https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/open-sse/services/usage.js";
/// Where an instance listens by default.
pub(crate) const DEFAULT_BASE_URL: &str = "http://127.0.0.1:20128";

/// Hard caps of the discovery (PRD 0036 RF3.2).
const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 10;
/// Connections whose usage is read at the same time (RF4.1).
const CONCURRENCY: usize = 2;
/// Name of the dashboard session cookie.
const COOKIE: &str = "auth_token";
/// A login is renewed this long before its JWT expires (RF3.1).
const RENEW_MARGIN: ChronoDuration = ChronoDuration::minutes(10);
/// A rejected password is not tried again for this long.
const REJECTED_FOR: ChronoDuration = ChronoDuration::minutes(10);
/// A JWT without a readable `exp` is trusted for this long.
const UNKNOWN_EXP_FOR: ChronoDuration = ChronoDuration::hours(1);

/// Time budget of a refresh (RF4.1).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub per_request: Duration,
    pub instance: Duration,
}

impl Limits {
    pub(crate) const DEFAULT: Self = Self {
        per_request: Duration::from_secs(15),
        instance: Duration::from_secs(60),
    };
}

// ---------------------------------------------------------------------------
// Routes: the only requests this module can build.
// ---------------------------------------------------------------------------

/// The allow-list of RF3.6, as a type: a request outside it cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Route {
    Health,
    Version,
    Login,
    Connections { page: usize },
    Usage(String),
}

impl Route {
    pub(crate) fn method(&self) -> Method {
        match self {
            Self::Login => Method::POST,
            _ => Method::GET,
        }
    }

    /// Path relative to the base URL, with its query string.
    pub(crate) fn path(&self) -> String {
        match self {
            Self::Health => "api/health".into(),
            Self::Version => "api/version".into(),
            Self::Login => "api/auth/login".into(),
            Self::Connections { page } => {
                format!("api/providers/client?accountStatus=all&pageSize={PAGE_SIZE}&page={page}")
            }
            Self::Usage(id) => format!("api/usage/{id}"),
        }
    }

    /// The usage route of a connection id, or `None` when the id is not
    /// `[A-Za-z0-9_-]{1,128}` (it goes into a path, so anything else is
    /// refused rather than escaped).
    pub(crate) fn usage(id: &str) -> Option<Self> {
        let valid = !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        valid.then(|| Self::Usage(id.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Base URL rules (RF2.3).
// ---------------------------------------------------------------------------

/// Why a base URL is refused. Never carries the URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaseUrlError {
    Malformed,
    /// Userinfo, query or fragment present.
    Forbidden,
    /// Plain HTTP to a host that is neither loopback nor an allowed private
    /// network.
    Insecure,
}

/// True for the hosts that never leave the machine.
fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(ip) => IpAddr::V4(*ip).is_loopback(),
        Host::Ipv6(ip) => IpAddr::V6(*ip).is_loopback(),
    }
}

fn private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_private() || ip.is_link_local() || (a == 100 && (64..128).contains(&b))
}

fn private_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    // Unique local fc00::/7 and link-local fe80::/10.
    (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
}

/// RFC 1918, link-local, CGNAT/Tailscale `100.64.0.0/10`, ULA and `.local` /
/// `.ts.net` names: the only hosts the private-network opt-in covers.
pub(crate) fn is_private_network(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => {
            let domain = domain.to_ascii_lowercase();
            domain.ends_with(".local") || domain.ends_with(".ts.net")
        }
        Host::Ipv4(ip) => private_v4(*ip),
        Host::Ipv6(ip) => private_v6(*ip),
    }
}

/// Parse and normalize a base URL (scheme, host, optional port, optional
/// path prefix for a reverse proxy). The result ends without a slash.
pub(crate) fn parse_base(raw: &str, allow_private_http: bool) -> Result<Url, BaseUrlError> {
    let mut url = Url::parse(raw.trim()).map_err(|_| BaseUrlError::Malformed)?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(BaseUrlError::Malformed);
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(BaseUrlError::Forbidden);
    }
    if url.scheme() == "http" {
        let host = url.host().ok_or(BaseUrlError::Malformed)?;
        let host = match &host {
            Host::Domain(domain) => Host::Domain(*domain),
            Host::Ipv4(ip) => Host::Ipv4(*ip),
            Host::Ipv6(ip) => Host::Ipv6(*ip),
        };
        if !(is_loopback(&host) || (allow_private_http && is_private_network(&host))) {
            return Err(BaseUrlError::Insecure);
        }
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&format!("{path}/"));
    Ok(url)
}

/// True when the base URL is a loopback host.
fn base_is_loopback(base: &Url) -> bool {
    base.host().is_some_and(|host| match host {
        Host::Domain(domain) => is_loopback(&Host::Domain(domain)),
        Host::Ipv4(ip) => is_loopback(&Host::Ipv4(ip)),
        Host::Ipv6(ip) => is_loopback(&Host::Ipv6(ip)),
    })
}

fn truthy(value: Option<&String>) -> bool {
    value.is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1"))
}

/// The validated base URL of `account`; used by `Config::validate`.
pub(crate) fn account_base(account: &Account) -> Result<Url, BaseUrlError> {
    let raw = account
        .params
        .get("base_url")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_BASE_URL);
    parse_base(raw, truthy(account.params.get("allow_private_http")))
}

fn base_error(error: BaseUrlError) -> QueryError {
    let (id, text) = match error {
        BaseUrlError::Malformed => (
            "query.nine_router.base_url",
            "Endereço base do 9router inválido: informe http(s)://host[:porta][/prefixo].",
        ),
        BaseUrlError::Forbidden => (
            "query.nine_router.base_url_forbidden",
            "O endereço base do 9router não pode conter usuário, senha, consulta ou fragmento.",
        ),
        BaseUrlError::Insecure => (
            "query.nine_router.base_url_insecure",
            "Fora desta máquina o 9router exige HTTPS (HTTP só em rede privada, com a opção explícita). Nada foi enviado.",
        ),
    };
    QueryError::config(id, text)
}

// ---------------------------------------------------------------------------
// Mapping tables (RF3.4, RF3.5).
// ---------------------------------------------------------------------------

/// The Cuotai provider of a 9router upstream slug; `None` for an upstream
/// Cuotai has no tile for (it keeps a generic tile and the verbatim slug).
pub(crate) fn map_upstream(slug: &str) -> Option<Provider> {
    Some(match slug {
        "claude" => Provider::Claude,
        "codex" => Provider::ChatGpt,
        "github" => Provider::Copilot,
        "gemini-cli" => Provider::GeminiAi,
        "antigravity" => Provider::Antigravity,
        "kiro" => Provider::Kiro,
        "qoder" | "qoder-cn" => Provider::Qoder,
        "ollama" => Provider::OllamaCloud,
        "minimax" | "minimax-cn" => Provider::MiniMax,
        "kimi" => Provider::Kimi,
        "codebuddy-cn" | "codebuddy-intl" => Provider::CodeBuddy,
        "grok-cli" => Provider::Grok,
        "opencode-go" => Provider::OpenCodeGo,
        "groq" => Provider::Groq,
        "zed" => Provider::Zed,
        "xiaomi-mimo" => Provider::XiaomiMimo,
        "cursor" => Provider::Cursor,
        _ => return None,
    })
}

/// The window a 9router quota label stands for. 9router's labels are free
/// text ("session (5h)", "weekly (7d)", "weekly Opus (7d)", Codex `session` /
/// `weekly`); an unknown label has no window and therefore no pace.
pub(crate) fn window_of(label: &str) -> Option<MetricWindow> {
    let label = label.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| label.contains(needle));
    if has(&["5h", "5 h", "5-hour", "five hour"]) || label.trim() == "session" {
        Some(MetricWindow::Window5h)
    } else if has(&["weekly", "7d", "week"]) {
        Some(MetricWindow::Weekly)
    } else if has(&["monthly", "30d", "month"]) {
        Some(MetricWindow::Monthly)
    } else if has(&["daily", "24h", "day"]) {
        Some(MetricWindow::Daily)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Parsers: allow-lists, never a Debug of a response.
// ---------------------------------------------------------------------------

/// One connection of `GET /api/providers/client`, reduced to the fields
/// Cuotai may read. Serde drops every other field, including `lastError`,
/// `connectionProxyUrl` and any token-like value.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    id: String,
    provider: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    is_active: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    #[serde(default, alias = "total_pages")]
    total_pages: Option<usize>,
}

#[derive(Deserialize)]
struct ConnectionsPage {
    connections: Vec<Connection>,
    #[serde(default)]
    pagination: Option<Pagination>,
}

/// Strip control characters, trim and bound a text taken from the instance.
fn clean_text(raw: &str, max: usize) -> String {
    raw.chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(max)
        .collect()
}

/// True for a label that looks like a secret (an API key) rather than a name.
fn looks_like_key(text: &str) -> bool {
    let compact = !text.contains(char::is_whitespace);
    compact
        && (text.starts_with("sk-")
            || text.starts_with("ghp_")
            || text.starts_with("eyJ")
            || (text.len() >= 24
                && text
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')))
}

/// `a***@domain`: an e-mail used as a connection name is identity data and
/// is never shown in full.
fn mask_email(text: &str) -> String {
    match text.split_once('@') {
        Some((local, domain)) if !local.is_empty() && !domain.is_empty() => {
            let first: String = local.chars().take(1).collect();
            format!("{first}***@{domain}")
        }
        _ => text.to_string(),
    }
}

/// The label of a connection: display name, then name, then the masked
/// e-mail; a key-looking value is replaced by `<upstream> #<n>`.
fn label_of(connection: &Connection, ordinal: usize) -> String {
    let candidate = [
        &connection.display_name,
        &connection.name,
        &connection.email,
    ]
    .into_iter()
    .flatten()
    .map(|value| clean_text(value, 60))
    .find(|value| !value.is_empty() && !looks_like_key(value));
    match candidate {
        Some(value) if value.contains('@') => mask_email(&value),
        Some(value) => value,
        None => format!("{} #{ordinal}", clean_text(&connection.provider, 40)),
    }
}

/// Opaque, stable id of a connection: SHA-256 of the 9router id, truncated.
/// The raw connection UUID is never a public key (RF5.1).
pub(crate) fn sub_id(connection_id: &str) -> String {
    let digest = Sha256::digest(connection_id.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn number(value: Option<&Value>) -> Option<f64> {
    let number = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

/// The usage of one connection after normalization by 9router.
#[derive(Debug, Default)]
pub(crate) struct ParsedUsage {
    plan: Option<String>,
    metrics: Vec<Metric>,
    limit_reached: bool,
}

/// A soft failure (`{ message }`): never shown verbatim, only classified.
#[derive(Debug)]
pub(crate) enum UsageOutcome {
    Usage(ParsedUsage),
    Message { auth_expired: bool },
}

fn message_means_auth_expired(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "expired",
        "reconnect",
        "re-auth",
        "reauth",
        "unauthorized",
        "invalid token",
        "invalid_token",
        "sign in",
        "log in again",
        "login again",
        "authentication",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

pub(crate) fn parse_usage(value: &Value) -> Result<UsageOutcome, QueryError> {
    let body = value.as_object().ok_or_else(QueryError::schema)?;
    let quotas = body.get("quotas").and_then(Value::as_object);
    if quotas.is_none() {
        if let Some(message) = body.get("message").and_then(Value::as_str) {
            return Ok(UsageOutcome::Message {
                auth_expired: message_means_auth_expired(message),
            });
        }
    }
    let mut parsed = ParsedUsage {
        plan: body
            .get("plan")
            .and_then(Value::as_str)
            .map(|plan| clean_text(plan, 40))
            .filter(|plan| !plan.is_empty()),
        metrics: Vec::new(),
        limit_reached: body.get("limitReached").and_then(Value::as_bool) == Some(true),
    };
    for (label, quota) in quotas.into_iter().flatten() {
        let Some(quota) = quota.as_object() else {
            continue;
        };
        // "Unlimited" is not a meter.
        if quota.get("unlimited").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let label = clean_text(label, 60);
        if label.is_empty() {
            continue;
        }
        let total = number(quota.get("total")).filter(|total| *total > 0.0);
        let mut used = number(quota.get("used"));
        let mut remaining = number(quota.get("remaining"));
        let percentage = number(quota.get("remainingPercentage"));
        let mut limit = total;
        let mut unit = String::new();
        if limit.is_none() {
            if let Some(percentage) = percentage {
                // Only a percentage is known: say so, do not invent a cap.
                limit = Some(100.0);
                remaining = remaining.or(Some(percentage.clamp(0.0, 100.0)));
                used = used.or(Some(100.0 - percentage.clamp(0.0, 100.0)));
                unit = "%".into();
            }
        } else if limit == Some(100.0) && percentage.is_some() {
            unit = "%".into();
        }
        if used.is_none() && remaining.is_none() {
            // Nothing measured: a missing value is not a zero.
            continue;
        }
        parsed.metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            window: window_of(&label),
            label,
            used,
            limit,
            remaining,
            unit,
            kind: MetricKind::Quota,
            period: None,
            resets_at: quota
                .get("resetAt")
                .and_then(Value::as_str)
                .and_then(|text| DateTime::parse_from_rfc3339(text.trim()).ok())
                .map(|at| at.with_timezone(&Utc)),
        });
    }
    if parsed.metrics.is_empty() && !parsed.limit_reached && quotas.is_none() {
        return Err(QueryError::schema());
    }
    Ok(UsageOutcome::Usage(parsed))
}

// ---------------------------------------------------------------------------
// Session: the dashboard JWT, in memory, shared per instance.
// ---------------------------------------------------------------------------

enum Slot {
    Token {
        value: String,
        renew_at: DateTime<Utc>,
    },
    /// A login that failed: not retried before `until` (and, for a rejected
    /// password, only while the password is the same one).
    Rejected {
        until: DateTime<Utc>,
        fingerprint: Option<String>,
        status: Status,
        note: Localized,
    },
}

static SESSIONS: LazyLock<tokio::sync::Mutex<HashMap<String, Slot>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

fn fingerprint(password: &str) -> String {
    let digest = Sha256::digest(password.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `exp` of a JWT, read without verifying it.
fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let exp = serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("exp")?
        .as_i64()?;
    DateTime::from_timestamp(exp, 0)
}

fn cookie_value(response: &Response) -> Option<String> {
    response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .find_map(|header| {
            let first = header.split(';').next()?;
            let (name, value) = first.split_once('=')?;
            (name.trim() == COOKIE && !value.trim().is_empty()).then(|| value.trim().to_string())
        })
}

/// One configured instance: where it is and how long a request may take.
pub(crate) struct Instance<'a> {
    http: &'a reqwest::Client,
    base: Url,
    limits: Limits,
    timeout: Duration,
    /// PRD 0036 RF2.3: raised by the pinned verifier when the instance
    /// presented another certificate.
    pin_mismatch: Option<Arc<AtomicBool>>,
}

impl<'a> Instance<'a> {
    fn new(http: &'a reqwest::Client, base: Url, limits: Limits, timeout: Duration) -> Self {
        Self {
            http,
            base,
            limits,
            timeout,
            pin_mismatch: None,
        }
    }

    fn with_pin_flag(mut self, flag: Option<Arc<AtomicBool>>) -> Self {
        self.pin_mismatch = flag;
        self
    }

    fn pin_changed(&self) -> bool {
        self.pin_mismatch
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    fn request_timeout(&self) -> Duration {
        self.limits
            .per_request
            .min(self.timeout.max(Duration::from_secs(1)))
    }

    fn key(&self) -> String {
        self.base.as_str().to_string()
    }

    /// Build the request of `route`. The URL is `base` + the route path: no
    /// other host or path can result.
    fn request(&self, route: &Route, token: Option<&str>) -> Result<RequestBuilder, QueryError> {
        let url = self
            .base
            .join(&route.path())
            .map_err(|_| QueryError::transport())?;
        if url.origin() != self.base.origin() {
            return Err(QueryError::transport());
        }
        let mut request = self
            .http
            .request(route.method(), url)
            .header("Accept", "application/json")
            .timeout(self.request_timeout());
        if let Some(token) = token {
            request = request.header("Cookie", format!("{COOKIE}={token}"));
        }
        Ok(request)
    }

    async fn send(&self, request: RequestBuilder) -> Result<Response, QueryError> {
        request.send().await.map_err(|error| {
            if self.pin_changed() {
                tls::changed()
            } else if error.is_timeout() {
                QueryError::timeout()
            } else {
                QueryError::transport()
            }
        })
    }

    async fn get(&self, route: &Route, token: Option<&str>) -> Result<Response, QueryError> {
        let response = self.send(self.request(route, token)?).await?;
        check_status(&response)?;
        Ok(response)
    }

    /// Log in once and cache the cookie; reuse it until it is near its
    /// expiry. `force` drops the cached token first (a 401 on a request).
    async fn token(
        &self,
        password: Option<&str>,
        now: DateTime<Utc>,
        force: bool,
    ) -> Result<Option<String>, QueryError> {
        let Some(password) = password else {
            return Ok(None);
        };
        let key = self.key();
        let print = fingerprint(password);
        let mut sessions = SESSIONS.lock().await;
        if force {
            sessions.remove(&key);
        }
        match sessions.get(&key) {
            Some(Slot::Token { value, renew_at }) if now < *renew_at => {
                return Ok(Some(value.clone()));
            }
            Some(Slot::Rejected {
                until,
                fingerprint,
                status,
                note,
            }) if now < *until && fingerprint.as_ref().is_none_or(|known| *known == print) => {
                return Err(QueryError {
                    diagnostic: None,
                    status: *status,
                    note: note.clone(),
                });
            }
            _ => {}
        }
        sessions.remove(&key);
        let body = serde_json::json!({ "password": password });
        let request = self.request(&Route::Login, None)?.json(&body);
        let response = self.send(request).await?;
        let code = response.status().as_u16();
        match code {
            200..=299 => {
                let Some(value) = cookie_value(&response) else {
                    return Err(QueryError::schema());
                };
                let renew_at = jwt_expiry(&value)
                    .map(|exp| exp - RENEW_MARGIN)
                    .unwrap_or(now + UNKNOWN_EXP_FOR);
                sessions.insert(
                    key,
                    Slot::Token {
                        value: value.clone(),
                        renew_at,
                    },
                );
                Ok(Some(value))
            }
            429 => {
                let wait = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<i64>().ok())
                    .filter(|seconds| *seconds > 0)
                    .unwrap_or(60)
                    .min(24 * 3600);
                let note = Localized::with_params(
                    "query.nine_router.locked",
                    params([("seconds", ParamValue::from(wait as u64))]),
                    format!("Muitas tentativas de login no 9router; aguarde {wait} s antes de tentar de novo."),
                );
                sessions.insert(
                    key,
                    Slot::Rejected {
                        until: now + ChronoDuration::seconds(wait),
                        fingerprint: None,
                        status: Status::RateLimited,
                        note: note.clone(),
                    },
                );
                Err(QueryError {
                    diagnostic: None,
                    status: Status::RateLimited,
                    note,
                })
            }
            401 => {
                let note = Localized::fixed(
                    "query.nine_router.password_wrong",
                    "Senha do painel do 9router recusada.",
                );
                sessions.insert(
                    key,
                    Slot::Rejected {
                        until: now + REJECTED_FOR,
                        fingerprint: Some(print),
                        status: Status::AuthenticationError,
                        note: note.clone(),
                    },
                );
                Err(QueryError {
                    diagnostic: None,
                    status: Status::AuthenticationError,
                    note,
                })
            }
            403 => Err(QueryError {
                diagnostic: None,
                status: Status::AuthenticationError,
                note: Localized::fixed(
                    "query.nine_router.login_refused",
                    "O 9router não aceita login por senha por aqui (SSO ativo ou acesso bloqueado pelo túnel).",
                ),
            }),
            _ => Err(http_error(code)),
        }
    }
}

fn http_error(code: u16) -> QueryError {
    QueryError {
        diagnostic: None,
        status: match code {
            401 | 403 => Status::AuthenticationError,
            429 => Status::RateLimited,
            _ => Status::Error,
        },
        note: Localized::with_params(
            "query.http",
            params([("code", ParamValue::from(code))]),
            format!("Consulta retornou HTTP {code}. Verifique acesso e permissões."),
        ),
    }
}

/// Classify a response that is not a success without reading its body. A
/// redirect (the client never follows one) is the instance sending the
/// request to its login page.
fn check_status(response: &Response) -> Result<(), QueryError> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    if status.is_redirection() {
        return Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.nine_router.redirected",
                "O 9router redirecionou a consulta para o login; informe a senha do painel.",
            ),
        });
    }
    if status.as_u16() == 401 {
        return Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.nine_router.unauthorized",
                "O 9router recusou o acesso: informe a senha do painel.",
            ),
        });
    }
    Err(http_error(status.as_u16()))
}

// ---------------------------------------------------------------------------
// Refresh.
// ---------------------------------------------------------------------------

fn password_of(account: &Account, ctx: &Context<'_>) -> Result<Option<String>, QueryError> {
    let Some(name) = account
        .credential_env
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return Ok(None);
    };
    ctx.credentials
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .map(Some)
        .ok_or_else(|| {
            QueryError::config_params(
                "query.missing_credential",
                params([("name", ParamValue::from(name.to_string()))]),
                format!("Credencial ausente: {name}"),
            )
        })
}

/// Comma-separated list parameter, lower-cased.
fn list_param(account: &Account, key: &str) -> Vec<String> {
    account
        .params
        .get(key)
        .map(|raw| {
            raw.split(',')
                .map(|item| item.trim().to_ascii_lowercase())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

async fn discover(
    instance: &Instance<'_>,
    token: &mut Option<String>,
    password: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(Vec<Connection>, bool), QueryError> {
    let mut all = Vec::new();
    let mut truncated = false;
    let mut relogged = false;
    let mut page = 1;
    loop {
        let response = match instance
            .get(&Route::Connections { page }, token.as_deref())
            .await
        {
            Err(error)
                if error.status == Status::AuthenticationError
                    && password.is_some()
                    && !relogged =>
            {
                // The cookie may have been revoked: log in once more.
                relogged = true;
                *token = instance.token(password, now, true).await?;
                instance
                    .get(&Route::Connections { page }, token.as_deref())
                    .await?
            }
            other => other?,
        };
        let body: ConnectionsPage = response.json().await.map_err(|_| QueryError::schema())?;
        let count = body.connections.len();
        all.extend(body.connections);
        let total_pages = body
            .pagination
            .and_then(|pagination| pagination.total_pages);
        let more = match total_pages {
            Some(total) => page < total,
            None => count >= PAGE_SIZE,
        };
        if !more || count == 0 {
            break;
        }
        if page >= MAX_PAGES {
            truncated = true;
            break;
        }
        page += 1;
    }
    Ok((all, truncated))
}

fn sub_notes(extra: Option<Localized>) -> Vec<Localized> {
    let mut notes = vec![
        Localized::fixed(
            "note.nine_router.cached",
            "O 9router pode mostrar uma leitura em cache; a idade dela não é informada.",
        ),
        Localized::fixed(
            "note.nine_router.zero_caveat",
            "Valores como normalizados pelo 9router; um valor ausente pode aparecer como 0.",
        ),
    ];
    notes.extend(extra);
    notes
}

fn sub_report(
    connection: &Connection,
    ordinal: usize,
    now: DateTime<Utc>,
    status: Status,
    metrics: Vec<Metric>,
    plan: Option<String>,
    extra: Option<Localized>,
) -> SubReport {
    SubReport {
        id: sub_id(&connection.id),
        upstream: clean_text(&connection.provider, 40),
        provider: map_upstream(&connection.provider),
        label: label_of(connection, ordinal),
        status,
        metrics,
        notes_l10n: sub_notes(extra),
        checked_at: now,
        as_of: None,
        enabled_upstream: connection.is_active,
        duplicate_of: None,
        plan,
        via: Some(VIA.to_string()),
    }
}

async fn read_usage(
    instance: &Instance<'_>,
    connection: &Connection,
    ordinal: usize,
    token: Option<&str>,
    now: DateTime<Utc>,
) -> SubReport {
    let failed = |error: QueryError| {
        sub_report(
            connection,
            ordinal,
            now,
            error.status,
            Vec::new(),
            None,
            Some(error.note),
        )
    };
    let Some(route) = Route::usage(&connection.id) else {
        return failed(QueryError::schema());
    };
    let response = match instance.get(&route, token).await {
        Ok(response) => response,
        Err(error) => {
            // 401 here is the connection's own credential refresh failing
            // inside 9router; 404 is a connection that no longer exists.
            return failed(usage_error(error));
        }
    };
    let value: Value = match response.json().await {
        Ok(value) => value,
        Err(_) => return failed(QueryError::schema()),
    };
    match parse_usage(&value) {
        Ok(UsageOutcome::Usage(mut usage)) => {
            if usage.limit_reached && usage.metrics.is_empty() {
                // No detail but the limit is reached: exhausted, no cap.
                usage.metrics.push(Metric {
                    billing: Default::default(),
                    scope: None,
                    label: "Limite atingido".into(),
                    used: None,
                    limit: None,
                    remaining: Some(0.0),
                    unit: String::new(),
                    kind: MetricKind::Quota,
                    window: None,
                    period: None,
                    resets_at: None,
                });
            }
            sub_report(
                connection,
                ordinal,
                now,
                Status::Ready,
                usage.metrics,
                usage.plan,
                None,
            )
        }
        Ok(UsageOutcome::Message { auth_expired: true }) => failed(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "note.nine_router.reconnect",
                "Reconecte esta conta no 9router.",
            ),
        }),
        Ok(UsageOutcome::Message {
            auth_expired: false,
        }) => failed(QueryError::unsupported(
            "note.nine_router.no_usage",
            "O 9router não tem uso para esta conexão.",
        )),
        Err(error) => failed(error),
    }
}

/// One connection: listed as off without a query, or read within the
/// instance deadline.
async fn read_one(
    instance: &Instance<'_>,
    connection: &Connection,
    ordinal: usize,
    token: Option<&str>,
    now: DateTime<Utc>,
    deadline: tokio::time::Instant,
) -> (usize, SubReport) {
    if connection.is_active == Some(false) {
        // Off in 9router: listed, never queried.
        let report = sub_report(
            connection,
            ordinal,
            now,
            Status::Unsupported,
            Vec::new(),
            None,
            Some(Localized::fixed(
                "note.nine_router.off",
                "Desligada no 9router.",
            )),
        );
        return (ordinal, report);
    }
    let read = tokio::time::timeout_at(
        deadline,
        read_usage(instance, connection, ordinal, token, now),
    )
    .await;
    let report = read.unwrap_or_else(|_| {
        sub_report(
            connection,
            ordinal,
            now,
            Status::Error,
            Vec::new(),
            None,
            Some(QueryError::timeout().note),
        )
    });
    (ordinal, report)
}

/// Map the transport-level error of a usage request to the sub-account's.
fn usage_error(error: QueryError) -> QueryError {
    match error.note.id.as_ref() {
        "query.nine_router.unauthorized" => QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "note.nine_router.reconnect",
                "Reconecte esta conta no 9router.",
            ),
        },
        _ => {
            let gone = matches!(
                error.note.params.get("code"),
                Some(ParamValue::Text(code)) if code == "404"
            );
            if gone {
                QueryError {
                    diagnostic: None,
                    status: Status::Error,
                    note: Localized::fixed(
                        "note.nine_router.gone",
                        "A conexão não existe mais no 9router.",
                    ),
                }
            } else {
                error
            }
        }
    }
}

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    fetch_at(account, ctx, Limits::DEFAULT, Utc::now()).await
}

pub(crate) async fn fetch_at(
    account: &Account,
    ctx: &Context<'_>,
    limits: Limits,
    now: DateTime<Utc>,
) -> Result<Data, QueryError> {
    let allow_private = truthy(account.params.get("allow_private_http"));
    let base = account_base(account).map_err(base_error)?;
    let password = password_of(account, ctx)?;
    let trust = tls::mode(account, &base).map_err(tls_error)?;
    let own = tls::client(&trust, ctx.timeout)?;
    let (http, flag) = match &own {
        Some((client, flag)) => (client, flag.clone()),
        None => (ctx.http, None),
    };
    let instance = Instance::new(http, base, limits, ctx.timeout).with_pin_flag(flag);
    let deadline = tokio::time::Instant::now() + limits.instance;

    let mut token = instance.token(password.as_deref(), now, false).await?;
    let (connections, truncated) =
        discover(&instance, &mut token, password.as_deref(), now).await?;

    let include = list_param(account, "include");
    let exclude = list_param(account, "exclude");
    let hidden = list_param(account, "hidden");
    let selected: Vec<(usize, &Connection)> = connections
        .iter()
        .filter(|connection| {
            let slug = connection.provider.to_ascii_lowercase();
            (include.is_empty() || include.contains(&slug))
                && !exclude.contains(&slug)
                && !hidden.contains(&sub_id(&connection.id))
        })
        .enumerate()
        .map(|(index, connection)| (index + 1, connection))
        .collect();

    let token_ref = token.as_deref();
    let mut readings: Vec<(usize, SubReport)> = Vec::with_capacity(selected.len());
    // Two connections at a time (RF4.1), in order; the instance deadline
    // bounds the whole loop.
    for chunk in selected.chunks(CONCURRENCY) {
        let batch = chunk.iter().map(|(ordinal, connection)| {
            read_one(&instance, connection, *ordinal, token_ref, now, deadline)
        });
        readings.extend(futures_util::future::join_all(batch).await);
    }
    readings.sort_by_key(|(ordinal, _)| *ordinal);
    let sub_reports: Vec<SubReport> = readings.into_iter().map(|(_, report)| report).collect();

    let mut notes = Notes::new();
    let ready = sub_reports
        .iter()
        .filter(|sub| sub.status == Status::Ready)
        .count();
    notes.push_params(
        "note.nine_router.summary",
        params([
            ("ready", ParamValue::from(ready as u64)),
            ("total", ParamValue::from(sub_reports.len() as u64)),
        ]),
        format!(
            "{ready} de {} conexões do 9router com uso.",
            sub_reports.len()
        ),
    );
    if let Some(version) = best_effort_version(&instance, token.as_deref()).await {
        notes.push_params(
            "note.nine_router.version",
            params([("version", ParamValue::from(version.clone()))]),
            format!("9router {version}."),
        );
    }
    if truncated {
        notes.push(
            "note.nine_router.truncated",
            "O 9router tem mais conexões do que o limite lido (1000); as demais foram omitidas.",
        );
    }
    if password.is_none() && !base_is_loopback(&instance.base) {
        notes.push(
            "note.nine_router.exposed",
            "Esta instância do 9router responde a consultas de gerenciamento sem senha. Qualquer pessoa que a alcance pode ler e alterar as conexões dela.",
        );
    }
    if instance.base.scheme() == "http" && !base_is_loopback(&instance.base) && allow_private {
        notes.push(
            "note.nine_router.private_http",
            "Conexão sem criptografia na rede privada, por escolha sua.",
        );
    }
    let mut data = Data::new(
        Vec::new(),
        notes,
        "9router: api/providers/client, api/usage/{id}",
    );
    data.sub_reports = sub_reports;
    Ok(data)
}

/// `GET /api/version`: the instance's version, or nothing. A failure here
/// never changes the reading.
async fn best_effort_version(instance: &Instance<'_>, token: Option<&str>) -> Option<String> {
    let response = instance.get(&Route::Version, token).await.ok()?;
    let value: Value = response.json().await.ok()?;
    let version = clean_text(value.get("currentVersion")?.as_str()?, 32);
    (!version.is_empty()
        && version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+')))
    .then_some(version)
}

// ---------------------------------------------------------------------------
// Test connection (RF2.2).
// ---------------------------------------------------------------------------

/// Outcome of [`test_connection`]; each has one core sentence and no raw body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    /// Reachable and authorized.
    Connected {
        version: Option<String>,
        connections: usize,
    },
    /// The address answers but is not a 9router instance.
    NotNineRouter,
    /// The dashboard password was refused.
    PasswordWrong,
    /// Too many login attempts; the instance asks to wait.
    LockedOut { seconds: u64 },
    /// The instance refuses password login (SSO, or a blocked tunnel).
    LoginRefused,
    /// No password was set but the instance requires one.
    PasswordRequired,
    /// The address could not be used or reached.
    Unreachable,
    /// PRD 0036 RF2.3: the instance answers over HTTPS with a certificate
    /// the platform (or the chosen CA file) does not trust. Its SHA-256
    /// fingerprint is offered for trust on first use; nothing else was
    /// sent.
    UntrustedCertificate { fingerprint: String },
    /// PRD 0036 RF2.3: the pinned certificate no longer matches; the
    /// instance now presents `fingerprint`. Never re-pinned silently.
    CertificateChanged { fingerprint: String },
    /// PRD 0036 RF2.3: the chosen CA file is missing or not PEM.
    CaFileUnreadable,
}

/// The localized sentence of an outcome (no URL, no body).
pub(crate) fn outcome_text(outcome: &TestOutcome) -> Localized {
    match outcome {
        TestOutcome::Connected {
            version,
            connections,
        } => {
            let version = version.clone().unwrap_or_default();
            Localized::with_params(
                "test.nine_router.connected",
                params([
                    ("connections", ParamValue::from(*connections as u64)),
                    ("version", ParamValue::from(version.clone())),
                ]),
                format!("9router {version}: {connections} conexões com uso encontradas."),
            )
        }
        TestOutcome::NotNineRouter => Localized::fixed(
            "test.nine_router.not_nine_router",
            "O endereço responde, mas não parece ser um 9router.",
        ),
        TestOutcome::PasswordWrong => Localized::fixed(
            "query.nine_router.password_wrong",
            "Senha do painel do 9router recusada.",
        ),
        TestOutcome::LockedOut { seconds } => Localized::with_params(
            "query.nine_router.locked",
            params([("seconds", ParamValue::from(*seconds))]),
            format!("Muitas tentativas de login no 9router; aguarde {seconds} s antes de tentar de novo."),
        ),
        TestOutcome::LoginRefused => Localized::fixed(
            "query.nine_router.login_refused",
            "O 9router não aceita login por senha por aqui (SSO ativo ou acesso bloqueado pelo túnel).",
        ),
        TestOutcome::PasswordRequired => Localized::fixed(
            "test.nine_router.password_required",
            "Esta instância exige a senha do painel; escolha \"Senha do painel\".",
        ),
        TestOutcome::Unreachable => Localized::fixed(
            "test.nine_router.unreachable",
            "Não foi possível alcançar o endereço informado.",
        ),
        TestOutcome::UntrustedCertificate { fingerprint } => Localized::with_param(
            "test.nine_router.untrusted_certificate",
            "fingerprint",
            fingerprint.as_str(),
            format!("O certificado da instância não é confiável para este computador. Impressão digital SHA-256: {fingerprint}. Confira-a no servidor antes de confiar nela."),
        ),
        TestOutcome::CertificateChanged { fingerprint } => Localized::with_param(
            "test.nine_router.certificate_changed",
            "fingerprint",
            fingerprint.as_str(),
            format!("O certificado da instância mudou e não corresponde ao fixado. Nova impressão digital SHA-256: {fingerprint}. Confira-a no servidor antes de confiar nela."),
        ),
        TestOutcome::CaFileUnreadable => Localized::fixed(
            "query.nine_router.ca_file",
            "O arquivo de CA do 9router não pôde ser lido como PEM.",
        ),
    }
}

/// PRD 0036 RF2.3: after a transport failure of the health check over
/// HTTPS, tell a certificate problem from an unreachable address: the
/// capture verifier completes a handshake (and reads the certificate's
/// fingerprint) where the real one failed. Nothing but the public health
/// route is requested.
async fn certificate_outcome(instance: &Instance<'_>, trust: &tls::TlsMode) -> TestOutcome {
    if instance.base.scheme() != "https" {
        return TestOutcome::Unreachable;
    }
    let Ok(health) = instance.base.join(&Route::Health.path()) else {
        return TestOutcome::Unreachable;
    };
    let seen = tls::capture_fingerprint(health, instance.request_timeout()).await;
    match (seen, trust) {
        (Some(digest), tls::TlsMode::Pin(_)) if instance.pin_changed() => {
            TestOutcome::CertificateChanged {
                fingerprint: tls::format_fingerprint(&digest),
            }
        }
        (Some(digest), tls::TlsMode::Platform | tls::TlsMode::CaFile(_)) => {
            TestOutcome::UntrustedCertificate {
                fingerprint: tls::format_fingerprint(&digest),
            }
        }
        _ => TestOutcome::Unreachable,
    }
}

/// The note of unusable TLS parameters (`Config::validate` refuses them
/// first; this covers a configuration built without it).
fn tls_error(error: tls::TlsError) -> QueryError {
    match error {
        tls::TlsError::PinFormat => QueryError::config(
            "query.nine_router.pin_format",
            "A impressão digital do certificado do 9router deve ter 64 dígitos hexadecimais (SHA-256).",
        ),
        tls::TlsError::Conflict => QueryError::config(
            "query.nine_router.tls_conflict",
            "Use um arquivo de CA ou uma impressão digital fixada, não os dois, e só com HTTPS.",
        ),
    }
}

/// `GET /api/health` → `GET /api/version` → login (when a password is set)
/// → `GET /api/providers/client?pageSize=…`. Read-only; nothing is stored
/// besides the in-memory login.
pub(crate) async fn test_connection(account: &Account, ctx: &Context<'_>) -> TestOutcome {
    test_connection_at(account, ctx, Limits::DEFAULT, Utc::now()).await
}

pub(crate) async fn test_connection_at(
    account: &Account,
    ctx: &Context<'_>,
    limits: Limits,
    now: DateTime<Utc>,
) -> TestOutcome {
    let Ok(base) = account_base(account) else {
        return TestOutcome::Unreachable;
    };
    let Ok(password) = password_of(account, ctx) else {
        return TestOutcome::PasswordRequired;
    };
    let Ok(trust) = tls::mode(account, &base) else {
        return TestOutcome::Unreachable;
    };
    let own = match tls::client(&trust, ctx.timeout) {
        Ok(own) => own,
        Err(_) => return TestOutcome::CaFileUnreadable,
    };
    let (http, flag) = match &own {
        Some((client, flag)) => (client, flag.clone()),
        None => (ctx.http, None),
    };
    let instance = Instance::new(http, base, limits, ctx.timeout).with_pin_flag(flag);
    let health = match instance.request(&Route::Health, None) {
        Ok(request) => match request.send().await {
            Ok(response) if response.status().is_success() => response.json::<Value>().await.ok(),
            Ok(_) => return TestOutcome::Unreachable,
            Err(_) => return certificate_outcome(&instance, &trust).await,
        },
        Err(_) => return TestOutcome::Unreachable,
    };
    if health
        .as_ref()
        .and_then(|value| value.get("ok"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        return TestOutcome::NotNineRouter;
    }
    let version = best_effort_version(&instance, None).await;
    let token = match instance.token(password.as_deref(), now, false).await {
        Ok(token) => token,
        Err(error) => {
            return match error.note.id.as_ref() {
                "query.nine_router.password_wrong" => TestOutcome::PasswordWrong,
                "query.nine_router.locked" => TestOutcome::LockedOut {
                    seconds: match error.note.params.get("seconds") {
                        Some(ParamValue::Text(text)) => text.parse().unwrap_or(60),
                        _ => 60,
                    },
                },
                "query.nine_router.login_refused" => TestOutcome::LoginRefused,
                _ => TestOutcome::Unreachable,
            };
        }
    };
    match instance
        .get(&Route::Connections { page: 1 }, token.as_deref())
        .await
    {
        Ok(response) => match response.json::<ConnectionsPage>().await {
            Ok(page) => TestOutcome::Connected {
                version,
                connections: page.connections.len(),
            },
            Err(_) => TestOutcome::NotNineRouter,
        },
        Err(error) if error.status == Status::AuthenticationError => {
            if password.is_none() {
                TestOutcome::PasswordRequired
            } else {
                TestOutcome::PasswordWrong
            }
        }
        Err(_) => TestOutcome::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use serde_json::json;
    use std::collections::BTreeMap;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    struct Env;
    impl Credentials for Env {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_NINE_ROUTER_PASSWORD").then(|| "correct horse".into())
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    fn account(server: &MockServer, password: bool, extra: &[(&str, &str)]) -> Account {
        let mut params = BTreeMap::from([("base_url".to_string(), server.uri())]);
        params.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        Account {
            name: "home".into(),
            provider: Provider::NineRouter,
            credential_env: password.then(|| "TEST_NINE_ROUTER_PASSWORD".into()),
            params,
            ..Account::default()
        }
    }

    fn context(http: &reqwest::Client) -> Context<'_> {
        Context {
            http,
            credentials: &Env,
            timeout: Duration::from_secs(5),
        }
    }

    fn jwt(exp: i64) -> String {
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.sig",
            engine.encode(r#"{"alg":"HS256"}"#),
            engine.encode(format!(r#"{{"exp":{exp}}}"#))
        )
    }

    fn page(connections: Value, total_pages: usize) -> Value {
        json!({
            "connections": connections,
            "providerOptions": [],
            "pagination": { "page": 1, "pageSize": 100, "totalPages": total_pages },
            "totals": {}
        })
    }

    async fn mount_json(server: &MockServer, route: &str, body: Value) {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    fn claude_usage() -> Value {
        json!({
            "plan": "Max",
            "quotas": {
                "session (5h)": { "used": 20, "total": 100, "remaining": 80, "remainingPercentage": 80,
                                  "resetAt": "2026-10-25T18:00:00Z" },
                "weekly (7d)": { "used": 40, "total": 100, "remaining": 60, "remainingPercentage": 60,
                                 "resetAt": "2026-10-30T00:00:00Z" },
                "weekly Opus (7d)": { "used": 100, "total": 100, "remaining": 0, "remainingPercentage": 0,
                                      "resetAt": "not a date" }
            }
        })
    }

    fn requests_of(requests: &[Request]) -> Vec<(String, String)> {
        requests
            .iter()
            .map(|request| {
                (
                    request.method.to_string(),
                    format!(
                        "{}{}",
                        request.url.path(),
                        request
                            .url
                            .query()
                            .map(|query| format!("?{query}"))
                            .unwrap_or_default()
                    ),
                )
            })
            .collect()
    }

    // --- allow-list -------------------------------------------------------

    #[test]
    fn routes_are_a_closed_allow_list_and_never_force() {
        assert_eq!(Route::Health.path(), "api/health");
        assert_eq!(Route::Version.path(), "api/version");
        assert_eq!(Route::Login.method(), Method::POST);
        assert_eq!(Route::Health.method(), Method::GET);
        assert_eq!(
            Route::Connections { page: 2 }.path(),
            "api/providers/client?accountStatus=all&pageSize=100&page=2"
        );
        let usage = Route::usage("abc-DEF_123").unwrap();
        assert_eq!(usage.path(), "api/usage/abc-DEF_123");
        assert!(!usage.path().contains("force"));
        // Ids that could change the path are refused, not escaped.
        let long = "x".repeat(129);
        for bad in [
            "",
            "../providers",
            "a/claude-reset",
            "a?force=1",
            "a b",
            "a%2f",
            long.as_str(),
        ] {
            assert!(Route::usage(bad).is_none(), "{bad}");
        }
        // None of the named dangerous routes is reachable from any route.
        let all = [
            Route::Health,
            Route::Version,
            Route::Login,
            Route::Connections { page: 1 },
            Route::usage("id").unwrap(),
        ];
        for route in all {
            let built = route.path();
            assert!(
                built == "api/health"
                    || built == "api/version"
                    || built == "api/auth/login"
                    || built.starts_with("api/providers/client?")
                    || built == "api/usage/id",
                "{built}"
            );
            for forbidden in [
                "claude-reset",
                "codex-reset-credits",
                "api/settings",
                "api/keys",
                "v1/",
                "force",
            ] {
                assert!(!built.contains(forbidden), "{built} contains {forbidden}");
            }
        }
    }

    #[test]
    fn base_url_rules_per_host_class() {
        let ok = |raw: &str, private: bool| parse_base(raw, private).map(|url| url.to_string());
        // Loopback may use HTTP, with a port and a proxy prefix.
        assert_eq!(
            ok("http://127.0.0.1:20128", false).unwrap(),
            "http://127.0.0.1:20128/"
        );
        assert!(ok("http://localhost:20128/", false).is_ok());
        assert!(ok("http://[::1]:20128", false).is_ok());
        assert_eq!(
            ok("https://example.com/9router/", false).unwrap(),
            "https://example.com/9router/"
        );
        // Anything else needs HTTPS...
        assert_eq!(ok("http://example.com", false), Err(BaseUrlError::Insecure));
        assert_eq!(
            ok("http://192.168.1.5:20128", false),
            Err(BaseUrlError::Insecure)
        );
        // ...except private networks with the explicit opt-in.
        for private in [
            "http://192.168.1.5:20128",
            "http://10.0.0.2",
            "http://172.20.1.1",
            "http://169.254.1.1",
            "http://100.101.102.103",
            "http://nas.local:20128",
            "http://box.tailnet.ts.net",
            "http://[fd00::1]:20128",
        ] {
            assert!(ok(private, true).is_ok(), "{private}");
        }
        // The opt-in never covers public hosts or look-alike names.
        for public in [
            "http://example.com",
            "http://8.8.8.8",
            "http://100.128.0.1",
            "http://172.32.0.1",
            "http://localhost.evil.com",
            "http://evil.local.example.com",
        ] {
            assert_eq!(ok(public, true), Err(BaseUrlError::Insecure), "{public}");
        }
        // Userinfo, query, fragment, other schemes and garbage.
        for forbidden in [
            "https://user:pw@example.com",
            "https://user@example.com",
            "https://example.com/?x=1",
            "https://example.com/#frag",
        ] {
            assert_eq!(
                ok(forbidden, false),
                Err(BaseUrlError::Forbidden),
                "{forbidden}"
            );
        }
        for malformed in ["ftp://example.com", "file:///etc/passwd", "not a url", ""] {
            assert_eq!(
                ok(malformed, false),
                Err(BaseUrlError::Malformed),
                "{malformed}"
            );
        }
    }

    #[test]
    fn config_validation_uses_the_same_rules() {
        let validate = |params: &[(&str, &str)]| {
            let account = Account {
                provider: Provider::NineRouter,
                params: params
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                ..Account::default()
            };
            crate::Config {
                accounts: vec![account],
                ..crate::Config::default()
            }
            .validate()
        };
        // The default (no base_url) is the local instance.
        assert!(validate(&[]).is_ok());
        assert!(validate(&[("base_url", "http://127.0.0.1:20128")]).is_ok());
        assert!(matches!(
            validate(&[("base_url", "http://example.com")]),
            Err(crate::ConfigError::BaseUrl)
        ));
        assert!(matches!(
            validate(&[("base_url", "https://u:p@example.com")]),
            Err(crate::ConfigError::BaseUrl)
        ));
        assert!(validate(&[
            ("base_url", "http://192.168.1.5"),
            ("allow_private_http", "true")
        ])
        .is_ok());
    }

    #[test]
    fn upstream_mapping_and_windows() {
        for (slug, provider) in [
            ("claude", Provider::Claude),
            ("codex", Provider::ChatGpt),
            ("github", Provider::Copilot),
            ("gemini-cli", Provider::GeminiAi),
            ("antigravity", Provider::Antigravity),
            ("kiro", Provider::Kiro),
            ("qoder", Provider::Qoder),
            ("qoder-cn", Provider::Qoder),
            ("ollama", Provider::OllamaCloud),
            ("minimax", Provider::MiniMax),
            ("minimax-cn", Provider::MiniMax),
            ("kimi", Provider::Kimi),
            ("codebuddy-cn", Provider::CodeBuddy),
            ("codebuddy-intl", Provider::CodeBuddy),
            ("grok-cli", Provider::Grok),
            ("opencode-go", Provider::OpenCodeGo),
            ("groq", Provider::Groq),
            ("zed", Provider::Zed),
            ("xiaomi-mimo", Provider::XiaomiMimo),
            ("cursor", Provider::Cursor),
        ] {
            assert_eq!(map_upstream(slug), Some(provider), "{slug}");
        }
        for slug in [
            "glm",
            "deepseek",
            "iflow",
            "vercel-ai-gateway",
            "commandcode",
            "opencode-zen",
            "minimax-code",
            "future-thing",
            "",
        ] {
            assert_eq!(map_upstream(slug), None, "{slug}");
        }
        for (label, window) in [
            ("session (5h)", Some(MetricWindow::Window5h)),
            ("session", Some(MetricWindow::Window5h)),
            ("weekly (7d)", Some(MetricWindow::Weekly)),
            ("weekly Opus (7d)", Some(MetricWindow::Weekly)),
            ("weekly", Some(MetricWindow::Weekly)),
            ("monthly", Some(MetricWindow::Monthly)),
            ("daily", Some(MetricWindow::Daily)),
            ("premium requests", None),
            ("credits", None),
        ] {
            assert_eq!(window_of(label), window, "{label}");
        }
    }

    // --- usage parsing ----------------------------------------------------

    fn parsed(value: Value) -> ParsedUsage {
        match parse_usage(&value).unwrap() {
            UsageOutcome::Usage(usage) => usage,
            other => panic!("expected usage, got {other:?}"),
        }
    }

    #[test]
    fn claude_usage_maps_quotas_windows_and_resets() {
        let usage = parsed(claude_usage());
        assert_eq!(usage.plan.as_deref(), Some("Max"));
        assert_eq!(usage.metrics.len(), 3);
        let by_label = |label: &str| usage.metrics.iter().find(|m| m.label == label).unwrap();
        let session = by_label("session (5h)");
        assert_eq!(session.window, Some(MetricWindow::Window5h));
        assert_eq!(
            (session.used, session.limit, session.remaining),
            (Some(20.0), Some(100.0), Some(80.0))
        );
        assert_eq!(session.unit, "%");
        assert_eq!(session.kind, MetricKind::Quota);
        assert_eq!(
            session.resets_at.map(|at| at.to_rfc3339()),
            Some("2026-10-25T18:00:00+00:00".to_string())
        );
        // An invalid reset date is no date, not a guess.
        assert_eq!(by_label("weekly Opus (7d)").resets_at, None);
        assert_eq!(by_label("weekly (7d)").window, Some(MetricWindow::Weekly));
    }

    #[test]
    fn missing_values_are_never_turned_into_zero() {
        // Nothing measured: no metric at all.
        let usage = parsed(json!({ "quotas": { "credits": { "total": 100 } } }));
        assert!(usage.metrics.is_empty());
        // total = 0 is no cap, and the used value is kept.
        let usage = parsed(json!({ "quotas": { "tokens": { "used": 7, "total": 0 } } }));
        assert_eq!(usage.metrics[0].limit, None);
        assert_eq!(usage.metrics[0].used, Some(7.0));
        // Only a percentage: say it is a percentage.
        let usage = parsed(json!({ "quotas": { "weekly": { "remainingPercentage": 25 } } }));
        let metric = &usage.metrics[0];
        assert_eq!(
            (metric.used, metric.limit, metric.remaining),
            (Some(75.0), Some(100.0), Some(25.0))
        );
        assert_eq!(metric.unit, "%");
        // Numeric strings are numbers; other types are nothing.
        let usage = parsed(
            json!({ "quotas": { "a": { "used": "3", "total": "10" }, "b": { "used": true } } }),
        );
        assert_eq!(usage.metrics.len(), 1);
        assert_eq!(usage.metrics[0].limit, Some(10.0));
        // Unlimited is not a meter.
        let usage =
            parsed(json!({ "quotas": { "x": { "unlimited": true, "used": 1, "total": 2 } } }));
        assert!(usage.metrics.is_empty());
    }

    #[test]
    fn limit_reached_and_soft_failures_are_classified_without_the_text() {
        assert!(parsed(json!({ "limitReached": true, "quotas": {} })).limit_reached);
        for (message, auth) in [
            ("Token expired, please reconnect", true),
            ("Invalid token", true),
            (
                "GitHub Copilot connected. Usage API not available for your plan.",
                false,
            ),
            ("Antigravity: usage unsupported", false),
        ] {
            match parse_usage(&json!({ "message": message })).unwrap() {
                UsageOutcome::Message { auth_expired } => {
                    assert_eq!(auth_expired, auth, "{message}")
                }
                other => panic!("{other:?}"),
            }
        }
        // Not an object, or nothing known: schema error.
        assert_eq!(
            parse_usage(&json!([1, 2])).unwrap_err().note.id,
            "query.schema"
        );
        assert_eq!(
            parse_usage(&json!({ "hello": 1 })).unwrap_err().note.id,
            "query.schema"
        );
    }

    #[test]
    fn labels_never_show_keys_or_full_emails() {
        let conn = |name: Option<&str>, display: Option<&str>, email: Option<&str>| Connection {
            id: "id".into(),
            provider: "claude".into(),
            name: name.map(str::to_string),
            display_name: display.map(str::to_string),
            email: email.map(str::to_string),
            is_active: Some(true),
        };
        assert_eq!(label_of(&conn(Some("work"), None, None), 1), "work");
        assert_eq!(
            label_of(&conn(Some("work"), Some("Work Max"), None), 1),
            "Work Max"
        );
        // A key-looking name is replaced.
        assert_eq!(
            label_of(&conn(Some("sk-ant-api03-abcdef"), None, None), 3),
            "claude #3"
        );
        assert_eq!(
            label_of(&conn(Some("A1b2C3d4E5f6G7h8I9j0K1l2M3"), None, None), 2),
            "claude #2"
        );
        // An e-mail used as a name is masked.
        assert_eq!(
            label_of(&conn(Some("ana@example.com"), None, None), 1),
            "a***@example.com"
        );
        assert_eq!(
            label_of(&conn(None, None, Some("ana@example.com")), 1),
            "a***@example.com"
        );
        assert_eq!(label_of(&conn(None, None, None), 4), "claude #4");
        // The id is a hash, never the 9router id.
        let id = sub_id("11111111-2222-3333-4444-555555555555");
        assert_eq!(id.len(), 16);
        assert!(!id.contains("1111"));
        assert_eq!(id, sub_id("11111111-2222-3333-4444-555555555555"));
    }

    // --- refresh ----------------------------------------------------------

    fn connection(id: &str, provider: &str, name: &str) -> Value {
        json!({
            "id": id, "provider": provider, "authType": "oauth", "name": name,
            "email": "private.person@example.com", "isActive": true, "testStatus": "active",
            "priority": 1,
            // Everything below must be dropped by the deserializer.
            "accessToken": "SECRET-ACCESS-TOKEN",
            "refreshToken": "SECRET-REFRESH-TOKEN",
            "lastError": "Bearer SECRET-LAST-ERROR",
            "connectionProxyUrl": "http://proxyuser:SECRET-PROXY@10.0.0.1:3128",
            "providerSpecificData": { "copilotToken": "SECRET-COPILOT-TOKEN" }
        })
    }

    #[tokio::test]
    async fn refresh_without_password_reads_every_connection_and_leaks_nothing() {
        let server = MockServer::start().await;
        mount_json(
            &server,
            "/api/providers/client",
            page(
                json!([
                    connection("c-1", "claude", "Work"),
                    connection("c-2", "glm", "GLM key"),
                    { "id": "c-3", "provider": "codex", "name": "Off one", "isActive": false },
                    connection("c-4", "kiro", "Kiro"),
                ]),
                1,
            ),
        )
        .await;
        mount_json(
            &server,
            "/api/version",
            json!({ "currentVersion": "0.5.109", "hasUpdate": false }),
        )
        .await;
        mount_json(&server, "/api/usage/c-1", claude_usage()).await;
        mount_json(
            &server,
            "/api/usage/c-2",
            json!({ "message": "GLM usage not supported" }),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/api/usage/c-4"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Bearer SECRET-BODY"))
            .mount(&server)
            .await;

        let http = client();
        let data = fetch_at(
            &account(&server, false, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap();
        assert!(
            data.metrics.is_empty(),
            "the instance has no metrics of its own"
        );
        assert_eq!(data.sub_reports.len(), 4);
        let by = |label: &str| data.sub_reports.iter().find(|s| s.label == label).unwrap();
        let claude = by("Work");
        assert_eq!(claude.status, Status::Ready);
        assert_eq!(claude.provider, Some(Provider::Claude));
        assert_eq!(claude.upstream, "claude");
        assert_eq!(claude.plan.as_deref(), Some("Max"));
        assert_eq!(claude.metrics.len(), 3);
        assert_eq!(claude.as_of, None, "9router exposes no age");
        assert!(claude
            .via
            .as_deref()
            .unwrap()
            .contains("f39d8f07481ae8518e93175f0ecaaafa61573c6c"));
        assert!(claude
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.nine_router.cached"));
        assert!(claude
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.nine_router.zero_caveat"));
        // Unmapped upstream: generic tile, verbatim slug, soft failure.
        let glm = by("GLM key");
        assert_eq!((glm.provider, glm.upstream.as_str()), (None, "glm"));
        assert_eq!(glm.status, Status::Unsupported);
        assert!(glm
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.nine_router.no_usage"));
        // Off in 9router: listed, never queried.
        let off = by("Off one");
        assert_eq!(
            (off.status, off.enabled_upstream),
            (Status::Unsupported, Some(false))
        );
        // A 5xx fails that connection only.
        assert_eq!(by("Kiro").status, Status::Error);
        // Notes: summary and version, no exposure warning on loopback.
        let ids: Vec<_> = data.notes_l10n.iter().map(|n| n.id.as_ref()).collect();
        assert!(ids.contains(&"note.nine_router.summary"));
        assert!(ids.contains(&"note.nine_router.version"));
        assert!(!ids.contains(&"note.nine_router.exposed"));

        // No injected secret, raw id, e-mail or proxy URL reaches the data.
        let all = format!(
            "{}{}{}{}",
            serde_json::to_string(&data.sub_reports).unwrap(),
            serde_json::to_string(&data.notes_l10n).unwrap(),
            data.notes.join("|"),
            data.source
        );
        for leaked in [
            "SECRET",
            "proxyuser",
            "private.person",
            "c-1",
            "c-2",
            "c-4",
            "127.0.0.1",
        ] {
            assert!(!all.contains(leaked), "leaked {leaked}");
        }

        // The recorder: only allow-listed GETs, never force=1, never a write.
        let seen = requests_of(&server.received_requests().await.unwrap());
        assert!(!seen.is_empty());
        for (verb, target) in &seen {
            assert_eq!(verb, "GET", "{target}");
            assert!(!target.contains("force"), "{target}");
            let allowed = target == "/api/version"
                || target.starts_with("/api/providers/client?")
                || (target.starts_with("/api/usage/") && !target.contains('?'));
            assert!(allowed, "{target}");
            assert!(!target.contains("reset"), "{target}");
        }
        // Off connections were not queried.
        assert!(!seen.iter().any(|(_, target)| target == "/api/usage/c-3"));
    }

    #[tokio::test]
    async fn discovery_follows_pages_and_stops_at_the_cap() {
        let server = MockServer::start().await;
        for (number, id) in [(1, "p1"), (2, "p2")] {
            Mock::given(method("GET"))
                .and(path("/api/providers/client"))
                .and(query_param("page", number.to_string()))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(page(json!([connection(id, "claude", id)]), 2)),
                )
                .mount(&server)
                .await;
        }
        mount_json(&server, "/api/usage/p1", claude_usage()).await;
        mount_json(&server, "/api/usage/p2", claude_usage()).await;
        let http = client();
        let data = fetch_at(
            &account(&server, false, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(data.sub_reports.len(), 2);
        assert!(!data
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.nine_router.truncated"));

        // A never-ending list stops at the page cap with a note.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/providers/client"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(
                json!([{ "id": "x", "provider": "glm", "isActive": false }]),
                999,
            )))
            .mount(&server)
            .await;
        let data = fetch_at(
            &account(&server, false, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(data.sub_reports.len(), MAX_PAGES);
        assert!(data
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.nine_router.truncated"));
    }

    #[tokio::test]
    async fn include_exclude_and_hidden_filters_apply_before_any_usage_request() {
        let server = MockServer::start().await;
        mount_json(
            &server,
            "/api/providers/client",
            page(
                json!([
                    connection("a", "claude", "A"),
                    connection("b", "codex", "B"),
                    connection("c", "github", "C"),
                ]),
                1,
            ),
        )
        .await;
        for id in ["a", "b", "c"] {
            mount_json(&server, &format!("/api/usage/{id}"), claude_usage()).await;
        }
        let http = client();
        let hidden_b = sub_id("b");
        let go = |extra: &[(&str, &str)]| {
            let acct = account(&server, false, extra);
            let http = &http;
            async move {
                fetch_at(&acct, &context(http), Limits::DEFAULT, Utc::now())
                    .await
                    .unwrap()
                    .sub_reports
                    .iter()
                    .map(|s| s.label.clone())
                    .collect::<Vec<_>>()
            }
        };
        assert_eq!(go(&[("include", "claude, CODEX")]).await, ["A", "B"]);
        assert_eq!(go(&[("exclude", "codex")]).await, ["A", "C"]);
        assert_eq!(go(&[("hidden", hidden_b.as_str())]).await, ["A", "C"]);
        // `b` was queried only by the include run; the others never asked.
        let requests = requests_of(&server.received_requests().await.unwrap());
        let count = |target: &str| requests.iter().filter(|(_, t)| t == target).count();
        assert_eq!(count("/api/usage/b"), 1);
        assert_eq!(count("/api/usage/c"), 2);
    }

    #[tokio::test]
    async fn usage_failures_fail_one_connection_and_never_the_instance() {
        let server = MockServer::start().await;
        mount_json(
            &server,
            "/api/providers/client",
            page(
                json!([
                    connection("u401", "claude", "Expired"),
                    connection("u404", "codex", "Gone"),
                    connection("bad", "github", "Malformed"),
                    connection("msg", "kiro", "Reconnect"),
                    connection("lim", "kimi", "Limit"),
                ]),
                1,
            ),
        )
        .await;
        for (id, code) in [("u401", 401), ("u404", 404)] {
            Mock::given(method("GET"))
                .and(path(format!("/api/usage/{id}")))
                .respond_with(ResponseTemplate::new(code).set_body_string("Bearer SECRET-BODY"))
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/api/usage/bad"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
            .mount(&server)
            .await;
        mount_json(
            &server,
            "/api/usage/msg",
            json!({ "message": "Token expired, reconnect needed" }),
        )
        .await;
        mount_json(&server, "/api/usage/lim", json!({ "limitReached": true })).await;
        let http = client();
        let data = fetch_at(
            &account(&server, false, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap();
        let by = |label: &str| data.sub_reports.iter().find(|s| s.label == label).unwrap();
        assert_eq!(by("Expired").status, Status::AuthenticationError);
        assert!(by("Expired")
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.nine_router.reconnect"));
        assert_eq!(by("Gone").status, Status::Error);
        assert!(by("Gone")
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.nine_router.gone"));
        assert_eq!(by("Malformed").status, Status::Error);
        assert!(by("Malformed")
            .notes_l10n
            .iter()
            .any(|n| n.id == "query.schema"));
        assert_eq!(by("Reconnect").status, Status::AuthenticationError);
        // limitReached without detail: exhausted, from a zero remaining.
        let limit = by("Limit");
        assert_eq!(limit.status, Status::Ready);
        assert_eq!(limit.metrics[0].remaining, Some(0.0));
        assert_eq!(
            crate::aggregators::sub_class(limit),
            crate::metrics::AccountClass::Exhausted
        );
        let text = serde_json::to_string(&data.sub_reports).unwrap();
        assert!(!text.contains("SECRET"));
    }

    #[tokio::test]
    async fn the_instance_deadline_turns_slow_connections_into_timeouts() {
        let server = MockServer::start().await;
        mount_json(
            &server,
            "/api/providers/client",
            page(json!([connection("slow", "claude", "Slow")]), 1),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/api/usage/slow"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(claude_usage())
                    .set_delay(Duration::from_secs(2)),
            )
            .mount(&server)
            .await;
        let http = client();
        let limits = Limits {
            per_request: Duration::from_millis(200),
            instance: Duration::from_millis(400),
        };
        let data = fetch_at(
            &account(&server, false, &[]),
            &context(&http),
            limits,
            Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(data.sub_reports[0].status, Status::Error);
        assert!(data.sub_reports[0]
            .notes_l10n
            .iter()
            .any(|n| n.id == "query.timeout"));
    }

    // --- login ------------------------------------------------------------

    async fn mount_login(server: &MockServer, response: ResponseTemplate, expect: u64) {
        // A dropped test server's port can be reused by the next one: start
        // from a clean login state for this base URL.
        SESSIONS.lock().await.remove(&format!("{}/", server.uri()));
        Mock::given(method("POST"))
            .and(path("/api/auth/login"))
            .respond_with(response)
            .expect(expect)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn login_is_cached_per_instance_and_the_cookie_travels_only_to_it() {
        let server = MockServer::start().await;
        let token = jwt(Utc::now().timestamp() + 86_400);
        mount_login(
            &server,
            ResponseTemplate::new(200)
                .insert_header(
                    "set-cookie",
                    format!("auth_token={token}; Path=/; HttpOnly; Max-Age=86400"),
                )
                .set_body_json(json!({ "success": true })),
            1,
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/api/providers/client"))
            .and(header("cookie", format!("auth_token={token}").as_str()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(page(json!([connection("c", "claude", "C")]), 1)),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/usage/c"))
            .and(header("cookie", format!("auth_token={token}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(claude_usage()))
            .mount(&server)
            .await;
        let http = client();
        let acct = account(&server, true, &[]);
        for _ in 0..2 {
            let data = fetch_at(&acct, &context(&http), Limits::DEFAULT, Utc::now())
                .await
                .unwrap();
            assert_eq!(data.sub_reports[0].status, Status::Ready);
        }
        // The password went only in the login body (`expect(1)` proves one
        // login), never in a URL or an Authorization header.
        let requests = server.received_requests().await.unwrap();
        for request in &requests {
            assert!(!request.url.as_str().contains("correct"));
            assert!(request.headers.get("authorization").is_none());
        }
        // And the JWT is not in anything the report carries.
        let data = fetch_at(&acct, &context(&http), Limits::DEFAULT, Utc::now())
            .await
            .unwrap();
        assert!(!serde_json::to_string(&data.sub_reports)
            .unwrap()
            .contains(&token));
    }

    #[tokio::test]
    async fn an_expiring_login_is_renewed_before_it_expires() {
        let server = MockServer::start().await;
        // Expires in 5 minutes: inside the 10-minute margin, so every
        // refresh logs in again.
        let token = jwt(Utc::now().timestamp() + 300);
        mount_login(
            &server,
            ResponseTemplate::new(200)
                .insert_header("set-cookie", format!("auth_token={token}; Path=/")),
            2,
        )
        .await;
        mount_json(&server, "/api/providers/client", page(json!([]), 1)).await;
        let http = client();
        let acct = account(&server, true, &[]);
        for _ in 0..2 {
            fetch_at(&acct, &context(&http), Limits::DEFAULT, Utc::now())
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn a_rejected_password_is_not_retried_while_it_is_the_same() {
        let server = MockServer::start().await;
        mount_login(
            &server,
            ResponseTemplate::new(401).set_body_json(json!({ "error": "Invalid password" })),
            1,
        )
        .await;
        let http = client();
        let acct = account(&server, true, &[]);
        let now = Utc::now();
        for _ in 0..3 {
            let error = fetch_at(&acct, &context(&http), Limits::DEFAULT, now)
                .await
                .unwrap_err();
            assert_eq!(error.status, Status::AuthenticationError);
            assert_eq!(error.note.id, "query.nine_router.password_wrong");
            assert!(!error.note.fallback.contains("correct horse"));
        }
    }

    #[tokio::test]
    async fn a_lockout_waits_for_retry_after_without_retrying() {
        let server = MockServer::start().await;
        mount_login(
            &server,
            ResponseTemplate::new(429).insert_header("retry-after", "120"),
            1,
        )
        .await;
        let http = client();
        let acct = account(&server, true, &[]);
        let now = Utc::now();
        let error = fetch_at(&acct, &context(&http), Limits::DEFAULT, now)
            .await
            .unwrap_err();
        assert_eq!(error.status, Status::RateLimited);
        assert_eq!(error.note.id, "query.nine_router.locked");
        assert_eq!(
            error.note.params.get("seconds"),
            Some(&ParamValue::Text("120".into()))
        );
        // Inside the window: no new request (the mock expects exactly one).
        let again = fetch_at(
            &acct,
            &context(&http),
            Limits::DEFAULT,
            now + ChronoDuration::seconds(60),
        )
        .await
        .unwrap_err();
        assert_eq!(again.status, Status::RateLimited);
    }

    #[tokio::test]
    async fn login_refused_by_sso_or_tunnel_is_an_authentication_error() {
        let server = MockServer::start().await;
        mount_login(
            &server,
            ResponseTemplate::new(403).set_body_string("SSO required: SECRET-TEXT"),
            1,
        )
        .await;
        let http = client();
        let error = fetch_at(
            &account(&server, true, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::AuthenticationError);
        assert_eq!(error.note.id, "query.nine_router.login_refused");
        assert!(!error.note.fallback.contains("SECRET"));
    }

    #[tokio::test]
    async fn a_redirect_to_the_login_page_is_authentication_and_is_not_followed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/providers/client"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", "http://other.example.com/login"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let http = client();
        let error = fetch_at(
            &account(&server, false, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::AuthenticationError);
        assert_eq!(error.note.id, "query.nine_router.redirected");
    }

    #[tokio::test]
    async fn a_missing_password_variable_needs_configuration() {
        let server = MockServer::start().await;
        let http = client();
        let mut acct = account(&server, true, &[]);
        acct.credential_env = Some("UNSET_VARIABLE".into());
        let error = fetch_at(&acct, &context(&http), Limits::DEFAULT, Utc::now())
            .await
            .unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unsafe_base_url_sends_nothing() {
        let http = client();
        let acct = Account {
            provider: Provider::NineRouter,
            params: BTreeMap::from([("base_url".to_string(), "http://example.com".to_string())]),
            ..Account::default()
        };
        let error = fetch_at(&acct, &context(&http), Limits::DEFAULT, Utc::now())
            .await
            .unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
        assert_eq!(error.note.id, "query.nine_router.base_url_insecure");
        assert!(!error.note.fallback.contains("example.com"));
    }

    // --- test connection --------------------------------------------------

    #[tokio::test]
    async fn test_connection_reports_each_outcome_in_one_sentence() {
        let http = client();
        let run = |acct: Account| {
            let http = &http;
            async move { test_connection_at(&acct, &context(http), Limits::DEFAULT, Utc::now()).await }
        };

        // Reachable and authorized, no password.
        let server = MockServer::start().await;
        mount_json(&server, "/api/health", json!({ "ok": true })).await;
        mount_json(
            &server,
            "/api/version",
            json!({ "currentVersion": "0.5.109" }),
        )
        .await;
        mount_json(
            &server,
            "/api/providers/client",
            page(
                json!([
                    connection("a", "claude", "A"),
                    connection("b", "codex", "B")
                ]),
                1,
            ),
        )
        .await;
        let outcome = run(account(&server, false, &[])).await;
        assert_eq!(
            outcome,
            TestOutcome::Connected {
                version: Some("0.5.109".into()),
                connections: 2
            }
        );
        assert_eq!(outcome_text(&outcome).id, "test.nine_router.connected");

        // Not a 9router.
        let server = MockServer::start().await;
        mount_json(&server, "/api/health", json!({ "status": "fine" })).await;
        assert_eq!(
            run(account(&server, false, &[])).await,
            TestOutcome::NotNineRouter
        );
        let server = MockServer::start().await;
        assert_eq!(
            run(account(&server, false, &[])).await,
            TestOutcome::Unreachable
        );

        // No password chosen but the instance requires one.
        let server = MockServer::start().await;
        mount_json(&server, "/api/health", json!({ "ok": true })).await;
        Mock::given(method("GET"))
            .and(path("/api/providers/client"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        assert_eq!(
            run(account(&server, false, &[])).await,
            TestOutcome::PasswordRequired
        );

        // Wrong password, lockout and SSO.
        for (login, expected) in [
            (ResponseTemplate::new(401), TestOutcome::PasswordWrong),
            (
                ResponseTemplate::new(429).insert_header("retry-after", "30"),
                TestOutcome::LockedOut { seconds: 30 },
            ),
            (ResponseTemplate::new(403), TestOutcome::LoginRefused),
        ] {
            let server = MockServer::start().await;
            mount_json(&server, "/api/health", json!({ "ok": true })).await;
            mount_login(&server, login, 1).await;
            assert_eq!(run(account(&server, true, &[])).await, expected);
        }

        // Nothing listening.
        let dead = MockServer::start().await;
        let acct = account(&dead, false, &[]);
        drop(dead);
        assert_eq!(run(acct).await, TestOutcome::Unreachable);
    }

    // --- whole monitor ----------------------------------------------------

    struct NoCredentials;
    impl Credentials for NoCredentials {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    #[tokio::test]
    async fn a_monitor_refresh_carries_the_sub_reports_in_json() {
        let server = MockServer::start().await;
        mount_json(
            &server,
            "/api/providers/client",
            page(
                json!([
                    connection("k", "claude", "Work"),
                    connection("z", "glm", "Z")
                ]),
                1,
            ),
        )
        .await;
        mount_json(&server, "/api/usage/k", claude_usage()).await;
        mount_json(&server, "/api/usage/z", json!({ "message": "n/a" })).await;
        let config = crate::Config {
            accounts: vec![account(&server, false, &[])],
            ..crate::Config::default()
        };
        let monitor =
            crate::Monitor::with_credentials(config, std::sync::Arc::new(NoCredentials)).unwrap();
        let snapshot = monitor.refresh().await;
        let report = &snapshot.reports[0];
        assert_eq!(report.provider, Provider::NineRouter);
        assert_eq!(report.status, Status::Ready);
        assert_eq!(report.sub_reports.len(), 2);
        // The Opus window of the first connection is used up, so by the rules
        // of a direct account it is exhausted, and so is the pool.
        assert_eq!(
            crate::metrics::classify_report(Some(report)),
            crate::metrics::AccountClass::Exhausted
        );
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["reports"][0]["sub_reports"][0]["label"], "Work");
        assert_eq!(snapshot.units().len(), 3);

        // A failed instance is one failed report and no sub-reports.
        let config = crate::Config {
            accounts: vec![account(&server, true, &[])],
            ..crate::Config::default()
        };
        let monitor =
            crate::Monitor::with_credentials(config, std::sync::Arc::new(NoCredentials)).unwrap();
        let snapshot = monitor.refresh().await;
        assert_eq!(snapshot.reports[0].status, Status::NeedsConfiguration);
        assert!(snapshot.reports[0].sub_reports.is_empty());
    }

    #[tokio::test]
    async fn the_monitor_test_method_reports_the_outcome() {
        let server = MockServer::start().await;
        mount_json(&server, "/api/health", json!({ "ok": true })).await;
        mount_json(
            &server,
            "/api/providers/client",
            page(json!([connection("k", "claude", "Work")]), 1),
        )
        .await;
        let config = crate::Config {
            accounts: vec![account(&server, false, &[])],
            ..crate::Config::default()
        };
        let monitor =
            crate::Monitor::with_credentials(config.clone(), std::sync::Arc::new(NoCredentials))
                .unwrap();
        let test = monitor.test_nine_router(&config.accounts[0]).await;
        assert!(test.is_ok());
        assert_eq!(test.message.id, "test.nine_router.connected");
    }

    #[test]
    fn aggregator_accounts_are_polled_no_faster_than_the_floor() {
        let fast = Account {
            provider: Provider::NineRouter,
            refresh_seconds: Some(30),
            ..Account::default()
        };
        assert_eq!(fast.effective_refresh_seconds(300), 120);
        let default = Account {
            provider: Provider::NineRouter,
            ..Account::default()
        };
        assert_eq!(default.effective_refresh_seconds(300), 300);
        assert_eq!(default.effective_refresh_seconds(60), 120);
        let other = Account {
            provider: Provider::Claude,
            refresh_seconds: Some(30),
            ..Account::default()
        };
        assert_eq!(other.effective_refresh_seconds(300), 30);
        assert!(Provider::NineRouter.is_aggregator());
        assert!(!Provider::Claude.is_aggregator());
    }

    #[test]
    fn loopback_detection_for_the_exposure_warning() {
        for (url, loopback) in [
            ("http://127.0.0.1:20128", true),
            ("http://localhost:20128", true),
            ("http://[::1]:20128", true),
            ("https://example.com", false),
            ("http://192.168.1.2", false),
        ] {
            let base = parse_base(url, true).unwrap();
            assert_eq!(base_is_loopback(&base), loopback, "{url}");
        }
    }
}
