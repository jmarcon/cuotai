//! Read-only Meta Model API (Muse models) key check and console usage
//! (PRD 0049).
//!
//! Meta documents no usage, spend, credit or balance endpoint for the
//! Model API (<https://dev.meta.ai/docs/pricing-rate-limits>); usage is only
//! shown in the dev.meta.ai console. The adapter makes one documented,
//! token-free read with the key:
//!
//! - `GET https://api.meta.ai/v1/models` with `Authorization: Bearer <key>`
//!   (<https://dev.meta.ai/docs/api-reference/models/list-models>);
//! - the four documented `x-ratelimit-{limit,remaining}-{requests,tokens}`
//!   headers become rate-limit metrics when the response carries them. Meta
//!   documents them on token-based responses, so the model listing may omit
//!   them: an absent header is "unknown", never zero.
//!
//! PRD 0049 RF7–RF9: with the console session (`params.session_env`, the
//! `llama_dev_sess` cookie captured in a Cuotai sign-in window), it also
//! reads the console's own backend, an **undocumented** interface found in
//! the console bundle (`/_next/static/immutable/chunks/*.js`, 2026-10-27):
//!
//! - `GET https://dev.meta.ai/api/portal/teams` answers `{teams:[{team_id,
//!   role, ...}]}` (the bundle reads `.teams` and `team_id`); it picks the
//!   team when `params.team_id` is empty;
//! - `GET https://dev.meta.ai/api/portal/teams/{team_id}/usage/batch?metrics=...
//!   &start_date=YYYY-MM-DD&end_date=YYYY-MM-DD&timezone=<IANA>` with the
//!   `Referer` of the usage page: the request the signed-in console makes
//!   (confirmed by the owner, 2026-10-27). It answers `{as_of, models,
//!   metrics: {<NAME>: {metric, series: [{type: "TOTAL", data_points:
//!   [{date, value[, amount]}]}]}}}`; the cost carries `currency: {code,
//!   offset}`. Cuotai sums the `TOTAL` series of the current month (the
//!   cost from `amount` only); an empty series is unknown, never zero. That `offset: 100` means amounts in
//!   cents is inferred (the confirmed sample was all zeros).
//!
//! The console's GETs carry only the cookie and `Content-Type:
//! application/json` (no CSRF token). No inference, purchase or billing
//! endpoint is called, and model ids are never copied into notes or metrics.

use super::{secret, send, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Status};
use chrono::{DateTime, Datelike, Months, NaiveDate, Utc};
use serde_json::Value;

/// The console origin; in tests the mock's URL is injected.
pub(crate) const META_CONSOLE_BASE: &str = "https://dev.meta.ai";
/// The session slot of the console (`crate::user_session` descriptor).
const SESSION_SLOT: &str = "session_env";
/// Browser UA: the console backend serves its own web pages.
const CONSOLE_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";
pub(crate) const META_MODELS_URL: &str = "https://api.meta.ai/v1/models";
/// Largest model listing read; a bigger body is a schema error.
const BODY_LIMIT: usize = 1024 * 1024;

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    fetch_at(account, ctx, META_MODELS_URL, META_CONSOLE_BASE).await
}

/// The key check, then the console usage when the account has a session.
pub(crate) async fn fetch_at(
    account: &Account,
    ctx: &Context<'_>,
    models_url: &str,
    console: &str,
) -> Result<Data, QueryError> {
    // A malformed session is refused before any request.
    let session = super::web_session(account, ctx, SESSION_SLOT)?;
    // PRD 0049 RF8: an invalid Team ID is refused before any request, the
    // key check included.
    let team = match param_id(account, "team_id") {
        Param::Set(team) => Some(team),
        Param::Empty => None,
        Param::Invalid if session.is_none() => None,
        Param::Invalid => return Err(QueryError {
            diagnostic: None,
            status: Status::NeedsConfiguration,
            note: Localized::fixed(
                "query.meta.team_id_invalid",
                "The Team ID is not a dev.meta.ai team id; copy the number after team_id= in the usage page address.",
            ),
        }),
    };
    // The console session reads usage on its own; without a key the key
    // check is skipped instead of failing the whole account (the session
    // test before save runs with the typed session only).
    let has_key = secret(account, ctx, "META_API_KEY").is_ok();
    let (mut metrics, mut notes) = if session.is_some() && !has_key {
        let mut notes = Notes::new();
        notes.push(
            "note.meta.no_key_session_only",
            "No API key for this account; usage comes from the dev.meta.ai session and the key check is skipped.",
        );
        (Vec::new(), notes)
    } else {
        key_check(account, ctx, models_url).await?
    };
    match session {
        Some(session) => {
            let usage = console_usage(account, ctx, console, &session.cookie, team, has_key).await?;
            metrics.extend(usage.0);
            notes.extend(usage.1);
        }
        None => notes.push(
            "note.meta.sign_in_for_usage",
            "Meta documents no usage, spend or balance API; sign in to dev.meta.ai from the account to read the console usage.",
        ),
    }
    Ok(Data::new(metrics, notes, META_MODELS_URL))
}

/// The key check alone (the account without a session).
#[cfg(test)]
pub(crate) async fn meta_at(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
) -> Result<Data, QueryError> {
    let (metrics, notes) = key_check(account, ctx, url).await?;
    Ok(Data::new(metrics, notes, META_MODELS_URL))
}

