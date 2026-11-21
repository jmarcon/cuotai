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
//!   window. A one-shot command therefore logs in on each run: by the
//!   owner's decision of 2026-11-20 the session is never written to the
//!   vault nor read back from it, in any mode (the [`Context`] credential
//!   store is read-only; only the configured password or pasted SSO
//!   session is read).
//! - Never a secret first (re-validation S-01): before the first password
//!   or session goes to an instance in a process, and again after any
//!   transport failure, `GET /api/health` must answer `{ "ok": true }` and
//!   `GET /api/version` a plausible version, both without a credential.
//!   On loopback over plain HTTP, the program listening on the port must
//!   also belong to the current user or the superuser
//!   ([`crate::local_listener`]): the socket table is read again right
//!   before every request that carries the password or the session cookie
//!   (each login, each connection-list page and each usage request), so a
//!   verdict is never reused, not even within one refresh.
//! - Plain HTTP is accepted for loopback hosts, and for private-network
//!   hosts only with `allow_private_http`; everything else needs HTTPS.
//! - A secret over plain HTTP goes only to loopback (owner decision of
//!   2026-11-20): on a private-network host reached over plain HTTP, no
//!   request carries the password, the login session cookie or a pasted
//!   SSO session (`query.nine_router.secret_plain_http`, refused before
//!   anything is sent). `allow_private_http` still reads an instance whose
//!   dashboard login is off.

use super::{Context, Data, QueryError};
use crate::aggregators;
use crate::local_listener::{self, ListenerCheck, Verdict};
use crate::model::{Metric, MetricKind, MetricWindow, Provider, Status, SubReport};
use crate::providers::nine_router_tls as tls;
use crate::text::{params, Localized, Notes, ParamValue};
use crate::Account;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::{Method, RequestBuilder, Response};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;
use url::{Host, Url};

/// Public file that defines the usage handlers, at the revision the routes
/// and shapes were observed at (`f39d8f07481ae8518e93175f0ecaaafa61573c6c`,
/// PRD 0036 RF3.8).
pub(crate) const VIA: &str = "https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/open-sse/services/usage.js";
/// The oldest 9router version verified against the revision of [`VIA`] (PRD
/// 0036 RF3.8, decision D8). It is filled only after the owner's local
/// verification (task T2, Appendix B); while it is `None` no version is
/// called untested. An older instance still runs: the test and the reading
/// only add a warning.
pub(crate) const TESTED_MIN: Option<&str> = None;
/// Where an instance listens by default.
pub(crate) const DEFAULT_BASE_URL: &str = "http://127.0.0.1:20128";
/// The aggregator's name in texts that are shared with LiteLLM.
const AGGREGATOR: &str = "9router";

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

use crate::transport::is_loopback_host as is_loopback;
pub(crate) use crate::transport::is_private_network;

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
pub(crate) fn base_is_loopback(base: &Url) -> bool {
    base.host().is_some_and(|host| match host {
        Host::Domain(domain) => is_loopback(&Host::Domain(domain)),
        Host::Ipv4(ip) => is_loopback(&Host::Ipv4(ip)),
        Host::Ipv6(ip) => is_loopback(&Host::Ipv6(ip)),
    })
}

/// PRD 0036 RF2.3: whether `account` chose plain HTTP on its private
/// network (`allow_private_http`, `"true"` or `"1"`), for both aggregators.
pub(crate) fn allows_private_http(account: &Account) -> bool {
    crate::transport::truthy(account.params.get("allow_private_http").map(String::as_str))
}

/// The validated base URL of `account`; used by `Config::validate`.
pub(crate) fn account_base(account: &Account) -> Result<Url, BaseUrlError> {
    let raw = account
        .params
        .get("base_url")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_BASE_URL);
    parse_base(raw, allows_private_http(account))
}

fn base_error(error: BaseUrlError) -> QueryError {
    let (id, text) = match error {
        BaseUrlError::Malformed => (
            "query.nine_router.base_url",
            "Invalid 9router base address: enter http(s)://host[:port][/prefix].",
        ),
        BaseUrlError::Forbidden => (
            "query.nine_router.base_url_forbidden",
            "The 9router base address cannot contain a user, password, query or fragment.",
        ),
        BaseUrlError::Insecure => (
            "query.nine_router.base_url_insecure",
            "Outside this machine 9router requires HTTPS (HTTP only on a private network, with the explicit option). Nothing was sent.",
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
    /// Copilot connections carry the GitHub e-mail here (RF5.2).
    #[serde(default)]
    github_email: Option<String>,
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

/// Strip control characters, trim and bound a text taken from the instance
/// to `max` characters; a cut text ends with "…" (re-validation V-16), so a
/// reader and the export can tell it was cut.
fn clean_text(raw: &str, max: usize) -> String {
    let clean: String = raw.chars().filter(|ch| !ch.is_control()).collect();
    let clean = clean.trim();
    if clean.chars().count() <= max {
        return clean.to_string();
    }
    let mut cut: String = clean.chars().take(max.saturating_sub(1)).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
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

/// RF5.2: the normalized e-mail of a connection (`email`, else
/// `githubEmail`), compared in memory with direct accounts only.
fn identity_of(connection: &Connection) -> Option<String> {
    [&connection.email, &connection.github_email]
        .into_iter()
        .flatten()
        .map(|value| crate::aggregators::normalize_email(value))
        .find(|value| value.contains('@'))
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
    /// S-04: more quotas than [`aggregators::MAX_METRICS_PER_CONNECTION`].
    truncated: bool,
    /// S-08: a figure was out of range and left absent.
    refused: bool,
}

impl ParsedUsage {
    /// The connection notes of this reading (S-04, S-08).
    fn notes(&self) -> Vec<Localized> {
        let mut notes = Vec::new();
        if self.truncated {
            notes.push(aggregators::metrics_truncated_note());
        }
        if self.refused {
            notes.push(aggregators::figure_refused_note(AGGREGATOR));
        }
        notes
    }
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
    let body = super::object_of(value)?;
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
        truncated: false,
        refused: false,
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
        // S-08: a negative or absurd figure is left absent, never 0, and
        // the connection says a value was refused.
        let mut checked = |value: Option<f64>| {
            let (value, refused) = aggregators::check_figure(value);
            parsed.refused |= refused;
            value
        };
        let total = checked(number(quota.get("total"))).filter(|total| *total > 0.0);
        let mut used = checked(number(quota.get("used")));
        let mut remaining = checked(number(quota.get("remaining")));
        let mut percentage = checked(number(quota.get("remainingPercentage")));
        if percentage.is_some_and(|percentage| percentage > 100.0) {
            percentage = None;
            parsed.refused = true;
        }
        // A given remaining above the cap is impossible; used above the
        // cap is real (over quota) and is kept.
        if let (Some(total), Some(given)) = (total, remaining) {
            if given > total {
                remaining = None;
                parsed.refused = true;
            }
        }
        let mut limit = total;
        let mut unit = String::new();
        if limit.is_none() {
            if let Some(percentage) = percentage {
                // Only a percentage is known: say so, do not invent a cap.
                // Without a total, `used` and `remaining` are not percents
                // (9router normalizes a missing one to 0), so neither is kept.
                limit = Some(100.0);
                remaining = Some(percentage);
                used = Some(100.0 - percentage);
                unit = "%".into();
            }
        } else if limit == Some(100.0) && percentage.is_some() {
            unit = "%".into();
        }
        if used.is_none() && remaining.is_none() {
            // Nothing measured: a missing value is not a zero.
            continue;
        }
        if parsed.metrics.len() >= aggregators::MAX_METRICS_PER_CONNECTION {
            // S-04: one answer cannot flood memory or the history.
            parsed.truncated = true;
            break;
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
            derived_from: None,
            unlimited: false,
        });
    }
    if parsed.metrics.is_empty() && !parsed.limit_reached && quotas.is_none() {
        return Err(QueryError::schema_at(value, &["quotas"], "object"));
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

/// Re-validation S-01: the instances (by base URL) whose health and version
/// answered as a 9router, without a credential, since their last transport
/// failure. It belongs with the session slot (a refresh holding a valid
/// token adds no request), under its own lock because a failed send clears
/// it while a login holds the session lock.
static VERIFIED: LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(Default::default);

fn verified() -> std::sync::MutexGuard<'static, std::collections::HashSet<String>> {
    VERIFIED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Tests: forget the in-memory login and verification of `base` (a reused
/// test port).
#[cfg(test)]
pub(crate) async fn forget_login(base: &str) {
    SESSIONS.lock().await.remove(base);
    verified().remove(base);
}

/// A random key drawn once per process for [`fingerprint`].
static FINGERPRINT_KEY: LazyLock<[u8; 32]> = LazyLock::new(|| {
    let mut key = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut key);
    key
});

/// Tells whether a rejected password is the one tried again, without keeping
/// it: an HMAC-SHA256 under a random key of this process, so the value held
/// in memory is no plain password hash and means nothing to another process.
fn fingerprint(password: &str) -> String {
    use hmac::{Hmac, Mac as _};
    let mut mac = Hmac::<Sha256>::new_from_slice(FINGERPRINT_KEY.as_slice())
        .expect("HMAC takes a key of any length");
    mac.update(password.as_bytes());
    let tag = mac.finalize().into_bytes();
    tag.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `exp` of a JWT, read without verifying it.
fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    crate::user_session::jwt_expiry(token)
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
    /// Re-validation S-01: who owns a loopback listening socket.
    listener: &'a dyn ListenerCheck,
    /// The version the health-first exchange read, reused by the reading.
    version_seen: std::sync::Mutex<Option<String>>,
}

impl<'a> Instance<'a> {
    fn new(http: &'a reqwest::Client, base: Url, limits: Limits, timeout: Duration) -> Self {
        Self {
            http,
            // L-1: the check and the connection dial the same address.
            base: local_listener::pin_plain_localhost(base),
            limits,
            timeout,
            pin_mismatch: None,
            listener: &local_listener::System,
            version_seen: std::sync::Mutex::new(None),
        }
    }

    fn with_listener(mut self, listener: &'a dyn ListenerCheck) -> Self {
        self.listener = listener;
        self
    }

    /// Re-validation S-01 part 2: before a request that carries a password
    /// or a session to a loopback address over plain HTTP, every program
    /// listening on that port must belong to the current user or the
    /// superuser. HTTPS is authenticated by its certificate, and a
    /// private-network host is not on this machine. To-do 48: the verdict
    /// is read again for every such request and never cached, so a port
    /// that changes hands during a refresh stops the next secret. One read
    /// of the table costs a few milliseconds (about 2 ms on macOS in a debug
    /// build, 2026-11-20), against a network request per check.
    fn check_listener(&self) -> Result<(), QueryError> {
        if self.base.scheme() != "http" {
            return Ok(());
        }
        let Some(targets) = self
            .base
            .host()
            .and_then(|host| local_listener::loopback_targets(&host))
        else {
            return Ok(());
        };
        let port = self.base.port_or_known_default().unwrap_or(80);
        match self.listener.verdict(&targets, port) {
            Verdict::Trusted => Ok(()),
            Verdict::Foreign | Verdict::Unconfirmed => Err(listener_foreign(port)),
        }
    }

    /// Owner decision of 2026-11-20: the password, the login session and
    /// a pasted SSO session (one secret class: each opens the dashboard)
    /// travel over plain HTTP only to loopback. A private-network host
    /// reached with `allow_private_http` gets them only over HTTPS.
    fn secret_may_go(&self) -> Result<(), QueryError> {
        if self.base.scheme() == "http" && !base_is_loopback(&self.base) {
            return Err(secret_plain_http(&self.base));
        }
        Ok(())
    }

    fn is_verified(&self) -> bool {
        verified().contains(&self.key())
    }

    /// A transport failure (a restart, a port that changed hands): the next
    /// secret waits for the health-first exchange (the listener check runs
    /// before every secret anyway).
    fn forget_verification(&self) {
        verified().remove(&self.key());
    }

    /// Re-validation S-01 part 1: never a secret first. Unless this process
    /// already saw it since the last transport failure, `GET /api/health`
    /// must answer `{ "ok": true }` and `GET /api/version` a plausible
    /// version, both without a credential; otherwise nothing is sent.
    async fn ensure_genuine(&self) -> Result<(), QueryError> {
        if self.is_verified() {
            return Ok(());
        }
        let health = self.send(self.request(&Route::Health, None)?).await?;
        let healthy = health.status().is_success()
            && super::read_json::<Value>(health)
                .await
                .ok()
                .and_then(|value| value.get("ok").and_then(Value::as_bool))
                == Some(true);
        if !healthy {
            return Err(not_nine_router());
        }
        let response = self.send(self.request(&Route::Version, None)?).await?;
        let version = if response.status().is_success() {
            read_version(response).await
        } else {
            None
        };
        let Some(version) = version.filter(|version| plausible_version(version)) else {
            return Err(not_nine_router());
        };
        *self
            .version_seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(version);
        verified().insert(self.key());
        Ok(())
    }

    /// Test connection already read both routes: record the result.
    fn mark_verified(&self) {
        verified().insert(self.key());
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
        if token.is_some() || *route == Route::Login {
            self.secret_may_go()?;
            self.check_listener()?;
        }
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
            self.forget_verification();
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
                let value = value.clone();
                // A transport failure since: check who answers first.
                self.ensure_genuine().await?;
                return Ok(Some(value));
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
        self.ensure_genuine().await?;
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
                    "Too many 9router login attempts; wait {seconds} s before trying again.",
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
                    "The 9router dashboard password was refused.",
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
                    "9router does not accept password login here (SSO is on or the tunnel blocks access).",
                ),
            }),
            // S-07: a redirect at login is the instance (or a proxy in front
            // of it) sending the request to its login page: authentication,
            // never followed and never retried.
            300..=399 => Err(redirected()),
            _ => Err(super::http_failure(code)),
        }
    }
}

