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
//!   offset}`. Cuotai sums the `TOTAL` series of the current month; an empty
//!   series is unknown, never zero. That `offset: 100` means amounts in
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
            let usage = console_usage(account, ctx, console, &session.cookie).await?;
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
        .with_retry_after(retry_after(response.headers())));
    }
    let headers = response.headers().clone();
    let bytes = response
        .bytes()
        .await
        .map_err(|_| QueryError::transport())?;
    if bytes.len() > BODY_LIMIT {
        return Err(QueryError::schema());
    }
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
/// and notes. An expired session is an authentication error; anything else
/// the console answers degrades to a note, so the key check still shows.
async fn console_usage(
    account: &Account,
    ctx: &Context<'_>,
    console: &str,
    cookie: &str,
) -> Result<(Vec<Metric>, Notes), QueryError> {
    let console = console.trim_end_matches('/');
    let mut notes = Notes::new();
    notes.push(
        "note.meta.console_undocumented",
        "Usage read from the dev.meta.ai console's internal interface; it may change without notice.",
    );
    let team = match param_id(account, "team_id") {
        Param::Set(team) => team,
        Param::Invalid => {
            return Err(QueryError {
                diagnostic: None,
                status: Status::NeedsConfiguration,
                note: Localized::fixed(
                    "query.meta.team_id_invalid",
                    "The Team ID is not a dev.meta.ai team id; copy the number after team_id= in the usage page address.",
                ),
            })
        }
        Param::Empty => {
            let Some(body) = console_get(ctx, &format!("{console}/api/portal/teams"), &[], cookie, None)
                .await?
            else {
                notes.push(
                    "note.meta.teams_unavailable",
                    "The dev.meta.ai console did not list the teams; set the Team ID from the usage page address.",
                );
                return Ok((Vec::new(), notes));
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
                    notes.push_params(
                        "note.meta.several_teams",
                        params([("count", ParamValue::from(many.len() as u64))]),
                        "The session belongs to {count} teams; set the Team ID from the usage page address to choose one.",
                    );
                    return Ok((Vec::new(), notes));
                }
            }
        }
    };
    // `project_id` stays a validated optional param, but the console's own
    // usage request carries only the team (owner, 2026-10-27), so it is not
    // sent.
    if let Param::Invalid = param_id(account, "project_id") {
        return Err(QueryError {
            diagnostic: None,
            status: Status::NeedsConfiguration,
            note: Localized::fixed(
                "query.meta.project_id_invalid",
                "The Project ID is not a dev.meta.ai project id; copy the number after project_id= in the usage page address.",
            ),
        });
    }
    let period = MonthToDate::at(Utc::now());
    let query = usage_query(&period);
    let url = format!("{console}/api/portal/teams/{team}/usage/batch");
    let referer = format!("{console}/usage?team_id={team}");
    let Some(body) = console_get(ctx, &url, &query, cookie, Some(&referer)).await? else {
        notes.push(
            "note.meta.usage_unavailable",
            "The dev.meta.ai console did not return the usage; the card shows only the key check.",
        );
        return Ok((Vec::new(), notes));
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

/// A console GET with the session cookie. `Ok(None)` for an answer that is
/// not usable (another code, a non-object body); `Err` for an expired or
/// rejected session.
async fn console_get(
    ctx: &Context<'_>,
    url: &str,
    query: &[(&str, String)],
    cookie: &str,
    referer: Option<&str>,
) -> Result<Option<Value>, QueryError> {
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
    match response.status().as_u16() {
        401 | 403 => Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.meta.session_expired",
                "dev.meta.ai session expired or revoked; sign in again from the account.",
            ),
        }),
        200..=299 => {
            let body = super::body_value(response).await.ok();
            Ok(body.filter(Value::is_object))
        }
        _ => Ok(None),
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

/// The sum of the `TOTAL` series of one metric. `None` when the metric,
/// its `TOTAL` series or every usable data point is missing: an empty
/// series is unknown, never zero. Points explicitly reported as `0` count.
/// `field` reads `amount` first (cost) and falls back to `value`.
fn total(metric: &Value, amount_first: bool) -> Option<f64> {
    let series = metric.get("series")?.as_array()?;
    let total = series
        .iter()
        .find(|s| s.get("type").and_then(Value::as_str) == Some("TOTAL"))?;
    let points = total.get("data_points")?.as_array()?;
    let mut sum = None;
    for point in points {
        let v = if amount_first {
            point
                .get("amount")
                .and_then(number)
                .or_else(|| point.get("value").and_then(number))
        } else {
            point.get("value").and_then(number)
        };
        if let Some(v) = v {
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
    };
    let mut out = Vec::new();
    if let Some(cost) = series.get("USAGE_BILLABLE_COST") {
        if let Some(sum) = total(cost, true) {
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
        if let Some(sum) = series.get(key).and_then(|m| total(m, false)) {
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

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
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

    #[tokio::test]
    async fn invalid_ids_make_no_console_request() {
        for (key, value) in [("team_id", "../x"), ("project_id", "1&a=b")] {
            let mut extra = vec![(key, value)];
            if key == "project_id" {
                extra.push(("team_id", "1001"));
            }
            let err = console(&session_account(&extra), None, ResponseTemplate::new(200))
                .await
                .expect_err("invalid");
            assert_eq!(err.status, Status::NeedsConfiguration, "{key}");
        }
    }

    #[test]
    fn team_listing_keeps_only_valid_ids() {
        let body = serde_json::json!({ "teams": [{ "team_id": 7 }, { "team_id": "a/b" }, { "name": "x" }] });
        assert_eq!(team_ids(&body), ["7"]);
        assert!(team_ids(&serde_json::json!({})).is_empty());
    }
}
