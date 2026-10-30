//! Quota queries for key-routed providers: OpenCode Go,
//! Groq, Kimi Coding (opt-in) and Xiaomi MiMo.
//!
//! All endpoints here are internal/publicly undocumented routes,
//! ported from the 9router project (MIT license, commit
//! f01fb909e37189008080632ddaf404f096345cde, github.com/decolua/9router):
//! - open-sse/services/usage/opencode-go.js — GET https://opencode.ai/zen/go/v1/usage
//! - open-sse/services/usage/groq.js — GET https://api.groq.com/openai/v1/models
//!   (quota via x-ratelimit-* headers, without consuming tokens)
//! - open-sse/services/usage/kimi.js — GET https://api.kimi.com/coding/v1/usages
//! - open-sse/services/usage/xiaomi-mimo.js + open-sse/shared/mimoAccount.js
//!
//! Being internal routes, they may change without notice; each response includes a
//! stability note. No generation, purchase, reset or credit consumption
//! endpoint is called — only idempotent GET reads.

use super::{secret, send as secure_send, Context, Data, QueryError};
use crate::text::{params, provider_l10n, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider, Status};
use chrono::{DateTime, Utc};

const OPENCODE_GO_USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
pub(crate) const GROQ_MODELS_URL: &str = "https://api.groq.com/openai/v1/models";
const KIMI_CODING_USAGES_URL: &str = "https://api.kimi.com/coding/v1/usages";
const MIMO_AISTUDIO_USAGE_URL: &str = "https://aistudio.xiaomimimo.com/open-apis/v1/user/usage";

/// Regional clusters of the MiMo account service (1:1 with mimoAccount.js from
/// 9router). The default is `sgp`, the international cluster.
const MIMO_REGION_BASES: [(&str, &str); 5] = [
    ("cn", "https://mimo-server-cn.xiaomimimo.com"),
    ("sgp", "https://mimo-server-sgp.xiaomimimo.com"),
    ("ams", "https://mimo-server-ams.xiaomimimo.com"),
    ("ru", "https://mimo-server-ru.xiaomimimo.com"),
    ("in", "https://mimo-server-in.xiaomimimo.com"),
];
const MIMO_DEFAULT_REGION: &str = "sgp";
/// The report source of a MiMo account session: the cluster is a named
/// placeholder (PRD 0052 RF3.6); the card label names the region.
const MIMO_SESSION_USAGE_SOURCE: &str =
    "https://mimo-server-{region}.xiaomimimo.com/api/user/usage";
/// User-Agent expected by the mimo-server backend (mimoAccount.js).
const MIMO_API_UA: &str =
    "miNative PC/Normal Windows_NT/10.0.19045 SDKV/1.0.0 DEVT/PC DEVS/Windows APP/miaccount_desktop APPV/0.1.0";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::OpenCodeGo => opencode_go_at(account, ctx, OPENCODE_GO_USAGE_URL).await,
        Provider::Groq => groq_at(account, ctx, GROQ_MODELS_URL).await,
        Provider::Kimi => kimi_coding_at(account, ctx, KIMI_CODING_USAGES_URL).await,
        Provider::XiaomiMimo => xiaomi_mimo(account, ctx).await,
        other => Err(QueryError::unsupported_params(
            "query.router_keys.provider_handled_elsewhere",
            params([("provider", ParamValue::from(provider_l10n(other)))]),
            "{provider} is handled by another adapter.",
        )),
    }
}

// ----------------------------------------------------------------- helpers

fn opt_param<'a>(account: &'a Account, key: &str) -> Option<&'a str> {
    account
        .params
        .get(key)
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Converts a number or numeric string (9router accepts both via
/// `toFiniteNumber`) into a finite f64; absent/invalid → None.
fn finite(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(n) => n.as_f64().filter(|v| v.is_finite()),
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                s.parse::<f64>().ok().filter(|v| v.is_finite())
            }
        }
        _ => None,
    }
}

/// Port of `parseResetTime` (shared.js): epoch in s or ms (number or numeric
/// string) or an ISO date string.
fn parse_reset_time(value: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    match value? {
        serde_json::Value::Number(n) => n.as_i64().and_then(epoch_to_utc),
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else if s.chars().all(|c| c.is_ascii_digit()) {
                s.parse::<i64>().ok().and_then(epoch_to_utc)
            } else {
                DateTime::parse_from_rfc3339(s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            }
        }
        _ => None,
    }
}

fn epoch_to_utc(ts: i64) -> Option<DateTime<Utc>> {
    if ts <= 0 {
        return None;
    }
    if ts < 1_000_000_000_000 {
        DateTime::from_timestamp(ts, 0)
    } else {
        DateTime::from_timestamp_millis(ts)
    }
}

/// Sends GET and maps the status to QueryError without exposing the body or URL.
async fn send(req: reqwest::RequestBuilder) -> Result<reqwest::Response, QueryError> {
    let response = secure_send(req).await?;
    let code = response.status();
    if !code.is_success() {
        return Err(QueryError {
            diagnostic: None,
            status: match code.as_u16() {
                401 | 403 => Status::AuthenticationError,
                429 => Status::RateLimited,
                _ => Status::Error,
            },
            note: Localized::with_params(
                "query.router_keys.http",
                params([("code", ParamValue::from(code.as_u16()))]),
                "Query returned HTTP {code}.",
            ),
        });
    }
    Ok(response)
}

// ------------------------------------------------------------- OpenCode Go