/// Re-validation S-01: the address does not answer like a 9router.
fn not_nine_router() -> QueryError {
    QueryError {
        diagnostic: None,
        status: Status::Error,
        note: Localized::fixed(
            "query.nine_router.not_nine_router",
            "The address answers but does not look like a 9router; the password or session was not sent.",
        ),
    }
}

/// Re-validation S-01: the listener on a loopback port is not provably the
/// current user's (or the superuser's).
fn listener_foreign(port: u16) -> QueryError {
    QueryError {
        diagnostic: None,
        status: Status::Error,
        note: Localized::with_param(
            "query.nine_router.listener_foreign",
            "port",
            port,
            "The program listening on port {port} could not be confirmed as yours (it may belong to another user), so the 9router password or session was not sent. Start 9router as yourself, or reach it over HTTPS with a pinned certificate.",
        ),
    }
}

/// Owner decision of 2026-11-20: a password or session is never sent over
/// plain HTTP to a host off this computer. Names only the host and port.
fn secret_plain_http(base: &Url) -> QueryError {
    QueryError {
        diagnostic: None,
        status: Status::NeedsConfiguration,
        note: secret_plain_http_note(&host_port(base)),
    }
}

fn secret_plain_http_note(host: &str) -> Localized {
    Localized::with_param(
        "query.nine_router.secret_plain_http",
        "host",
        host,
        "9router at {host} uses plain HTTP, and Cuotai sends the dashboard password or session over plain HTTP only to this computer; nothing was sent. Serve 9router over HTTPS (Test connection can pin its certificate), or reach it through a loopback address such as an SSH tunnel.",
    )
}

/// `host[:port]` of a base URL: no scheme, path, user or query.
fn host_port(base: &Url) -> String {
    let host = base.host_str().unwrap_or_default();
    match base.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// The note of a LiteLLM proxy reached over plain HTTP on the private
/// network by the account's choice (the overlay chip reads it). A 9router
/// never sends a secret there, so it never carries this note.
pub(crate) fn private_http_note() -> Localized {
    Localized::fixed(
        "note.nine_router.private_http",
        "Plain HTTP on your private network, by your choice: the password or key travels unencrypted on that network. Prefer HTTPS with a pinned certificate.",
    )
}

fn redirected() -> QueryError {
    QueryError {
        diagnostic: None,
        status: Status::AuthenticationError,
        note: Localized::fixed(
            "query.nine_router.redirected",
            "9router redirected the request to its login; enter the dashboard password.",
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
        return Err(redirected());
    }
    if status.as_u16() == 401 {
        return Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.nine_router.unauthorized",
                "9router refused access: enter the dashboard password.",
            ),
        });
    }
    Err(super::http_failure(status.as_u16()))
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
                "Missing credential: {name}.",
            )
        })
}

/// PRD 0036 RF2.6: the pasted SSO `auth_token` of `account` (vault or the
/// variable `params.auth_session_env` names), if any. It is sent only as
/// the cookie of requests built from the account's own base URL.
fn sso_token(account: &Account, ctx: &Context<'_>) -> Result<Option<String>, QueryError> {
    let Some(session) =
        super::web_session(account, ctx, crate::user_session::NINE_ROUTER_SSO_SLOT)?
    else {
        return Ok(None);
    };
    Ok(session.cookie.split(';').find_map(|item| {
        item.trim()
            .strip_prefix("auth_token=")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }))
}

/// The credentials of a query: an SSO session wins over the password (and
/// is never refreshed by a login); otherwise the password, if any.
async fn start(
    instance: &Instance<'_>,
    account: &Account,
    ctx: &Context<'_>,
    now: DateTime<Utc>,
) -> Result<(Option<String>, Option<String>, bool), QueryError> {
    if let Some(token) = sso_token(account, ctx)? {
        instance.secret_may_go()?;
        instance.ensure_genuine().await?;
        return Ok((Some(token), None, true));
    }
    let password = password_of(account, ctx)?;
    if password.is_some() {
        instance.secret_may_go()?;
    }
    let token = instance.token(password.as_deref(), now, false).await?;
    Ok((token, password, false))
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
        let body: ConnectionsPage = super::decode_value(&super::body_value(response).await?)?;
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

/// S-08: the connections of a listing with each id once, the first kept,
/// and how many repeats were left out. A repeated id would collide in the
/// history, the preferences and "Refresh now".
fn dedupe(connections: Vec<Connection>) -> (Vec<Connection>, usize) {
    let mut seen = std::collections::HashSet::new();
    let before = connections.len();
    let kept: Vec<Connection> = connections
        .into_iter()
        .filter(|connection| seen.insert(connection.id.clone()))
        .collect();
    let repeated = before - kept.len();
    (kept, repeated)
}

fn sub_notes(extra: Vec<Localized>) -> Vec<Localized> {
    let mut notes = vec![
        Localized::fixed(
            "note.nine_router.cached",
            "9router may show a cached reading; its age is not reported.",
        ),
        Localized::fixed(
            "note.nine_router.zero_caveat",
            "Values as normalized by 9router; a missing value may show as 0.",
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
    extra: Vec<Localized>,
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
        identity: identity_of(connection),
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
            vec![error.note],
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
    let value: Value = match super::body_value(response).await {
        Ok(value) => value,
        Err(error) => return failed(error),
    };
    match parse_usage(&value) {
        Ok(UsageOutcome::Usage(mut usage)) => {
            if usage.limit_reached && usage.metrics.is_empty() {
                // No detail but the limit is reached (RF3.3: an exhausted
                // signal): a full percentage meter, so no amount is shown
                // as "0 left" and no unknown figure appears (V-18).
                usage.metrics.push(Metric {
                    billing: Default::default(),
                    scope: None,
                    label: "Limite atingido".into(),
                    used: Some(100.0),
                    limit: Some(100.0),
                    remaining: None,
                    unit: "%".into(),
                    kind: MetricKind::Quota,
                    window: None,
                    period: None,
                    resets_at: None,
                    derived_from: None,
                    unlimited: false,
                });
            }
            let notes = usage.notes();
            sub_report(
                connection,
                ordinal,
                now,
                Status::Ready,
                usage.metrics,
                usage.plan,
                notes,
            )
        }
        Ok(UsageOutcome::Message { auth_expired: true }) => failed(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "note.nine_router.reconnect",
                "Reconnect this account in 9router.",
            ),
        }),
        Ok(UsageOutcome::Message {
            auth_expired: false,
        }) => failed(QueryError::unsupported(
            "note.nine_router.no_usage",
            "9router has no usage for this connection.",
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
            vec![Localized::fixed("note.nine_router.off", "Off in 9router.")],
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
            vec![QueryError::timeout().note],
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
                "Reconnect this account in 9router.",
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
                        "The connection no longer exists in 9router.",
                    ),
                }
            } else {
                error
            }
        }
    }
}

