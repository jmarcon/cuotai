//! Quota via sessão web da plataforma Xiaomi MiMo (`platform.xiaomimimo.com`).
//!
//! A chave `sk-` emitida pelo login nativo (fluxo authorize/ECDH) é somente
//! uma chave de inferência e NÃO lê quota em nenhum endpoint conhecido:
//!   - `aistudio.xiaomimimo.com/open-apis/v1/user/usage` recusa `sk-` com 401
//!     (9router `open-sse/services/usage/xiaomi-mimo.js`: "the sk- API key
//!     cannot read the quota");
//!   - o console `platform.xiaomimimo.com/api/v1/*` autentica exclusivamente
//!     por cookie de sessão web — o bundle oficial
//!     (`/static/main.*.chunk.js`) chama `fetch(..., {credentials:
//!     "same-origin"})` sem qualquer cabeçalho Authorization.
//!
//! Modo real alcançável (referência: CodexBar MIT,
//! `Sources/CodexBarCore/Providers/MiMo/MiMoUsageFetcher.swift` e
//! `MiMoCookieImporter.swift`): o usuário entra em platform.xiaomimimo.com no
//! navegador e exporta o cookie da sessão; `params.platform_session_env`
//! referencia a variável com esse cookie. São obrigatórios
//! `api-platform_serviceToken` e `userId` no valor.
//!
//! Endpoints (GET, cookie de sessão web):
//!   /balance           -> {code, data:{balance,currency,cashBalance?,giftBalance?}}
//!                         (valores monetários como strings)
//!   /tokenPlan/detail  -> {code, data:{planCode?,currentPeriodEnd?,expired}}
//!   /tokenPlan/usage   -> {code, data:{monthUsage:{percent, items:[{name,used,limit,percent}]}}}
//! `percent` do tokenPlan é a FRAÇÃO USADA (0–1): o CodexBar converte com
//! `clamp(percent*100)` para usedPercent e o bundle oficial usa
//! `min(100, max(0, 100*e))` na barra de "Current plan usage".
//! `currentPeriodEnd` vem como "yyyy-MM-dd HH:mm:ss" em UTC.
//!
//! `/balance` é obrigatório (como no CodexBar); os dois de tokenPlan são
//! complementares — falha neles degrada para nota, nunca para zero inventado.

use super::{Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Metric, MetricKind, MetricWindow, Status};
use chrono::{DateTime, Utc};

/// Base fixa da API do console web. Em testes, a URL do mock é injetada.
pub(crate) const PLATFORM_API_BASE: &str = "https://platform.xiaomimimo.com/api/v1";
const PLATFORM_ORIGIN: &str = "https://platform.xiaomimimo.com";
/// UA de navegador, como o CodexBar envia; o backend espera sessão web real.
const PLATFORM_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

/// Cookies exigidos pela sessão web da plataforma (MiMoCookieImporter.swift).
const REQUIRED_COOKIES: [&str; 2] = ["api-platform_serviceToken", "userId"];

const REF_NOTE: &str = "Rotas do console web não documentadas publicamente (referência: CodexBar MIT, MiMoUsageFetcher.swift); podem mudar sem aviso.";