/// GET {base} with Bearer; `usage.{rolling,weekly,monthly}.{percent,resetsAt}`.
/// `percent` is the USED percentage (0–100); a numeric string is accepted.
async fn opencode_go_at(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "OPENCODE_GO_API_KEY")?;
    let request = ctx
        .http
        .get(url)
        .bearer_auth(key.trim())
        .header("Accept", "application/json")
        .timeout(ctx.timeout);

    let response = secure_send(request).await?;
    let code = response.status();
    if !code.is_success() {
        // 403 with error.type == "EntitlementError" distinguishes "no subscription"
        // from an invalid credential (opencode-go.js). The body never leaks into the
        // message — only the structured type is inspected.
        let status = match code.as_u16() {
            401 | 403 => Status::AuthenticationError,
            429 => Status::RateLimited,
            _ => Status::Error,
        };
        let note = match code.as_u16() {
            401 => Localized::with_params(
                "query.router_keys.opencode_authentication_failed",
                params([("code", ParamValue::from(401u16))]),
                "OpenCode authentication failed.",
            ),
            403 => {
                let body = super::read_json::<serde_json::Value>(response).await.ok();
                let entitlement = body
                    .as_ref()
                    .and_then(|b| b.get("error"))
                    .and_then(|e| e.get("type"))
                    .and_then(|t| t.as_str())
                    == Some("EntitlementError");
                if entitlement {
                    Localized::with_params(
                        "query.router_keys.opencode_subscription_required",
                        params([("code", ParamValue::from(403u16))]),
                        "OpenCode connected, but no usage permission — a paid subscription is required.",
                    )
                } else {
                    Localized::with_params(
                        "query.router_keys.opencode_access_denied",
                        params([("code", ParamValue::from(403u16))]),
                        "Access denied by OpenCode.",
                    )
                }
            }
            other => Localized::with_params(
                "query.router_keys.opencode_http",
                params([("code", ParamValue::from(other))]),
                "OpenCode returned HTTP {code}.",
            ),
        };
        return Err(QueryError {
            diagnostic: None,
            status,
            note,
        });
    }

    let body = super::body_value(response).await?;
    let usage = body
        .get("usage")
        .and_then(|u| u.as_object())
        .ok_or_else(|| QueryError::schema_at(&body, &["usage"], "object"))?;

    let mut metrics = Vec::new();
    for (period_key, label, period_pt, window) in [
        (
            "rolling",
            "Janela móvel",
            "janela móvel",
            MetricWindow::Other,
        ),
        ("weekly", "Semanal", "semanal", MetricWindow::Weekly),
        ("monthly", "Mensal", "mensal", MetricWindow::Monthly),
    ] {
        let Some(quota) = usage.get(period_key).and_then(|q| q.as_object()) else {
            continue;
        };
        // Absent/invalid percentage → window ignored (never an invented zero).
        let Some(percent) = finite(quota.get("percent")) else {
            continue;
        };
        let used = percent.clamp(0.0, 100.0);
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label: label.into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: Some(100.0 - used),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(window),
            period: Some(period_pt.into()),
            resets_at: parse_reset_time(quota.get("resetsAt")),
        });
    }

    if metrics.is_empty() {
        return Err(QueryError::schema_at(
            &body,
            &["usage", "weekly", "percent"],
            "number",
        ));
    }

    let mut notes = Notes::new();
    notes.push(
        "note.router_keys.opencode_usage_percentages",
        "Usage is reported as percentages per metric (0..100); no absolute counts are exposed.",
    );
    notes.push(
        "note.router_keys.opencode_internal_route",
        "OpenCode internal route observed in 9router; subject to change and provider restrictions.",
    );
    Ok(Data::new(metrics, notes, OPENCODE_GO_USAGE_URL))
}

// -------------------------------------------------------------------- Groq

/// Groq has no quota endpoint: limits travel in the x-ratelimit-*
/// headers of every response. GET /openai/v1/models is read (listing,
/// generates no tokens). Absent header ≠ zero: the metric is omitted.
pub(crate) async fn groq_at(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "GROQ_API_KEY")?;
    let request = ctx
        .http
        .get(url)
        .bearer_auth(key.trim())
        .header("Accept", "application/json")
        .timeout(ctx.timeout);
    let response = send(request).await?;
    let headers = response.headers().clone();
    // Drains the body to release the connection; the content is not used.
    let _ = response.bytes().await;

    let mut metrics = Vec::new();
    let now = Utc::now();
    for (label, unit, limit_h, remaining_h, reset_h) in [
        (
            "Requisições",
            "requisições",
            "x-ratelimit-limit-requests",
            "x-ratelimit-remaining-requests",
            "x-ratelimit-reset-requests",
        ),
        (
            "Tokens",
            "tokens",
            "x-ratelimit-limit-tokens",
            "x-ratelimit-remaining-tokens",
            "x-ratelimit-reset-tokens",
        ),
    ] {
        let Some(limit) = header_number(&headers, limit_h) else {
            continue;
        };
        let Some(remaining) = header_number(&headers, remaining_h) else {
            continue;
        };
        let resets_at = headers
            .get(reset_h)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_groq_duration)
            .and_then(|d| chrono::Duration::from_std(d).ok())
            .map(|d| now + d);
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label: format!("{label} (rate limit)"),
            used: Some((limit - remaining).max(0.0)),
            limit: Some(limit),
            remaining: Some(remaining.max(0.0)),
            unit: unit.into(),
            kind: MetricKind::RateLimit,
            window: Some(MetricWindow::Other),
            period: Some("janela corrente do rate limit".into()),
            resets_at,
        });
    }

    let mut notes = Notes::new();
    notes.push(
        "note.router_keys.groq_rate_limit_headers",
        "Rate limit window: remaining requests / limit parsed from `x-ratelimit-*` headers (per request).",
    );
    notes.push(
        "note.router_keys.groq_internal_route",
        "Groq internal route observed in 9router; subject to change and provider restrictions.",
    );
    if metrics.is_empty() {
        // Valid key, but no rate-limit bucket reported: distinct
        // from an authentication error.
        notes.push(
            "note.router_keys.groq_rate_limit_headers_missing",
            "Groq response without `x-ratelimit-*` headers in this request; the limit window is unknown.",
        );
    }

    Ok(Data::new(metrics, notes, GROQ_MODELS_URL))
}

fn header_number(headers: &reqwest::header::HeaderMap, name: &str) -> Option<f64> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// Port of `parseGroqDurationMs` (groq.js): Go-style durations
/// ("2m59.56s", "7.66s", "1h2m3s", "150ms").
fn parse_groq_duration(value: &str) -> Option<std::time::Duration> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut total_ms = 0.0f64;
    let mut matched = false;
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        if start == i {
            // Unexpected separators: aborts if it is not a known digit.
            if !matches!(bytes[i], b' ' | b'-' | b'+') {
                return None;
            }
            i += 1;
            continue;
        }
        let amount: f64 = value[start..i].parse().ok()?;
        let unit_start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        let unit_ms = match &value[unit_start..i] {
            "h" => 3_600_000.0,
            "m" => 60_000.0,
            "s" => 1_000.0,
            "ms" => 1.0,
            _ => return None,
        };
        matched = true;
        total_ms += amount * unit_ms;
    }
    if !matched || !total_ms.is_finite() || total_ms < 0.0 {
        return None;
    }
    std::time::Duration::try_from_secs_f64(total_ms / 1000.0).ok()
}

// ------------------------------------------------------------- Kimi Coding

