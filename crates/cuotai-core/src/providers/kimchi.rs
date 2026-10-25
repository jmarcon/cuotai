//! Consulta read-only de créditos e orçamentos do Kimchi.
//!
//! Contrato confirmado no cliente oficial `getkimchi/kimchi`, revisão
//! `36cc0406339a50f5ced984266d0306fb8a150408` (2026-09-29), em
//! `src/extensions/billing/status.ts`:
//!
//! - `GET https://llm.kimchi.dev/v1/credits`
//! - `GET https://llm.kimchi.dev/v1/budget`
//! - ambos usam `Authorization: Bearer $KIMCHI_API_KEY`;
//! - `/v1/credits` devolve `serverless`, `tier`, `is_paid_tier`,
//!   `billing_status`, `has_credits` e `remaining` (número ou string numérica);
//! - `/v1/budget` devolve `period.{startTime,endTime}` e `budgets[]`, com
//!   `scope`, `budgetLimitUsd`, `totalSpendUsd` e `providerBudgets[]`.
//!
//! O endpoint de orçamento é complementar: o próprio cliente oficial tolera
//! `404` de proxies antigos. Uma falha apenas nessa segunda consulta não apaga
//! o saldo real já obtido de `/v1/credits`. Nenhum endpoint de geração é usado.
//! A criação manual da chave está documentada em
//! <https://docs.kimchi.dev/docs/inference-quickstart.md>.

use super::{json, secret, send as secure_send, Context, Data, Miss, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Status};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

const KIMCHI_BASE: &str = "https://llm.kimchi.dev";
const CREDITS_PATH: &str = "/v1/credits";
const BUDGET_PATH: &str = "/v1/budget";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    kimchi_at(account, ctx, KIMCHI_BASE).await
}

async fn kimchi_at(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "KIMCHI_API_KEY")?;
    let base = base.trim_end_matches('/');
    let credits_url = format!("{base}{CREDITS_PATH}");
    let budget_url = format!("{base}{BUDGET_PATH}");

    let credits_body: Value = json(
        ctx.http
            .get(&credits_url)
            .bearer_auth(key.trim())
            .header("Accept", "application/json")
            .timeout(ctx.timeout),
    )
    .await?;
    let credits = parse_credits(&credits_body)?;

    if credits.serverless == Some(false) {
        return Err(QueryError::unsupported(
            "note.kimchi.byo_self_hosted",
            "A chave Kimchi pertence a uma instalação BYO/self-hosted sem saldo de inferência gerenciado; /v1/credits respondeu serverless=false.",
        ));
    }

    let remaining = credits
        .remaining
        .ok_or_else(|| QueryError::schema_at(&credits_body, &["remaining"], "number"))?;
    let mut metrics = vec![Metric {
        billing: Default::default(),
        scope: None,
        label: "Créditos restantes".into(),
        used: None,
        limit: None,
        remaining: Some(remaining),
        unit: "USD".into(),
        kind: MetricKind::Credits,
        window: None,
        period: None,
        resets_at: None,
    }];
    let mut notes = credits_notes(&credits);

    match fetch_budget(ctx, &key, &budget_url).await {
        Ok(Some(payload)) => match parse_budget(&payload) {
            Ok(budget_metrics) => metrics.extend(budget_metrics),
            Err(_) => notes.push(
                "note.kimchi.budget_contract_mismatch",
                "Kimchi retornou o saldo, mas a resposta de /v1/budget não correspondeu ao contrato oficial; os orçamentos foram omitidos.",
            ),
        },
        Ok(None) => notes.push(
            "note.kimchi.budget_404",
            "O proxy Kimchi respondeu 404 em /v1/budget (compatível com proxies antigos); somente o saldo de créditos foi exibido.",
        ),
        Err(error) => notes.push_params(
            "note.kimchi.budget_unavailable",
            params([("status", ParamValue::L10n(Box::new(crate::text::status_l10n(error.status))))]),
            format!(
                "Saldo de créditos atualizado; orçamento indisponível nesta atualização ({}).",
                error.status.label()
            ),
        ),
    }

    Ok(Data::new(
        metrics,
        notes,
        format!("{credits_url}; {budget_url}"),
    ))
}

#[derive(Debug)]
struct Credits {
    serverless: Option<bool>,
    tier: Option<String>,
    billing_status: Option<String>,
    has_credits: Option<bool>,
    remaining: Option<f64>,
}