/// Consulta saldo e plano de tokens com o cookie da sessão web da plataforma.
pub(crate) async fn fetch(ctx: &Context<'_>, base: &str, cookie: &str) -> Result<Data, QueryError> {
    let cookie = cookie.trim();
    validate_cookie(cookie)?;

    let base = base.trim_end_matches('/');
    let balance_body = get(ctx, &format!("{base}/balance"), cookie)
        .await?
        .ok_or_else(QueryError::schema)?;
    // tokenPlan/* são complementares: indisponibilidade vira nota.
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

/// O cookie precisa carregar `api-platform_serviceToken` e `userId` com valor
/// não vazio (validação local, antes de qualquer requisição).
fn validate_cookie(cookie: &str) -> Result<(), QueryError> {
    let has = |name: &str| {
        cookie.split(';').any(|pair| {
            let mut kv = pair.trim().splitn(2, '=');
            kv.next().map(str::trim) == Some(name)
                && kv.next().map(str::trim).is_some_and(|v| !v.is_empty())
        })
    };
    if REQUIRED_COOKIES.iter().all(|name| has(name)) {
        return Ok(());
    }
    Err(QueryError::config(
        "query.mimo_platform.missing_cookie_fields",
        "Xiaomi MiMo (plataforma): o cookie em params.platform_session_env precisa incluir api-platform_serviceToken e userId. Exporte o cookie da sessão após entrar em platform.xiaomimimo.com no navegador.",
    ))
}

/// GET autenticado por cookie. Ok(None) quando o endpoint complementar falha
/// ou não traz envelope válido; Err apenas para falhas de autenticação/transporte
/// que comprometem a sessão como um todo.
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
        // O servidor formata datas conforme este cabeçalho; UTC mantém o
        // parse de currentPeriodEnd estável (CodexBar assume UTC).
        .header("x-timeZone", "UTC")
        .header("Origin", PLATFORM_ORIGIN)
        .header("Referer", format!("{PLATFORM_ORIGIN}/"))
        .header("User-Agent", PLATFORM_UA)
        .timeout(ctx.timeout);
    let response = request.send().await.map_err(|_| QueryError::transport())?;
    let status = response.status().as_u16();
    match status {
        // Sessão expirada redireciona para o fluxo de login (3xx) ou 401.
        s @ (300..=399 | 401) => {
            return Err(QueryError {
                status: Status::AuthenticationError,
                note: Localized::with_params(
                    "query.mimo_platform.session_expired_http",
                    params([("code", ParamValue::from(s))]),
                    "Xiaomi MiMo (plataforma): sessão web expirada. Entre em platform.xiaomimimo.com no navegador e exporte o cookie novamente.",
                ),
            });
        }
        s @ 403 => {
            return Err(QueryError {
                status: Status::AuthenticationError,
                note: Localized::with_params(
                    "query.mimo_platform.session_rejected_http",
                    params([("code", ParamValue::from(s))]),
                    "Xiaomi MiMo (plataforma): cookie de sessão recusado (HTTP 403).",
                ),
            });
        }
        s if !(200..300).contains(&s) => {
            return Err(QueryError {
                status: Status::Error,
                note: Localized::with_params(
                    "query.mimo_platform.http",
                    params([("code", ParamValue::from(s))]),
                    format!("Consulta retornou HTTP {s}. Verifique acesso e permissões."),
                ),
            });
        }
        _ => {}
    }
    let body: serde_json::Value = response.json().await.map_err(|_| QueryError::schema())?;
    match body.get("code").and_then(|c| c.as_i64()) {
        Some(0) => Ok(Some(body)),
        // O envelope também sinaliza sessão no corpo (CodexBar trata igual).
        Some(code @ (401 | 403)) => Err(QueryError {
            status: Status::AuthenticationError,
            note: Localized::with_params(
                "query.mimo_platform.session_invalid_envelope",
                params([("code", ParamValue::from(code.to_string()))]),
                "Xiaomi MiMo (plataforma): sessão web expirada ou inválida. Exporte o cookie novamente.",
            ),
        }),
        // Endpoint complementar sem assinatura/permissão: degrada para nota.
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

    // ---- Plano de tokens (mensal): percent é a fração USADA (0–1).
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
                format!("Plano de tokens: {plan}."),
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
            "O plano de tokens está expirado.",
        );
    }

    match usage_body
        .and_then(|b| b.get("data"))
        .and_then(|d| d.get("monthUsage"))
    {
        Some(month) => {
            // Referência (CodexBar parseTokenPlanUsage): os valores vêm de
            // items[0]; monthUsage.percent só é fallback quando o item não
            // traz nada utilizável. `percent` é a fração USADA (0–1).
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
                // Sem clamp: uso acima do limite é real e aparece como
                // remaining negativo, nunca como 0 artificial.
                metrics.push(Metric {
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
                // Fração usada pode passar de 1 (estouro do plano): sem clamp.
                let used_pct = fraction * 100.0;
                metrics.push(Metric {
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
                    "A API retornou apenas o percentual do plano; `percent` é a fração USADA (0–1).",
                );
            } else {
                // monthUsage malformado: endpoint complementar — degrada para
                // nota e preserva o saldo, nunca zera a consulta inteira.
                notes.push(
                    "note.mimo_platform.month_usage_unusable",
                    "Plano de tokens retornou monthUsage sem valores utilizáveis; nenhuma quota foi estimada.",
                );
            }
        }
        None => {
            notes.push(
                "note.mimo_platform.token_plan_unavailable",
                "Plano de tokens não retornado (sem assinatura Token Plan ou endpoint indisponível); nenhuma quota foi estimada.",
            );
        }
    }

    // ---- Saldo pay-as-you-go (obrigatório, como no CodexBar).
    let balance_data = balance_body.get("data").ok_or_else(QueryError::schema)?;
    let balance = balance_data
        .get("balance")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .or_else(|| v.as_f64())
        })
        .filter(|v| v.is_finite())
        .ok_or_else(QueryError::schema)?;
    let currency = balance_data
        .get("currency")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(QueryError::schema)?;
    metrics.push(Metric {
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
    for (key, label) in [("cashBalance", "pago"), ("giftBalance", "bonificado")] {
        if let Some(v) = balance_data.get(key).and_then(|v| {
            v.as_str()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .or_else(|| v.as_f64())
        }) {
            if v.is_finite() {
                let id = match key {
                    "cashBalance" => "note.mimo_platform.cash_balance",
                    "giftBalance" => "note.mimo_platform.gift_balance",
                    _ => unreachable!("chave de saldo controlada pelo adaptador"),
                };
                notes.push_params(
                    id,
                    params([
                        ("amount", ParamValue::from(v.to_string())),
                        ("currency", ParamValue::from(currency.to_string())),
                    ]),
                    format!("Saldo {label}: {v} {currency}."),
                );
            }
        }
    }

    notes.push_params(
        "note.mimo_platform.console_api_reference",
        params([("base", ParamValue::from(base.to_string()))]),
        format!("Fonte: {base}. {REF_NOTE}"),
    );
    Ok(Data::new(metrics, notes, base))
}

/// "yyyy-MM-dd HH:mm:ss" em UTC (formato de currentPeriodEnd no CodexBar).
fn parse_platform_datetime(s: &str) -> Option<DateTime<Utc>> {
    chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
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
        assert!(data.notes.iter().any(|n| n.contains("bonificado")));
    }

    #[tokio::test]
    async fn plataforma_cookie_sem_campos_obrigatorios_nao_faz_requisicao() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let err = fetch(
            &ctx_with(&http, &NoCredentials),
            &server.uri(),
            "session=abc",
        )
        .await
        .expect_err("cookie incompleto");
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("api-platform_serviceToken"));
        server.verify().await;
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
        assert!(err.note.fallback.contains("expirada"));
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
        // tokenPlan sem assinatura: envelope com code != 0 e sem dados.
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
        // Nenhum plano inventado; apenas o saldo real.
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].label, "Saldo da plataforma");
        assert_eq!(data.metrics[0].remaining, Some(0.0));
        assert!(data.notes.iter().any(|n| n.contains("não retornado")));
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
        // monthUsage presente mas sem items nem percent utilizável.
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
            .any(|n| n.contains("sem valores utilizáveis")));
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
        // Estouro real: 1200 usados de 1000 → remaining negativo, sem clamp.
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
        // Item sem contadores mas com percent próprio; o do mês diverge.
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
        // CodexBar lê items[0]: percent do item (0.3 → 30%), não o do mês.
        assert_eq!(plan.used, Some(30.0));
        assert_eq!(plan.remaining, Some(70.0));
        assert_eq!(plan.unit, "%");
    }
}