/// GET https://api.kimi.com/coding/v1/usages (kimi.js).
///
/// Explicit opt-in: `params.product = "coding"`. Without it, the Kimi provider
/// stays on the legacy international balance adapter (asia.rs) — the parent
/// only routes here when the opt-in is present; the error below guards against
/// accidental routing.
///
/// Dual authentication (1:1 with the reference):
/// - `credential_env` (default KIMI_API_KEY) → `x-api-key` header only;
/// - `params.token_env` pointing to an OAuth access token → `Bearer` +
///   X-Msh-* headers (optional `params.device_id` pins the device id).
///
/// The API key takes precedence when both exist (as in the reference).
async fn kimi_coding_at(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
) -> Result<Data, QueryError> {
    if opt_param(account, "product") != Some("coding") {
        return Err(QueryError::config(
            "query.router_keys.kimi_coding_opt_in_required",
            "Kimi: set params.product=coding to query the Kimi Coding subscription quota; without it the international platform balance applies.",
        ));
    }

    let api_key = ctx
        .credentials
        .get(account.credential_env.as_deref().unwrap_or("KIMI_API_KEY"))
        .filter(|s| !s.trim().is_empty());
    let token_env = opt_param(account, "token_env");
    let access_token =
        token_env.and_then(|name| ctx.credentials.get(name).filter(|s| !s.trim().is_empty()));

    let mut request = ctx.http.get(url).timeout(ctx.timeout);
    if let Some(key) = api_key {
        // Platform key path: x-api-key only (no Bearer/X-Msh).
        request = request.header("x-api-key", key.trim());
    } else if let Some(token) = access_token {
        // Caminho OAuth (device-code): Bearer + X-Msh-*.
        let device_id = opt_param(account, "device_id")
            .map(str::to_string)
            .unwrap_or_else(|| "cuotai".to_string());
        request = request
            .bearer_auth(token.trim())
            .header("X-Msh-Platform", "cuotai")
            .header("X-Msh-Version", env!("CARGO_PKG_VERSION"))
            .header("X-Msh-Device-Name", "cuotai-cli")
            .header(
                "X-Msh-Device-Model",
                format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            )
            .header("X-Msh-Device-Id", device_id);
    } else {
        let env = account.credential_env.as_deref().unwrap_or("KIMI_API_KEY");
        return Err(QueryError::config_params(
            "query.router_keys.kimi_missing_credential",
            params([("env", ParamValue::from(env.to_string()))]),
            "Missing credential: {env} (or params.token_env with a Kimi OAuth access token).",
        ));
    }
    request = request
        .header("Content-Type", "application/json")
        .header("Accept", "application/json");

    let response = secure_send(request).await?;
    let code = response.status();
    let body: serde_json::Value = super::read_json(response)
        .await
        .unwrap_or(serde_json::Value::Null);

    if !code.is_success() {
        return Err(kimi_coding_error(code.as_u16(), &body));
    }

    let mut metrics = Vec::new();

    // Subscription weekly quota: data.usage.{limit,used,remaining,resetTime}
    // (fields may come as numeric strings; PascalCase variants accepted).
    if let Some(usage) = body.get("usage").and_then(|u| u.as_object()) {
        let pick = |names: &[&str]| -> Option<&serde_json::Value> {
            names.iter().find_map(|n| usage.get(*n))
        };
        let limit = finite(pick(&["limit", "Limit"]));
        let used = finite(pick(&["used", "Used"])).map(|used| used.max(0.0));
        let remaining = finite(pick(&["remaining", "Remaining"]));
        let reset = parse_reset_time(pick(&["resetTime", "ResetTime", "reset_at", "resetAt"]));
        if let Some(limit) = limit.filter(|l| *l > 0.0) {
            // One of the two gives the other; with neither the limit stays
            // alone (no consumption is known, which is not zero).
            let (used, remaining) = match (used, remaining) {
                (None, Some(left)) => (Some((limit - left).max(0.0)), Some(left)),
                (Some(spent), None) => (Some(spent), Some((limit - spent).max(0.0))),
                known => known,
            };
            metrics.push(Metric {
                billing: Default::default(),
                scope: None,
                label: "Semanal".into(),
                used,
                limit: Some(limit),
                remaining,
                unit: "créditos".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Weekly),
                period: Some("semanal".into()),
                resets_at: reset,
            });
        }
    }

    // Rate limit per window: data.limits[].detail.{limit,remaining,resetTime}.
    if let Some(limits) = body.get("limits").and_then(|l| l.as_array()) {
        for item in limits {
            let Some(detail) = item.get("detail").and_then(|d| d.as_object()) else {
                continue;
            };
            let pick = |names: &[&str]| -> Option<&serde_json::Value> {
                names.iter().find_map(|n| detail.get(*n))
            };
            let Some(limit) = finite(pick(&["limit", "Limit"])).filter(|l| *l > 0.0) else {
                continue;
            };
            let remaining = finite(pick(&["remaining", "Remaining"]));
            metrics.push(Metric {
                billing: Default::default(),
                scope: None,
                label: "Rate limit".into(),
                used: remaining.map(|remaining| (limit - remaining).max(0.0)),
                limit: Some(limit),
                remaining: remaining.map(|remaining| remaining.max(0.0)),
                unit: "requisições".into(),
                kind: MetricKind::RateLimit,
                window: Some(MetricWindow::Other),
                period: Some("janela corrente do rate limit".into()),
                resets_at: parse_reset_time(pick(&["resetTime", "reset_at", "resetAt"])),
            });
        }
    }

    let plan = body
        .get("user")
        .and_then(|u| u.get("membership"))
        .and_then(|m| m.get("level"))
        .and_then(|l| l.as_str())
        .map(kimi_plan_name)
        .unwrap_or("Kimi Coding");

    let mut notes = Notes::new();
    notes.push_params(
        "note.router_keys.kimi_plan",
        params([("plan", ParamValue::from(plan))]),
        "Plan: {plan}.",
    );
    notes.push(
        "note.router_keys.kimi_internal_route",
        "Kimi internal route observed in 9router; subject to change and provider restrictions.",
    );
    if metrics.is_empty() {
        notes.push(
            "note.router_keys.kimi_no_numeric_quota",
            "Kimi response without numeric quota block; absent does not mean unlimited.",
        );
    }

    Ok(Data::new(metrics, notes, KIMI_CODING_USAGES_URL))
}

/// Porta de `getKimiPlanName` (kimi.js).
fn kimi_plan_name(level: &str) -> &'static str {
    match level {
        "LEVEL_BASIC" => "Moderato",
        "LEVEL_INTERMEDIATE" => "Allegretto",
        "LEVEL_ADVANCED" => "Allegro",
        "LEVEL_STANDARD" => "Vivace",
        _ => "Kimi Coding",
    }
}

