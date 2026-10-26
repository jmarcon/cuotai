//! MiniMax Token Plan / Coding Plan quota through the internal
//! international endpoints (read-only, GET) used by 9router.
//!
//! Default route of this adapter: `Provider::MiniMax` with a plan key
//! (any value that does **not** start with `sk-api-`; those are
//! pay-as-you-go and stay in the `asia` adapter). No `params.*` is
//! required: the credential comes from `account.credential_env` or
//! `MINIMAX_IO_API_KEY`.
//!
//! Primary sources (9router @ f01fb909e37189008080632ddaf404f096345cde,
//! MIT license — see THIRD_PARTY_NOTICES):
//! - `open-sse/services/usage/minimax.js` — handler (fallback order,
//!   counter semantics, percent-only buckets);
//! - `open-sse/providers/registry/minimax.js` — international URLs;
//! - `tests/unit/minimax-usage.test.js` — behavior fixtures.
//!
//! Deliberate limits: no quota reset or credit consumption operation;
//! no browser credential extraction; no inference
//! generation. The China region (`minimaxi.com`) is never contacted with this
//! credential — only the registry's international URLs exist here.

use super::{secret, send as secure_send, Context, Data, QueryError};
use crate::text::{params, provider_l10n, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider, Status};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;

const REF: &str = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/minimax.js";

/// Official international endpoints, in 9router's order
/// (`registry/minimax.js`): Token Plan first; Coding Plan is tried
/// only when the previous one fails with a transient error (404/405/5xx/network).
const TOKEN_PLAN_URL: &str = "https://www.minimax.io/v1/token_plan/remains";
const CODING_PLAN_URL: &str = "https://api.minimax.io/v1/api/openplatform/coding_plan/remains";

/// Response body ceiling: the plan quota is a small JSON; larger
/// bodies are rejected instead of being loaded whole into memory.
const MAX_BODY_BYTES: usize = 256 * 1024;

/// `base_resp.status_code` returned by MiniMax for an invalid or
/// inactive key (9router `minimax.js`).
const STATUS_INVALID_KEY: i64 = 1004;

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::MiniMax => {
            let urls = [TOKEN_PLAN_URL.to_string(), CODING_PLAN_URL.to_string()];
            fetch_at(account, ctx, &urls).await
        }
        other => Err(QueryError::unsupported_params(
            "query.router_minimax.provider_handled_elsewhere",
            params([("provider", ParamValue::from(provider_l10n(other)))]),
            "{provider} is handled by another adapter.",
        )),
    }
}

// ------------------------------------------------------------- esquema

/// Tolerant envelope: 9router treats a non-JSON body as an empty payload and
/// `base_resp` is optional in upstream responses. Fields accepted in
/// snake_case and camelCase (`getMiniMaxField`).
#[derive(Deserialize, Default)]
struct Envelope {
    #[serde(default, alias = "baseResp")]
    base_resp: Option<BaseResp>,
    #[serde(default, alias = "modelRemains")]
    model_remains: Vec<ModelRemain>,
}

#[derive(Deserialize)]
struct BaseResp {
    #[serde(default, alias = "statusCode")]
    status_code: Option<i64>,
    #[serde(default, alias = "statusMsg")]
    status_msg: Option<String>,
}

/// All fields are optional: M-series buckets (the M3 era) arrive without
/// counters or timestamps — the remaining percentage is enough.
#[derive(Deserialize)]
struct ModelRemain {
    #[serde(default, alias = "modelName")]
    model_name: Option<String>,
    #[serde(default, alias = "currentIntervalTotalCount")]
    current_interval_total_count: Option<f64>,
    #[serde(default, alias = "currentIntervalUsageCount")]
    current_interval_usage_count: Option<f64>,
    #[serde(default, alias = "currentIntervalRemainingPercent")]
    current_interval_remaining_percent: Option<f64>,
    #[serde(default, alias = "currentWeeklyTotalCount")]
    current_weekly_total_count: Option<f64>,
    #[serde(default, alias = "currentWeeklyUsageCount")]
    current_weekly_usage_count: Option<f64>,
    #[serde(default, alias = "currentWeeklyRemainingPercent")]
    current_weekly_remaining_percent: Option<f64>,
    /// Remaining window duration in **milliseconds** (preferred over the
    /// timestamp, per `getMiniMaxResetAt`).
    #[serde(default, alias = "remainsTime")]
    remains_time: Option<f64>,
    #[serde(default, alias = "weeklyRemainsTime")]
    weekly_remains_time: Option<f64>,
    /// Window end timestamp: epoch (s or ms), numeric string or ISO
    /// 8601 (`parseResetTime`).
    #[serde(default, alias = "endTime")]
    end_time: Option<Value>,
    #[serde(default, alias = "weeklyEndTime")]
    weekly_end_time: Option<Value>,
}