/// The connections `account` shows (its `include`, `exclude` and `hidden`
/// parameters), each with its 1-based ordinal among them.
fn select<'c>(account: &Account, connections: &'c [Connection]) -> Vec<(usize, &'c Connection)> {
    let include = list_param(account, "include");
    let exclude = list_param(account, "exclude");
    let hidden = list_param(account, "hidden");
    connections
        .iter()
        .filter(|connection| {
            let slug = connection.provider.to_ascii_lowercase();
            (include.is_empty() || include.contains(&slug))
                && !exclude.contains(&slug)
                && !hidden.contains(&sub_id(&connection.id))
        })
        .enumerate()
        .map(|(index, connection)| (index + 1, connection))
        .collect()
}

/// PRD 0036 RF4.4: "Refresh now" on one connection: the connection list
/// (to find it; the raw id is never kept) and that connection's usage
/// only, never `force=1`. `None` when the connection is gone or hidden.
pub(crate) async fn fetch_connection(
    account: &Account,
    ctx: &Context<'_>,
    wanted: &str,
) -> Result<Option<SubReport>, QueryError> {
    fetch_connection_at(account, ctx, wanted, Limits::DEFAULT, Utc::now()).await
}

pub(crate) async fn fetch_connection_at(
    account: &Account,
    ctx: &Context<'_>,
    wanted: &str,
    limits: Limits,
    now: DateTime<Utc>,
) -> Result<Option<SubReport>, QueryError> {
    let base = account_base(account).map_err(base_error)?;
    let trust = tls::mode(account, &base).map_err(tls_error)?;
    let own = tls::client(&trust, ctx.timeout, &base)?;
    let (http, flag) = match &own {
        Some((client, flag)) => (client, flag.clone()),
        None => (ctx.client_for(&base), None),
    };
    let instance = Instance::new(http, base, limits, ctx.timeout).with_pin_flag(flag);
    let deadline = tokio::time::Instant::now() + limits.instance;
    let (mut token, password, _) = start(&instance, account, ctx, now).await?;
    let (connections, _) = discover(&instance, &mut token, password.as_deref(), now).await?;
    let (connections, _) = dedupe(connections);
    let Some((ordinal, connection)) = select(account, &connections)
        .into_iter()
        .find(|(_, connection)| sub_id(&connection.id) == wanted)
    else {
        return Ok(None);
    };
    let (_, report) = read_one(
        &instance,
        connection,
        ordinal,
        token.as_deref(),
        now,
        deadline,
    )
    .await;
    Ok(Some(report))
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
    fetch_with(account, ctx, limits, now, &local_listener::System).await
}