async fn key_check(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
) -> Result<(Vec<Metric>, Notes), QueryError> {
    let key = secret(account, ctx, "META_API_KEY")?;
    let request = ctx
        .http
        .get(url)
        .bearer_auth(key.trim())
        .header("Accept", "application/json")
        .timeout(ctx.timeout);
    let response = send(request).await?;
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
                "query.meta.http",
                params([("code", ParamValue::from(code.as_u16()))]),
                "Meta Model API returned HTTP {code}.",
            ),
        }
        .with_retry_after(super::retry_after_seconds(response.headers())));
    }
    let headers = response.headers().clone();
    // Bounded while streaming: an oversize body is a schema error.
    let bytes = super::read_body(response, BODY_LIMIT).await?;
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| QueryError::schema())?;
    if !body.get("data").is_some_and(Value::is_array) {
        return Err(QueryError::schema_at(&body, &["data"], "array"));
    }

    let mut metrics = Vec::new();
    for (label, unit, limit_h, remaining_h) in [
        (
            "Requisições",
            "requisições",
            "x-ratelimit-limit-requests",
            "x-ratelimit-remaining-requests",
        ),
        (
            "Tokens",
            "tokens",
            "x-ratelimit-limit-tokens",
            "x-ratelimit-remaining-tokens",
        ),
    ] {
        let (Some(limit), Some(remaining)) = (
            header_number(&headers, limit_h),
            header_number(&headers, remaining_h),
        ) else {
            continue;
        };
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
            resets_at: None,
            derived_from: None,
            unlimited: false,
        });
    }

    let mut notes = Notes::new();
    notes.push(
        "note.meta.key_valid",
        "Key accepted by the Meta Model API model listing; no tokens are used.",
    );
    if metrics.is_empty() {
        notes.push(
            "note.meta.rate_limit_headers_missing",
            "This response carried no `x-ratelimit-*` headers; the team's rate-limit window is unknown.",
        );
    } else {
        notes.push(
            "note.meta.rate_limit_team",
            "Rate limits are per team and shared by every key of the team.",
        );
    }
    Ok((metrics, notes))
}

/// PRD 0049 RF8/RF9: the console usage of the account's team, as metrics
/// and notes. An expired session is an authentication error. Any other
/// unusable console answer degrades to a note when the key check ran
/// (`key_checked`), so the key check still shows; without a key it is the
/// account's error, since nothing was read.
async fn console_usage(
    account: &Account,
    ctx: &Context<'_>,
    console: &str,
    cookie: &str,
    team: Option<String>,
    key_checked: bool,
) -> Result<(Vec<Metric>, Notes), QueryError> {
    let console = console.trim_end_matches('/');
    let mut notes = Notes::new();
    notes.push(
        "note.meta.console_undocumented",
        "Usage read from the dev.meta.ai console's internal interface; it may change without notice.",
    );
    let team = match team {
        Some(team) => team,
        None => {
            let body = match console_get(
                ctx,
                &format!("{console}/api/portal/teams"),
                &[],
                cookie,
                None,
            )
            .await?
            {
                Ok(body) => body,
                Err(error) if !key_checked => return Err(error),
                Err(_) => {
                    notes.push(
                        "note.meta.teams_unavailable",
                        "The dev.meta.ai console did not list the teams; set the Team ID from the usage page address.",
                    );
                    return Ok((Vec::new(), notes));
                }
            };
            let teams = team_ids(&body);
            match teams.as_slice() {
                [team] => team.clone(),
                [] => {
                    notes.push(
                        "note.meta.teams_unavailable",
                        "The dev.meta.ai console did not list the teams; set the Team ID from the usage page address.",
                    );
                    return Ok((Vec::new(), notes));
                }
                many => {
                    // A plural (PRD 0035 re-validation V35-12): ru "21 команде".
                    notes.push_note(Localized::plural(
                        "note.meta.several_teams",
                        params([("count", ParamValue::from(many.len() as u64))]),
                        "The session belongs to {count} team; set the Team ID from the usage page address to choose one.",
                        "The session belongs to {count} teams; set the Team ID from the usage page address to choose one.",
                    ));
                    return Ok((Vec::new(), notes));
                }
            }
        }
    };
    // The console's own usage request carries only the team (owner,
    // 2026-10-27) and has no project filter, so a legacy `project_id` is
    // ignored, never sent, and the card says so (re-validation R49-02).
    if account
        .params
        .get("project_id")
        .is_some_and(|v| !v.trim().is_empty())
    {
        notes.push(
            "note.meta.project_ignored",
            "The Project ID is ignored: the dev.meta.ai console reports usage per team, so the card shows the whole team.",
        );
    }
    let period = MonthToDate::at(Utc::now());
    let query = usage_query(&period);
    let url = format!("{console}/api/portal/teams/{team}/usage/batch");
    let referer = format!("{console}/usage?team_id={team}");
    let body = match console_get(ctx, &url, &query, cookie, Some(&referer)).await? {
        Ok(body) => body,
        Err(error) if !key_checked => return Err(error),
        Err(_) => {
            notes.push(
                "note.meta.usage_unavailable",
                "The dev.meta.ai console did not return the usage; the card shows only the key check.",
            );
            return Ok((Vec::new(), notes));
        }
    };
    let Some(series) = body.get("metrics").and_then(Value::as_object) else {
        if cfg!(debug_assertions) {
            // Debug builds only: the field names (never values) of the
            // unrecognised answer, so the owner can report its shape.
            eprintln!(
                "meta usage: response without a metrics object, keys {:?}",
                key_paths(&body)
            );
        }
        return Err(QueryError::schema());
    };
    if cfg!(debug_assertions) {
        let unknown: Vec<&str> = series
            .keys()
            .map(String::as_str)
            .filter(|k| !USAGE_METRICS.contains(k))
            .collect();
        if !unknown.is_empty() {
            eprintln!("meta usage: unrecognised metric keys {unknown:?}");
        }
    }
    let metrics = usage_metrics(series, &period);
    if metrics.is_empty() {
        notes.push(
            "note.meta.usage_empty",
            "The dev.meta.ai console returned no usage series for this month; usage is unknown, not zero.",
        );
    }
    Ok((metrics, notes))
}

