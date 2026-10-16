//! Read-only internal quota interfaces observed in 9router f01fb909e37189008080632ddaf404f096345cde.
//! Qoder: PAT (pt-...) → job-token exchange, then GET /api/v2/quota/usage (intl/CN).
//! Ollama Cloud: GET /api/usage (limits are 0..1 ratios) + POST /api/me (plan, fail-open).
//! Unofficial endpoints, subject to change. See THIRD_PARTY_NOTICES for MIT attribution.
//! No token generation/reset/purchase calls; credentials come only from explicit env refs.
use super::{json, secret, Context, Data, QueryError};
use crate::text::{params, Notes, ParamValue};
use crate::{Account, Metric, Provider};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;

const REF_USAGE: &str = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/misc.js";
const REF_QODER_MODELS: &str = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/qoderModels.js";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::Qoder => {
            let region = account
                .params
                .get("region")
                .map(String::as_str)
                .unwrap_or("intl");
            let base = match region {
                "intl" => "https://openapi.qoder.sh",
                "cn" => "https://openapi.qoder.com.cn",
                other => {
                    return Err(QueryError::config_params(
                        "query.router_accounts.invalid_qoder_region",
                        params([("region", ParamValue::from(other.to_string()))]),
                        format!("params.region '{other}' inválido; use intl ou cn."),
                    ))
                }
            };
            qoder(account, ctx, base).await
        }
        Provider::OllamaCloud => ollama(account, ctx, "https://ollama.com", Utc::now()).await,
        _ => Err(QueryError::unsupported(
            "query.router_accounts.provider_not_supported",
            "Provedor sem consulta neste adapter.",
        )),
    }
}

fn finite(value: f64) -> Result<f64, QueryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(QueryError::schema())
    }
}

// ---------------------------------------------------------------------------
// Qoder
// ---------------------------------------------------------------------------

const QODER_PAT_PREFIX: &str = "pt-";