// ------------------------------------------------------------- helpers

fn finite_or_zero(value: Option<f64>) -> f64 {
    value.filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0)
}

fn clamp_percent(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 100.0))
}

/// `hasMiniMaxQuota`: contadores reais **ou** percentual bastam.
fn has_quota(model: &ModelRemain) -> bool {
    finite_or_zero(model.current_interval_total_count) > 0.0
        || finite_or_zero(model.current_weekly_total_count) > 0.0
        || clamp_percent(model.current_interval_remaining_percent).is_some()
        || clamp_percent(model.current_weekly_remaining_percent).is_some()
}

/// `formatMiniMaxQuotaName`: the shared M3+ bucket (`MiniMax-M*` in
/// old responses, `general` in new ones) becomes the friendly label
/// "M-series"; other names are title-cased with To/Tts/Hd adjustments.
fn display_name(model: &ModelRemain) -> String {
    let raw = model.model_name.as_deref().unwrap_or("").trim();
    if raw.is_empty() {
        return "MiniMax".into();
    }
    if raw == "MiniMax-M*" || raw == "general" {
        return "M-series".into();
    }
    raw.split(|c: char| c == '_' || c == '-' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            let titled = match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            };
            match titled.as_str() {
                "To" => "to".to_string(),
                "Tts" => "TTS".to_string(),
                "Hd" => "HD".to_string(),
                other => other.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `getMiniMaxResetAt`: the remaining duration in ms (`remains_time`) takes
/// precedence over the timestamp (`end_time`).
fn reset_at(
    remains_ms: Option<f64>,
    end: Option<&Value>,
    captured: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if let Some(ms) = remains_ms.filter(|v| v.is_finite() && *v > 0.0) {
        if let Some(at) =
            Duration::try_milliseconds(ms as i64).and_then(|d| captured.checked_add_signed(d))
        {
            return Some(at);
        }
    }
    end.and_then(parse_reset_value)
}

/// `parseResetTime`: epoch in seconds or milliseconds (number or numeric
/// string) or ISO 8601 date. Absent/invalid value ⇒ `None` (an unknown
/// window is not invented).
fn parse_reset_value(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::Number(number) => number.as_f64().and_then(epoch_to_utc),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            if text.bytes().all(|b| b.is_ascii_digit()) {
                text.parse::<f64>().ok().and_then(epoch_to_utc)
            } else {
                DateTime::parse_from_rfc3339(text)
                    .map(|t| t.with_timezone(&Utc))
                    .ok()
            }
        }
        _ => None,
    }
}

fn epoch_to_utc(timestamp: f64) -> Option<DateTime<Utc>> {
    if !timestamp.is_finite() {
        return None;
    }
    let millis = if timestamp.abs() < 1e12 {
        timestamp * 1000.0
    } else {
        timestamp
    };
    if millis <= 0.0 {
        return None;
    }
    DateTime::<Utc>::from_timestamp_millis(millis as i64)
}

