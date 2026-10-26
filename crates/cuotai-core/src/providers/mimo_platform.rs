//! Quota via the Xiaomi MiMo platform web session (`platform.xiaomimimo.com`).
//!
//! The `sk-` key issued by native login (authorize/ECDH flow) is only
//! an inference key and does NOT read quota at any known endpoint:
//!   - `aistudio.xiaomimimo.com/open-apis/v1/user/usage` refuses `sk-` with 401
//!     (9router `open-sse/services/usage/xiaomi-mimo.js`: "the sk- API key
//!     cannot read the quota");
//!   - the `platform.xiaomimimo.com/api/v1/*` console authenticates exclusively
//!     by web session cookie — the official bundle
//!     (`/static/main.*.chunk.js`) calls `fetch(..., {credentials:
//!     "same-origin"})` without any Authorization header.
//!
//! Reachable real mode (reference: CodexBar MIT,
//! `Sources/CodexBarCore/Providers/MiMo/MiMoUsageFetcher.swift` and
//! `MiMoCookieImporter.swift`): the user signs in to platform.xiaomimimo.com in the
//! browser and pastes the session into the account wizard (system vault, PRD
//! 0016) or exports it in a variable referenced by
//! `params.platform_session_env`. The session descriptor
//! (`crate::user_session::descriptors`) requires `api-platform_serviceToken` and
//! `userId`; `super::web_session` checks them before calling [`fetch`].
//!
//! Endpoints (GET, web session cookie):
//!   /balance           -> {code, data:{balance,currency,cashBalance?,giftBalance?}}
//!                         (monetary values as strings)
//!   /tokenPlan/detail  -> {code, data:{planCode?,currentPeriodEnd?,expired}}
//!   /tokenPlan/usage   -> {code, data:{monthUsage:{percent, items:[{name,used,limit,percent}]}}}
//! The tokenPlan `percent` is the USED FRACTION (0–1): CodexBar converts with
//! `clamp(percent*100)` to usedPercent and the official bundle uses
//! `min(100, max(0, 100*e))` in the "Current plan usage" bar.
//! `currentPeriodEnd` comes as "yyyy-MM-dd HH:mm:ss" in UTC.
//!
//! `/balance` is mandatory (as in CodexBar); the two tokenPlan ones are
//! complementary — a failure in them degrades to a note, never to an invented zero.

use super::{Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Metric, MetricKind, MetricWindow, Status};
use chrono::{DateTime, Utc};

/// Fixed base of the web console API. In tests, the mock's URL is injected.
pub(crate) const PLATFORM_API_BASE: &str = "https://platform.xiaomimimo.com/api/v1";
const PLATFORM_ORIGIN: &str = "https://platform.xiaomimimo.com";
/// Browser UA, as CodexBar sends; the backend expects a real web session.
const PLATFORM_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

/// Queries balance and token plan with the platform web session cookie
/// (already checked against the descriptor by `super::web_session`).
pub(crate) async fn fetch(ctx: &Context<'_>, base: &str, cookie: &str) -> Result<Data, QueryError> {
    let cookie = cookie.trim();

    let base = base.trim_end_matches('/');
    let balance_body = get(ctx, &format!("{base}/balance"), cookie)
        .await?
        .ok_or_else(QueryError::schema)?;
    // tokenPlan/* are complementary: unavailability becomes a note.
    let detail_body = get(ctx, &format!("{base}/tokenPlan/detail"), cookie)
        .await
        .ok()
        .flatten();
    let usage_body = get(ctx, &format!("{base}/tokenPlan/usage"), cookie)
        .await
        .ok()
        .flatten();

    build_data(
        &balance_body,
        detail_body.as_ref(),
        usage_body.as_ref(),
        base,
    )
}