#[derive(Deserialize)]
struct QoderExchange {
    token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QoderUsage {
    user_quota: Option<QoderQuota>,
    org_resource_package: Option<QoderQuota>,
    /// Epoch milliseconds; 9router accepts numeric strings via Number().
    expires_at: Option<Value>,
    total_usage_percentage: Option<f64>,
    is_quota_exceeded: Option<bool>,
}

#[derive(Deserialize)]
struct QoderQuota {
    total: Option<f64>,
    used: Option<f64>,
    remaining: Option<f64>,
    unit: Option<String>,
}

fn qoder_metric(
    label: &str,
    quota: &QoderQuota,
    resets_at: Option<DateTime<Utc>>,
) -> Result<Option<Metric>, QueryError> {
    if quota.total.is_none() && quota.used.is_none() && quota.remaining.is_none() {
        return Ok(None);
    }
    Ok(Some(Metric {
        label: label.into(),
        used: quota.used.map(finite).transpose()?,
        limit: quota.total.map(finite).transpose()?,
        remaining: quota.remaining.map(finite).transpose()?,
        unit: quota
            .unit
            .clone()
            .filter(|unit| !unit.trim().is_empty())
            .unwrap_or_else(|| "créditos".into()),
        period: Some("ciclo de quota Qoder".into()),
        resets_at,
    }))
}

fn qoder_reset(value: Option<&Value>) -> Result<Option<DateTime<Utc>>, QueryError> {
    let Some(value) = value else { return Ok(None) };
    let ms = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    match ms {
        Some(ms) if ms.is_finite() && ms > 0.0 => DateTime::from_timestamp_millis(ms as i64)
            .map(Some)
            .ok_or_else(QueryError::schema),
        // 9router treats missing/invalid expiresAt as "no reset", not zero.
        _ => Ok(None),
    }
}

async fn qoder(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "QODER_API_KEY")?;
    let credential_type = account
        .params
        .get("credential_type")
        .map(String::as_str)
        .unwrap_or("auto");
    let is_pat = match credential_type {
        "pat" => true,
        "bearer" => false,
        "auto" => key.starts_with(QODER_PAT_PREFIX),
        other => {
            return Err(QueryError::config_params(
                "query.router_accounts.invalid_qoder_credential_type",
                params([("type", ParamValue::from(other.to_string()))]),
                format!("params.credential_type '{other}' inválido; use auto, pat ou bearer."),
            ))
        }
    };
    // PATs (pt-...) cannot call the quota endpoint directly: exchange for a
    // short-lived job token first (plain JSON POST, never persisted).
    let (token, exchanged) = if is_pat {
        let exchange: QoderExchange = json(
            ctx.http
                .post(format!("{base}/api/v1/jobToken/exchange"))
                .header("Accept", "application/json")
                .header("User-Agent", "qodercli/1.0.0")
                .header("Cosy-Version", "1.0.0")
                .header("Cosy-ClientType", "5")
                .json(&serde_json::json!({ "personal_token": key })),
        )
        .await?;
        let token = exchange
            .token
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(QueryError::schema)?;
        (token, true)
    } else {
        (key, false)
    };
    let usage: QoderUsage = json(
        ctx.http
            .get(format!("{base}/api/v2/quota/usage"))
            .bearer_auth(token)
            .header("Accept", "application/json"),
    )
    .await?;
    let resets_at = qoder_reset(usage.expires_at.as_ref())?;
    let mut metrics = Vec::new();
    if let Some(quota) = &usage.user_quota {
        if let Some(metric) = qoder_metric("Qoder: usuário", quota, resets_at)? {
            metrics.push(metric);
        }
    }
    if let Some(quota) = &usage.org_resource_package {
        if let Some(metric) = qoder_metric("Qoder: organização", quota, resets_at)? {
            metrics.push(metric);
        }
    }
    if metrics.is_empty() {
        return Err(QueryError::schema());
    }
    let mut notes = Notes::new();
    notes.push(
        "note.router_accounts.qoder_internal_endpoint",
        "Endpoint interno /api/v2/quota/usage observado no 9router; não oficial e sujeito a mudanças.",
    );
    notes.push(
        "note.router_accounts.qoder_missing_fields_unknown",
        "Diferente do 9router, campos ausentes ficam desconhecidos em vez de zero.",
    );
    if exchanged {
        notes.push(
            "note.router_accounts.qoder_pat_exchange",
            "PAT (pt-...) trocado por job token efêmero via /api/v1/jobToken/exchange; nada é armazenado.",
        );
    }
    if let Some(percent) = usage.total_usage_percentage {
        let percent = finite(percent)?;
        notes.push_params(
            "note.router_accounts.qoder_total_usage",
            params([("percent", ParamValue::from(percent.to_string()))]),
            format!("Uso total reportado pelo provedor: {percent}%."),
        );
    }
    if usage.is_quota_exceeded == Some(true) {
        notes.push(
            "note.router_accounts.qoder_quota_exceeded",
            "O provedor sinaliza quota excedida.",
        );
    }
    Ok(Data::new(
        metrics,
        notes,
        format!("{REF_USAGE} (getQoderUsage) + {REF_QODER_MODELS} (resolveQoderCredentials)"),
    ))
}

// ---------------------------------------------------------------------------
// Ollama Cloud
// ---------------------------------------------------------------------------

/// (label, period) per window key, in 9router's OLLAMA_LIMIT_WINDOWS order.
const OLLAMA_WINDOWS: [(&str, &str, &str); 3] = [
    ("session", "Ollama Cloud: sessão", "janela de 5h"),
    ("weekly", "Ollama Cloud: semanal", "janela de 7d"),
    (
        "monthly",
        "Ollama Cloud: mensal",
        "mês da assinatura gratuita",
    ),
];

#[derive(Deserialize)]
struct OllamaUsage {
    limits: Option<OllamaLimits>,
}

#[derive(Deserialize)]
struct OllamaLimits {
    session: Option<OllamaLimit>,
    weekly: Option<OllamaLimit>,
    monthly: Option<OllamaLimit>,
}