fn parse_credits(value: &Value) -> Result<Credits, QueryError> {
    credits_fields(value).map_err(|miss| miss.into_error(value))
}

fn credits_fields(value: &Value) -> Result<Credits, Miss> {
    let body = value.as_object().ok_or_else(|| Miss::root("object"))?;
    let remaining = match body.get("remaining") {
        None | Some(Value::Null) => None,
        value => Some(finite(value).ok_or_else(|| Miss::new("remaining", "number"))?),
    };
    let serverless = optional_bool(body, "serverless")?;
    let tier = first_string(body, &["tier", "tier_name", "tierName"])?;
    let billing_status = first_string(body, &["billing_status", "billingStatus"])?;
    let has_credits = first_bool(body, &["has_credits", "hasCredits"])?;

    Ok(Credits {
        serverless,
        tier,
        billing_status,
        has_credits,
        remaining,
    })
}

fn credits_notes(credits: &Credits) -> Notes {
    let mut notes = Notes::new();
    notes.push(
        "note.kimchi.credits_source",
        "Saldo real lido por GET /v1/credits com Bearer KIMCHI_API_KEY; a consulta não gera tokens.",
    );
    if let Some(tier) = credits.tier.as_deref() {
        notes.push_params(
            "note.kimchi.tier",
            params([("tier", ParamValue::from(tier.to_string()))]),
            format!("Plano reportado pelo Kimchi: {tier}."),
        );
    }
    if let Some(status) = credits.billing_status.as_deref() {
        notes.push_params(
            "note.kimchi.billing_status",
            params([("status", ParamValue::from(status.to_string()))]),
            format!("Estado de faturamento reportado pelo Kimchi: {status}."),
        );
    }
    if credits.has_credits == Some(false) {
        notes.push(
            "note.kimchi.no_credits",
            "Kimchi reportou has_credits=false; novas inferências podem estar bloqueadas ou restritas.",
        );
    }
    notes
}

async fn fetch_budget(
    ctx: &Context<'_>,
    key: &str,
    url: &str,
) -> Result<Option<Value>, QueryError> {
    let response = secure_send(
        ctx.http
            .get(url)
            .bearer_auth(key.trim())
            .header("Accept", "application/json")
            .timeout(ctx.timeout),
    )
    .await?;
    let code = response.status();
    if code.as_u16() == 404 {
        return Ok(None);
    }
    if !code.is_success() {
        return Err(QueryError {
            diagnostic: None,
            status: match code.as_u16() {
                401 | 403 => Status::AuthenticationError,
                429 => Status::RateLimited,
                _ => Status::Error,
            },
            note: Localized::with_params(
                "query.kimchi.budget_http",
                params([("code", ParamValue::from(code.as_u16()))]),
                format!(
                    "Consulta de orçamento retornou HTTP {}. Verifique acesso e permissões.",
                    code.as_u16()
                ),
            ),
        });
    }
    Ok(Some(super::body_value(response).await?))
}

fn parse_budget(value: &Value) -> Result<Vec<Metric>, QueryError> {
    budget_fields(value).map_err(|miss| miss.into_error(value))
}

fn budget_fields(value: &Value) -> Result<Vec<Metric>, Miss> {
    let body = value.as_object().ok_or_else(|| Miss::root("object"))?;
    let period = body
        .get("period")
        .and_then(Value::as_object)
        .ok_or_else(|| Miss::new("period", "object"))?;
    let in_period = |miss: Miss| miss.under(&["period"]);
    let start = required_string(period, "startTime").map_err(in_period)?;
    let end = required_string(period, "endTime").map_err(in_period)?;
    let resets_at = DateTime::parse_from_rfc3339(end)
        .map_err(|_| in_period(Miss::new("endTime", "string")))?
        .with_timezone(&Utc);
    // Validar também o início evita rotular como período uma resposta truncada.
    DateTime::parse_from_rfc3339(start).map_err(|_| in_period(Miss::new("startTime", "string")))?;
    let period_label = format!("{start} — {end}");

    let budgets = match body.get("budgets") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(values)) => values,
        Some(_) => return Err(Miss::new("budgets", "array")),
    };

    let mut metrics = Vec::new();
    for value in budgets {
        budget_entry(value, &period_label, resets_at, &mut metrics)
            .map_err(|miss| miss.under(&["budgets", "[]"]))?;
    }
    Ok(metrics)
}