/// The query of the console's `usage/batch` request.
fn usage_query(period: &MonthToDate) -> [(&'static str, String); 4] {
    [
        ("metrics", USAGE_METRICS.join(",")),
        ("start_date", period.start.format("%Y-%m-%d").to_string()),
        ("end_date", period.end.format("%Y-%m-%d").to_string()),
        ("timezone", USAGE_TIMEZONE.to_string()),
    ]
}

enum Param {
    Empty,
    Set(String),
    Invalid,
}

/// A console id from `params.<key>`: digits, letters, `_` or `-`, at most
/// 64 characters, so it can only name a path segment or a query value.
fn param_id(account: &Account, key: &str) -> Param {
    match account.params.get(key).map(|v| v.trim()) {
        None | Some("") => Param::Empty,
        Some(v) if valid_id(v) => Param::Set(v.to_string()),
        Some(_) => Param::Invalid,
    }
}

fn valid_id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `teams[].team_id` (string or number) of the console's team listing.
fn team_ids(body: &Value) -> Vec<String> {
    body.get("teams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|team| match team.get("team_id")? {
            Value::String(s) => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|id| valid_id(id))
        .collect()
}

/// A console GET with the session cookie. The outer `Err` is an expired or
/// rejected session (or a refused redirect, or a transport failure); the
/// inner `Err` an answer that is not usable: another HTTP code (429 is a
/// rate limit with its `Retry-After`) or a 2xx body that is not a JSON
/// object (a schema error). The caller decides whether the inner one fails
/// the account.
async fn console_get(
    ctx: &Context<'_>,
    url: &str,
    query: &[(&str, String)],
    cookie: &str,
    referer: Option<&str>,
) -> Result<Result<Value, QueryError>, QueryError> {
    let mut request = ctx
        .http
        .get(url)
        .query(query)
        .header("Cookie", cookie.trim())
        .header("Accept", "application/json")
        // What the console's own fetch helper sends (module 415234).
        .header("Content-Type", "application/json")
        .header("User-Agent", CONSOLE_UA)
        .timeout(ctx.timeout);
    if let Some(referer) = referer {
        request = request.header("Referer", referer);
    }
    // A redirect (the sign-in of an expired session) is refused, never followed.
    let response = super::send_session(request, crate::Provider::Meta, SESSION_SLOT).await?;
    let code = response.status().as_u16();
    match code {
        401 | 403 => Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.meta.session_expired",
                "dev.meta.ai session expired or revoked; sign in again from the account.",
            ),
        }),
        200..=299 => Ok(super::body_value(response).await.and_then(|body| {
            if body.is_object() {
                Ok(body)
            } else {
                Err(QueryError::schema())
            }
        })),
        _ => Ok(Err(QueryError {
            diagnostic: None,
            status: if code == 429 {
                Status::RateLimited
            } else {
                Status::Error
            },
            note: Localized::with_params(
                "query.meta.console_http",
                params([("code", ParamValue::from(code))]),
                "The dev.meta.ai console returned HTTP {code}; nothing was read.",
            ),
        }
        .with_retry_after(super::retry_after_seconds(response.headers())))),
    }
}

/// The metrics the console's usage page asks for (owner, 2026-10-27), in
/// its order.
const USAGE_METRICS: [&str; 15] = [
    "REQUESTS",
    "USAGE_BILLABLE_COST",
    "INPUT_TOKENS",
    "CACHED_PROMPT_TOKENS",
    "UNCACHED_PROMPT_TOKENS",
    "BATCH_INPUT_TOKENS",
    "BATCH_CACHED_PROMPT_TOKENS",
    "BATCH_UNCACHED_PROMPT_TOKENS",
    "OUTPUT_TOKENS",
    "BATCH_OUTPUT_TOKENS",
    "IMAGES_GENERATED_STANDARD",
    "SEARCH_GROUNDING_REQUESTS",
    "AUDIO_TRANSCRIPTION_SECONDS",
    "VIDEO_FRAMES_PROCESSED",
    "IMAGES_PROCESSED",
];

/// No adapter reads the system's IANA zone (`iana-time-zone` is not a
/// direct dependency), so the month is a UTC month, as in the other
/// month-to-date adapters.
const USAGE_TIMEZONE: &str = "UTC";

/// The calendar month to date (UTC): the first day, today and the next
/// month's start (the reset).
struct MonthToDate {
    start: NaiveDate,
    end: NaiveDate,
    resets_at: DateTime<Utc>,
}

impl MonthToDate {
    fn at(now: DateTime<Utc>) -> Self {
        let end = now.date_naive();
        let start = end.with_day(1).expect("day 1 of any month is valid");
        let resets_at = (start + Months::new(1))
            .and_hms_opt(0, 0, 0)
            .expect("midnight is valid")
            .and_utc();
        Self {
            start,
            end,
            resets_at,
        }
    }

    fn label(&self) -> String {
        format!(
            "mês corrente ({} a {}, {USAGE_TIMEZONE})",
            self.start.format("%Y-%m-%d"),
            self.end.format("%Y-%m-%d")
        )
    }
}

/// A non-negative finite number from a JSON number or numeric string.
fn number(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    (n.is_finite() && n >= 0.0).then_some(n)
}

/// The sum of `field` over the `TOTAL` series of one metric. `None` when
/// the metric, its `TOTAL` series or every usable data point is missing: an
/// empty series is unknown, never zero. Points explicitly reported as `0`
/// count. The cost reads `amount` only (the figure the console's
/// `{symbol}{amount}` format shows): a cost point without `amount` is not
/// counted, since nothing says which unit its `value` is in (PRD 0049
/// re-validation R49-03). The usage counters read `value`.
fn total(metric: &Value, field: &str) -> Option<f64> {
    let series = metric.get("series")?.as_array()?;
    let total = series
        .iter()
        .find(|s| s.get("type").and_then(Value::as_str) == Some("TOTAL"))?;
    let points = total.get("data_points")?.as_array()?;
    let mut sum = None;
    for point in points {
        if let Some(v) = point.get(field).and_then(number) {
            sum = Some(sum.unwrap_or(0.0) + v);
        }
    }
    sum
}