#[derive(Deserialize)]
struct OllamaLimit {
    usage: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OllamaMe {
    plan: Option<String>,
    created_at: Option<String>,
}

fn add_utc_months(date: DateTime<Utc>, months: i64) -> DateTime<Utc> {
    let total = i64::from(date.year()) * 12 + i64::from(date.month0()) + months;
    let year = i32::try_from(total.div_euclid(12)).unwrap_or(date.year());
    let month = u32::try_from(total.rem_euclid(12)).unwrap_or(0) + 1;
    // Clamp the signup day into shorter months (e.g. Jan 31 → Feb 28).
    let mut day = date.day();
    let naive = loop {
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
            break date;
        }
        day -= 1;
    };
    DateTime::from_naive_utc_and_offset(naive.and_time(date.time()), Utc)
}

/// Free plan resets monthly from the signup date (ollama.com/pricing), so the
/// next reset is the first monthly anniversary strictly after `now`.
fn next_monthly_reset_from_signup(
    created: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let elapsed =
        (now.year() - created.year()) * 12 + (now.month() as i32 - created.month() as i32);
    for months in i64::from(elapsed.max(0))..=i64::from(elapsed) + 1 {
        let candidate = add_utc_months(created, months);
        if candidate > now {
            return Some(candidate);
        }
    }
    None
}