/// `addMiniMaxQuota` + `buildMiniMaxQuota`: with no `total` and no percentage the
/// window is omitted (never fabricated). Percent-only buckets (zeroed
/// counters) become a `%` metric — without inventing a request total.
#[allow(clippy::too_many_arguments)]
fn push_quota(
    metrics: &mut Vec<Metric>,
    name: &str,
    suffix: &str,
    window: MetricWindow,
    total: f64,
    count: f64,
    percent: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
    count_means_remaining: bool,
) {
    let total = total.max(0.0);
    let count = count.max(0.0);
    let percent = clamp_percent(percent);
    if total <= 0.0 && percent.is_none() {
        return;
    }
    let label = format!("{name} {suffix}");
    if total <= 0.0 {
        let remaining = percent.unwrap_or(0.0);
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label,
            used: Some(100.0 - remaining),
            limit: Some(100.0),
            remaining: Some(remaining),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(window),
            period: None,
            resets_at,
        });
        return;
    }
    // In /token_plan/remains the counter is "consumed"; in
    // /coding_plan/remains it is "remaining" (9router's per-URL semantics).
    let used = if count_means_remaining {
        (total - count).max(0.0)
    } else {
        count.min(total)
    };
    let remaining = (total - used).max(0.0);
    metrics.push(Metric {
        billing: Default::default(),
        scope: None,
        label,
        used: Some(used),
        limit: Some(total),
        remaining: Some(remaining),
        unit: "consultas".into(),
        kind: MetricKind::Quota,
        window: Some(window),
        period: None,
        resets_at,
    });
}

// --------------------------------------------------------------- HTTP

/// Endpoint failure: `transient` indicates that the next URL in the list
/// may be tried (404/405/5xx or network error), as in 9router's loop.
struct Failure {
    error: QueryError,
    transient: bool,
}

impl Failure {
    fn transient(error: QueryError) -> Self {
        Self {
            error,
            transient: true,
        }
    }
    fn fatal(error: QueryError) -> Self {
        Self {
            error,
            transient: false,
        }
    }
}

/// Bounded body read: refuses responses above `MAX_BODY_BYTES`
/// instead of loading them in full.
async fn read_bounded(response: &mut reqwest::Response) -> Result<String, Failure> {
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Failure::transient(QueryError::transport()))?
    {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(Failure::fatal(QueryError::schema()));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| Failure::fatal(QueryError::schema()))
}