/// PRD 0049 RF9: the month-to-date totals of the console's
/// `usage/batch` answer (`metrics.<NAME>.series[type=TOTAL].data_points`).
fn usage_metrics(series: &serde_json::Map<String, Value>, period: &MonthToDate) -> Vec<Metric> {
    let metric = |label: &str, used: f64, unit: &str, kind: MetricKind| Metric {
        billing: Default::default(),
        scope: None,
        label: label.into(),
        used: Some(used),
        limit: None,
        remaining: None,
        unit: unit.into(),
        kind,
        window: Some(MetricWindow::Monthly),
        period: Some(period.label()),
        resets_at: Some(period.resets_at),
        derived_from: None,
        unlimited: false,
    };
    let mut out = Vec::new();
    if let Some(cost) = series.get("USAGE_BILLABLE_COST") {
        let sum = total(cost, "amount");
        if sum.is_none() && cfg!(debug_assertions) {
            // Debug builds only: the field names (never values) of the cost
            // answer, so the owner can report a shape without `amount`.
            eprintln!(
                "meta usage: USAGE_BILLABLE_COST without amount, keys {:?}",
                key_paths(cost)
            );
        }
        if let Some(sum) = sum {
            let currency = cost.get("currency");
            // Inferred (the confirmed sample is all zeros): `offset: 100`
            // says the amounts are minor units (cents).
            let offset = currency
                .and_then(|c| c.get("offset"))
                .and_then(number)
                .filter(|o| *o > 0.0)
                .unwrap_or(1.0);
            let code = currency
                .and_then(|c| c.get("code"))
                .and_then(Value::as_str)
                .map(|c| c.trim().to_ascii_uppercase())
                .filter(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_alphabetic()))
                // Meta prices the Model API in USD (dev.meta.ai/docs/pricing-rate-limits).
                .unwrap_or_else(|| "USD".into());
            out.push(metric("Gasto", sum / offset, &code, MetricKind::Spend));
        }
    }
    for (key, label, unit) in [
        ("REQUESTS", "Requisições", "requisições"),
        ("INPUT_TOKENS", "Tokens de entrada", "tokens"),
        (
            "CACHED_PROMPT_TOKENS",
            "Tokens de entrada em cache",
            "tokens",
        ),
        (
            "UNCACHED_PROMPT_TOKENS",
            "Tokens de entrada sem cache",
            "tokens",
        ),
        ("OUTPUT_TOKENS", "Tokens de saída", "tokens"),
        ("BATCH_INPUT_TOKENS", "Tokens de entrada (lote)", "tokens"),
        (
            "BATCH_CACHED_PROMPT_TOKENS",
            "Tokens de entrada em cache (lote)",
            "tokens",
        ),
        (
            "BATCH_UNCACHED_PROMPT_TOKENS",
            "Tokens de entrada sem cache (lote)",
            "tokens",
        ),
        ("BATCH_OUTPUT_TOKENS", "Tokens de saída (lote)", "tokens"),
        ("IMAGES_GENERATED_STANDARD", "Imagens geradas", "imagens"),
        (
            "SEARCH_GROUNDING_REQUESTS",
            "Buscas de grounding",
            "requisições",
        ),
        ("AUDIO_TRANSCRIPTION_SECONDS", "Transcrição de áudio", "s"),
        (
            "VIDEO_FRAMES_PROCESSED",
            "Quadros de vídeo processados",
            "quadros",
        ),
        ("IMAGES_PROCESSED", "Imagens processadas", "imagens"),
    ] {
        if let Some(sum) = series.get(key).and_then(|m| total(m, "value")) {
            out.push(metric(label, sum, unit, MetricKind::Usage));
        }
    }
    out
}

/// Field names (never values) of `body`, two levels deep, for the debug
/// diagnostic.
fn key_paths(body: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(o) = body.as_object() {
        for (k, v) in o {
            out.push(k.clone());
            let inner = match v {
                Value::Array(a) => a.first(),
                other => Some(other),
            };
            if let Some(io) = inner.and_then(Value::as_object) {
                out.extend(io.keys().map(|ik| format!("{k}.{ik}")));
            }
        }
    }
    out.truncate(64);
    out
}