/// Port of `formatKimiUsageError` (kimi.js): 401 = expired session; 403
/// REASON_FEATURE_NO_PERMISSION = missing Kimi Code subscription (NOT an
/// expired session). The body never leaks into the message.
fn kimi_coding_error(status: u16, body: &serde_json::Value) -> QueryError {
    if status == 401 {
        return QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::with_params(
                "query.router_keys.kimi_authentication_expired",
                params([("code", ParamValue::from(status))]),
                "Kimi Coding: authentication expired. Renew the token or the API key.",
            ),
        };
    }
    if status == 403 {
        let reason = body
            .get("details")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .and_then(|d| d.get("debug"))
            .and_then(|d| d.get("reason"))
            .and_then(|r| r.as_str())
            .or_else(|| {
                body.get("debug")
                    .and_then(|d| d.get("reason"))
                    .and_then(|r| r.as_str())
            })
            .or_else(|| body.get("reason").and_then(|r| r.as_str()));
        let code = body.get("code").and_then(|c| c.as_str()).unwrap_or("");
        if reason == Some("REASON_FEATURE_NO_PERMISSION")
            || code.eq_ignore_ascii_case("permission_denied")
        {
            return QueryError { diagnostic: None,
                status: Status::AuthenticationError,
                note: Localized::with_params(
                    "query.router_keys.kimi_subscription_required",
                    params([("code", ParamValue::from(status))]),
                    "Kimi Coding: account connected, but no usage permission — a Kimi Code subscription is required.",
                ),
            };
        }
        return QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::with_params(
                "query.router_keys.kimi_access_denied",
                params([("code", ParamValue::from(status))]),
                "Query returned HTTP 403. Check access and permissions.",
            ),
        };
    }
    QueryError {
        diagnostic: None,
        status: if status == 429 {
            Status::RateLimited
        } else {
            Status::Error
        },
        note: Localized::with_params(
            "query.router_keys.kimi_http",
            params([("code", ParamValue::from(status))]),
            "Kimi Coding query returned HTTP {code}.",
        ),
    }
}

// -------------------------------------------------------------- Xiaomi MiMo

/// Xiaomi MiMo quota: real modes, in priority order.
///
/// 1. `session_env` session (PRD 0016: pasted into the system vault or, alternatively,
///    in a variable named by `params.session_env`) — Xiaomi ACCOUNT
///    cookie (`serviceToken=...`, from MiMo Desktop/SSO); GET
///    `{mimo-server-<region>}/api/user/usage`.
///    Region in `params.region` ∈ {cn, sgp, ams, ru, in} (default `sgp`).
///    Response: `{ code: 0, data: { percent, resetDate, resetAt? } }`, where
///    `percent` is the REMAINING percentage of the weekly quota (e.g. 94).
///    (xiaomi-mimo.js + mimoAccount.js from 9router.)
///
/// 2. `platform_session_env` session (vault or variable) — the platform's
///    WEB session cookie (login at platform.xiaomimimo.com; needs
///    `api-platform_serviceToken` and `userId`). Pay-as-you-go balance and monthly
///    token plan; implemented in `super::mimo_platform` (reference:
///    CodexBar).
///
/// The cookies required for each session come from the descriptor
/// (`crate::user_session::descriptors`), checked by `super::web_session`
/// before any request.
///
/// 3. Native `sk-` key (authorize/ECDH login): does NOT read quota at any endpoint
///    — the AI Studio route refuses `sk-` with 401 (verified by 9router)
///    and the web console requires a session cookie. Instead of a request doomed
///    to 401, we return configuration guidance WITHOUT asking for a new login
///    code: the code is encrypted per attempt and a new code produces the
///    same key without quota.
///
/// 4. Manual credential with no sign of a native key: a Bearer attempt at the
///    `aistudio.xiaomimimo.com/open-apis/v1/user/usage` endpoint — it only works
///    if the credential is a session token, not an inference `sk-`.
async fn xiaomi_mimo(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    if let Some(session) = super::web_session(account, ctx, "session_env")? {
        let region = opt_param(account, "region")
            .unwrap_or(MIMO_DEFAULT_REGION)
            .to_ascii_lowercase();
        let base = MIMO_REGION_BASES
            .iter()
            .find(|(code, _)| *code == region)
            .map(|(_, base)| *base)
            .ok_or_else(|| {
                QueryError::config_params(
                    "query.router_keys.xiaomi_invalid_region",
                    params([("region", ParamValue::from(region.clone()))]),
                    "params.region invalid: {region}. Values: cn, sgp, ams, ru, in.",
                )
            })?;
        return mimo_session_at(ctx, base, &session.cookie, &region).await;
    }

    if let Some(session) = super::web_session(account, ctx, "platform_session_env")? {
        return super::mimo_platform::fetch(
            ctx,
            super::mimo_platform::PLATFORM_API_BASE,
            &session.cookie,
        )
        .await;
    }

    // Account created by native login (oauth present) or explicitly
    // marked: the credential is the inference sk- key.
    let native_key =
        account.oauth.is_some() || opt_param(account, "auth_method") == Some("native_key");
    if native_key {
        return Err(QueryError::config(
            "query.router_keys.xiaomi_native_key_has_no_quota",
            "Xiaomi MiMo: the permanent native login key (sk-) is inference-only and does not read quota — do not repeat the login or the code, the result would be the same key. For quota, paste in the account wizard the Xiaomi account session (serviceToken cookie; weekly quota) or the platform.xiaomimimo.com one (api-platform_serviceToken and userId; balance and token plan), or, as an alternative, name the variable with the cookie in params.session_env or params.platform_session_env.",
        ));
    }

    let key = secret(account, ctx, "XIAOMI_MIMO_API_KEY")?;
    mimo_aistudio_at(ctx, MIMO_AISTUDIO_USAGE_URL, key.trim()).await
}