/// Cookie-authenticated GET. Ok(None) when the complementary endpoint fails
/// or carries no valid envelope; Err only for authentication/transport failures
/// that compromise the session as a whole.
async fn get(
    ctx: &Context<'_>,
    url: &str,
    cookie: &str,
) -> Result<Option<serde_json::Value>, QueryError> {
    let request = ctx
        .http
        .get(url)
        .header("Cookie", cookie)
        .header("Accept", "application/json, text/plain, */*")
        .header("Accept-Language", "en-US,en;q=0.9")
        // The server formats dates according to this header; UTC keeps the
        // parse of currentPeriodEnd stable (CodexBar assumes UTC).
        .header("x-timeZone", "UTC")
        .header("Origin", PLATFORM_ORIGIN)
        .header("Referer", format!("{PLATFORM_ORIGIN}/"))
        .header("User-Agent", PLATFORM_UA)
        .timeout(ctx.timeout);
    // A redirect (the login flow of an expired session) is refused by
    // `send_session` and never followed.
    let response =
        super::send_session(request, crate::Provider::XiaomiMimo, "platform_session_env").await?;
    let status = response.status().as_u16();
    match status {
        s @ 401 => {
            return Err(QueryError { diagnostic: None,
                status: Status::AuthenticationError,
                note: Localized::with_params(
                    "query.mimo_platform.session_expired_http",
                    params([("code", ParamValue::from(s))]),
                    "Xiaomi MiMo (platform): web session expired. Sign in to platform.xiaomimimo.com in the browser and re-export the cookie.",
                ),
            });
        }
        s @ 403 => {
            return Err(QueryError {
                diagnostic: None,
                status: Status::AuthenticationError,
                note: Localized::with_params(
                    "query.mimo_platform.session_rejected_http",
                    params([("code", ParamValue::from(s))]),
                    "Xiaomi MiMo (platform): session cookie rejected (HTTP 403).",
                ),
            });
        }
        s if !(200..300).contains(&s) => {
            return Err(QueryError {
                diagnostic: None,
                status: Status::Error,
                note: Localized::with_params(
                    "query.mimo_platform.http",
                    params([("code", ParamValue::from(s))]),
                    "Query returned HTTP {code}. Check access and permissions.",
                ),
            });
        }
        _ => {}
    }
    let body = super::body_value(response).await?;
    match body.get("code").and_then(|c| c.as_i64()) {
        Some(0) => Ok(Some(body)),
        // The envelope also signals a session in the body (CodexBar treats it the same).
        Some(code @ (401 | 403)) => Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::with_params(
                "query.mimo_platform.session_invalid_envelope",
                params([("code", ParamValue::from(code.to_string()))]),
                "Xiaomi MiMo (platform): web session expired or invalid. Re-export the cookie.",
            ),
        }),
        // Complementary endpoint without subscription/permission: degrades to a note.
        _ => Ok(None),
    }
}