/// [`fetch_at`] with an injected listener check (tests).
pub(crate) async fn fetch_with(
    account: &Account,
    ctx: &Context<'_>,
    limits: Limits,
    now: DateTime<Utc>,
    listener: &dyn ListenerCheck,
) -> Result<Data, QueryError> {
    let base = account_base(account).map_err(base_error)?;
    let trust = tls::mode(account, &base).map_err(tls_error)?;
    let own = tls::client(&trust, ctx.timeout, &base)?;
    let (http, flag) = match &own {
        Some((client, flag)) => (client, flag.clone()),
        None => (ctx.client_for(&base), None),
    };
    let instance = Instance::new(http, base, limits, ctx.timeout)
        .with_pin_flag(flag)
        .with_listener(listener);
    let deadline = tokio::time::Instant::now() + limits.instance;

    let (mut token, password, sso) = start(&instance, account, ctx, now).await?;
    let (connections, truncated) =
        discover(&instance, &mut token, password.as_deref(), now).await?;
    let (connections, repeated) = dedupe(connections);

    let selected = select(account, &connections);

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
    let mut sub_reports: Vec<SubReport> = readings.into_iter().map(|(_, report)| report).collect();
    let capped = aggregators::cap_instance_metrics(&mut sub_reports);

    let mut notes = Notes::new();
    let ready = sub_reports
        .iter()
        .filter(|sub| sub.status == Status::Ready)
        .count();
    notes.push_note(Localized::plural(
        "note.nine_router.summary",
        params([
            ("ready", ParamValue::from(ready as u64)),
            ("count", ParamValue::from(sub_reports.len() as u64)),
        ]),
        "{ready} of {count} 9router connection with usage.",
        "{ready} of {count} 9router connections with usage.",
    ));
    let seen = instance
        .version_seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    let version = match seen {
        Some(version) => Some(version),
        None => best_effort_version(&instance, token.as_deref()).await,
    };
    if let Some(version) = version {
        notes.push_params(
            "note.nine_router.version",
            params([("version", ParamValue::from(version.clone()))]),
            "9router {version}.",
        );
        if let Some(note) = untested_version_note(&version, TESTED_MIN) {
            notes.push_note(note);
        }
    }
    if truncated {
        notes.push(
            "note.nine_router.truncated",
            "9router has more connections than the limit read (1000); the rest were left out.",
        );
    }
    if repeated > 0 {
        notes.push_note(aggregators::duplicate_ids_note(repeated));
    }
    if capped {
        notes.push_note(aggregators::instance_truncated_note());
    }
    if password.is_none() && !sso && !base_is_loopback(&instance.base) {
        notes.push(
            "note.nine_router.exposed",
            "This 9router instance answers management requests without a password. Anyone who can reach it can read and change its connections.",
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
    read_version(response).await
}

/// `currentVersion` of a `GET /api/version` answer, cleaned, or nothing.
async fn read_version(response: Response) -> Option<String> {
    let value: Value = super::read_json(response).await.ok()?;
    let version = clean_text(value.get("currentVersion")?.as_str()?, 32);
    (!version.is_empty()
        && version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+')))
    .then_some(version)
}

/// The numeric parts of a version and whether it is a release
/// ("v0.5.109-beta" → `([0, 5, 109], false)`; build metadata after `+` is
/// ignored), or `None` when it does not start with a number.
fn version_parts(version: &str) -> Option<(Vec<u64>, bool)> {
    let core = version.trim().trim_start_matches(['v', 'V']);
    let core = core.split('+').next().unwrap_or_default();
    let (numbers, pre) = match core.split_once('-') {
        Some((numbers, _)) => (numbers, true),
        None => (core, false),
    };
    let parts: Vec<u64> = numbers
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (!parts.is_empty()).then_some((parts, !pre))
}

/// Re-validation S-01: a version the health-first exchange accepts: it
/// parses, and its major is below 1000 (a plausibility bound, not a block;
/// an old version stays a warning).
fn plausible_version(version: &str) -> bool {
    version_parts(version).is_some_and(|(parts, _)| parts[0] < 1000)
}

/// PRD 0036 RF2.2/RF3.8: true when `version` is older than `min`. A
/// pre-release sorts before its release (B-a: `0.5.109-beta` < `0.5.109`).
/// An unparsable version is never called older (no warning without
/// evidence), and neither is any version while `min` is `None`.
pub(crate) fn below_tested_min(version: &str, min: Option<&str>) -> bool {
    let (Some((have, have_release)), Some((want, want_release))) =
        (version_parts(version), min.and_then(version_parts))
    else {
        return false;
    };
    let len = have.len().max(want.len());
    let pad = |parts: &[u64]| -> Vec<u64> {
        (0..len)
            .map(|i| parts.get(i).copied().unwrap_or(0))
            .collect()
    };
    (pad(&have), have_release) < (pad(&want), want_release)
}

/// The reading's warning for an instance older than `min` (RF3.8).
fn untested_version_note(version: &str, min: Option<&str>) -> Option<Localized> {
    let min = min.filter(|_| below_tested_min(version, min))?;
    Some(Localized::with_params(
        "note.nine_router.version_untested",
        params([
            ("version", ParamValue::from(version.to_owned())),
            ("min", ParamValue::from(min.to_owned())),
        ]),
        "9router {version} is older than {min}, the oldest version Cuotai was tested with; some readings may fail until you update it.",
    ))
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
    /// Re-validation V-03: the account names the variable `name` for the
    /// password, and it is not set; nothing was sent.
    VariableUnset { name: String },
    /// PRD 0036 RF2.6: the pasted SSO session is missing its `auth_token`,
    /// expired or was revoked.
    SessionRejected,
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
    /// Re-validation S-01: the program listening on this loopback `port`
    /// could not be confirmed as the current user's (or the superuser's);
    /// the password or session was not sent.
    ListenerForeign { port: u16 },
    /// Owner decision of 2026-11-20: a password or session is set and the
    /// instance is a private-network host over plain HTTP (`host` is
    /// `host[:port]`); nothing was sent.
    SecretOverPlainHttp { host: String },
}

/// PRD 0036 RF10.3: the sentence of a LiteLLM proxy test, which never
/// names 9router; `None` for the outcomes whose 9router text is generic.
pub(crate) fn litellm_outcome_text(outcome: &TestOutcome) -> Option<Localized> {
    Some(match outcome {
        TestOutcome::Connected { connections, .. } => Localized::plural(
            "test.litellm.connected",
            params([("count", ParamValue::from(*connections as u64))]),
            "LiteLLM: {count} provider with a budget found.",
            "LiteLLM: {count} providers with a budget found.",
        ),
        TestOutcome::NotNineRouter => Localized::fixed(
            "test.litellm.not_litellm",
            "The address answers but does not look like a LiteLLM proxy (no /provider/budgets).",
        ),
        TestOutcome::PasswordWrong => {
            Localized::fixed("test.litellm.key_wrong", "The LiteLLM key was refused.")
        }
        TestOutcome::PasswordRequired => Localized::fixed(
            "test.litellm.key_required",
            "Enter the LiteLLM master key or a virtual key.",
        ),
        TestOutcome::ListenerForeign { port } => super::litellm::listener_outcome_text(*port),
        _ => return None,
    })
}

/// The localized sentence of an outcome (no URL, no body).
pub(crate) fn outcome_text(outcome: &TestOutcome) -> Localized {
    outcome_text_with_min(outcome, TESTED_MIN)
}

/// [`outcome_text`] against an explicit tested minimum (RF2.2: a version
/// below it is a warning, never a block).
fn outcome_text_with_min(outcome: &TestOutcome, min: Option<&str>) -> Localized {
    match outcome {
        TestOutcome::Connected {
            version: Some(version),
            connections,
        } if below_tested_min(version, min) => Localized::plural(
            "test.nine_router.connected_untested",
            params([
                ("count", ParamValue::from(*connections as u64)),
                ("version", ParamValue::from(version.clone())),
                ("min", ParamValue::from(min.unwrap_or_default().to_owned())),
            ]),
            "9router {version}: {count} connection with usage found. This version is older than {min}, the oldest one Cuotai was tested with; some readings may fail.",
            "9router {version}: {count} connections with usage found. This version is older than {min}, the oldest one Cuotai was tested with; some readings may fail.",
        ),
        TestOutcome::Connected {
            version,
            connections,
        } => {
            let version = version.clone().unwrap_or_default();
            Localized::plural(
                "test.nine_router.connected",
                params([
                    ("count", ParamValue::from(*connections as u64)),
                    ("version", ParamValue::from(version.clone())),
                ]),
                "9router {version}: {count} connection with usage found.",
                "9router {version}: {count} connections with usage found.",
            )
        }
        TestOutcome::NotNineRouter => Localized::fixed(
            "test.nine_router.not_nine_router",
            "The address answers but does not look like a 9router.",
        ),
        TestOutcome::PasswordWrong => Localized::fixed(
            "query.nine_router.password_wrong",
            "The 9router dashboard password was refused.",
        ),
        TestOutcome::LockedOut { seconds } => Localized::with_params(
            "query.nine_router.locked",
            params([("seconds", ParamValue::from(*seconds))]),
            "Too many 9router login attempts; wait {seconds} s before trying again.",
        ),
        TestOutcome::LoginRefused => Localized::fixed(
            "query.nine_router.login_refused",
            "9router does not accept password login here (SSO is on or the tunnel blocks access).",
        ),
        TestOutcome::PasswordRequired => Localized::fixed(
            "test.nine_router.password_required",
            "This instance requires the dashboard password; choose \"Dashboard password\".",
        ),
        TestOutcome::VariableUnset { name } => Localized::with_param(
            "test.nine_router.variable_unset",
            "name",
            name.as_str(),
            "The variable {name} is not set, so no password was sent. Set it, or leave the password empty if the dashboard login is off.",
        ),
        TestOutcome::SessionRejected => crate::user_session::rejected(),
        TestOutcome::Unreachable => Localized::fixed(
            "test.nine_router.unreachable",
            "The address could not be reached.",
        ),
        TestOutcome::UntrustedCertificate { fingerprint } => Localized::with_param(
            "test.nine_router.untrusted_certificate",
            "fingerprint",
            fingerprint.as_str(),
            "This computer does not trust the instance's certificate. SHA-256 fingerprint: {fingerprint}. Check it on the server before trusting it.",
        ),
        TestOutcome::CertificateChanged { fingerprint } => Localized::with_param(
            "test.nine_router.certificate_changed",
            "fingerprint",
            fingerprint.as_str(),
            "The instance's certificate changed and no longer matches the pinned one. New SHA-256 fingerprint: {fingerprint}. Check it on the server before trusting it.",
        ),
        TestOutcome::CaFileUnreadable => Localized::fixed(
            "query.nine_router.ca_file",
            "The 9router CA file could not be read as PEM.",
        ),
        TestOutcome::ListenerForeign { port } => Localized::with_param(
            "test.nine_router.listener_foreign",
            "port",
            *port,
            "The program listening on port {port} could not be confirmed as yours (it may belong to another user); the password or session was not sent. Start 9router as yourself, or reach it over HTTPS with a pinned certificate.",
        ),
        TestOutcome::SecretOverPlainHttp { host } => secret_plain_http_note(host),
    }
}

/// PRD 0036 RF2.3: after a transport failure of the health check over
/// HTTPS, tell a certificate problem from an unreachable address: the
/// capture verifier completes a handshake (and reads the certificate's
/// fingerprint) where the real one failed. Nothing but the public health
/// route is requested.
async fn handshake_outcome(instance: &Instance<'_>, trust: &tls::TlsMode) -> TestOutcome {
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
            "The 9router certificate fingerprint must have 64 hexadecimal digits (SHA-256).",
        ),
        tls::TlsError::Conflict => QueryError::config(
            "query.nine_router.tls_conflict",
            "Use a CA file or a pinned fingerprint, not both, and only with HTTPS.",
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
    test_connection_with(account, ctx, limits, now, &local_listener::System).await
}

/// [`test_connection_at`] with an injected listener check (tests).
pub(crate) async fn test_connection_with(
    account: &Account,
    ctx: &Context<'_>,
    limits: Limits,
    now: DateTime<Utc>,
    listener: &dyn ListenerCheck,
) -> TestOutcome {
    let Ok(base) = account_base(account) else {
        return TestOutcome::Unreachable;
    };
    let sso = match sso_token(account, ctx) {
        Ok(sso) => sso,
        Err(_) => return TestOutcome::SessionRejected,
    };
    let password = if sso.is_some() {
        None
    } else {
        let Ok(password) = password_of(account, ctx) else {
            // Re-validation V-03: a named variable that is not set is not
            // "the instance requires a password".
            return TestOutcome::VariableUnset {
                name: account
                    .credential_env
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_owned(),
            };
        };
        password
    };
    let Ok(trust) = tls::mode(account, &base) else {
        return TestOutcome::Unreachable;
    };
    let own = match tls::client(&trust, ctx.timeout, &base) {
        Ok(own) => own,
        Err(_) => return TestOutcome::CaFileUnreadable,
    };
    let (http, flag) = match &own {
        Some((client, flag)) => (client, flag.clone()),
        None => (ctx.client_for(&base), None),
    };
    let port = base.port_or_known_default().unwrap_or(80);
    let instance = Instance::new(http, base, limits, ctx.timeout)
        .with_pin_flag(flag)
        .with_listener(listener);
    if (sso.is_some() || password.is_some()) && instance.secret_may_go().is_err() {
        return TestOutcome::SecretOverPlainHttp {
            host: host_port(&instance.base),
        };
    }
    let health = match instance.request(&Route::Health, None) {
        Ok(request) => match request.send().await {
            Ok(response) if response.status().is_success() => {
                super::read_json::<Value>(response).await.ok()
            }
            // Re-validation V-19: the address answers, but not like a
            // 9router (a wrong prefix answers 404).
            Ok(_) => return TestOutcome::NotNineRouter,
            Err(_) => return handshake_outcome(&instance, &trust).await,
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
    // Re-validation S-01: a password or session goes only to an address
    // that answered both public routes like a 9router.
    if sso.is_some() || password.is_some() {
        if !version.as_deref().is_some_and(plausible_version) {
            return TestOutcome::NotNineRouter;
        }
        instance.mark_verified();
    }
    let login = match &sso {
        Some(token) => Ok(Some(token.clone())),
        None => instance.token(password.as_deref(), now, false).await,
    };
    let mut token = match login {
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
                "query.nine_router.listener_foreign" => TestOutcome::ListenerForeign { port },
                "query.nine_router.not_nine_router" => TestOutcome::NotNineRouter,
                _ => TestOutcome::Unreachable,
            };
        }
    };
    // Re-validation R-02: every page, each connection once, and only the
    // connections that are on, as a refresh would read them.
    let relogin = if sso.is_some() {
        None
    } else {
        password.as_deref()
    };
    match discover(&instance, &mut token, relogin, now).await {
        Ok((connections, _)) => TestOutcome::Connected {
            version,
            connections: dedupe(connections)
                .0
                .iter()
                .filter(|connection| connection.is_active != Some(false))
                .count(),
        },
        Err(error) if error.note.id == "query.nine_router.listener_foreign" => {
            TestOutcome::ListenerForeign { port }
        }
        Err(error) if error.note.id == QueryError::schema().note.id => TestOutcome::NotNineRouter,
        Err(error) if error.status == Status::AuthenticationError => {
            if sso.is_some() {
                TestOutcome::SessionRejected
            } else if password.is_none() {
                TestOutcome::PasswordRequired
            } else {
                TestOutcome::PasswordWrong
            }
        }
        Err(_) => TestOutcome::Unreachable,
    }
}

/// PRD 0028 RF11: the fixture whose metric identities `label_snapshot`
/// records. Add a case when the adapter learns a new metric.
#[cfg(test)]
pub(super) fn label_fixtures() -> Vec<(&'static str, Vec<Metric>)> {
    let body = serde_json::json!({
        "plan": "Pro",
        "quotas": {
            "Session": {
                "total": 100, "used": 10, "remaining": 90, "resetAt": "2026-10-01T00:00:00Z"
            },
            // Re-validation V-10: the window labels 9router writes.
            "session (5h)": {"total": 100, "used": 20, "remainingPercentage": 80},
            "weekly (7d)": {"total": 100, "used": 30, "remainingPercentage": 70},
            "weekly Opus (7d)": {"total": 100, "used": 5, "remainingPercentage": 95},
            "weekly Sonnet (7d)": {"total": 100, "used": 5, "remainingPercentage": 95},
            "weekly Gemini (7d)": {"total": 100, "used": 5, "remainingPercentage": 95}
        }
    });
    let metrics = match parse_usage(&body).expect("fixture decodes") {
        UsageOutcome::Usage(usage) => usage.metrics,
        UsageOutcome::Message { .. } => Vec::new(),
    };
    vec![("nine_router", metrics)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use serde_json::json;

    /// PRD 0036 RF2.2/RF3.8: the version-below-minimum warning compares
    /// numeric parts, stays silent without evidence and while `TESTED_MIN`
    /// is unset (until the owner's verification T2 fills it).
    #[test]
    fn the_untested_version_warning_needs_a_tested_minimum() {
        assert!(below_tested_min("0.5.35", Some("0.5.109")));
        assert!(below_tested_min("v0.4", Some("0.5.0")));
        assert!(below_tested_min("0.5.108-beta.1", Some("0.5.109")));
        assert!(!below_tested_min("0.5.109", Some("0.5.109")));
        assert!(!below_tested_min("0.5.109+build", Some("0.5.109")));
        assert!(!below_tested_min("0.6", Some("0.5.109")));
        assert!(!below_tested_min("1.0.0", Some("0.5.109")));
        assert!(!below_tested_min("main", Some("0.5.109")), "unparsable");
        assert!(
            !below_tested_min("0.1.0", Some("next")),
            "unparsable minimum"
        );
        assert!(!below_tested_min("0.1.0", None));
        // B-a: a pre-release sorts before its release; build metadata does
        // not count.
        assert!(below_tested_min("0.5.109-beta", Some("0.5.109")));
        assert!(below_tested_min("v0.5.109-rc.1", Some("0.5.109")));
        assert!(!below_tested_min("0.5.109", Some("0.5.109-beta")));
        assert!(!below_tested_min("0.5.109-beta", Some("0.5.109-beta")));
        assert!(!below_tested_min("0.5.110-beta", Some("0.5.109")));
        // The health-first plausibility bound.
        assert!(plausible_version("0.5.109"));
        assert!(plausible_version("v999.0.0-beta"));
        assert!(!plausible_version("1000.0.0"));
        assert!(!plausible_version("garbage"));
        assert!(!plausible_version(""));
        let note = untested_version_note("0.5.35", Some("0.5.109")).unwrap();
        assert_eq!(note.id, "note.nine_router.version_untested");
        assert!(note
            .resolve("en-US")
            .contains("0.5.35 is older than 0.5.109"));
        assert!(untested_version_note("0.5.109", Some("0.5.109")).is_none());
        if TESTED_MIN.is_none() {
            assert!(untested_version_note("0.0.1", TESTED_MIN).is_none());
        }
    }

    /// 9router normalizes a missing value to 0, and without a total the
    /// other fields may be in another unit: on a percentage meter only the
    /// percentage counts.
    #[test]
    fn a_percentage_meter_ignores_values_in_other_units() {
        for quota in [
            json!({ "total": 0, "used": 0, "remaining": 0, "remainingPercentage": 40 }),
            json!({ "used": 5000, "remainingPercentage": 40 }),
            json!({ "remaining": 7, "remainingPercentage": 40 }),
        ] {
            let UsageOutcome::Usage(usage) =
                parse_usage(&json!({ "quotas": { "weekly": quota } })).expect("decodes")
            else {
                panic!("usage expected");
            };
            let metric = &usage.metrics[0];
            assert_eq!(metric.unit, "%");
            assert_eq!(metric.limit, Some(100.0));
            assert_eq!(metric.used, Some(60.0));
            assert_eq!(metric.remaining, Some(40.0));
        }
    }
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

    /// `env_set`: the account names the test password variable (`credential_env`).
    fn account(server: &MockServer, env_set: bool, extra: &[(&str, &str)]) -> Account {
        let mut params = BTreeMap::from([("base_url".to_string(), server.uri())]);
        params.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        Account {
            name: "home".into(),
            provider: Provider::NineRouter,
            credential_env: env_set.then(|| "TEST_NINE_ROUTER_PASSWORD".into()),
            params,
            ..Account::default()
        }
    }

    fn context(http: &reqwest::Client) -> Context<'_> {
        Context {
            http,
            direct: http,
            credentials: &Env,
            timeout: Duration::from_secs(5),
        }
    }

    fn jwt(exp: i64) -> String {
        use base64::Engine as _;
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
            Err(crate::ConfigError::BaseUrl { .. })
        ));
        assert!(matches!(
            validate(&[("base_url", "https://u:p@example.com")]),
            Err(crate::ConfigError::BaseUrl { .. })
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

    /// Re-validation S-08: the explicit figure rule. A negative or absurd
    /// figure, or a given remaining above the cap, is left absent (never 0)
    /// with a note naming the aggregator; over quota is real and kept.
    #[test]
    fn impossible_figures_are_left_absent_with_a_note() {
        let ids = |usage: &ParsedUsage| -> Vec<String> {
            usage
                .notes()
                .iter()
                .map(|note| note.id.to_string())
                .collect()
        };
        let usage = parsed(json!({ "quotas": { "q": { "used": 150, "total": 100 } } }));
        let metric = &usage.metrics[0];
        assert_eq!((metric.used, metric.limit), (Some(150.0), Some(100.0)));
        assert!(ids(&usage).is_empty());

        let usage =
            parsed(json!({ "quotas": { "q": { "used": 10, "remaining": -5, "total": 100 } } }));
        let metric = &usage.metrics[0];
        assert_eq!(
            (metric.used, metric.limit, metric.remaining),
            (Some(10.0), Some(100.0), None)
        );
        assert_eq!(ids(&usage), ["note.aggregator.figure_refused"]);
        let note = &usage.notes()[0];
        assert!(note.fallback.contains("9router"), "{}", note.fallback);
        assert!(!note.resolve("de").contains("{aggregator}"));

        // The security run's figures: nothing of them is kept.
        let usage = parsed(
            json!({ "quotas": { "q": { "used": 1e308, "remaining": -5, "total": 1e308 } } }),
        );
        assert!(usage.metrics.is_empty());
        assert!(usage.refused);

        // A given remaining above the cap; a negative used.
        let usage =
            parsed(json!({ "quotas": { "q": { "used": 5, "remaining": 120, "total": 100 } } }));
        assert_eq!(usage.metrics[0].remaining, None);
        assert!(usage.refused);
        let usage =
            parsed(json!({ "quotas": { "q": { "used": -1, "remaining": 40, "total": 100 } } }));
        assert_eq!(
            (usage.metrics[0].used, usage.metrics[0].remaining),
            (None, Some(40.0))
        );
        assert!(usage.refused);

        // A percentage outside 0..=100 is no reading.
        for bad in [-3.0, 150.0] {
            let usage = parsed(json!({ "quotas": { "weekly": { "remainingPercentage": bad } } }));
            assert!(usage.metrics.is_empty(), "{bad}");
            assert!(usage.refused, "{bad}");
        }
    }

    /// Re-validation S-04: one answer keeps at most 64 limits, and says so.
    #[test]
    fn a_connection_keeps_at_most_64_limits() {
        let mut quotas = serde_json::Map::new();
        for n in 0..70 {
            quotas.insert(format!("q{n:03}"), json!({ "used": n, "total": 100 }));
        }
        let usage = parsed(json!({ "quotas": quotas }));
        assert_eq!(
            usage.metrics.len(),
            crate::aggregators::MAX_METRICS_PER_CONNECTION
        );
        assert!(usage.truncated);
        let ids: Vec<String> = usage
            .notes()
            .iter()
            .map(|note| note.id.to_string())
            .collect();
        assert_eq!(ids, ["note.aggregator.metrics_truncated"]);

        let mut quotas = serde_json::Map::new();
        for n in 0..64 {
            quotas.insert(format!("q{n:03}"), json!({ "used": n, "total": 100 }));
        }
        let usage = parsed(json!({ "quotas": quotas }));
        assert_eq!(usage.metrics.len(), 64);
        assert!(usage.notes().is_empty());
    }

    /// Re-validation V-16: a cut label ends with an ellipsis, within the
    /// same bound; a short one is unchanged.
    #[test]
    fn a_cut_label_ends_with_an_ellipsis() {
        let long = "Very long connection label for the shared Claude Max subscription of the team";
        let cut = clean_text(long, 60);
        assert_eq!(cut.chars().count(), 60);
        assert!(cut.ends_with('…'), "{cut}");
        assert!(cut.starts_with("Very long connection label"));
        assert_eq!(clean_text("  work\u{7}  ", 60), "work");
        assert_eq!(clean_text(&"x".repeat(60), 60), "x".repeat(60));
        // No space before the ellipsis.
        assert_eq!(clean_text("abcd efgh", 6), "abcd…");
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
            github_email: None,
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
        // Every page repeats the same id: it is listed once (S-08).
        assert_eq!(data.sub_reports.len(), 1);
        assert!(data
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.nine_router.truncated"));
        assert!(data.notes_l10n.iter().any(|n| {
            n.id == "note.aggregator.duplicate_ids"
                && n.params.get("count") == Some(&ParamValue::from((MAX_PAGES - 1).to_string()))
        }));
    }

    #[tokio::test]
    async fn one_connection_refresh_reads_only_its_usage() {
        let server = MockServer::start().await;
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
        for id in ["a", "b"] {
            mount_json(&server, &format!("/api/usage/{id}"), claude_usage()).await;
        }
        let http = client();
        let acct = account(&server, false, &[]);
        let one = fetch_connection_at(
            &acct,
            &context(&http),
            &sub_id("b"),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(one.label, "B");
        assert_eq!(one.id, sub_id("b"));
        // A hidden or unknown connection is not read.
        let hidden = account(&server, false, &[("hidden", sub_id("a").as_str())]);
        assert!(fetch_connection_at(
            &hidden,
            &context(&http),
            &sub_id("a"),
            Limits::DEFAULT,
            Utc::now()
        )
        .await
        .unwrap()
        .is_none());
        let requests = requests_of(&server.received_requests().await.unwrap());
        let count = |target: &str| requests.iter().filter(|(_, t)| t == target).count();
        assert_eq!(count("/api/usage/b"), 1);
        assert_eq!(count("/api/usage/a"), 0);
        assert!(requests.iter().all(|(_, t)| !t.contains("force")));
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
        // limitReached without detail: exhausted, a full meter with no
        // made-up remaining (V-18).
        let limit = by("Limit");
        assert_eq!(limit.status, Status::Ready);
        assert_eq!(limit.metrics[0].remaining, None);
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

    /// The two public routes a genuine instance answers (re-validation
    /// S-01: checked before any secret).
    async fn mount_genuine(server: &MockServer) {
        mount_json(server, "/api/health", json!({ "ok": true })).await;
        mount_json(
            server,
            "/api/version",
            json!({ "currentVersion": "0.5.109" }),
        )
        .await;
    }

    async fn mount_login(server: &MockServer, response: ResponseTemplate, expect: u64) {
        // A dropped test server's port can be reused by the next one: start
        // from a clean login state for this base URL.
        forget_login(&format!("{}/", server.uri())).await;
        mount_genuine(server).await;
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
        // RF2.2/RF3.8: below the tested minimum it still connects, with a
        // warning in the sentence; at or above it, the plain sentence.
        let old = outcome_text_with_min(&outcome, Some("0.6.0"));
        assert_eq!(old.id, "test.nine_router.connected_untested");
        assert!(
            old.resolve("en-US").contains("older than 0.6.0"),
            "the untested sentence names the tested minimum"
        );
        assert_eq!(
            outcome_text_with_min(&outcome, Some("0.5.109")).id,
            "test.nine_router.connected"
        );

        // Not a 9router.
        let server = MockServer::start().await;
        mount_json(&server, "/api/health", json!({ "status": "fine" })).await;
        assert_eq!(
            run(account(&server, false, &[])).await,
            TestOutcome::NotNineRouter
        );
        // Re-validation V-19: an address that answers 404 on the health
        // route (a wrong prefix) is "not a 9router", not unreachable.
        let server = MockServer::start().await;
        assert_eq!(
            run(account(&server, false, &[])).await,
            TestOutcome::NotNineRouter
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

        // Nothing listening (wiremock keeps a dropped server's port
        // answering 404, so a port freed by the OS stands in for it).
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let server = MockServer::start().await;
        let mut acct = account(&server, false, &[]);
        acct.params
            .insert("base_url".into(), format!("http://127.0.0.1:{port}"));
        assert_eq!(run(acct).await, TestOutcome::Unreachable);
    }

    /// Re-validation V-03: a named password variable that is not set has
    /// its own outcome, naming the variable, and nothing is sent.
    #[tokio::test]
    async fn an_unset_password_variable_is_named_and_nothing_is_sent() {
        let server = MockServer::start().await;
        mount_json(&server, "/api/health", json!({ "ok": true })).await;
        let http = client();
        let mut acct = account(&server, false, &[]);
        acct.credential_env = Some("UNSET_NINE_ROUTER_VARIABLE".into());
        let outcome = test_connection_at(&acct, &context(&http), Limits::DEFAULT, Utc::now()).await;
        assert_eq!(
            outcome,
            TestOutcome::VariableUnset {
                name: "UNSET_NINE_ROUTER_VARIABLE".into()
            }
        );
        let text = outcome_text(&outcome);
        assert_eq!(text.id, "test.nine_router.variable_unset");
        for locale in std::iter::once("en-US").chain(crate::text::catalog_locales()) {
            assert!(
                text.resolve(locale).contains("UNSET_NINE_ROUTER_VARIABLE"),
                "{locale}"
            );
        }
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    /// Re-validation R-02: Test connection counts the connections of every
    /// page, each id once, and only those that are on.
    #[tokio::test]
    async fn test_connection_counts_every_page_once_and_only_active_connections() {
        let server = MockServer::start().await;
        mount_json(&server, "/api/health", json!({ "ok": true })).await;
        mount_json(
            &server,
            "/api/version",
            json!({ "currentVersion": "0.5.109" }),
        )
        .await;
        let mut off = connection("c", "codex", "C");
        off["isActive"] = json!(false);
        for (number, body) in [
            (1, json!([connection("a", "claude", "A"), off])),
            (
                2,
                json!([connection("b", "glm", "B"), connection("a", "claude", "A")]),
            ),
        ] {
            Mock::given(method("GET"))
                .and(path("/api/providers/client"))
                .and(query_param("page", number.to_string()))
                .respond_with(ResponseTemplate::new(200).set_body_json(page(body, 2)))
                .mount(&server)
                .await;
        }
        let http = client();
        assert_eq!(
            test_connection_at(
                &account(&server, false, &[]),
                &context(&http),
                Limits::DEFAULT,
                Utc::now()
            )
            .await,
            TestOutcome::Connected {
                version: Some("0.5.109".into()),
                connections: 2
            }
        );
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

    // --- re-validation S-01, S-07, R-01 ------------------------------------

    /// A listener check with a fixed answer.
    struct FixedListener(Verdict);
    impl ListenerCheck for FixedListener {
        fn verdict(&self, _: &[std::net::IpAddr], _: u16) -> Verdict {
            self.0
        }
    }

    /// The pasted SSO session of `TEST_NR_SSO`.
    struct SsoOnly(String);
    impl Credentials for SsoOnly {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_NR_SSO").then(|| self.0.clone())
        }
    }

    fn sso_account(server: &MockServer) -> Account {
        account(
            server,
            false,
            &[(crate::user_session::NINE_ROUTER_SSO_SLOT, "TEST_NR_SSO")],
        )
    }

    async fn mount_refused(server: &MockServer) {
        // Nothing that carries a secret may be requested.
        for (verb, route) in [
            ("POST", "/api/auth/login"),
            ("GET", "/api/providers/client"),
        ] {
            Mock::given(method(verb))
                .and(path(route))
                .respond_with(ResponseTemplate::new(500))
                .expect(0)
                .mount(server)
                .await;
        }
    }

    /// S-01 part 1 (R-01 item 6): no login and no cookie before the address
    /// answers `{ok:true}` and a plausible version.
    #[tokio::test]
    async fn no_password_or_session_goes_out_before_the_instance_answers_like_a_9router() {
        let token = jwt(Utc::now().timestamp() + 3_600);
        let http = client();
        for (health, version) in [
            (
                Some(json!({ "status": "fine" })),
                Some(json!({ "currentVersion": "0.5.109" })),
            ),
            (None, Some(json!({ "currentVersion": "0.5.109" }))),
            (Some(json!({ "ok": true })), None),
            (
                Some(json!({ "ok": true })),
                Some(json!({ "currentVersion": "nightly" })),
            ),
            (
                Some(json!({ "ok": true })),
                Some(json!({ "currentVersion": "4000.1.0" })),
            ),
        ] {
            for sso in [false, true] {
                let server = MockServer::start().await;
                forget_login(&format!("{}/", server.uri())).await;
                if let Some(body) = &health {
                    mount_json(&server, "/api/health", body.clone()).await;
                }
                if let Some(body) = &version {
                    mount_json(&server, "/api/version", body.clone()).await;
                }
                mount_refused(&server).await;
                let error = if sso {
                    let credentials = SsoOnly(format!("auth_token={token}"));
                    let ctx = Context {
                        http: &http,
                        direct: &http,
                        credentials: &credentials,
                        timeout: Duration::from_secs(5),
                    };
                    fetch_at(&sso_account(&server), &ctx, Limits::DEFAULT, Utc::now()).await
                } else {
                    fetch_at(
                        &account(&server, true, &[]),
                        &context(&http),
                        Limits::DEFAULT,
                        Utc::now(),
                    )
                    .await
                }
                .unwrap_err();
                assert_eq!(error.note.id, "query.nine_router.not_nine_router");
                for request in server.received_requests().await.unwrap() {
                    assert!(request.headers.get("cookie").is_none());
                    assert_eq!(request.method.to_string(), "GET");
                }
            }
        }
    }

    /// S-01 part 1: health and version come first, once per process; a
    /// refresh holding a valid token adds no request.
    #[tokio::test]
    async fn the_health_first_exchange_runs_once_before_the_first_login() {
        let server = MockServer::start().await;
        let token = jwt(Utc::now().timestamp() + 86_400);
        mount_login(
            &server,
            ResponseTemplate::new(200).insert_header("set-cookie", format!("auth_token={token}")),
            1,
        )
        .await;
        mount_json(&server, "/api/providers/client", page(json!([]), 1)).await;
        let http = client();
        let acct = account(&server, true, &[]);
        let data = fetch_at(&acct, &context(&http), Limits::DEFAULT, Utc::now())
            .await
            .unwrap();
        // The version the exchange read is reused by the reading.
        assert!(data
            .notes_l10n
            .iter()
            .any(|note| note.id == "note.nine_router.version"));
        let first = requests_of(&server.received_requests().await.unwrap());
        assert_eq!(
            first[..3],
            [
                ("GET".to_string(), "/api/health".to_string()),
                ("GET".to_string(), "/api/version".to_string()),
                ("POST".to_string(), "/api/auth/login".to_string()),
            ]
        );
        fetch_at(&acct, &context(&http), Limits::DEFAULT, Utc::now())
            .await
            .unwrap();
        let all = requests_of(&server.received_requests().await.unwrap());
        let health = all
            .iter()
            .filter(|(_, target)| target == "/api/health")
            .count();
        assert_eq!(health, 1, "{all:?}");
    }

    /// S-01 part 2 (R-01 item 6): a listener the check refuses gets neither
    /// the password nor the session, in a refresh, a one-connection refresh
    /// and Test connection.
    #[tokio::test]
    async fn no_secret_goes_to_a_port_whose_listener_is_not_confirmed() {
        let token = jwt(Utc::now().timestamp() + 3_600);
        let http = client();
        for verdict in [Verdict::Foreign, Verdict::Unconfirmed] {
            let check = FixedListener(verdict);
            for sso in [false, true] {
                let server = MockServer::start().await;
                forget_login(&format!("{}/", server.uri())).await;
                mount_genuine(&server).await;
                mount_refused(&server).await;
                let credentials = SsoOnly(format!("auth_token={token}"));
                let ctx = if sso {
                    Context {
                        http: &http,
                        direct: &http,
                        credentials: &credentials,
                        timeout: Duration::from_secs(5),
                    }
                } else {
                    context(&http)
                };
                let acct = if sso {
                    sso_account(&server)
                } else {
                    account(&server, true, &[])
                };
                let port = server.address().port();
                let error = fetch_with(&acct, &ctx, Limits::DEFAULT, Utc::now(), &check)
                    .await
                    .unwrap_err();
                assert_eq!(error.note.id, "query.nine_router.listener_foreign");
                let text = error.note.resolve("en-US");
                assert!(text.contains(&port.to_string()), "{text}");
                assert!(text.contains("not sent"), "{text}");
                let outcome =
                    test_connection_with(&acct, &ctx, Limits::DEFAULT, Utc::now(), &check).await;
                assert_eq!(outcome, TestOutcome::ListenerForeign { port });
                assert_eq!(
                    outcome_text(&outcome).id,
                    "test.nine_router.listener_foreign"
                );
                for request in server.received_requests().await.unwrap() {
                    assert!(request.headers.get("cookie").is_none());
                    assert_eq!(request.method.to_string(), "GET");
                }
            }
        }
        // A trusted listener lets the login through (the system check passes
        // for this test process's own server on every OS).
        let server = MockServer::start().await;
        mount_login(
            &server,
            ResponseTemplate::new(200).insert_header("set-cookie", format!("auth_token={token}")),
            1,
        )
        .await;
        mount_json(&server, "/api/providers/client", page(json!([]), 1)).await;
        fetch_at(
            &account(&server, true, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap();
    }

    /// A listener check that answers `Trusted` for its first `trusted`
    /// calls and `Foreign` after them, counting every call.
    struct TurnsForeign {
        trusted: usize,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl TurnsForeign {
        fn after(trusted: usize) -> Self {
            Self {
                trusted,
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }
    impl ListenerCheck for TurnsForeign {
        fn verdict(&self, _: &[std::net::IpAddr], _: u16) -> Verdict {
            if self.calls.fetch_add(1, Ordering::SeqCst) < self.trusted {
                Verdict::Trusted
            } else {
                Verdict::Foreign
            }
        }
    }

    /// To-do 48: the listener verdict is read again before every request
    /// that carries the password or the session, never reused within a
    /// refresh: a port that turns foreign mid-refresh gets no further
    /// secret.
    #[tokio::test]
    async fn a_listener_that_turns_foreign_mid_refresh_gets_no_further_secret() {
        let token = jwt(Utc::now().timestamp() + 86_400);
        let http = client();
        let connections = json!([
            connection("a", "claude", "A"),
            connection("b", "claude", "B")
        ]);
        let usage_requests = |requests: &[Request]| {
            requests
                .iter()
                .filter(|request| request.url.path().starts_with("/api/usage/"))
                .count()
        };
        let list_requests = |requests: &[Request]| {
            requests
                .iter()
                .filter(|request| request.url.path() == "/api/providers/client")
                .count()
        };

        // Every request trusted: one verdict per secret-bearing request
        // (login, one list page, two usage reads).
        let server = MockServer::start().await;
        mount_login(
            &server,
            ResponseTemplate::new(200).insert_header("set-cookie", format!("auth_token={token}")),
            1,
        )
        .await;
        mount_json(
            &server,
            "/api/providers/client",
            page(connections.clone(), 1),
        )
        .await;
        mount_json(&server, "/api/usage/a", claude_usage()).await;
        mount_json(&server, "/api/usage/b", claude_usage()).await;
        let check = TurnsForeign::after(usize::MAX);
        let data = fetch_with(
            &account(&server, true, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
            &check,
        )
        .await
        .unwrap();
        assert!(data
            .sub_reports
            .iter()
            .all(|sub| sub.status == Status::Ready));
        assert_eq!(check.calls(), 4);

        // Foreign right after the login: the connection list (cookie) is
        // never requested.
        let server = MockServer::start().await;
        mount_login(
            &server,
            ResponseTemplate::new(200).insert_header("set-cookie", format!("auth_token={token}")),
            1,
        )
        .await;
        mount_json(
            &server,
            "/api/providers/client",
            page(connections.clone(), 1),
        )
        .await;
        let check = TurnsForeign::after(1);
        let error = fetch_with(
            &account(&server, true, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
            &check,
        )
        .await
        .unwrap_err();
        assert_eq!(error.note.id, "query.nine_router.listener_foreign");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(list_requests(&requests), 0);
        assert_eq!(usage_requests(&requests), 0);

        // Foreign after the login and the list: no usage request carries
        // the cookie, with the password or with a pasted SSO session.
        for sso in [false, true] {
            let server = MockServer::start().await;
            mount_login(
                &server,
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", format!("auth_token={token}")),
                u64::from(!sso),
            )
            .await;
            mount_json(
                &server,
                "/api/providers/client",
                page(connections.clone(), 1),
            )
            .await;
            mount_json(&server, "/api/usage/a", claude_usage()).await;
            mount_json(&server, "/api/usage/b", claude_usage()).await;
            let credentials = SsoOnly(format!("auth_token={token}"));
            let (acct, ctx, check) = if sso {
                (
                    sso_account(&server),
                    Context {
                        http: &http,
                        direct: &http,
                        credentials: &credentials,
                        timeout: Duration::from_secs(5),
                    },
                    TurnsForeign::after(1),
                )
            } else {
                (
                    account(&server, true, &[]),
                    context(&http),
                    TurnsForeign::after(2),
                )
            };
            let data = fetch_with(&acct, &ctx, Limits::DEFAULT, Utc::now(), &check)
                .await
                .unwrap();
            let requests = server.received_requests().await.unwrap();
            assert_eq!(list_requests(&requests), 1, "sso={sso}");
            assert_eq!(usage_requests(&requests), 0, "sso={sso}");
            assert!(
                data.sub_reports
                    .iter()
                    .all(|sub| sub.status != Status::Ready),
                "sso={sso}"
            );
        }
    }

    /// S-07 (R-01 item 2): a redirect at login is authentication, is not
    /// followed and is not tried again.
    #[tokio::test]
    async fn a_login_answered_with_a_redirect_is_authentication_and_is_not_followed() {
        let elsewhere = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&elsewhere)
            .await;
        let server = MockServer::start().await;
        mount_login(
            &server,
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/steal", elsewhere.uri())),
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
        assert_eq!(error.note.id, "query.nine_router.redirected");
        let seen = requests_of(&server.received_requests().await.unwrap());
        assert_eq!(
            seen.iter().filter(|(verb, _)| verb == "POST").count(),
            1,
            "{seen:?}"
        );
        assert!(!seen
            .iter()
            .any(|(_, target)| target.starts_with("/api/providers")));
    }

    /// R-01 item 1: hostile connection ids in a recorded full refresh: only
    /// allow-listed targets are requested, none of those ids, never `force`.
    /// RF3.3 with re-validation V-18: `limitReached` with no detail is an
    /// exhausted connection, shown as a full meter, never "— (0 left)".
    #[tokio::test]
    async fn a_reached_limit_without_detail_is_exhausted_with_no_made_up_remaining() {
        let server = MockServer::start().await;
        mount_json(
            &server,
            "/api/providers/client",
            page(json!([connection("cx1", "codex", "Codex")]), 1),
        )
        .await;
        mount_json(
            &server,
            "/api/usage/cx1",
            json!({ "limitReached": true, "quotas": {} }),
        )
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
        let metric = &data.sub_reports[0].metrics[0];
        assert_eq!(metric.remaining, None);
        assert_eq!((metric.used, metric.limit), (Some(100.0), Some(100.0)));
        assert_eq!(metric.unit, "%");
        assert!(matches!(
            crate::metrics::classify_metric(metric),
            crate::metrics::MetricState::Exhausted { .. }
        ));
        assert_eq!(
            crate::metric_text::metric_text(&metric.label).resolve("en-US"),
            "Limit reached"
        );
    }

    /// Re-validation V-09: the counted texts of the instance and of Test
    /// connection are plurals.
    #[test]
    fn counted_texts_are_plurals() {
        let connected = |connections| {
            outcome_text(&TestOutcome::Connected {
                version: Some("0.5.109".into()),
                connections,
            })
        };
        assert_eq!(
            connected(1).resolve("en-US"),
            "9router 0.5.109: 1 connection with usage found."
        );
        assert_eq!(
            connected(3).resolve("en-US"),
            "9router 0.5.109: 3 connections with usage found."
        );
        assert!(connected(1).resolve("pt-BR").contains("1 conexão"));
        assert!(connected(5).resolve("ru").contains("5 подключений"));
        assert!(connected(2).resolve("ru").contains("2 подключения"));
        let lite = |connections| {
            litellm_outcome_text(&TestOutcome::Connected {
                version: None,
                connections,
            })
            .unwrap()
        };
        assert_eq!(
            lite(1).resolve("en-US"),
            "LiteLLM: 1 provider with a budget found."
        );
        assert!(lite(1).resolve("es").contains("1 proveedor "));
        let untested = outcome_text_with_min(
            &TestOutcome::Connected {
                version: Some("0.4.0".into()),
                connections: 1,
            },
            Some("0.5.0"),
        );
        assert!(untested
            .resolve("en-US")
            .starts_with("9router 0.4.0: 1 connection with"));
        assert!(untested.resolve("de").contains("1 Verbindung "));
    }

    #[tokio::test]
    async fn a_refresh_never_requests_a_hostile_connection_id() {
        let server = MockServer::start().await;
        let long = "a".repeat(129);
        let hostile = [
            "../../api/providers",
            "abc%2F..%2Fsettings",
            "a?force=1",
            long.as_str(),
        ];
        let mut list: Vec<Value> = hostile
            .iter()
            .map(|id| connection(id, "claude", "Hostile"))
            .collect();
        list.push(connection("dup", "claude", "One"));
        list.push(connection("dup", "claude", "Two"));
        mount_json(
            &server,
            "/api/providers/client",
            page(Value::Array(list), 1),
        )
        .await;
        mount_json(
            &server,
            "/api/version",
            json!({ "currentVersion": "0.5.109" }),
        )
        .await;
        mount_json(&server, "/api/usage/dup", claude_usage()).await;
        let http = client();
        let data = fetch_at(
            &account(&server, false, &[]),
            &context(&http),
            Limits::DEFAULT,
            Utc::now(),
        )
        .await
        .unwrap();
        // S-08: the repeated id is listed once, the first kept, and the
        // instance says how many were left out.
        assert_eq!(data.sub_reports.len(), 5);
        let dup = data
            .sub_reports
            .iter()
            .filter(|sub| sub.id == sub_id("dup"))
            .collect::<Vec<_>>();
        assert_eq!(dup.len(), 1);
        assert_eq!(dup[0].label, "One");
        assert!(data
            .notes_l10n
            .iter()
            .any(|note| note.id == "note.aggregator.duplicate_ids"
                && note.params.get("count") == Some(&ParamValue::from("1".to_string()))));
        let seen = requests_of(&server.received_requests().await.unwrap());
        for (verb, target) in &seen {
            assert_eq!(verb, "GET", "{target}");
            assert!(!target.contains("force"), "{target}");
            let allowed = target == "/api/version"
                || target.starts_with("/api/providers/client?")
                || target == "/api/usage/dup";
            assert!(allowed, "{target}");
            for id in hostile {
                assert!(!target.contains(id), "{target}");
            }
            assert!(!target.contains("settings"), "{target}");
            assert!(!target.contains(".."), "{target}");
        }
        assert!(seen.iter().any(|(_, target)| target == "/api/usage/dup"));
    }

    /// Re-validation S-02: on the platform roots, a loopback instance is
    /// read through the direct client, never the shared one that carries
    /// the environment's proxy (a recording proxy stands in for it).
    #[tokio::test]
    async fn a_local_instance_never_goes_through_the_environment_proxy() {
        let proxy = crate::network::recording_proxy::RecordingProxy::start().await;
        let proxied = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .proxy(proxy.proxy())
            .build()
            .unwrap();
        let direct = client();
        let server = MockServer::start().await;
        let token = jwt(Utc::now().timestamp() + 86_400);
        mount_login(
            &server,
            ResponseTemplate::new(200).insert_header("set-cookie", format!("auth_token={token}")),
            1,
        )
        .await;
        mount_json(&server, "/api/providers/client", page(json!([]), 1)).await;
        let ctx = Context {
            http: &proxied,
            direct: &direct,
            credentials: &Env,
            timeout: Duration::from_secs(5),
        };
        let acct = account(&server, true, &[]);
        fetch_at(&acct, &ctx, Limits::DEFAULT, Utc::now())
            .await
            .unwrap();
        assert!(matches!(
            test_connection_at(&acct, &ctx, Limits::DEFAULT, Utc::now()).await,
            TestOutcome::Connected { .. }
        ));
        assert!(proxy.seen().is_empty(), "{:?}", proxy.seen());
        assert!(!server.received_requests().await.unwrap().is_empty());
    }

    /// Owner decision of 2026-11-20 (PRD 0036 RF2.3): a password or
    /// session over plain HTTP goes only to loopback. A private-network
    /// host with `allow_private_http` gets no secret, refused before any
    /// request; a password-less instance there is still read. Every
    /// request goes through a local wiremock used as the forward proxy, so
    /// nothing leaves this machine.
    #[tokio::test]
    async fn a_private_network_instance_over_plain_http_never_gets_a_secret() {
        const BASE: &str = "http://192.168.77.10:20128";
        let proxy = MockServer::start().await;
        mount_genuine(&proxy).await;
        mount_json(
            &proxy,
            "/api/providers/client",
            page(json!([connection("a", "claude", "A")]), 1),
        )
        .await;
        mount_json(&proxy, "/api/usage/a", claude_usage()).await;
        let proxied = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .proxy(reqwest::Proxy::http(proxy.uri()).unwrap())
            .build()
            .unwrap();
        let private = |password: bool, sso: bool| {
            let mut params = BTreeMap::from([
                ("base_url".to_string(), BASE.to_string()),
                ("allow_private_http".to_string(), "true".to_string()),
            ]);
            if sso {
                params.insert(
                    crate::user_session::NINE_ROUTER_SSO_SLOT.to_string(),
                    "TEST_NR_SSO".to_string(),
                );
            }
            Account {
                name: "lan".into(),
                provider: Provider::NineRouter,
                credential_env: password.then(|| "TEST_NINE_ROUTER_PASSWORD".into()),
                params,
                ..Account::default()
            }
        };
        let token = jwt(Utc::now().timestamp() + 3_600);
        let sso_credentials = SsoOnly(format!("auth_token={token}"));
        let check = FixedListener(Verdict::Trusted);
        for sso in [false, true] {
            forget_login(&format!("{BASE}/")).await;
            let credentials: &dyn Credentials = if sso { &sso_credentials } else { &Env };
            let ctx = Context {
                http: &proxied,
                direct: &proxied,
                credentials,
                timeout: Duration::from_secs(5),
            };
            let acct = private(!sso, sso);
            let error = fetch_with(&acct, &ctx, Limits::DEFAULT, Utc::now(), &check)
                .await
                .unwrap_err();
            assert_eq!(error.note.id, "query.nine_router.secret_plain_http");
            assert_eq!(error.status, Status::NeedsConfiguration);
            let text = error.note.resolve("en-US");
            assert!(text.contains("192.168.77.10:20128"), "{text}");
            assert!(
                text.contains("HTTPS") && text.contains("nothing was sent"),
                "{text}"
            );
            assert!(
                !text.contains("http://") && !text.contains("correct"),
                "{text}"
            );
            let outcome =
                test_connection_with(&acct, &ctx, Limits::DEFAULT, Utc::now(), &check).await;
            assert_eq!(
                outcome,
                TestOutcome::SecretOverPlainHttp {
                    host: "192.168.77.10:20128".into()
                }
            );
            assert_eq!(
                outcome_text(&outcome).id,
                "query.nine_router.secret_plain_http"
            );
            assert_eq!(outcome.field(), Some("params.base_url"));
            assert!(
                proxy.received_requests().await.unwrap().is_empty(),
                "sso={sso}: nothing may be requested"
            );
        }

        // The barrier under the early refusal: no request that carries the
        // password or a cookie can even be built for that host.
        let instance = Instance::new(
            &proxied,
            parse_base(BASE, true).unwrap(),
            Limits::DEFAULT,
            Duration::from_secs(5),
        )
        .with_listener(&check);
        for (route, token) in [
            (Route::Login, None),
            (Route::Connections { page: 1 }, Some("t")),
            (Route::Usage("a".into()), Some("t")),
        ] {
            let error = instance.request(&route, token).unwrap_err();
            assert_eq!(error.note.id, "query.nine_router.secret_plain_http");
        }
        assert!(instance.request(&Route::Health, None).is_ok());

        // Without a password the instance is still read over plain HTTP,
        // with the "no password" warning and no private-HTTP note.
        forget_login(&format!("{BASE}/")).await;
        let ctx = Context {
            http: &proxied,
            direct: &proxied,
            credentials: &Env,
            timeout: Duration::from_secs(5),
        };
        let data = fetch_with(
            &private(false, false),
            &ctx,
            Limits::DEFAULT,
            Utc::now(),
            &check,
        )
        .await
        .unwrap();
        assert_eq!(data.sub_reports[0].status, Status::Ready);
        let ids: Vec<_> = data
            .notes_l10n
            .iter()
            .map(|note| note.id.as_ref())
            .collect();
        assert!(ids.contains(&"note.nine_router.exposed"), "{ids:?}");
        assert!(!ids.contains(&"note.nine_router.private_http"), "{ids:?}");
        let requests = proxy.received_requests().await.unwrap();
        assert!(!requests.is_empty());
        for request in &requests {
            assert!(request.headers.get("cookie").is_none());
            assert_eq!(request.method.to_string(), "GET");
        }
    }

    /// A credential store that records every name asked for. The core's
    /// `Credentials` can only read, so a refresh has no way to write a
    /// session to the vault; this records what it reads.
    struct Recording(std::sync::Mutex<Vec<String>>);
    impl Credentials for Recording {
        fn get(&self, name: &str) -> Option<String> {
            self.0.lock().unwrap().push(name.to_owned());
            Env.get(name)
        }
    }

    /// Owner decision of 2026-11-20 (PRD 0036 RF3.1): the 9router login
    /// session lives only in this process's memory. Runs in one process
    /// share it (one login); a new process, as each one-shot command is
    /// (`--once`, `--json`, `--json-v1`), starts without it and logs in
    /// again, and no run asks the credential store for anything but the
    /// password it was configured with: no session is read back from the
    /// vault, and none can be written (the store is read-only).
    #[tokio::test]
    async fn the_login_session_lives_only_in_memory_and_never_in_the_vault() {
        let server = MockServer::start().await;
        let token = jwt(Utc::now().timestamp() + 86_400);
        mount_login(
            &server,
            ResponseTemplate::new(200).insert_header("set-cookie", format!("auth_token={token}")),
            2,
        )
        .await;
        mount_json(&server, "/api/providers/client", page(json!([]), 1)).await;
        let http = client();
        let store = Recording(std::sync::Mutex::new(Vec::new()));
        let ctx = Context {
            http: &http,
            direct: &http,
            credentials: &store,
            timeout: Duration::from_secs(5),
        };
        let acct = account(&server, true, &[]);
        // Two refreshes of one long-running process: one login.
        for _ in 0..2 {
            fetch_at(&acct, &ctx, Limits::DEFAULT, Utc::now())
                .await
                .unwrap();
        }
        let logins = |requests: &[Request]| {
            requests
                .iter()
                .filter(|request| request.url.path() == "/api/auth/login")
                .count()
        };
        assert_eq!(logins(&server.received_requests().await.unwrap()), 1);
        // A new process (the memory is empty): it logs in again rather than
        // finding a session anywhere else.
        SESSIONS.lock().await.remove(&format!("{}/", server.uri()));
        fetch_at(&acct, &ctx, Limits::DEFAULT, Utc::now())
            .await
            .unwrap();
        assert_eq!(logins(&server.received_requests().await.unwrap()), 2);
        let asked = store.0.lock().unwrap().clone();
        assert!(!asked.is_empty());
        assert!(
            asked.iter().all(|name| name == "TEST_NINE_ROUTER_PASSWORD"),
            "{asked:?}"
        );
    }
}