fn header_number(headers: &reqwest::header::HeaderMap, name: &str) -> Option<f64> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// PRD 0028 RF11: the fixture whose metric identities `label_snapshot`
/// records: every month-to-date series the console reports.
#[cfg(test)]
pub(super) fn label_fixtures() -> Vec<(&'static str, Vec<Metric>)> {
    use chrono::TimeZone;
    let total = |value: f64| serde_json::json!({"series": [{"type": "TOTAL", "data_points": [{"value": value}]}]});
    let mut series = serde_json::Map::new();
    series.insert(
        "USAGE_BILLABLE_COST".into(),
        serde_json::json!({"currency": {"code": "usd", "offset": 100},
            "series": [{"type": "TOTAL", "data_points": [{"amount": 250}]}]}),
    );
    for key in [
        "REQUESTS",
        "INPUT_TOKENS",
        "CACHED_PROMPT_TOKENS",
        "UNCACHED_PROMPT_TOKENS",
        "OUTPUT_TOKENS",
        "BATCH_INPUT_TOKENS",
        "BATCH_CACHED_PROMPT_TOKENS",
        "BATCH_UNCACHED_PROMPT_TOKENS",
        "BATCH_OUTPUT_TOKENS",
        "IMAGES_GENERATED_STANDARD",
        "SEARCH_GROUNDING_REQUESTS",
        "AUDIO_TRANSCRIPTION_SECONDS",
        "VIDEO_FRAMES_PROCESSED",
        "IMAGES_PROCESSED",
    ] {
        series.insert(key.into(), total(3.0));
    }
    let now = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap();
    vec![("meta", usage_metrics(&series, &MonthToDate::at(now)))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Provider};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Key(&'static str);
    impl Credentials for Key {
        fn get(&self, name: &str) -> Option<String> {
            (name == "META_API_KEY").then(|| self.0.to_string())
        }
    }

    fn account() -> Account {
        Account {
            id: None,
            name: "muse".into(),
            provider: Provider::Meta,
            credential_env: Some("META_API_KEY".into()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
            budgets: Vec::new(),
            floors: Vec::new(),
            breakdown: Vec::new(),
        }
    }

    async fn run(template: ResponseTemplate) -> Result<Data, QueryError> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(template)
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let creds = Key("test-key");
        let ctx = Context {
            http: &http,
            direct: &http,
            credentials: &creds,
            timeout: Duration::from_secs(5),
        };
        meta_at(&account(), &ctx, &format!("{}/v1/models", server.uri())).await
    }

    fn models() -> serde_json::Value {
        serde_json::json!({ "object": "list", "data": [{ "id": "example-model" }] })
    }

    #[tokio::test]
    async fn rate_limit_headers_become_metrics() {
        let data = run(ResponseTemplate::new(200)
            .set_body_json(models())
            .insert_header("x-ratelimit-limit-requests", "3000")
            .insert_header("x-ratelimit-remaining-requests", "2990")
            .insert_header("x-ratelimit-limit-tokens", "1000000")
            .insert_header("x-ratelimit-remaining-tokens", "999000"))
        .await
        .expect("valid");
        assert_eq!(data.metrics.len(), 2);
        assert!(data.metrics.iter().all(|m| m.kind == MetricKind::RateLimit));
        assert_eq!(data.metrics[0].used, Some(10.0));
        assert_eq!(data.metrics[0].limit, Some(3000.0));
        assert_eq!(data.metrics[1].remaining, Some(999_000.0));
        assert!(data.notes.iter().all(|n| !n.contains("example-model")));
    }

    #[tokio::test]
    async fn absent_headers_are_unknown_not_zero() {
        let data = run(ResponseTemplate::new(200).set_body_json(models()))
            .await
            .expect("valid key");
        assert!(data.metrics.is_empty());
        assert!(data.notes.iter().any(|n| n.contains("x-ratelimit")));
    }

    #[tokio::test]
    async fn partial_headers_skip_the_bucket() {
        let data = run(ResponseTemplate::new(200)
            .set_body_json(models())
            .insert_header("x-ratelimit-limit-requests", "3000")
            .insert_header("x-ratelimit-limit-tokens", "1000")
            .insert_header("x-ratelimit-remaining-tokens", "abc"))
        .await
        .expect("valid key");
        assert!(data.metrics.is_empty());
    }

    #[tokio::test]
    async fn unauthorized_is_an_authentication_error() {
        for code in [401, 403] {
            let err = run(ResponseTemplate::new(code).set_body_string("{\"error\":\"secret\"}"))
                .await
                .expect_err("rejected");
            assert_eq!(err.status, Status::AuthenticationError);
            assert!(!err.note.text().contains("secret"));
        }
    }

    #[tokio::test]
    async fn too_many_requests_is_rate_limited() {
        let err = run(ResponseTemplate::new(429).insert_header("retry-after", "30"))
            .await
            .expect_err("429");
        assert_eq!(err.status, Status::RateLimited);
    }

    /// PRD 0038 re-validation (R-06): Meta reads `Retry-After` with the
    /// shared parser, so the HTTP-date form is honored too.
    #[tokio::test]
    async fn retry_after_in_the_date_form_is_honored() {
        let at = chrono::Utc::now() + chrono::Duration::hours(1);
        let date = at.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let err = run(ResponseTemplate::new(429).insert_header("retry-after", date.as_str()))
            .await
            .expect_err("429");
        assert_eq!(err.status, Status::RateLimited);
        let Some(crate::text::ParamValue::Text(seconds)) =
            err.note.params.get(crate::Report::RETRY_AFTER_PARAM)
        else {
            panic!("the wait is kept");
        };
        let seconds: u64 = seconds.parse().unwrap();
        assert!((3500..=3600).contains(&seconds), "{seconds}");
    }

    #[tokio::test]
    async fn unexpected_body_is_a_schema_error() {
        for body in ["not json", "{\"models\":[]}", "{\"data\":{}}"] {
            let err = run(ResponseTemplate::new(200).set_body_string(body))
                .await
                .expect_err("schema");
            assert_eq!(err.status, Status::Error, "{body}");
        }
    }

    #[tokio::test]
    async fn missing_key_makes_no_request() {
        let http = reqwest::Client::new();
        struct None_;
        impl Credentials for None_ {
            fn get(&self, _: &str) -> Option<String> {
                None
            }
        }
        let ctx = Context {
            http: &http,
            direct: &http,
            credentials: &None_,
            timeout: Duration::from_secs(1),
        };
        let err = meta_at(&account(), &ctx, "https://127.0.0.1:1/v1/models")
            .await
            .expect_err("no key");
        assert_ne!(err.status, Status::Ready);
    }

    // ---- PRD 0049 RF8/RF9: console usage with the dev.meta.ai session.

    struct KeyAndSession;
    impl Credentials for KeyAndSession {
        fn get(&self, name: &str) -> Option<String> {
            match name {
                "META_API_KEY" => Some("test-key".into()),
                "META_SESSION" => Some("llama_dev_sess=placeholder-session".into()),
                _ => None,
            }
        }
    }

    fn session_account(extra: &[(&str, &str)]) -> Account {
        let mut account = account();
        account
            .params
            .insert("session_env".into(), "META_SESSION".into());
        for (k, v) in extra {
            account.params.insert((*k).into(), (*v).into());
        }
        account
    }

    async fn console(
        account: &Account,
        teams: Option<ResponseTemplate>,
        usage: ResponseTemplate,
    ) -> Result<Data, QueryError> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(models()))
            .mount(&server)
            .await;
        if let Some(teams) = teams {
            Mock::given(method("GET"))
                .and(path("/api/portal/teams"))
                .and(header("Cookie", "llama_dev_sess=placeholder-session"))
                .respond_with(teams)
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/api/portal/teams/1001/usage/batch"))
            .and(header("Cookie", "llama_dev_sess=placeholder-session"))
            .and(header("Content-Type", "application/json"))
            .respond_with(usage)
            .mount(&server)
            .await;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let ctx = Context {
            http: &http,
            direct: &http,
            credentials: &KeyAndSession,
            timeout: Duration::from_secs(5),
        };
        fetch_at(
            account,
            &ctx,
            &format!("{}/v1/models", server.uri()),
            &server.uri(),
        )
        .await
    }

    struct SessionOnly;
    impl Credentials for SessionOnly {
        fn get(&self, name: &str) -> Option<String> {
            (name == "META_SESSION").then(|| "llama_dev_sess=placeholder-session".into())
        }
    }

    /// The session test before save carries the typed session only: usage is
    /// read and the key check is skipped, never failed.
    #[tokio::test]
    async fn session_without_key_reads_usage_and_skips_the_key_check() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(models()))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/portal/teams/1001/usage/batch"))
            .respond_with(ResponseTemplate::new(200).set_body_json(zero_sample()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            direct: &http,
            credentials: &SessionOnly,
            timeout: Duration::from_secs(5),
        };
        let data = fetch_at(
            &session_account(&[("team_id", "1001")]),
            &ctx,
            &format!("{}/v1/models", server.uri()),
            &server.uri(),
        )
        .await
        .expect("usage without a key");
        assert!(!data.metrics.is_empty());
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("key check is skipped")));
    }

    fn one_team() -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({ "teams": [{ "team_id": 1001, "role": "OWNER" }] }))
    }

    /// A `usage/batch` metric with one `TOTAL` series.
    fn series(name: &str, points: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "metric": name, "series": [
            { "type": "TOTAL", "identifier": name, "data_points": points }
        ]})
    }

    /// The owner's confirmed sample (2026-10-27), values zeroed.
    fn zero_sample() -> serde_json::Value {
        let zeros = serde_json::json!([
            { "date": "2026-10-22", "value": 0 }, { "date": "2026-10-23", "value": 0 }
        ]);
        let mut cost = series(
            "usage_billable_cost",
            serde_json::json!([{ "date": "2026-10-22", "value": 0, "amount": 0 }]),
        );
        cost["currency"] = serde_json::json!(
            { "code": "USD", "symbol": "$", "format": "{symbol}{amount}", "offset": 100 }
        );
        serde_json::json!({ "as_of": "2026-10-28T00:50:13Z", "models": [], "metrics": {
            "REQUESTS": series("num_requests", zeros.clone()),
            "USAGE_BILLABLE_COST": cost,
            "INPUT_TOKENS": series("num_prompt_tokens", zeros.clone()),
            "CACHED_PROMPT_TOKENS": series("num_cached_prompt_tokens", zeros.clone()),
            "UNCACHED_PROMPT_TOKENS": series("num_uncached_prompt_tokens", zeros.clone()),
            "BATCH_INPUT_TOKENS": { "metric": "num_batch_prompt_tokens", "series": [] },
            "OUTPUT_TOKENS": series("num_completion_tokens", zeros),
        }})
    }

    #[tokio::test]
    async fn session_reads_the_usage_of_the_only_team() {
        let data = console(
            &session_account(&[]),
            Some(one_team()),
            ResponseTemplate::new(200).set_body_json(zero_sample()),
        )
        .await
        .expect("usage");
        let spend = data
            .metrics
            .iter()
            .find(|m| m.kind == MetricKind::Spend)
            .unwrap();
        assert_eq!((spend.used, spend.unit.as_str()), (Some(0.0), "USD"));
        assert_eq!(spend.limit, None);
        assert_eq!(spend.window, Some(MetricWindow::Monthly));
        assert!(spend.period.as_deref().unwrap().contains("UTC"));
        assert!(spend.resets_at.is_some());
        let labels: Vec<_> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Gasto",
                "Requisições",
                "Tokens de entrada",
                "Tokens de entrada em cache",
                "Tokens de entrada sem cache",
                "Tokens de saída"
            ]
        );
        // Explicit zero points are a real zero.
        assert!(data.metrics.iter().all(|m| m.used == Some(0.0)));
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("may change without notice")));
        assert!(data.notes.iter().all(|n| !n.contains("placeholder")));
    }

    #[tokio::test]
    async fn usage_request_matches_the_console() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(models()))
            .mount(&server)
            .await;
        let period = MonthToDate::at(Utc::now());
        let mut mock = Mock::given(method("GET"))
            .and(path("/api/portal/teams/1000000000000001/usage/batch"))
            .and(header("Cookie", "llama_dev_sess=placeholder-session"))
            .and(header("Content-Type", "application/json"))
            .and(header(
                "Referer",
                format!("{}/usage?team_id=1000000000000001", server.uri()).as_str(),
            ));
        for (k, v) in usage_query(&period) {
            mock = mock.and(query_param(k, v.as_str()));
        }
        mock.and(query_param_is_missing("project_id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(zero_sample()))
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let ctx = Context {
            http: &http,
            direct: &http,
            credentials: &KeyAndSession,
            timeout: Duration::from_secs(5),
        };
        let account = session_account(&[("team_id", "1000000000000001"), ("project_id", "2002")]);
        let data = fetch_at(
            &account,
            &ctx,
            &format!("{}/v1/models", server.uri()),
            &server.uri(),
        )
        .await
        .expect("usage");
        assert!(data.metrics.iter().any(|m| m.kind == MetricKind::Spend));
        // PRD 0052 RF3.6: the team and project ids are in the request only.
        data.assert_id_free(&["1000000000000001", "2002", &server.uri()]);
        let q = usage_query(&period);
        assert!(q[0]
            .1
            .starts_with("REQUESTS,USAGE_BILLABLE_COST,INPUT_TOKENS,"));
        assert!(q[0].1.ends_with(",IMAGES_PROCESSED"));
        assert!(q[1].1.ends_with("-01"));
    }

    #[tokio::test]
    async fn cost_amount_is_divided_by_the_currency_offset() {
        let mut cost = series(
            "usage_billable_cost",
            serde_json::json!([
                { "date": "2026-10-01", "value": 1000, "amount": 1000 },
                { "date": "2026-10-02", "value": 234, "amount": 234 }
            ]),
        );
        cost["currency"] = serde_json::json!({ "code": "usd", "offset": 100 });
        let body = serde_json::json!({ "metrics": {
            "USAGE_BILLABLE_COST": cost,
            "REQUESTS": series("num_requests", serde_json::json!([
                { "date": "2026-10-01", "value": 5 }, { "date": "2026-10-02", "value": "2" }
            ])),
            "SOMETHING_NEW": series("x", serde_json::json!([{ "value": 9 }])),
        }});
        let data = console(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(200).set_body_json(body),
        )
        .await
        .expect("usage");
        let spend = data
            .metrics
            .iter()
            .find(|m| m.kind == MetricKind::Spend)
            .unwrap();
        assert_eq!((spend.used, spend.unit.as_str()), (Some(12.34), "USD"));
        assert!(data
            .metrics
            .iter()
            .any(|m| m.label == "Requisições" && m.used == Some(7.0)));
        assert_eq!(data.metrics.len(), 2);
    }

    /// PRD 0049 re-validation R49-03: a cost point without `amount` is not
    /// counted (no unit evidences its `value`), so a series of such points
    /// leaves the spend unknown while the counters still read.
    #[tokio::test]
    async fn cost_points_without_amount_leave_the_spend_unknown() {
        let mut cost = series(
            "usage_billable_cost",
            serde_json::json!([{ "date": "2026-10-01", "value": 12.34 }]),
        );
        cost["currency"] = serde_json::json!({ "code": "USD", "offset": 100 });
        let body = serde_json::json!({ "metrics": {
            "USAGE_BILLABLE_COST": cost,
            "REQUESTS": series("num_requests", serde_json::json!([{ "value": 5 }])),
        }});
        let data = console(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(200).set_body_json(body),
        )
        .await
        .expect("usage");
        assert!(data.metrics.iter().all(|m| m.kind != MetricKind::Spend));
        assert!(data
            .metrics
            .iter()
            .any(|m| m.label == "Requisições" && m.used == Some(5.0)));
    }

    /// Only the cost points that carry `amount` are summed.
    #[tokio::test]
    async fn a_mixed_cost_series_sums_only_the_amounts() {
        let mut cost = series(
            "usage_billable_cost",
            serde_json::json!([
                { "date": "2026-10-01", "value": 1000, "amount": 1000 },
                { "date": "2026-10-02", "value": 999 }
            ]),
        );
        cost["currency"] = serde_json::json!({ "code": "USD", "offset": 100 });
        let body = serde_json::json!({ "metrics": { "USAGE_BILLABLE_COST": cost }});
        let data = console(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(200).set_body_json(body),
        )
        .await
        .expect("usage");
        let spend = data
            .metrics
            .iter()
            .find(|m| m.kind == MetricKind::Spend)
            .unwrap();
        assert_eq!(spend.used, Some(10.0));
    }

    #[tokio::test]
    async fn empty_series_is_unknown_not_zero() {
        let body = serde_json::json!({ "as_of": "x", "models": [], "metrics": {
            "REQUESTS": { "metric": "num_requests", "series": [] },
            "OUTPUT_TOKENS": series("num_completion_tokens", serde_json::json!([])),
        }});
        let data = console(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(200).set_body_json(body),
        )
        .await
        .expect("key still valid");
        assert!(data.metrics.iter().all(|m| m.kind == MetricKind::RateLimit));
        assert!(data.notes.iter().any(|n| n.contains("unknown, not zero")));
    }

    #[tokio::test]
    async fn malformed_usage_is_a_schema_error() {
        let err = console(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "series": [{ "day": "x" }] })),
        )
        .await
        .expect_err("schema");
        assert_eq!(err.status, QueryError::schema().status);
        assert_eq!(
            key_paths(&serde_json::json!({ "series": [{ "day": 1 }] })),
            ["series", "series.day"]
        );
    }

    #[tokio::test]
    async fn expired_session_is_an_authentication_error() {
        for template in [
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({ "error": "Not authenticated" })),
            ResponseTemplate::new(307).insert_header("location", "/"),
        ] {
            let err = console(&session_account(&[("team_id", "1001")]), None, template)
                .await
                .expect_err("expired");
            assert_eq!(err.status, Status::AuthenticationError);
            assert!(!err.note.text().contains("placeholder"));
        }
    }

    #[tokio::test]
    async fn several_teams_ask_for_team_id() {
        let teams = ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "teams": [{ "team_id": "1001" }, { "team_id": "1002" }]
        }));
        let data = console(
            &session_account(&[]),
            Some(teams),
            ResponseTemplate::new(500),
        )
        .await
        .expect("key still valid");
        assert!(data.metrics.is_empty());
        assert!(data.notes.iter().any(|n| n.contains("2 teams")));
    }

    /// PRD 0035 re-validation V35-12: the team count is a plural, so
    /// Russian says "21 команде" and "22 командам".
    #[tokio::test]
    async fn several_teams_note_is_a_plural() {
        for (count, english, russian) in [
            (21, "21 teams", "21 команде"),
            (22, "22 teams", "22 командам"),
            (25, "25 teams", "25 командам"),
        ] {
            let ids: Vec<_> = (0..count)
                .map(|i| serde_json::json!({ "team_id": format!("{}", 1001 + i) }))
                .collect();
            let teams =
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "teams": ids }));
            let data = console(
                &session_account(&[]),
                Some(teams),
                ResponseTemplate::new(500),
            )
            .await
            .expect("key still valid");
            let note = data
                .notes_l10n
                .iter()
                .find(|note| note.id == "note.meta.several_teams")
                .expect("team note");
            assert!(note.resolve("en-US").contains(english), "{count}");
            assert!(note.resolve("ru").contains(russian), "{count}");
        }
    }

    #[tokio::test]
    async fn invalid_team_id_makes_no_console_request() {
        let err = console(
            &session_account(&[("team_id", "../x")]),
            None,
            ResponseTemplate::new(200),
        )
        .await
        .expect_err("invalid");
        assert_eq!(err.status, Status::NeedsConfiguration);
    }

    /// PRD 0049 re-validation R49-02: a legacy Project ID, valid or not, is
    /// ignored and never sent; the card says the whole team is shown.
    #[tokio::test]
    async fn a_legacy_project_id_is_ignored_and_noted() {
        for project in ["2002", "1&a=b"] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/models"))
                .respond_with(ResponseTemplate::new(200).set_body_json(models()))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/portal/teams/1001/usage/batch"))
                .and(query_param_is_missing("project_id"))
                .respond_with(ResponseTemplate::new(200).set_body_json(zero_sample()))
                .expect(1)
                .mount(&server)
                .await;
            let http = reqwest::Client::new();
            let ctx = Context {
                http: &http,
                direct: &http,
                credentials: &KeyAndSession,
                timeout: Duration::from_secs(5),
            };
            let data = fetch_at(
                &session_account(&[("team_id", "1001"), ("project_id", project)]),
                &ctx,
                &format!("{}/v1/models", server.uri()),
                &server.uri(),
            )
            .await
            .expect("usage read");
            assert!(data.metrics.iter().any(|m| m.kind == MetricKind::Spend));
            assert!(data
                .notes_l10n
                .iter()
                .any(|n| n.id == "note.meta.project_ignored"));
            data.assert_id_free(&["1001", project, &server.uri()]);
        }
    }

    /// A session-only account (no key) against a console mock: `teams`
    /// answers the team listing when given, `usage` the usage request.
    async fn session_only(
        account: &Account,
        teams: Option<ResponseTemplate>,
        usage: ResponseTemplate,
    ) -> Result<Data, QueryError> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(models()))
            .expect(0)
            .mount(&server)
            .await;
        if let Some(teams) = teams {
            Mock::given(method("GET"))
                .and(path("/api/portal/teams"))
                .respond_with(teams)
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/api/portal/teams/1001/usage/batch"))
            .respond_with(usage)
            .mount(&server)
            .await;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let ctx = Context {
            http: &http,
            direct: &http,
            credentials: &SessionOnly,
            timeout: Duration::from_secs(5),
        };
        fetch_at(
            account,
            &ctx,
            &format!("{}/v1/models", server.uri()),
            &server.uri(),
        )
        .await
    }

    /// The report the pre-save session test sees for `result`.
    fn report_of(result: Result<Data, QueryError>) -> crate::Report {
        let account = session_account(&[]);
        let (status, metrics, notes_l10n) = match result {
            Ok(data) => (Status::Ready, data.metrics, data.notes_l10n),
            Err(error) => (error.status, Vec::new(), vec![error.note]),
        };
        crate::Report {
            account_id: None,
            diagnostic: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: Utc::now(),
            status,
            metrics,
            notes: notes_l10n.iter().map(|n| n.fallback.to_string()).collect(),
            notes_l10n,
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
            breakdown: Vec::new(),
        }
    }

    /// PRD 0049 re-validation R49-01: without a key, a console answer that
    /// reads nothing is the account's error, never "Up to date", and the
    /// pre-save session test rejects it.
    #[tokio::test]
    async fn session_only_console_failure_is_an_error() {
        let err = session_only(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(500).set_body_string("placeholder-session SECRET"),
        )
        .await
        .expect_err("nothing read");
        assert_eq!(err.status, Status::Error);
        assert_eq!(err.note.id, "query.meta.console_http");
        assert!(err.note.text().contains("500"));
        for absent in ["placeholder", "SECRET", "127.0.0.1"] {
            assert!(!err.note.text().contains(absent), "{absent}");
        }
        assert_eq!(
            crate::user_session::SessionCheck::of(&report_of(Err(err))),
            crate::user_session::SessionCheck::Rejected
        );
    }

    #[tokio::test]
    async fn session_only_console_rate_limit_keeps_the_wait() {
        let err = session_only(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(429).insert_header("retry-after", "120"),
        )
        .await
        .expect_err("rate limited");
        assert_eq!(err.status, Status::RateLimited);
        let Some(crate::text::ParamValue::Text(seconds)) =
            err.note.params.get(crate::Report::RETRY_AFTER_PARAM)
        else {
            panic!("the wait is kept");
        };
        let seconds: u64 = seconds.parse().unwrap();
        assert!((119..=120).contains(&seconds), "{seconds}");
    }

    #[tokio::test]
    async fn session_only_console_html_is_a_schema_error() {
        let err = session_only(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(200).set_body_raw("<html>sign in</html>", "text/html"),
        )
        .await
        .expect_err("schema");
        assert_eq!(err.note.id, QueryError::schema().note.id);
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn session_only_team_listing_failure_is_an_error() {
        let err = session_only(
            &session_account(&[]),
            Some(ResponseTemplate::new(500)),
            ResponseTemplate::new(200).set_body_json(zero_sample()),
        )
        .await
        .expect_err("no team listing");
        assert_eq!(err.status, Status::Error);
        assert_eq!(err.note.id, "query.meta.console_http");
    }

    /// The console answered with the session: several teams verify it, so
    /// the account stays readable and the pre-save test passes.
    #[tokio::test]
    async fn session_only_several_teams_is_still_ready() {
        let teams = ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "teams": [{ "team_id": "1001" }, { "team_id": "1002" }]
        }));
        let data = session_only(
            &session_account(&[]),
            Some(teams),
            ResponseTemplate::new(500),
        )
        .await
        .expect("the session answered");
        assert!(data.metrics.is_empty());
        assert!(data.notes.iter().any(|n| n.contains("2 teams")));
        assert_eq!(
            crate::user_session::SessionCheck::of(&report_of(Ok(data))),
            crate::user_session::SessionCheck::Passed
        );
    }

    /// RF9 unchanged: with a key, a console failure degrades to a note and
    /// the key check still shows.
    #[tokio::test]
    async fn keyed_console_failure_degrades_to_a_note() {
        let data = console(
            &session_account(&[("team_id", "1001")]),
            None,
            ResponseTemplate::new(500),
        )
        .await
        .expect("key check still shows");
        assert!(data
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.meta.usage_unavailable"));
        assert!(data
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.meta.key_valid"));
    }

    /// PRD 0049 RF8: an invalid Team ID makes no request at all, the key
    /// check included.
    #[tokio::test]
    async fn invalid_team_id_makes_no_request_at_all() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(models()))
            .expect(0)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            direct: &http,
            credentials: &KeyAndSession,
            timeout: Duration::from_secs(5),
        };
        let err = fetch_at(
            &session_account(&[("team_id", "../x")]),
            &ctx,
            &format!("{}/v1/models", server.uri()),
            &server.uri(),
        )
        .await
        .expect_err("invalid");
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert_eq!(err.note.id, "query.meta.team_id_invalid");
    }

    /// R49-04: the key check refuses an oversize body while streaming.
    #[tokio::test]
    async fn oversize_model_listing_is_a_schema_error() {
        let big = format!("{{\"data\":[],\"pad\":\"{}\"}}", "x".repeat(BODY_LIMIT));
        let err = run(ResponseTemplate::new(200).set_body_string(big))
            .await
            .expect_err("oversize");
        assert_eq!(err.status, Status::Error);
        assert_eq!(err.note.id, QueryError::schema().note.id);
    }

    #[test]
    fn team_listing_keeps_only_valid_ids() {
        let body = serde_json::json!({ "teams": [{ "team_id": 7 }, { "team_id": "a/b" }, { "name": "x" }] });
        assert_eq!(team_ids(&body), ["7"]);
        assert!(team_ids(&serde_json::json!({})).is_empty());
    }
}