fn build_data(
    balance_body: &serde_json::Value,
    detail_body: Option<&serde_json::Value>,
    usage_body: Option<&serde_json::Value>,
    base: &str,
) -> Result<Data, QueryError> {
    let mut metrics = Vec::new();
    let mut notes = Notes::new();

    // ---- Token plan (monthly): percent is the USED fraction (0–1).
    let detail = detail_body.and_then(|b| b.get("data"));
    let resets_at = detail
        .and_then(|d| d.get("currentPeriodEnd"))
        .and_then(|v| v.as_str())
        .and_then(parse_platform_datetime);
    if let Some(plan) = detail
        .and_then(|d| d.get("planCode"))
        .and_then(|v| v.as_str())
    {
        let plan = plan.trim();
        if !plan.is_empty() {
            notes.push_params(
                "note.mimo_platform.token_plan",
                params([("plan", ParamValue::from(plan.to_string()))]),
                "Token plan: {plan}.",
            );
        }
    }
    if detail
        .and_then(|d| d.get("expired"))
        .and_then(|v| v.as_bool())
        == Some(true)
    {
        notes.push(
            "note.mimo_platform.token_plan_expired",
            "The token plan is expired.",
        );
    }

    match usage_body
        .and_then(|b| b.get("data"))
        .and_then(|d| d.get("monthUsage"))
    {
        Some(month) => {
            // Reference (CodexBar parseTokenPlanUsage): values come from
            // items[0]; monthUsage.percent is only a fallback when the item
            // carries nothing usable. `percent` is the USED fraction (0–1).
            let item = month
                .get("items")
                .and_then(|v| v.as_array())
                .and_then(|items| items.first());
            let finite = |v: Option<&serde_json::Value>| {
                v.and_then(|v| v.as_f64()).filter(|v| v.is_finite())
            };
            let absolute = item.and_then(|i| {
                let limit = finite(i.get("limit")).filter(|v| *v > 0.0)?;
                let used = finite(i.get("used")).filter(|v| *v >= 0.0)?;
                Some((used, limit))
            });
            let fraction = item
                .and_then(|i| finite(i.get("percent")))
                .or_else(|| finite(month.get("percent")));
            if let Some((used, limit)) = absolute {
                // No clamp: usage above the limit is real and shows up as
                // negative remaining, never as an artificial 0.
                metrics.push(Metric {
                    billing: Default::default(),
                    scope: None,
                    label: "Plano de tokens MiMo".into(),
                    used: Some(used),
                    limit: Some(limit),
                    remaining: Some(limit - used),
                    // Unidade vista na UI oficial ("Credits"/积分) e no CodexBar.
                    unit: "créditos".into(),
                    kind: MetricKind::Quota,
                    window: Some(MetricWindow::Monthly),
                    period: Some("mensal".into()),
                    resets_at,
                });
            } else if let Some(fraction) = fraction {
                // Used fraction can exceed 1 (plan overrun): no clamp.
                let used_pct = fraction * 100.0;
                metrics.push(Metric {
                    billing: Default::default(),
                    scope: None,
                    label: "Plano de tokens MiMo".into(),
                    used: Some(used_pct),
                    limit: Some(100.0),
                    remaining: Some(100.0 - used_pct),
                    unit: "%".into(),
                    kind: MetricKind::Quota,
                    window: Some(MetricWindow::Monthly),
                    period: Some("mensal".into()),
                    resets_at,
                });
                notes.push(
                    "note.mimo_platform.percentage_only",
                    "API returned only the plan percentage; `percent` is the USED fraction (0–1).",
                );
            } else {
                // malformed monthUsage: complementary endpoint — degrades to a
                // note and preserves the balance, never zeroes the whole query.
                notes.push(
                    "note.mimo_platform.month_usage_unusable",
                    "Token plan returned monthUsage without usable values; no quota was estimated.",
                );
            }
        }
        None => {
            notes.push(
                "note.mimo_platform.token_plan_unavailable",
                "Token plan not returned (no Token Plan subscription or endpoint unavailable); no quota was estimated.",
            );
        }
    }

    // ---- Pay-as-you-go balance (mandatory, as in CodexBar).
    let balance_data = balance_body
        .get("data")
        .ok_or_else(|| QueryError::schema_at(balance_body, &["data"], "object"))?;
    let balance = balance_data
        .get("balance")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .or_else(|| v.as_f64())
        })
        .filter(|v| v.is_finite())
        .ok_or_else(|| QueryError::schema_at(balance_body, &["data", "balance"], "number"))?;
    let currency = balance_data
        .get("currency")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| QueryError::schema_at(balance_body, &["data", "currency"], "string"))?;
    metrics.push(Metric {
        billing: Default::default(),
        scope: None,
        label: "Saldo da plataforma".into(),
        used: None,
        limit: None,
        remaining: Some(balance),
        unit: currency.to_string(),
        kind: MetricKind::Balance,
        window: None,
        period: Some("saldo atual".into()),
        resets_at: None,
    });
    for key in ["cashBalance", "giftBalance"] {
        if let Some(v) = balance_data.get(key).and_then(|v| {
            v.as_str()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .or_else(|| v.as_f64())
        }) {
            if v.is_finite() {
                let (id, template) = match key {
                    "cashBalance" => (
                        "note.mimo_platform.cash_balance",
                        "Cash balance: {amount} {currency}.",
                    ),
                    "giftBalance" => (
                        "note.mimo_platform.gift_balance",
                        "Gift balance: {amount} {currency}.",
                    ),
                    _ => unreachable!("balance key controlled by the adapter"),
                };
                notes.push_params(
                    id,
                    params([
                        ("amount", ParamValue::from(v.to_string())),
                        ("currency", ParamValue::from(currency.to_string())),
                    ]),
                    template,
                );
            }
        }
    }

    notes.push_params(
        "note.mimo_platform.console_api_reference",
        params([("base", ParamValue::from(base.to_string()))]),
        "Source: {base}. Web console routes are not publicly documented (reference: CodexBar MIT, MiMoUsageFetcher.swift); they may change without notice.",
    );
    Ok(Data::new(metrics, notes, base))
}