async fn query_endpoint(ctx: &Context<'_>, key: &str, url: &str) -> Result<Data, Failure> {
    let mut response = secure_send(
        ctx.http
            .get(url)
            .bearer_auth(key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .timeout(ctx.timeout),
    )
    .await
    .map_err(Failure::transient)?;
    let status = response.status();
    let body = read_bounded(&mut response).await?;
    // 9router tolerates a non-JSON body (empty payload) and a missing
    // `base_resp`; the raw body never enters error messages.
    let payload: Envelope = serde_json::from_str(&body).unwrap_or_default();

    let status_code = payload.base_resp.as_ref().and_then(|b| b.status_code);
    let status_msg = payload
        .base_resp
        .as_ref()
        .and_then(|b| b.status_msg.as_deref())
        .unwrap_or("")
        .trim()
        .to_string();
    let lowered = status_msg.to_lowercase();
    let auth_like = [
        "token plan",
        "coding plan",
        "invalid api key",
        "invalid key",
        "unauthorized",
        "inactive",
    ]
    .iter()
    .any(|needle| lowered.contains(needle));

    if status.as_u16() == 401
        || status.as_u16() == 403
        || status_code == Some(STATUS_INVALID_KEY)
        || auth_like
    {
        return Err(Failure::fatal(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.router_minimax.invalid_key",
                "MiniMax key invalid or inactive. Use an active Token/Coding Plan key.",
            ),
        }));
    }

    if !status.is_success() {
        let code = status.as_u16();
        let error = if code == 429 {
            QueryError {
                diagnostic: None,
                status: Status::RateLimited,
                note: Localized::with_params(
                    "query.router_minimax.rate_limited",
                    params([("code", ParamValue::from(code))]),
                    "MiniMax rate-limited the usage query (HTTP 429).",
                ),
            }
        } else {
            QueryError {
                diagnostic: None,
                status: Status::Error,
                note: Localized::with_params(
                    "query.router_minimax.http",
                    params([("code", ParamValue::from(code))]),
                    "MiniMax query returned HTTP {code}.",
                ),
            }
        };
        let transient = matches!(code, 404 | 405) || status.is_server_error();
        return Err(Failure { error, transient });
    }

    if let Some(code) = status_code.filter(|c| *c != 0) {
        return Err(Failure::fatal(QueryError {
            diagnostic: None,
            status: Status::Error,
            note: Localized::with_params(
                "query.router_minimax.quota_error",
                params([("code", ParamValue::from(code.to_string()))]),
                "MiniMax responded with quota error (code {code}).",
            ),
        }));
    }

    let quota_models: Vec<&ModelRemain> = payload
        .model_remains
        .iter()
        .filter(|m| has_quota(m))
        .collect();
    if quota_models.is_empty() {
        return Err(Failure::fatal(QueryError::unsupported(
            "query.router_minimax.no_quota_data",
            "MiniMax connected. No quota data was returned.",
        )));
    }

    let captured_at = Utc::now();
    let count_means_remaining = url.contains("/coding_plan/remains");
    let mut metrics = Vec::new();
    for model in &quota_models {
        let name = display_name(model);
        push_quota(
            &mut metrics,
            &name,
            "(5h)",
            MetricWindow::Window5h,
            finite_or_zero(model.current_interval_total_count),
            finite_or_zero(model.current_interval_usage_count),
            model.current_interval_remaining_percent,
            reset_at(model.remains_time, model.end_time.as_ref(), captured_at),
            count_means_remaining,
        );
        push_quota(
            &mut metrics,
            &name,
            "(7d)",
            MetricWindow::Weekly,
            finite_or_zero(model.current_weekly_total_count),
            finite_or_zero(model.current_weekly_usage_count),
            model.current_weekly_remaining_percent,
            reset_at(
                model.weekly_remains_time,
                model.weekly_end_time.as_ref(),
                captured_at,
            ),
            count_means_remaining,
        );
    }
    if metrics.is_empty() {
        return Err(Failure::fatal(QueryError::unsupported(
            "query.router_minimax.quota_unextractable",
            "MiniMax connected. Could not extract quota usage from the response.",
        )));
    }

    let endpoint = if count_means_remaining {
        CODING_PLAN_URL
    } else {
        TOKEN_PLAN_URL
    };
    let mut notes = Notes::new();
    notes.push_params(
        "note.router_minimax.read_only_endpoint",
        params([("endpoint", ParamValue::from(endpoint))]),
        "Read-only GET query at {endpoint}. In /token_plan/remains the *_usage_count counters are consumed; in /coding_plan/remains they are remaining.",
    );
    notes.push(
        "note.router_minimax.percent_only_buckets",
        "MiniMax responded only with the plan percentage; `percent` is the USED fraction (0..1).",
    );
    // Capture the first reset timestamp the adapter actually derived so the
    // catalog template's `{reset_at}` placeholder resolves to a concrete
    // value (otherwise translation drops it inside the English text).
    let first_reset = metrics.iter().find_map(|m| m.resets_at);
    let reset_text = first_reset
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| "epoch not available".to_string());
    notes.push_params(
        "note.router_minimax.reset_source",
        params([("reset_at", ParamValue::from(reset_text.clone()))]),
        "Reset timestamp: {reset_at}; reported by the provider without timezone offset.",
    );
    Ok(Data::new(metrics, notes, REF))
}

/// Internal envelope that accepts the endpoint list (for wiremock). Production
/// passes the fixed international URLs; tests inject local
/// URLs — counter semantics remain derived from the path.
async fn fetch_at(
    account: &Account,
    ctx: &Context<'_>,
    urls: &[String],
) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "MINIMAX_IO_API_KEY")?;
    let mut last_error: Option<QueryError> = None;
    for (index, url) in urls.iter().enumerate() {
        let can_fallback = index + 1 < urls.len();
        match query_endpoint(ctx, &key, url).await {
            Ok(data) => return Ok(data),
            Err(failure) if failure.transient && can_fallback => {
                last_error = Some(failure.error);
            }
            Err(failure) => return Err(failure.error),
        }
    }
    Err(last_error.unwrap_or_else(QueryError::transport))
}