fn budget_entry(
    value: &Value,
    period_label: &str,
    resets_at: DateTime<Utc>,
    metrics: &mut Vec<Metric>,
) -> Result<(), Miss> {
    let budget = value.as_object().ok_or_else(|| Miss::root("object"))?;
    let scope = required_string(budget, "scope")?;
    let scope_label = scope_label(scope).ok_or_else(|| Miss::new("scope", "string"))?;
    // scopeId faz parte do contrato, mas não é exibido: pode identificar
    // usuário, equipe ou chave e não é necessário para a métrica.
    required_string(budget, "scopeId")?;
    let limit = usd_string(budget, "budgetLimitUsd")?;
    let used = usd_string(budget, "totalSpendUsd")?;
    require_nonnegative(limit, "budgetLimitUsd")?;
    require_nonnegative(used, "totalSpendUsd")?;
    push_budget_metric(
        metrics,
        format!("Orçamento — {scope_label}"),
        used,
        limit,
        period_label,
        resets_at,
    );

    let providers = match budget.get("providerBudgets") {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Array(values)) => values,
        Some(_) => return Err(Miss::new("providerBudgets", "array")),
    };
    for value in providers {
        provider_budget(value, period_label, resets_at, metrics)
            .map_err(|miss| miss.under(&["providerBudgets", "[]"]))?;
    }
    Ok(())
}

fn provider_budget(
    value: &Value,
    period_label: &str,
    resets_at: DateTime<Utc>,
    metrics: &mut Vec<Metric>,
) -> Result<(), Miss> {
    let provider = value.as_object().ok_or_else(|| Miss::root("object"))?;
    let name = required_string(provider, "provider")?;
    let usage = usd_string(provider, "usageUsd")?;
    require_nonnegative(usage, "usageUsd")?;
    let limit_type = provider
        .get("limitType")
        .and_then(Value::as_str)
        .unwrap_or("DISABLED");
    let capped = limit_type == "CAPPED" || limit_type.ends_with("_CAPPED");
    let uncapped = limit_type == "DISABLED"
        || limit_type.ends_with("_DISABLED")
        || limit_type == "UNLIMITED"
        || limit_type.ends_with("_UNLIMITED")
        || limit_type == "UNSPECIFIED"
        || limit_type.ends_with("_UNSPECIFIED");
    if !capped && !uncapped {
        return Err(Miss::new("limitType", "string"));
    }
    let limit = if capped {
        let value = usd_string(provider, "budgetLimitUsd")?;
        require_nonnegative(value, "budgetLimitUsd")?;
        value
    } else {
        0.0
    };
    push_budget_metric(
        metrics,
        format!("Orçamento do provedor — {name}"),
        usage,
        limit,
        period_label,
        resets_at,
    );
    Ok(())
}

fn push_budget_metric(
    metrics: &mut Vec<Metric>,
    label: String,
    used: f64,
    limit: f64,
    period: &str,
    resets_at: DateTime<Utc>,
) {
    // O cliente oficial interpreta budgetLimitUsd == 0 como ilimitado.
    let (limit, remaining) = if limit > 0.0 {
        (Some(limit), Some(limit - used))
    } else {
        (None, None)
    };
    metrics.push(Metric {
        billing: Default::default(),
        scope: None,
        label,
        used: Some(used),
        limit,
        remaining,
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: Some(MetricWindow::Other),
        period: Some(period.into()),
        resets_at: Some(resets_at),
    });
}

fn finite(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(number) => number
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite()),
        _ => None,
    }
}

fn optional_bool(body: &Map<String, Value>, key: &str) -> Result<Option<bool>, Miss> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(Miss::new(key, "boolean")),
    }
}

fn first_bool(body: &Map<String, Value>, keys: &[&str]) -> Result<Option<bool>, Miss> {
    for key in keys {
        if body.contains_key(*key) {
            return optional_bool(body, key);
        }
    }
    Ok(None)
}

fn first_string(body: &Map<String, Value>, keys: &[&str]) -> Result<Option<String>, Miss> {
    for key in keys {
        match body.get(*key) {
            None => continue,
            Some(Value::Null) => return Ok(None),
            Some(Value::String(value)) => return Ok(Some(value.clone())),
            Some(_) => return Err(Miss::new(key, "string")),
        }
    }
    Ok(None)
}