/// Internal envelope for wiremock: in production `base` is one of the fixed
/// clusters; in tests, the mock's local URL.
async fn mimo_session_at(
    ctx: &Context<'_>,
    base: &str,
    cookie: &str,
    region: &str,
) -> Result<Data, QueryError> {
    let is_fixed = MIMO_REGION_BASES.iter().any(|(_, b)| *b == base);
    let url = if is_fixed {
        format!("{base}/api/user/usage")
    } else {
        base.trim_end_matches('/').to_string()
    };
    let request = ctx
        .http
        .get(&url)
        .header("User-Agent", MIMO_API_UA)
        .header("Cookie", cookie.trim())
        .header("Accept", "application/json")
        .timeout(ctx.timeout);
    let response = super::send_session(request, crate::Provider::XiaomiMimo, "session_env").await?;
    if response.status().as_u16() == 401 {
        return Err(QueryError { diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::with_params(
                "query.router_keys.xiaomi_session_expired",
                params([("code", ParamValue::from(401u16))]),
                "Xiaomi MiMo: account session expired. Renew the session cookie (params.session_env).",
            ),
        });
    }
    if !response.status().is_success() {
        let code = response.status().as_u16();
        return Err(QueryError {
            diagnostic: None,
            status: Status::Error,
            note: Localized::with_params(
                "query.router_keys.xiaomi_session_http",
                params([("code", ParamValue::from(code))]),
                "Query returned HTTP {code}. Check access and permissions.",
            ),
        });
    }
    let body = super::body_value(response).await?;
    let label = if is_fixed {
        format!("Account session (cookie, region {region})")
    } else {
        "Account session (cookie)".to_string()
    };
    mimo_data_from_body(&body, &label, MIMO_SESSION_USAGE_SOURCE)
}

/// Internal envelope for wiremock: fallback with a Bearer token on aistudio.
async fn mimo_aistudio_at(ctx: &Context<'_>, url: &str, key: &str) -> Result<Data, QueryError> {
    let request = ctx
        .http
        .get(url)
        .bearer_auth(key)
        .header("X-Mimo-Source", "mimocode-cli")
        .header("Accept", "application/json")
        .timeout(ctx.timeout);
    let response = secure_send(request).await?;
    if response.status().as_u16() == 401 {
        return Err(QueryError { diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::with_params(
                "query.router_keys.xiaomi_aistudio_session_required",
                params([("code", ParamValue::from(401u16))]),
                "Xiaomi MiMo: the weekly quota requires the Xiaomi account session (paste it in the account wizard or name the variable in params.session_env); an `sk-` inference key alone is not accepted.",
            ),
        });
    }
    if !response.status().is_success() {
        let code = response.status().as_u16();
        return Err(QueryError {
            diagnostic: None,
            status: Status::Error,
            note: Localized::with_params(
                "query.router_keys.xiaomi_aistudio_http",
                params([("code", ParamValue::from(code))]),
                "Query returned HTTP {code}.",
            ),
        });
    }
    let body = super::body_value(response).await?;
    mimo_data_from_body(
        &body,
        "Bearer token (params.session_env not configured)",
        MIMO_AISTUDIO_USAGE_URL,
    )
}

/// Normalizes `{code, data:{percent, resetDate, resetAt}}`: `percent` is the
/// REMAINING percentage of the weekly quota (0–100).
fn mimo_data_from_body(
    body: &serde_json::Value,
    credential_label: &str,
    source: &'static str,
) -> Result<Data, QueryError> {
    if body.get("code").and_then(|c| c.as_i64()) != Some(0) {
        return Err(QueryError::schema_at(body, &["code"], "number"));
    }
    let data = body
        .get("data")
        .ok_or_else(|| QueryError::schema_at(body, &["data"], "object"))?;
    // percent absent/invalid ≠ zero.
    let percent = finite(data.get("percent"))
        .ok_or_else(|| QueryError::schema_at(body, &["data", "percent"], "number"))?;
    let remaining = percent.round().clamp(0.0, 100.0);
    let used = 100.0 - remaining;

    // resetAt (epoch s) takes precedence over resetDate ("YYYY-MM-DD").
    let resets_at = data
        .get("resetAt")
        .and_then(|v| v.as_i64())
        .filter(|ts| *ts > 0)
        .and_then(epoch_to_utc)
        .or_else(|| {
            data.get("resetDate")
                .and_then(|d| d.as_str())
                .and_then(|d| {
                    chrono::NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d")
                        .ok()
                        .and_then(|nd| nd.and_hms_opt(0, 0, 0))
                        .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
                })
        });

    let metrics = vec![Metric {
        billing: Default::default(),
        scope: None,
        label: "Semanal".into(),
        used: Some(used),
        limit: Some(100.0),
        remaining: Some(remaining),
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Weekly),
        period: Some("semanal".into()),
        resets_at,
    }];
    let mut notes = Notes::new();
    notes.push_params(
        "note.router_keys.xiaomi_credential",
        params([("credential", ParamValue::from(credential_label.to_string()))]),
        "Credential used: {credential}.",
    );
    notes.push(
        "note.router_keys.xiaomi_remaining_percentage",
        "Usage reported as a percentage of the weekly quota; cash_balance may be negative (debt) and voucher is always non-negative.",
    );
    notes.push(
        "note.router_keys.xiaomi_internal_route",
        "Xiaomi internal route observed in 9router; subject to change and provider restrictions.",
    );
    Ok(Data::new(metrics, notes, source))
}

// -------------------------------------------------------------------- testes