// -------------------------------------------------------------------- testes

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use std::collections::BTreeMap;
    use std::time::Duration as StdDuration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Creds;
    impl Credentials for Creds {
        fn get(&self, name: &str) -> Option<String> {
            (name == "MINIMAX_IO_API_KEY").then(|| "test-key".to_string())
        }
    }

    struct NoCreds;
    impl Credentials for NoCreds {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    fn account() -> Account {
        Account {
            id: None,
            name: "conta".into(),
            provider: Provider::MiniMax,
            credential_env: None,
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn ctx<'a>(http: &'a reqwest::Client, creds: &'a dyn Credentials) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: StdDuration::from_secs(2),
        }
    }

    /// Private endpoint list mirroring the production order.
    fn endpoints(server: &MockServer) -> [String; 2] {
        [
            format!("{}/v1/token_plan/remains", server.uri()),
            format!("{}/v1/api/openplatform/coding_plan/remains", server.uri()),
        ]
    }

    fn find<'a>(data: &'a Data, label: &str) -> &'a Metric {
        data.metrics
            .iter()
            .find(|m| m.label == label)
            .unwrap_or_else(|| panic!("faltando {label}"))
    }

    // Fixture do 9router: token-plan TTS, contadores = consumidos,
    // timestamps ISO (esquema antigo snake_case).
    #[tokio::test]
    async fn token_plan_contadores_sao_consumidos_esquema_antigo() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/token_plan/remains"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "base_resp": { "status_code": 0, "status_msg": "success" },
                "model_remains": [{
                    "model_name": "text to speech hd",
                    "current_interval_total_count": 4000,
                    "current_interval_usage_count": 25,
                    "current_weekly_total_count": 12000,
                    "current_weekly_usage_count": 100,
                    "end_time": "2026-05-12T10:00:00.000Z",
                    "weekly_end_time": "2026-05-19T10:00:00.000Z"
                }]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch_at(&account(), &ctx(&http, &Creds), &endpoints(&server))
            .await
            .expect("sucesso");

        let curto = find(&data, "Text to Speech HD (5h)");
        assert_eq!(curto.used, Some(25.0));
        assert_eq!(curto.limit, Some(4000.0));
        assert_eq!(curto.remaining, Some(3975.0));
        assert_eq!(curto.kind, MetricKind::Quota);
        assert_eq!(curto.window, Some(MetricWindow::Window5h));
        assert_eq!(curto.unit, "consultas");
        assert_eq!(
            curto.resets_at.expect("reset").to_rfc3339(),
            "2026-05-12T10:00:00+00:00"
        );

        let semanal = find(&data, "Text to Speech HD (7d)");
        assert_eq!(semanal.used, Some(100.0));
        assert_eq!(semanal.limit, Some(12000.0));
        assert_eq!(semanal.remaining, Some(11900.0));
        assert_eq!(semanal.kind, MetricKind::Quota);
        assert_eq!(semanal.window, Some(MetricWindow::Weekly));
        assert_eq!(
            semanal.resets_at.expect("reset").to_rfc3339(),
            "2026-05-19T10:00:00+00:00"
        );
        assert_eq!(data.source, REF);
    }

    // 9router fixture: coding-plan, counters = remaining, new camelCase
    // schema, resets via remains_time duration (ms).
    #[tokio::test]
    async fn coding_plan_contadores_sao_restantes_esquema_novo() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/api/openplatform/coding_plan/remains"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "baseResp": { "statusCode": 0, "statusMsg": "success" },
                "modelRemains": [{
                    "modelName": "Text to Speech HD",
                    "currentIntervalTotalCount": 4000,
                    "currentIntervalUsageCount": 4000,
                    "currentWeeklyTotalCount": 12000,
                    "currentWeeklyUsageCount": 11800,
                    "remainsTime": 1000,
                    "weeklyRemainsTime": 2000
                }]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let coding_only = [endpoints(&server)[1].clone()];
        let before = Utc::now();
        let data = fetch_at(&account(), &ctx(&http, &Creds), &coding_only)
            .await
            .expect("sucesso");
        let after = Utc::now();

        let curto = find(&data, "Text to Speech HD (5h)");
        assert_eq!(curto.used, Some(0.0));
        assert_eq!(curto.limit, Some(4000.0));
        assert_eq!(curto.remaining, Some(4000.0));
        let reset = curto.resets_at.expect("reset via remains_time");
        assert!(reset >= before + Duration::milliseconds(1000));
        assert!(reset <= after + Duration::milliseconds(1000));

        let semanal = find(&data, "Text to Speech HD (7d)");
        assert_eq!(semanal.used, Some(200.0));
        assert_eq!(semanal.limit, Some(12000.0));
        assert_eq!(semanal.remaining, Some(11800.0));
        let reset = semanal.resets_at.expect("reset via weekly_remains_time");
        assert!(reset >= before + Duration::milliseconds(2000));
        assert!(reset <= after + Duration::milliseconds(2000));
    }

    // 9router fixture: percent-only M-series bucket (no counters or
    // timestamps) in token-plan — the percentage is enough.
    #[tokio::test]
    async fn percent_only_m_series_token_plan_vira_metrica_percentual() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/token_plan/remains"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "base_resp": { "status_code": 0, "status_msg": "success" },
                "model_remains": [{
                    "model_name": "general",
                    "current_interval_remaining_percent": 70,
                    "current_weekly_remaining_percent": 64
                }]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch_at(&account(), &ctx(&http, &Creds), &endpoints(&server))
            .await
            .expect("sucesso");

        let curto = find(&data, "M-series (5h)");
        assert_eq!(curto.used, Some(30.0));
        assert_eq!(curto.limit, Some(100.0));
        assert_eq!(curto.remaining, Some(70.0));
        assert_eq!(curto.unit, "%");
        assert!(curto.resets_at.is_none());

        let semanal = find(&data, "M-series (7d)");
        assert_eq!(semanal.used, Some(36.0));
        assert_eq!(semanal.remaining, Some(64.0));
    }

    // 9router fixture: the same percent-only bucket in coding-plan must not
    // invert the percentage (countMeansRemaining).
    #[tokio::test]
    async fn percent_only_m_series_coding_plan_nao_inverte() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/api/openplatform/coding_plan/remains"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "base_resp": { "status_code": 0, "status_msg": "success" },
                "model_remains": [{
                    "model_name": "general",
                    "current_interval_remaining_percent": 80,
                    "current_weekly_remaining_percent": 95
                }]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let coding_only = [endpoints(&server)[1].clone()];
        let data = fetch_at(&account(), &ctx(&http, &Creds), &coding_only)
            .await
            .expect("sucesso");

        let curto = find(&data, "M-series (5h)");
        assert_eq!(curto.used, Some(20.0));
        assert_eq!(curto.remaining, Some(80.0));
        assert_eq!(curto.unit, "%");
        let semanal = find(&data, "M-series (7d)");
        assert_eq!(semanal.used, Some(5.0));
        assert_eq!(semanal.remaining, Some(95.0));
    }

    // 9router fixture: wildcard "MiniMax-M*" (M3 era) with counters in
    // coding-plan — friendly label and remaining semantics.
    #[tokio::test]
    async fn wildcard_minimax_m_vira_m_series_com_contadores() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/api/openplatform/coding_plan/remains"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "baseResp": { "statusCode": 0 },
                "modelRemains": [{
                    "modelName": "MiniMax-M*",
                    "currentIntervalTotalCount": 4000,
                    "currentIntervalUsageCount": 3200,
                    "currentWeeklyTotalCount": 24000,
                    "currentWeeklyUsageCount": 18000,
                    "remainsTime": 1000,
                    "weeklyRemainsTime": 2000
                }]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let coding_only = [endpoints(&server)[1].clone()];
        let data = fetch_at(&account(), &ctx(&http, &Creds), &coding_only)
            .await
            .expect("sucesso");

        let labels: Vec<&str> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["M-series (5h)", "M-series (7d)"]);
        let curto = find(&data, "M-series (5h)");
        assert_eq!(curto.used, Some(800.0));
        assert_eq!(curto.limit, Some(4000.0));
        assert_eq!(curto.remaining, Some(3200.0));
        let semanal = find(&data, "M-series (7d)");
        assert_eq!(semanal.used, Some(6000.0));
        assert_eq!(semanal.remaining, Some(18000.0));
    }

    // Fallback in registry order: token_plan 404 (transient) ⇒ tries
    // coding_plan.
    #[tokio::test]
    async fn fallback_para_coding_plan_em_erro_transitorio() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/token_plan/remains"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/api/openplatform/coding_plan/remains"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model_remains": [{
                    "model_name": "general",
                    "current_interval_remaining_percent": 80
                }]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch_at(&account(), &ctx(&http, &Creds), &endpoints(&server))
            .await
            .expect("fallback deve ter sucesso");
        // base_resp absent is tolerated (optional upstream).
        let curto = find(&data, "M-series (5h)");
        assert_eq!(curto.remaining, Some(80.0));
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("/coding_plan/remains")));
    }

    // Non-transient errors do not fall back: 401 is terminal.
    #[tokio::test]
    async fn http_401_e_autenticacao_sem_fallback() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = fetch_at(&account(), &ctx(&http, &Creds), &endpoints(&server))
            .await
            .unwrap_err();
        assert!(matches!(err.status, Status::AuthenticationError));
        assert!(err.note.fallback.contains("invalid or inactive"));
    }

    // base_resp.status_code 1004 ⇒ AuthenticationError, no raw body.
    #[tokio::test]
    async fn status_1004_e_autenticacao_sem_corpo_cru() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/token_plan/remains"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "base_resp": { "status_code": 1004, "status_msg": "invalid api key" },
                "debug": "RAW_BODY_MARKER"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let token_only = [endpoints(&server)[0].clone()];
        let err = fetch_at(&account(), &ctx(&http, &Creds), &token_only)
            .await
            .unwrap_err();
        assert!(matches!(err.status, Status::AuthenticationError));
        assert!(!err.note.fallback.contains("RAW_BODY_MARKER"));
        assert!(!err.note.fallback.contains("invalid api key"));
    }

    // base_resp entirely absent (optional upstream) with valid data.
    #[tokio::test]
    async fn base_resp_opcional_ausente() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/token_plan/remains"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model_remains": [{
                    "model_name": "music-2.6",
                    "current_interval_total_count": 100,
                    "current_interval_usage_count": 5
                }]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch_at(&account(), &ctx(&http, &Creds), &endpoints(&server))
            .await
            .expect("sucesso");
        let metric = find(&data, "Music 2.6 (5h)");
        assert_eq!(metric.used, Some(5.0));
        assert_eq!(metric.remaining, Some(95.0));
    }

    // A model with no quota at all generates no fabricated metrics.
    #[tokio::test]
    async fn sem_cota_retornada_nao_fabrica_janelas() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "base_resp": { "status_code": 0 },
                "model_remains": []
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let token_only = [endpoints(&server)[0].clone()];
        let err = fetch_at(&account(), &ctx(&http, &Creds), &token_only)
            .await
            .unwrap_err();
        assert!(matches!(err.status, Status::Unsupported));
        assert!(err.note.fallback.contains("No quota data"));
    }

    // A transient 500 on both endpoints exhausts the fallback and returns the
    // last error (no raw body).
    #[tokio::test]
    async fn fallback_esgotado_devolve_ultimo_erro() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500).set_body_string("RAW_BODY_MARKER"))
            .expect(2)
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = fetch_at(&account(), &ctx(&http, &Creds), &endpoints(&server))
            .await
            .unwrap_err();
        assert!(matches!(err.status, Status::Error));
        assert!(err.note.fallback.contains("HTTP 500"));
        assert!(!err.note.fallback.contains("RAW_BODY_MARKER"));
    }

    // Public default route: Provider::MiniMax, no required params, and the
    // missing credential becomes NeedsConfiguration before any network.
    #[tokio::test]
    async fn fetch_sem_credencial_e_needs_configuration() {
        let http = reqwest::Client::new();
        let err = fetch(&account(), &ctx(&http, &NoCreds)).await.unwrap_err();
        assert!(matches!(err.status, Status::NeedsConfiguration));
        assert!(err.note.fallback.contains("MINIMAX_IO_API_KEY"));
    }
}