fn required_string<'a>(body: &'a Map<String, Value>, key: &str) -> Result<&'a str, Miss> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Miss::new(key, "string"))
}

fn usd_string(body: &Map<String, Value>, key: &str) -> Result<f64, Miss> {
    let raw = required_string(body, key)?;
    raw.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| Miss::new(key, "string"))
}

fn require_nonnegative(value: f64, key: &str) -> Result<(), Miss> {
    if value >= 0.0 {
        Ok(())
    } else {
        Err(Miss::new(key, "number"))
    }
}

fn scope_label(scope: &str) -> Option<&'static str> {
    match scope {
        "API_KEY" => Some("chave de API"),
        "USER" => Some("usuário"),
        "TEAM_PER_USER" => Some("usuário da equipe"),
        "TEAM_POOLED" => Some("equipe compartilhada"),
        "ORGANIZATION_SOFT" => Some("organização (aviso)"),
        "ORGANIZATION_HARD" => Some("organização (limite rígido)"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Provider};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct TestCredentials;
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_KIMCHI_KEY").then(|| "kimchi-secret".into())
        }
    }

    fn account() -> Account {
        Account {
            id: None,
            name: "Kimchi".into(),
            provider: Provider::Kimchi,
            credential_env: Some("TEST_KIMCHI_KEY".into()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn context(http: &reqwest::Client) -> Context<'_> {
        Context {
            http,
            credentials: &TestCredentials,
            timeout: Duration::from_secs(5),
        }
    }

    async fn mount_credits(server: &MockServer, body: Value) {
        Mock::given(method("GET"))
            .and(path(CREDITS_PATH))
            .and(header("authorization", "Bearer kimchi-secret"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn consulta_saldo_e_orcamentos_com_bearer_sem_geracao() {
        let server = MockServer::start().await;
        mount_credits(
            &server,
            serde_json::json!({
                "serverless": true,
                "tier": "coder",
                "is_paid_tier": true,
                "billing_status": "ok",
                "has_credits": true,
                "remaining": "18.40"
            }),
        )
        .await;
        Mock::given(method("GET"))
            .and(path(BUDGET_PATH))
            .and(header("authorization", "Bearer kimchi-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "period": {
                    "startTime": "2026-09-01T00:00:00Z",
                    "endTime": "2026-10-01T00:00:00Z"
                },
                "budgets": [{
                    "scope": "USER",
                    "scopeId": "user-1",
                    "budgetLimitUsd": "25.000000",
                    "totalSpendUsd": "6.600000",
                    "providerBudgets": [{
                        "provider": "anthropic",
                        "limitType": "PROVIDER_BUDGET_LIMIT_TYPE_CAPPED",
                        "budgetLimitUsd": "10.000000",
                        "usageUsd": "4.000000"
                    }]
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = kimchi_at(&account(), &context(&client), &server.uri())
            .await
            .unwrap();

        assert_eq!(data.metrics.len(), 3);
        assert_eq!(data.metrics[0].label, "Créditos restantes");
        assert_eq!(data.metrics[0].remaining, Some(18.4));
        assert_eq!(data.metrics[0].kind, MetricKind::Credits);
        assert_eq!(data.metrics[0].window, None);
        assert_eq!(data.metrics[1].used, Some(6.6));
        assert_eq!(data.metrics[1].limit, Some(25.0));
        assert_eq!(data.metrics[1].remaining, Some(18.4));
        assert_eq!(data.metrics[2].used, Some(4.0));
        assert_eq!(data.metrics[2].remaining, Some(6.0));
        for metric in &data.metrics[1..] {
            assert_eq!(metric.kind, MetricKind::Spend);
            assert_eq!(metric.window, Some(MetricWindow::Other));
        }
        assert_eq!(
            data.metrics[2].resets_at,
            DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                .ok()
                .map(|date| date.with_timezone(&Utc))
        );
    }

    #[tokio::test]
    async fn aceita_remaining_numerico_e_budget_404_de_proxy_antigo() {
        let server = MockServer::start().await;
        mount_credits(
            &server,
            serde_json::json!({
                "serverless": true,
                "tierName": "teams",
                "billingStatus": "low_balance",
                "hasCredits": true,
                "remaining": 4.99
            }),
        )
        .await;
        Mock::given(method("GET"))
            .and(path(BUDGET_PATH))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = kimchi_at(&account(), &context(&client), &server.uri())
            .await
            .unwrap();

        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].remaining, Some(4.99));
        assert!(data.notes.iter().any(|note| note.contains("404")));
        assert!(data.notes.iter().any(|note| note.contains("low_balance")));
    }

    #[tokio::test]
    async fn serverless_false_e_byo_sem_saldo_gerenciado() {
        let server = MockServer::start().await;
        mount_credits(&server, serde_json::json!({ "serverless": false })).await;

        let client = reqwest::Client::new();
        let error = kimchi_at(&account(), &context(&client), &server.uri())
            .await
            .unwrap_err();

        assert_eq!(error.status, Status::Unsupported);
        assert_eq!(error.note.id, "note.kimchi.byo_self_hosted");
    }

    #[tokio::test]
    async fn saldo_malformado_nao_vira_zero() {
        let server = MockServer::start().await;
        mount_credits(
            &server,
            serde_json::json!({ "serverless": true, "remaining": "not-a-number" }),
        )
        .await;

        let client = reqwest::Client::new();
        let error = kimchi_at(&account(), &context(&client), &server.uri())
            .await
            .unwrap_err();

        assert_eq!(error.status, Status::Error);
        assert_eq!(error.note.id, "query.schema");
    }

    #[tokio::test]
    async fn falha_de_autenticacao_em_credits_e_reportada() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(CREDITS_PATH))
            .and(header("authorization", "Bearer kimchi-secret"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let error = kimchi_at(&account(), &context(&client), &server.uri())
            .await
            .unwrap_err();

        assert_eq!(error.status, Status::AuthenticationError);
    }

    #[tokio::test]
    async fn budget_incompativel_nao_apaga_saldo_valido() {
        let server = MockServer::start().await;
        mount_credits(
            &server,
            serde_json::json!({ "serverless": true, "remaining": "7.50" }),
        )
        .await;
        Mock::given(method("GET"))
            .and(path(BUDGET_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "period": { "startTime": "invalid", "endTime": "invalid" },
                "budgets": []
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = kimchi_at(&account(), &context(&client), &server.uri())
            .await
            .unwrap();

        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].remaining, Some(7.5));
        assert!(data
            .notes
            .iter()
            .any(|note| note.contains("não correspondeu")));
    }

    #[test]
    fn a_broken_nested_budget_field_names_its_path_without_its_value() {
        let error = parse_budget(&serde_json::json!({
            "period": {
                "startTime": "2026-09-01T00:00:00Z",
                "endTime": "2026-10-01T00:00:00Z"
            },
            "budgets": [{
                "scope": "USER",
                "scopeId": "SECRET-ID",
                "budgetLimitUsd": 7,
                "totalSpendUsd": "1.0"
            }]
        }))
        .unwrap_err();
        let drift = error.diagnostic.as_deref().expect("drift").drift().clone();
        assert_eq!(drift.paths[0].path, "budgets[].budgetLimitUsd");
        assert_eq!(
            (
                drift.paths[0].expected.as_str(),
                drift.paths[0].found.as_str()
            ),
            ("string", "number")
        );
        assert!(!format!("{drift:?}").contains("SECRET-ID"));
    }

    #[test]
    fn limite_zero_e_ilimitado_e_excesso_nao_e_truncado() {
        let metrics = parse_budget(&serde_json::json!({
            "period": {
                "startTime": "2026-09-01T00:00:00Z",
                "endTime": "2026-10-01T00:00:00Z"
            },
            "budgets": [
                {
                    "scope": "TEAM_POOLED",
                    "scopeId": "team-1",
                    "budgetLimitUsd": "0.000000",
                    "totalSpendUsd": "12.000000"
                },
                {
                    "scope": "ORGANIZATION_HARD",
                    "scopeId": "org-1",
                    "budgetLimitUsd": "10.000000",
                    "totalSpendUsd": "12.000000"
                }
            ]
        }))
        .unwrap();

        assert_eq!(metrics[0].limit, None);
        assert_eq!(metrics[0].remaining, None);
        assert_eq!(metrics[1].remaining, Some(-2.0));
    }
}