/// "yyyy-MM-dd HH:mm:ss" in UTC (currentPeriodEnd format in CodexBar).
fn parse_platform_datetime(s: &str) -> Option<DateTime<Utc>> {
    chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
}

/// PRD 0028 RF11: the fixture whose metric identities `label_snapshot`
/// records. Add a case when the adapter learns a new metric.
#[cfg(test)]
pub(super) fn label_fixtures() -> Vec<(&'static str, Vec<Metric>)> {
    let balance = serde_json::json!({"data": {"balance": "10.5", "currency": "USD"}});
    let detail = serde_json::json!({"data": {"planCode": "lite"}});
    let usage = serde_json::json!({"data": {"monthUsage": {"items": [
        {"limit": 100.0, "used": 10.0, "percent": 0.1}
    ]}}});
    let data = build_data(&balance, Some(&detail), Some(&usage), "https://example.com")
        .expect("fixture decodes");
    vec![("mimo_platform", data.metrics)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const COOKIE: &str = "api-platform_serviceToken=tok-1; userId=12345; api-platform_ph=x";

    struct NoCredentials;
    impl Credentials for NoCredentials {
        fn get(&self, _name: &str) -> Option<String> {
            None
        }
    }

    fn ctx_with<'a>(http: &'a reqwest::Client, creds: &'a dyn Credentials) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: Duration::from_secs(5),
        }
    }

    async fn server_with(balance: serde_json::Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .and(header("Cookie", COOKIE))
            .respond_with(ResponseTemplate::new(200).set_body_json(balance))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tokenPlan/detail"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {
                    "planCode": "mimo-pro",
                    "currentPeriodEnd": "2026-10-01 00:00:00",
                    "expired": false
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tokenPlan/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {
                    "monthUsage": {
                        "percent": 0.25,
                        "items": [{ "name": "Credits", "used": 250, "limit": 1000, "percent": 0.25 }]
                    }
                }
            })))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn plataforma_combina_plano_mensal_e_saldo() {
        let server = server_with(serde_json::json!({
            "code": 0,
            "data": { "balance": "12.50", "currency": "USD", "cashBalance": "10.00", "giftBalance": "2.50" }
        }))
        .await;
        let http = reqwest::Client::new();
        let data = fetch(&ctx_with(&http, &NoCredentials), &server.uri(), COOKIE)
            .await
            .expect("sucesso");

        let plan = &data.metrics[0];
        assert_eq!(plan.label, "Plano de tokens MiMo");
        assert_eq!(plan.used, Some(250.0));
        assert_eq!(plan.limit, Some(1000.0));
        assert_eq!(plan.kind, MetricKind::Quota);
        assert_eq!(plan.window, Some(MetricWindow::Monthly));
        assert_eq!(plan.remaining, Some(750.0));
        assert_eq!(plan.period.as_deref(), Some("mensal"));
        assert_eq!(
            plan.resets_at,
            DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                .ok()
                .map(|d| d.with_timezone(&Utc))
        );

        let saldo = &data.metrics[1];
        assert_eq!(saldo.remaining, Some(12.5));
        assert_eq!(saldo.used, None);
        assert_eq!(saldo.unit, "USD");
        assert_eq!(saldo.kind, MetricKind::Balance);
        assert_eq!(saldo.window, None);
        assert!(data.notes.iter().any(|n| n.contains("mimo-pro")));
        assert!(data.notes.iter().any(|n| n.contains("Gift balance")));
    }

    #[tokio::test]
    async fn plataforma_401_reporta_sessao_expirada() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let err = fetch(&ctx_with(&http, &NoCredentials), &server.uri(), COOKIE)
            .await
            .expect_err("401");
        assert_eq!(err.status, Status::AuthenticationError);
        assert!(err.note.fallback.contains("expired"));
    }

    #[tokio::test]
    async fn plataforma_sem_plano_nao_inventa_quota() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "balance": "0.00", "currency": "USD" }
            })))
            .mount(&server)
            .await;
        // tokenPlan without a subscription: envelope with code != 0 and no data.
        Mock::given(method("GET"))
            .and(path("/tokenPlan/detail"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 40010, "message": "no subscription"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tokenPlan/usage"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch(&ctx_with(&http, &NoCredentials), &server.uri(), COOKIE)
            .await
            .expect("saldo continua disponível");
        // No invented plan; only the real balance.
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].label, "Saldo da plataforma");
        assert_eq!(data.metrics[0].remaining, Some(0.0));
        assert!(data.notes.iter().any(|n| n.contains("not returned")));
    }

    #[tokio::test]
    async fn plataforma_percent_sem_itens_e_fracao_usada() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "balance": "5", "currency": "CNY" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tokenPlan/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "monthUsage": { "percent": 0.4, "items": [] } }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch(&ctx_with(&http, &NoCredentials), &server.uri(), COOKIE)
            .await
            .expect("sucesso");
        let plan = &data.metrics[0];
        // percent 0.4 = 40% USADOS → 60% restantes.
        assert_eq!(plan.used, Some(40.0));
        assert_eq!(plan.remaining, Some(60.0));
        assert_eq!(plan.kind, MetricKind::Quota);
        assert_eq!(plan.window, Some(MetricWindow::Monthly));
        assert_eq!(plan.unit, "%");
    }

    #[tokio::test]
    async fn plataforma_saldo_invalido_e_schema_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "currency": "USD" }
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let err = fetch(&ctx_with(&http, &NoCredentials), &server.uri(), COOKIE)
            .await
            .expect_err("sem balance");
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn plataforma_month_malformado_preserva_saldo() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "balance": "7.25", "currency": "USD" }
            })))
            .mount(&server)
            .await;
        // monthUsage present but with neither items nor usable percent.
        Mock::given(method("GET"))
            .and(path("/tokenPlan/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "monthUsage": { "items": [], "percent": "abc" } }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch(&ctx_with(&http, &NoCredentials), &server.uri(), COOKIE)
            .await
            .expect("saldo preservado");
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].label, "Saldo da plataforma");
        assert_eq!(data.metrics[0].remaining, Some(7.25));
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("without usable values")));
    }

    #[tokio::test]
    async fn plataforma_uso_acima_do_limite_sem_clamp() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "balance": "0", "currency": "USD" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tokenPlan/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "monthUsage": {
                    "percent": 1.2,
                    "items": [{ "name": "Credits", "used": 1200, "limit": 1000, "percent": 1.2 }]
                } }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch(&ctx_with(&http, &NoCredentials), &server.uri(), COOKIE)
            .await
            .expect("sucesso");
        let plan = &data.metrics[0];
        // Real overrun: 1200 used of 1000 → negative remaining, no clamp.
        assert_eq!(plan.used, Some(1200.0));
        assert_eq!(plan.limit, Some(1000.0));
        assert_eq!(plan.remaining, Some(-200.0));
    }

    #[tokio::test]
    async fn plataforma_percent_do_item_tem_precedencia_sobre_o_do_mes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "balance": "0", "currency": "USD" }
            })))
            .mount(&server)
            .await;
        // Item without counters but with its own percent; the month's one differs.
        Mock::given(method("GET"))
            .and(path("/tokenPlan/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": { "monthUsage": {
                    "percent": 0.9,
                    "items": [{ "name": "Credits", "percent": 0.3 }]
                } }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let data = fetch(&ctx_with(&http, &NoCredentials), &server.uri(), COOKIE)
            .await
            .expect("sucesso");
        let plan = &data.metrics[0];
        // CodexBar reads items[0]: the item's percent (0.3 → 30%), not the month's.
        assert_eq!(plan.used, Some(30.0));
        assert_eq!(plan.remaining, Some(70.0));
        assert_eq!(plan.unit, "%");
    }
}