/// PRD 0028 RF11: the fixture whose metric identities `label_snapshot`
/// records. Add a case when the adapter learns a new metric.
#[cfg(test)]
pub(super) fn label_fixtures() -> Vec<(&'static str, Vec<Metric>)> {
    let body = serde_json::json!({"code": 0, "data": {"percent": 80, "resetAt": 1_790_000_000}});
    let data =
        mimo_data_from_body(&body, "fixture", "https://example.com").expect("fixture decodes");
    vec![("router_keys.xiaomi_mimo", data.metrics)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct TestCredentials(&'static str, &'static str);
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            if name == self.0 {
                Some(self.1.to_string())
            } else {
                None
            }
        }
    }

    fn account(provider: Provider, env: &'static str) -> Account {
        Account {
            id: None,
            name: "conta".into(),
            provider,
            credential_env: Some(env.into()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn ctx_with<'a>(http: &'a reqwest::Client, creds: &'a dyn Credentials) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: Duration::from_secs(5),
        }
    }

    // ------------------------------------------------------- parse helpers

    #[test]
    fn finite_aceita_numero_e_string_numerica() {
        assert_eq!(finite(Some(&serde_json::json!(42))), Some(42.0));
        assert_eq!(finite(Some(&serde_json::json!("3.5"))), Some(3.5));
        assert_eq!(finite(Some(&serde_json::json!(""))), None);
        assert_eq!(finite(Some(&serde_json::json!("abc"))), None);
        assert_eq!(finite(Some(&serde_json::json!(true))), None);
        assert_eq!(finite(None), None);
    }

    #[test]
    fn parse_reset_time_cobre_epoch_e_iso() {
        let iso = parse_reset_time(Some(&serde_json::json!("2026-09-04T14:28:02.617Z")))
            .expect("ISO válido");
        assert_eq!(iso.timestamp_millis(), 1_788_532_082_617);
        // epoch em segundos
        assert_eq!(
            parse_reset_time(Some(&serde_json::json!(1_700_000_000))),
            DateTime::from_timestamp(1_700_000_000, 0)
        );
        // numeric string in ms
        assert_eq!(
            parse_reset_time(Some(&serde_json::json!("1700000000000"))),
            DateTime::from_timestamp_millis(1_700_000_000_000)
        );
        assert!(parse_reset_time(Some(&serde_json::json!(""))).is_none());
        assert!(parse_reset_time(None).is_none());
    }

    #[test]
    fn parse_groq_duration_estilo_go() {
        assert_eq!(
            parse_groq_duration("2m59.56s"),
            Some(Duration::from_millis(179_560))
        );
        assert_eq!(
            parse_groq_duration("7.66s"),
            Some(Duration::from_millis(7_660))
        );
        assert_eq!(
            parse_groq_duration("150ms"),
            Some(Duration::from_millis(150))
        );
        assert_eq!(parse_groq_duration("1h"), Some(Duration::from_secs(3600)));
        assert!(parse_groq_duration("").is_none());
        assert!(parse_groq_duration("abc").is_none());
        assert!(parse_groq_duration("10x").is_none());
        assert!(parse_groq_duration("99999999999999999999999h").is_none());
    }

    // ------------------------------------------------------------ OpenCode Go

    #[tokio::test]
    async fn opencode_go_normaliza_tres_janelas() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("Authorization", "Bearer sk-go-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "usage": {
                    "rolling": { "status": "ok", "percent": 13, "resetsAt": "2026-09-04T14:28:02.617Z" },
                    "weekly": { "status": "ok", "percent": "5", "resetsAt": "2026-09-07T00:00:00.617Z" },
                    "monthly": { "status": "ok", "percent": 2, "resetsAt": "2026-10-02T12:14:24.617Z" }
                }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("OPENCODE_GO_API_KEY", "sk-go-test");
        let acc = account(Provider::OpenCodeGo, "OPENCODE_GO_API_KEY");
        let data = opencode_go_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect("sucesso");

        assert_eq!(data.metrics.len(), 3);
        assert!(data.metrics.iter().all(|m| m.kind == MetricKind::Quota));
        let windows: Vec<_> = data.metrics.iter().map(|m| m.window).collect();
        assert_eq!(
            windows,
            [
                Some(MetricWindow::Other),
                Some(MetricWindow::Weekly),
                Some(MetricWindow::Monthly),
            ]
        );
        let rolling = &data.metrics[0];
        assert_eq!(rolling.used, Some(13.0));
        assert_eq!(rolling.limit, Some(100.0));
        assert_eq!(rolling.remaining, Some(87.0));
        assert_eq!(
            rolling.resets_at,
            DateTime::from_timestamp_millis(1_788_532_082_617)
        );
        // Percentage as a numeric string is also accepted (toFiniteNumber).
        assert_eq!(data.metrics[1].used, Some(5.0));
        assert!(data.source.contains("opencode.ai/zen/go/v1/usage"));
    }

    #[tokio::test]
    async fn opencode_go_403_entitlement_e_distinguido() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": { "type": "EntitlementError" }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("OPENCODE_GO_API_KEY", "sk-without-go");
        let acc = account(Provider::OpenCodeGo, "OPENCODE_GO_API_KEY");
        let err = opencode_go_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect_err("403 deve falhar");
        assert_eq!(err.status, Status::AuthenticationError);
        assert!(err.note.fallback.contains("subscription"));
    }

    #[tokio::test]
    async fn opencode_go_sem_percentual_valido_e_schema_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "usage": { "rolling": { "status": "ok" }, "future": { "percent": 10 } }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("OPENCODE_GO_API_KEY", "sk-go-test");
        let acc = account(Provider::OpenCodeGo, "OPENCODE_GO_API_KEY");
        let err = opencode_go_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect_err("sem quota válida");
        assert_eq!(err.status, Status::Error);
    }

    // ----------------------------------------------------------------- Groq

    #[tokio::test]
    async fn groq_converte_headers_de_rate_limit_em_metricas() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("Authorization", "Bearer gsk_test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": [] }))
                    .insert_header("x-ratelimit-limit-requests", "14400")
                    .insert_header("x-ratelimit-remaining-requests", "14370")
                    .insert_header("x-ratelimit-reset-requests", "2m59.56s")
                    .insert_header("x-ratelimit-limit-tokens", "18000")
                    .insert_header("x-ratelimit-remaining-tokens", "17997")
                    .insert_header("x-ratelimit-reset-tokens", "7.66s"),
            )
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("GROQ_API_KEY", "gsk_test");
        let acc = account(Provider::Groq, "GROQ_API_KEY");
        let data = groq_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect("sucesso");

        assert_eq!(data.metrics.len(), 2);
        assert!(data.metrics.iter().all(|m| m.kind == MetricKind::RateLimit));
        assert!(data
            .metrics
            .iter()
            .all(|m| m.window == Some(MetricWindow::Other)));
        let req = &data.metrics[0];
        assert_eq!(req.used, Some(30.0));
        assert_eq!(req.limit, Some(14_400.0));
        let tok = &data.metrics[1];
        assert_eq!(tok.used, Some(3.0));
        assert_eq!(tok.limit, Some(18_000.0));
        // Relative durations become real future timestamps.
        for m in &data.metrics {
            let reset = m.resets_at.expect("reset presente");
            assert!(reset > Utc::now());
            assert!(reset < Utc::now() + chrono::Duration::hours(1));
        }
    }

    #[tokio::test]
    async fn groq_header_ausente_nao_vale_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })),
            )
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("GROQ_API_KEY", "gsk_test");
        let acc = account(Provider::Groq, "GROQ_API_KEY");
        let data = groq_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect("chave válida sem buckets");
        // No invented metric; the note explains the state.
        assert!(data.metrics.is_empty());
        assert!(data.notes.iter().any(|n| n.contains("x-ratelimit")));
    }

    #[tokio::test]
    async fn groq_401_e_erro_de_autenticacao() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": { "code": "invalid_api_key" }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("GROQ_API_KEY", "bad");
        let acc = account(Provider::Groq, "GROQ_API_KEY");
        let err = groq_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect_err("401");
        assert_eq!(err.status, Status::AuthenticationError);
    }

    // ---------------------------------------------------------- Kimi Coding

    fn kimi_coding_account() -> Account {
        let mut acc = account(Provider::Kimi, "KIMI_API_KEY");
        acc.params.insert("product".into(), "coding".into());
        acc
    }

    #[tokio::test]
    async fn kimi_coding_apikey_usa_somente_x_api_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("x-api-key", "sk-test-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "user": { "membership": { "level": "LEVEL_ADVANCED" } },
                "usage": { "limit": "100", "used": "35", "remaining": "65", "resetTime": "2026-08-01T00:00:00Z" },
                "limits": [
                    { "window": { "type": "rate" }, "detail": { "limit": "60", "remaining": "40", "resetTime": "2026-07-29T12:00:00Z" } }
                ]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("KIMI_API_KEY", "sk-test-123");
        let acc = kimi_coding_account();
        let data = kimi_coding_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect("sucesso");

        assert_eq!(data.metrics.len(), 2);
        let weekly = &data.metrics[0];
        assert_eq!(weekly.used, Some(35.0));
        assert_eq!(weekly.limit, Some(100.0));
        assert_eq!(weekly.remaining, Some(65.0));
        assert_eq!(weekly.kind, MetricKind::Quota);
        assert_eq!(weekly.window, Some(MetricWindow::Weekly));
        assert_eq!(
            weekly.resets_at,
            DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
                .ok()
                .map(|d| d.with_timezone(&Utc))
        );
        let rate = &data.metrics[1];
        assert_eq!(rate.used, Some(20.0));
        assert_eq!(rate.limit, Some(60.0));
        assert_eq!(rate.remaining, Some(40.0));
        assert_eq!(rate.kind, MetricKind::RateLimit);
        assert_eq!(rate.window, Some(MetricWindow::Other));
        assert!(data.notes.iter().any(|n| n.contains("Allegro")));
    }

    async fn kimi_metrics_of(body: serde_json::Value) -> Vec<Metric> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let creds = TestCredentials("KIMI_API_KEY", "sk-test-123");
        kimi_coding_at(
            &kimi_coding_account(),
            &ctx_with(&http, &creds),
            &server.uri(),
        )
        .await
        .expect("sucesso")
        .metrics
    }

    #[tokio::test]
    async fn kimi_coding_weekly_used_absent_comes_from_remaining_never_from_zero() {
        let metrics = kimi_metrics_of(serde_json::json!({
            "usage": { "limit": 100, "remaining": 30 }
        }))
        .await;
        assert_eq!(metrics[0].used, Some(70.0));
        assert_eq!(metrics[0].remaining, Some(30.0));
        assert_eq!(
            crate::metrics::classify_metric(&metrics[0]),
            crate::metrics::MetricState::Available {
                ratio: 0.3,
                uncapped: false
            }
        );
    }

    #[tokio::test]
    async fn kimi_coding_limit_without_consumption_stays_unmeasured() {
        let metrics = kimi_metrics_of(serde_json::json!({
            "usage": { "limit": 100 },
            "limits": [{ "detail": { "limit": 60 } }]
        }))
        .await;
        assert_eq!(metrics.len(), 2);
        for metric in &metrics {
            assert_eq!((metric.used, metric.remaining), (None, None));
            assert_eq!(
                crate::metrics::classify_metric(metric),
                crate::metrics::MetricState::Unmeasured
            );
        }
        assert_eq!(metrics[0].limit, Some(100.0));
        assert_eq!(metrics[1].limit, Some(60.0));
    }

    #[tokio::test]
    async fn kimi_coding_oauth_usa_bearer_e_x_msh() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("Authorization", "Bearer tok-abc"))
            .and(header("X-Msh-Device-Id", "stable-device-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "usage": { "limit": 100, "used": 35, "remaining": 65 }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        // No KIMI_API_KEY; only the OAuth token.
        let creds = TestCredentials("KIMI_TOKEN", "tok-abc");
        let mut acc = kimi_coding_account();
        acc.params.insert("token_env".into(), "KIMI_TOKEN".into());
        acc.params
            .insert("device_id".into(), "stable-device-1".into());
        let data = kimi_coding_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect("sucesso");
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].remaining, Some(65.0));
    }

    #[tokio::test]
    async fn kimi_coding_sem_product_coding_exige_configuracao() {
        let http = reqwest::Client::new();
        let creds = TestCredentials("KIMI_API_KEY", "sk-test-123");
        let acc = account(Provider::Kimi, "KIMI_API_KEY");
        let err = kimi_coding_at(&acc, &ctx_with(&http, &creds), "http://127.0.0.1:1")
            .await
            .expect_err("opt-in obrigatório");
        assert_eq!(err.status, Status::NeedsConfiguration);
    }

    #[tokio::test]
    async fn kimi_coding_401_e_403_tem_status_distintos_na_mensagem() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "code": "unauthenticated",
                "details": [ { "debug": { "reason": "REASON_INVALID_AUTH_TOKEN" } } ]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("KIMI_API_KEY", "expired");
        let acc = kimi_coding_account();
        let err = kimi_coding_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect_err("401");
        assert_eq!(err.status, Status::AuthenticationError);
        assert!(err.note.fallback.contains("expired"));
    }

    #[tokio::test]
    async fn kimi_coding_403_sem_permissao_nao_e_sessao_expirada() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "code": "permission_denied",
                "details": [ {
                    "debug": {
                        "reason": "REASON_FEATURE_NO_PERMISSION",
                        "localizedMessage": { "message": "You do not have permission." }
                    }
                } ]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("KIMI_API_KEY", "valid-no-sub");
        let acc = kimi_coding_account();
        let err = kimi_coding_at(&acc, &ctx_with(&http, &creds), &server.uri())
            .await
            .expect_err("403");
        assert!(err.note.fallback.contains("Kimi Code subscription"));
        assert!(!err.note.fallback.contains("expired"));
    }

    // ---------------------------------------------------------- Xiaomi MiMo

    #[tokio::test]
    async fn mimo_sessao_cookie_no_cluster_regional() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("Cookie", "serviceToken=abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "percent": 94, "resetDate": "2026-10-05" }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("MIMO_SESSION", "serviceToken=abc");
        let mut acc = account(Provider::XiaomiMimo, "XIAOMI_MIMO_API_KEY");
        acc.params
            .insert("session_env".into(), "MIMO_SESSION".into());
        acc.params.insert("region".into(), "sgp".into());
        // Injects the mock's base as if it were the cluster (test envelope).
        let data = mimo_session_at(
            &ctx_with(&http, &creds),
            &server.uri(),
            "serviceToken=abc",
            "sgp",
        )
        .await
        .expect("sucesso");
        // PRD 0052 RF3.6: the source is the cluster template, never the URL
        // the request went to, and the cookie stays out.
        assert_eq!(data.source, MIMO_SESSION_USAGE_SOURCE);
        data.assert_id_free(&["serviceToken", &server.uri()]);

        let m = &data.metrics[0];
        // percent = RESTANTE: 94 restantes → 6 usados.
        assert_eq!(m.used, Some(6.0));
        assert_eq!(m.remaining, Some(94.0));
        assert_eq!(m.kind, MetricKind::Quota);
        assert_eq!(m.window, Some(MetricWindow::Weekly));
        assert_eq!(m.limit, Some(100.0));
        assert_eq!(
            m.resets_at,
            DateTime::parse_from_rfc3339("2026-10-05T00:00:00Z")
                .ok()
                .map(|d| d.with_timezone(&Utc))
        );
    }

    #[tokio::test]
    async fn mimo_reset_at_epoch_tem_precedencia_sobre_reset_date() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "percent": "50", "resetDate": "2026-10-05", "resetAt": 1_800_000_000 }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("XIAOMI_MIMO_API_KEY", "sk-mimo");
        let acc = account(Provider::XiaomiMimo, "XIAOMI_MIMO_API_KEY");
        let _ = acc;
        let data = mimo_aistudio_at(&ctx_with(&http, &creds), &server.uri(), "sk-mimo")
            .await
            .expect("sucesso");
        let m = &data.metrics[0];
        assert_eq!(m.used, Some(50.0));
        assert_eq!(m.resets_at, DateTime::from_timestamp(1_800_000_000, 0));
    }

    #[tokio::test]
    async fn mimo_401_na_sessao_reporta_expiracao() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("XIAOMI_MIMO_API_KEY", "sk-mimo");
        let err = mimo_aistudio_at(&ctx_with(&http, &creds), &server.uri(), "sk-mimo")
            .await
            .expect_err("401");
        assert_eq!(err.status, Status::AuthenticationError);
        assert!(err.note.fallback.contains("session_env"));
    }

    #[tokio::test]
    async fn mimo_percent_ausente_nao_vale_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "resetDate": "2026-10-05" }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = TestCredentials("XIAOMI_MIMO_API_KEY", "sk-mimo");
        let err = mimo_aistudio_at(&ctx_with(&http, &creds), &server.uri(), "sk-mimo")
            .await
            .expect_err("sem percent");
        assert_eq!(err.status, Status::Error);
    }

    #[test]
    fn mimo_regiao_invalida_e_config_error() {
        let http = reqwest::Client::new();
        let creds = TestCredentials("MIMO_SESSION", "serviceToken=abc");
        let mut acc = account(Provider::XiaomiMimo, "XIAOMI_MIMO_API_KEY");
        acc.params
            .insert("session_env".into(), "MIMO_SESSION".into());
        acc.params.insert("region".into(), "xx".into());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(xiaomi_mimo(&acc, &ctx_with(&http, &creds)))
            .expect_err("região inválida");
        assert_eq!(err.status, Status::NeedsConfiguration);
    }

    #[tokio::test]
    async fn mimo_chave_nativa_nao_faz_requisicao_e_orienta_config() {
        // Account created by native login: oauth present, credential is sk-.
        // The query must fail on configuration WITHOUT network traffic and WITHOUT
        // asking for a new login code.
        let http = reqwest::Client::new();
        let creds = TestCredentials("XIAOMI_MIMO_API_KEY", "sk-mimo-native");
        let mut acc = account(Provider::XiaomiMimo, "XIAOMI_MIMO_API_KEY");
        acc.oauth = Some(crate::oauth::OAuthConfig::new("mimocode".into()));
        let err = xiaomi_mimo(&acc, &ctx_with(&http, &creds))
            .await
            .expect_err("chave nativa não lê quota");
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("platform_session_env"));
        assert!(err.note.fallback.contains("session_env"));
        assert!(err.note.fallback.contains("do not repeat the login"));
    }

    #[tokio::test]
    async fn mimo_auth_method_native_key_param_tambem_evita_requisicao() {
        // Even without oauth on the account, params.auth_method=native_key marks the sk-.
        let http = reqwest::Client::new();
        let creds = TestCredentials("XIAOMI_MIMO_API_KEY", "sk-mimo-native");
        let mut acc = account(Provider::XiaomiMimo, "XIAOMI_MIMO_API_KEY");
        acc.params.insert("auth_method".into(), "native_key".into());
        let err = xiaomi_mimo(&acc, &ctx_with(&http, &creds))
            .await
            .expect_err("native_key marcado");
        assert_eq!(err.status, Status::NeedsConfiguration);
    }

    #[tokio::test]
    async fn mimo_platform_session_env_seleciona_modo_plataforma_sem_rede() {
        // Cookie without the fields the platform requires: local validation of the
        // platform mode rejects before any request, proving the
        // routing (the weekly mode would not know api-platform_serviceToken).
        let http = reqwest::Client::new();
        let creds = TestCredentials("MIMO_PLATFORM", "session=abc");
        let mut acc = account(Provider::XiaomiMimo, "XIAOMI_MIMO_API_KEY");
        acc.params
            .insert("platform_session_env".into(), "MIMO_PLATFORM".into());
        let err = xiaomi_mimo(&acc, &ctx_with(&http, &creds))
            .await
            .expect_err("cookie incompleto");
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("api-platform_serviceToken"));
    }

    #[tokio::test]
    async fn mimo_session_env_tem_precedencia_sobre_platform() {
        // With both cookies configured, the weekly mode (session_env) is
        // evaluated first — the invalid region betrays the chosen path.
        let http = reqwest::Client::new();
        let creds = TestCredentials("MIMO_SESSION", "serviceToken=abc");
        let mut acc = account(Provider::XiaomiMimo, "XIAOMI_MIMO_API_KEY");
        acc.params
            .insert("session_env".into(), "MIMO_SESSION".into());
        acc.params
            .insert("platform_session_env".into(), "MIMO_SESSION".into());
        acc.params.insert("region".into(), "xx".into());
        let err = xiaomi_mimo(&acc, &ctx_with(&http, &creds))
            .await
            .expect_err("região inválida");
        assert!(err.note.fallback.contains("params.region"));
    }
}