async fn ollama(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
    now: DateTime<Utc>,
) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "OLLAMA_CLOUD_API_KEY")?;
    let usage: OllamaUsage = json(
        ctx.http
            .get(format!("{base}/api/usage"))
            .bearer_auth(&key)
            .header("Accept", "application/json"),
    )
    .await?;
    // Plan label + signup date are best-effort (fail-open, as in 9router).
    // Body is bounded to 1 MiB; the endpoint returns small JSON metadata.
    let me: Option<OllamaMe> = match ctx
        .http
        .post(format!("{base}/api/me"))
        .bearer_auth(&key)
        .header("Accept", "application/json")
        .header("Content-Length", "0")
        .body("")
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => match response.bytes().await {
            Ok(bytes) if bytes.len() <= 1024 * 1024 => serde_json::from_slice(&bytes).ok(),
            _ => None,
        },
        _ => None,
    };
    let plan = me.as_ref().and_then(|me| me.plan.as_deref()).unwrap_or("");
    let mut notes = Notes::new();
    notes.push(
        "note.router_accounts.ollama_internal_endpoint",
        "Endpoint interno /api/usage observado no 9router; não oficial e sujeito a mudanças.",
    );
    notes.push(
        "note.router_accounts.ollama_usage_ratio",
        "usage é uma razão 0..1 do limite (1.0 = esgotado), exibida como percentual; não há contagem absoluta de requisições.",
    );
    if plan.is_empty() {
        notes.push(
            "note.router_accounts.ollama_plan_unknown",
            "Plano desconhecido: a consulta de metadados da conta falhou (fail-open).",
        );
    } else {
        let mut chars = plan.chars();
        let label = chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_default()
            + &chars.as_str().to_lowercase();
        notes.push_params(
            "note.router_accounts.ollama_plan",
            params([("plan", ParamValue::from(label.clone()))]),
            format!("Plano: {label}"),
        );
    }
    let monthly_reset = if plan.eq_ignore_ascii_case("free") {
        me.as_ref()
            .and_then(|me| me.created_at.as_deref())
            .and_then(|created| DateTime::parse_from_rfc3339(created).ok())
            .and_then(|created| next_monthly_reset_from_signup(created.with_timezone(&Utc), now))
    } else {
        None
    };
    if monthly_reset.is_some() {
        notes.push(
            "note.router_accounts.ollama_monthly_reset_estimated",
            "Renovação mensal estimada pelo aniversário de cadastro, como no 9router; /api/usage não fornece timestamp de renovação.",
        );
    }
    let limits = usage.limits.unwrap_or(OllamaLimits {
        session: None,
        weekly: None,
        monthly: None,
    });
    let by_window = [
        (&OLLAMA_WINDOWS[0], limits.session),
        (&OLLAMA_WINDOWS[1], limits.weekly),
        (&OLLAMA_WINDOWS[2], limits.monthly),
    ];
    let mut metrics = Vec::new();
    for ((key, label, period), limit) in by_window {
        let Some(ratio) = limit.and_then(|limit| limit.usage) else {
            continue;
        };
        // 9router skips missing/NaN windows and clamps the ratio to 0..1.
        if !ratio.is_finite() {
            continue;
        }
        let used = (ratio.clamp(0.0, 1.0) * 100.0).round();
        metrics.push(Metric {
            label: (*label).into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: Some(100.0 - used),
            unit: "%".into(),
            period: Some((*period).into()),
            resets_at: if *key == "monthly" {
                monthly_reset
            } else {
                None
            },
        });
    }
    if metrics.is_empty() {
        // Connected, but the account reported no known limit window. Avoid
        // a fake-Ready empty success; surface the schema gap instead.
        return Err(QueryError::schema());
    }
    Ok(Data::new(
        metrics,
        notes,
        format!("{REF_USAGE} (getOllamaUsage)"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Creds(&'static str);
    impl Credentials for Creds {
        fn get(&self, _: &str) -> Option<String> {
            Some(self.0.into())
        }
    }

    fn account(provider: Provider, params: &[(&str, &str)]) -> Account {
        Account {
            name: "test".into(),
            provider,
            credential_env: None,
            oauth: None,
            params: params
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect::<BTreeMap<_, _>>(),
            refresh_seconds: None,
        }
    }

    fn now(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[tokio::test]
    async fn ollama_paid_maps_session_and_weekly_ratios_to_percent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage"))
            .and(header("authorization", "Bearer dummy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "activity": {"cost": "0.00000"},
                "limits": {
                    "session": {"usage": 0, "models": []},
                    "weekly": {"usage": 1, "models": [{"name": "glm-5.2", "request_count": 5967}]}
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/me"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"Plan": "max"})),
            )
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds("dummy"),
            timeout: Duration::from_secs(2),
        };
        let data = ollama(
            &account(Provider::OllamaCloud, &[]),
            &ctx,
            &server.uri(),
            now("2026-09-18T15:03:00Z"),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 2);
        let session = &data.metrics[0];
        assert_eq!(session.used, Some(0.0));
        assert_eq!(session.limit, Some(100.0));
        assert_eq!(session.remaining, Some(100.0));
        assert_eq!(session.unit, "%");
        assert_eq!(session.resets_at, None);
        let weekly = &data.metrics[1];
        assert_eq!(weekly.used, Some(100.0));
        assert_eq!(weekly.remaining, Some(0.0));
        assert_eq!(weekly.resets_at, None);
    }

    #[tokio::test]
    async fn ollama_free_monthly_reset_comes_from_signup_date() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "limits": {"monthly": {"usage": 0.021, "models": []}}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/me"))
            .and(header("content-length", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"Plan": "free", "CreatedAt": "2025-09-06T22:15:39.871687Z"}),
            ))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds("dummy"),
            timeout: Duration::from_secs(2),
        };
        let data = ollama(
            &account(Provider::OllamaCloud, &[]),
            &ctx,
            &server.uri(),
            now("2026-09-18T15:03:00Z"),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        let monthly = &data.metrics[0];
        assert_eq!(monthly.used, Some(2.0));
        assert_eq!(monthly.remaining, Some(98.0));
        assert_eq!(monthly.resets_at, Some(now("2026-10-06T22:15:39.871687Z")));
    }

    #[test]
    fn monthly_reset_matches_reference_boundaries() {
        // Cases from tests/unit/ollama-usage.test.js in the reference.
        let signup = now("2026-09-18T09:50:49.514335Z");
        assert_eq!(
            next_monthly_reset_from_signup(signup, now("2026-09-18T15:33:33Z")),
            Some(now("2026-10-18T09:50:49.514335Z"))
        );
        let signup = now("2025-09-25T10:00:00Z");
        assert_eq!(
            next_monthly_reset_from_signup(signup, now("2026-09-18T15:33:33Z")),
            Some(now("2026-09-25T10:00:00Z"))
        );
        let signup = now("2026-01-31T12:00:00Z");
        assert_eq!(
            next_monthly_reset_from_signup(signup, now("2026-02-10T00:00:00Z")),
            Some(now("2026-02-28T12:00:00Z"))
        );
    }

    #[tokio::test]
    async fn ollama_empty_limits_is_schema_error_not_fake_ready() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "activity": {}, "limits": {}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/me"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"Plan": "free"})),
            )
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds("dummy"),
            timeout: Duration::from_secs(2),
        };
        let error = ollama(
            &account(Provider::OllamaCloud, &[]),
            &ctx,
            &server.uri(),
            now("2026-09-18T15:03:00Z"),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, crate::Status::Error);
    }

    #[tokio::test]
    async fn qoder_pat_is_exchanged_then_quota_read_with_job_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/jobToken/exchange"))
            .and(body_json(
                serde_json::json!({"personal_token": "pt-secret"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "jt-job", "expires_in": 86_400_000
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/quota/usage"))
            .and(header("authorization", "Bearer jt-job"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "userQuota": {"total": 1000, "used": 250, "remaining": 750, "unit": "credits"},
                "orgResourcePackage": {"total": 5000, "used": 100, "remaining": 4900, "unit": "credits"},
                "expiresAt": 1_767_225_600_000i64,
                "totalUsagePercentage": 25.0,
                "isQuotaExceeded": false
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds("pt-secret"),
            timeout: Duration::from_secs(2),
        };
        let data = qoder(&account(Provider::Qoder, &[]), &ctx, &server.uri())
            .await
            .unwrap();
        assert_eq!(data.metrics.len(), 2);
        let user = &data.metrics[0];
        assert_eq!(user.used, Some(250.0));
        assert_eq!(user.limit, Some(1000.0));
        assert_eq!(user.remaining, Some(750.0));
        assert_eq!(user.unit, "credits");
        assert_eq!(
            user.resets_at,
            DateTime::from_timestamp_millis(1_767_225_600_000)
        );
        let org = &data.metrics[1];
        assert_eq!(org.remaining, Some(4900.0));
    }

    #[tokio::test]
    async fn qoder_bearer_credential_skips_exchange() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/jobToken/exchange"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/quota/usage"))
            .and(header("authorization", "Bearer jt-direct"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "userQuota": {"total": 100, "used": 40, "remaining": 60, "unit": "credits"}
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds("jt-direct"),
            timeout: Duration::from_secs(2),
        };
        let data = qoder(
            &account(Provider::Qoder, &[("credential_type", "bearer")]),
            &ctx,
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].used, Some(40.0));
        assert_eq!(data.metrics[0].resets_at, None);
    }

    #[tokio::test]
    async fn qoder_missing_fields_stay_unknown_not_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/quota/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "userQuota": {"used": 10},
                "orgResourcePackage": {}
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds("jt-direct"),
            timeout: Duration::from_secs(2),
        };
        let data = qoder(&account(Provider::Qoder, &[]), &ctx, &server.uri())
            .await
            .unwrap();
        assert_eq!(data.metrics.len(), 1);
        let metric = &data.metrics[0];
        assert_eq!(metric.used, Some(10.0));
        assert_eq!(metric.limit, None);
        assert_eq!(metric.remaining, None);
        // Default unit only when the provider omits it, matching 9router.
        assert_eq!(metric.unit, "créditos");
    }

    #[tokio::test]
    async fn qoder_invalid_region_is_config_error() {
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds("dummy"),
            timeout: Duration::from_secs(2),
        };
        let error = fetch(&account(Provider::Qoder, &[("region", "moon")]), &ctx)
            .await
            .unwrap_err();
        assert_eq!(error.status, crate::Status::NeedsConfiguration);
    }
}
