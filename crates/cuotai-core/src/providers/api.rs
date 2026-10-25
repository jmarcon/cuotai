//! Consultas de uso e custo via APIs oficiais de organização (somente leitura).
//!
//! OpenAI (Admin API, chave de administração `OPENAI_ADMIN_KEY`):
//!   - GET https://api.openai.com/v1/organization/costs
//!   - GET https://api.openai.com/v1/organization/usage/completions
//!
//!   Referência: https://developers.openai.com/api/reference/resources/admin/subresources/organization
//!
//! Anthropic (Usage & Cost Admin API, chave `sk-ant-admin01-...` em `ANTHROPIC_ADMIN_KEY`):
//!   - GET https://api.anthropic.com/v1/organizations/cost_report
//!   - GET https://api.anthropic.com/v1/organizations/usage_report/messages
//!
//!   Referência: https://platform.claude.com/docs/en/manage-claude/usage-cost-api
//!
//! Nenhum dos dois provedores oferece endpoint oficial de saldo ou crédito
//! restante; `limit` e `remaining` ficam sempre desconhecidos (None).
//!
//! Parâmetros opcionais em `Account.params` (listas separadas por vírgula):
//! - OpenAI: `project_ids` (custos e uso), `models` (uso)
//! - Anthropic: `workspace_ids` (custos e uso), `models` (uso)

use super::{json, secret, Context, Data, QueryError};
use crate::text::{params, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider};
use chrono::{DateTime, Datelike, Months, Utc};
use reqwest::RequestBuilder;
use serde::Deserialize;
use std::collections::BTreeMap;

const OPENAI_BASE: &str = "https://api.openai.com";
const ANTHROPIC_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Diário com 31 buckets por página: o mês corrente nunca precisa de 2 páginas.
const MAX_PAGES: usize = 8;
const PAGE_LIMIT: &str = "31";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::OpenAiApi => openai(account, ctx, OPENAI_BASE).await,
        Provider::AnthropicApi => anthropic(account, ctx, ANTHROPIC_BASE).await,
        other => Err(QueryError::unsupported_params(
            "query.api.other_provider",
            params([(
                "provider",
                ParamValue::from(crate::text::provider_l10n(other)),
            )]),
            "{provider} is handled by another adapter.",
        )),
    }
}

/// Início do mês corrente (UTC) e o instante de reinício (próximo mês).
fn month_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = now
        .date_naive()
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
        .expect("o primeiro dia de qualquer mês é válido");
    (start, start + Months::new(1))
}

/// Lista opcional separada por vírgula em `Account.params`.
fn csv(account: &Account, key: &str) -> Vec<String> {
    account
        .params
        .get(key)
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Envelope de paginação idêntico nos dois provedores.
#[derive(Deserialize)]
struct Page<B> {
    data: Vec<B>,
    has_more: bool,
    next_page: Option<String>,
}

/// Consome todas as páginas via cursor; jamais devolve total parcial.
async fn buckets<B: serde::de::DeserializeOwned>(
    build: impl Fn(Option<String>) -> RequestBuilder,
) -> Result<Vec<B>, QueryError> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let page: Page<B> = json(build(cursor.clone())).await?;
        out.extend(page.data);
        match page.next_page {
            Some(next) if page.has_more => {
                if next.trim().is_empty() || cursor.as_deref() == Some(next.as_str()) {
                    return Err(QueryError::schema());
                }
                cursor = Some(next);
            }
            _ if page.has_more => return Err(QueryError::schema()),
            _ => return Ok(out),
        }
    }
    Err(QueryError::schema())
}

fn finite(value: f64) -> Result<f64, QueryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(QueryError::schema())
    }
}

// ---------------------------------------------------------------- OpenAI

#[derive(Deserialize)]
struct OpenAiCostBucket {
    /// Unix seconds, stated by the Costs API for every bucket.
    #[serde(default)]
    start_time: Option<i64>,
    #[serde(default)]
    end_time: Option<i64>,
    results: Vec<OpenAiCostResult>,
}

/// PRD 0033 RF3.1: the data time of the Costs API is the end of the last
/// complete daily bucket (`end_time`, a full `bucket_width` after its
/// `start_time` and not after `now`); the current day's bucket is clipped at
/// the query time and is not complete.
fn openai_costs_as_of(buckets: &[OpenAiCostBucket], now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    buckets
        .iter()
        .filter_map(|b| Some((b.start_time?, b.end_time?)))
        .filter(|(start, end)| end - start == 86_400 && *end <= now.timestamp())
        .map(|(_, end)| end)
        .max()
        .and_then(|end| DateTime::from_timestamp(end, 0))
}

#[derive(Deserialize)]
struct OpenAiCostResult {
    // Opcional no esquema, mas obrigatório aqui: ausência não pode virar zero.
    amount: OpenAiAmount,
}

#[derive(Deserialize)]
struct OpenAiAmount {
    value: f64,
    currency: String,
}

#[derive(Deserialize)]
struct OpenAiUsageBucket {
    results: Vec<OpenAiUsageResult>,
}

#[derive(Deserialize)]
struct OpenAiUsageResult {
    input_tokens: u64,
    output_tokens: u64,
    num_model_requests: u64,
}

async fn openai(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "OPENAI_ADMIN_KEY")?;
    let now = Utc::now();
    let (start, reset) = month_bounds(now);
    let period = start.format("%Y-%m").to_string();
    let project_ids = csv(account, "project_ids");
    let models = csv(account, "models");

    let mut base_query = vec![
        ("start_time".to_string(), start.timestamp().to_string()),
        ("end_time".to_string(), now.timestamp().to_string()),
        ("bucket_width".to_string(), "1d".to_string()),
        ("limit".to_string(), PAGE_LIMIT.to_string()),
    ];
    for id in &project_ids {
        base_query.push(("project_ids".to_string(), id.clone()));
    }

    let cost_query = base_query.clone();
    let cost_buckets: Vec<OpenAiCostBucket> = buckets(|cursor| {
        let mut q = cost_query.clone();
        if let Some(c) = cursor {
            q.push(("page".to_string(), c));
        }
        ctx.http
            .get(format!("{base}/v1/organization/costs"))
            .bearer_auth(&key)
            .query(&q)
            .timeout(ctx.timeout)
    })
    .await?;

    let as_of = openai_costs_as_of(&cost_buckets, now);
    let mut costs: BTreeMap<String, f64> = BTreeMap::new();
    for bucket in cost_buckets {
        for result in bucket.results {
            let value = finite(result.amount.value)?;
            let currency = result.amount.currency.trim();
            if currency.is_empty() {
                return Err(QueryError::schema());
            }
            let entry = costs.entry(currency.to_uppercase()).or_insert(0.0);
            *entry = finite(*entry + value)?;
        }
    }

    let mut usage_query = base_query;
    for model in &models {
        usage_query.push(("models".to_string(), model.clone()));
    }
    let usage_buckets: Vec<OpenAiUsageBucket> = buckets(|cursor| {
        let mut q = usage_query.clone();
        if let Some(c) = cursor {
            q.push(("page".to_string(), c));
        }
        ctx.http
            .get(format!("{base}/v1/organization/usage/completions"))
            .bearer_auth(&key)
            .query(&q)
            .timeout(ctx.timeout)
    })
    .await?;

    let mut input = 0u64;
    let mut output = 0u64;
    let mut requests = 0u64;
    for bucket in usage_buckets {
        for result in bucket.results {
            input = input
                .checked_add(result.input_tokens)
                .ok_or_else(QueryError::schema)?;
            output = output
                .checked_add(result.output_tokens)
                .ok_or_else(QueryError::schema)?;
            requests = requests
                .checked_add(result.num_model_requests)
                .ok_or_else(QueryError::schema)?;
        }
    }

    let mut metrics = Vec::new();
    let mut notes = Notes::new();
    notes.push(
        "note.openai_api.admin_key_required",
        "Requires an organization ADMINISTRATION key (Admin API); regular project keys do not have access.",
    );
    notes.push(
        "note.openai_api.monthly_costs_and_completion_tokens",
        "Costs are month-to-date values in UTC; tokens only cover completions (embeddings, images and audio are not included).",
    );
    notes.push(
        "note.openai_api.no_quota_endpoint",
        "No public balance/credit endpoint: limit and remaining remain unknown.",
    );
    if costs.is_empty() {
        notes.push(
            "note.openai_api.no_current_month_costs",
            "No costs recorded in the current month.",
        );
    }
    if !models.is_empty() {
        notes.push(
            "note.openai_api.model_filter_tokens_only",
            "The model filter applies only to tokens; costs continue to cover all models.",
        );
    }
    for (currency, total) in costs {
        let mut m = Metric::usage(
            format!("Custo no mês corrente ({currency})"),
            total,
            currency,
            MetricKind::Spend,
            Some(MetricWindow::Monthly),
        );
        m.period = Some(period.clone());
        m.resets_at = Some(reset);
        metrics.push(m);
    }
    for (label, value, unit) in [
        ("Tokens de entrada (completions)", input as f64, "tokens"),
        ("Tokens de saída (completions)", output as f64, "tokens"),
        (
            "Requisições de modelo (completions)",
            requests as f64,
            "requisições",
        ),
    ] {
        let mut m = Metric::usage(
            label,
            value,
            unit,
            MetricKind::Usage,
            Some(MetricWindow::Monthly),
        );
        m.period = Some(period.clone());
        m.resets_at = Some(reset);
        metrics.push(m);
    }

    Ok(Data::new(
        metrics,
        notes,
        "https://developers.openai.com/api/reference/resources/admin/subresources/organization",
    )
    .with_as_of(as_of))
}

// ------------------------------------------------------------- Anthropic

#[derive(Deserialize)]
struct AnthropicCostBucket {
    results: Vec<AnthropicCostResult>,
}

#[derive(Deserialize)]
struct AnthropicCostResult {
    /// String decimal na menor unidade da moeda (centavos para USD).
    amount: String,
    currency: String,
}

#[derive(Deserialize)]
struct AnthropicUsageBucket {
    results: Vec<AnthropicUsageResult>,
}

#[derive(Deserialize)]
struct AnthropicUsageResult {
    uncached_input_tokens: u64,
    cache_creation: AnthropicCacheCreation,
    cache_read_input_tokens: u64,
    output_tokens: u64,
}

#[derive(Deserialize)]
struct AnthropicCacheCreation {
    ephemeral_1h_input_tokens: u64,
    ephemeral_5m_input_tokens: u64,
}

async fn anthropic(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "ANTHROPIC_ADMIN_KEY")?;
    let now = Utc::now();
    let (start, reset) = month_bounds(now);
    let period = start.format("%Y-%m").to_string();
    let workspace_ids = csv(account, "workspace_ids");
    let models = csv(account, "models");

    let mut base_query = vec![
        ("starting_at".to_string(), start.to_rfc3339()),
        ("ending_at".to_string(), now.to_rfc3339()),
        ("bucket_width".to_string(), "1d".to_string()),
        ("limit".to_string(), PAGE_LIMIT.to_string()),
    ];
    for id in &workspace_ids {
        base_query.push(("workspace_ids[]".to_string(), id.clone()));
    }

    let cost_query = base_query.clone();
    let cost_buckets: Vec<AnthropicCostBucket> = buckets(|cursor| {
        let mut q = cost_query.clone();
        if let Some(c) = cursor {
            q.push(("page".to_string(), c));
        }
        ctx.http
            .get(format!("{base}/v1/organizations/cost_report"))
            .header("x-api-key", &key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .query(&q)
            .timeout(ctx.timeout)
    })
    .await?;

    let mut costs = 0.0f64;
    let mut saw_cost = false;
    for bucket in cost_buckets {
        for result in bucket.results {
            // Centavos documentados apenas para USD ("currently always USD").
            if !result.currency.trim().eq_ignore_ascii_case("usd") {
                return Err(QueryError::schema());
            }
            let cents: f64 = result.amount.parse().map_err(|_| QueryError::schema())?;
            costs = finite(costs + finite(cents)? / 100.0)?;
            saw_cost = true;
        }
    }

    let mut usage_query = base_query;
    for model in &models {
        usage_query.push(("models[]".to_string(), model.clone()));
    }
    let usage_buckets: Vec<AnthropicUsageBucket> = buckets(|cursor| {
        let mut q = usage_query.clone();
        if let Some(c) = cursor {
            q.push(("page".to_string(), c));
        }
        ctx.http
            .get(format!("{base}/v1/organizations/usage_report/messages"))
            .header("x-api-key", &key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .query(&q)
            .timeout(ctx.timeout)
    })
    .await?;

    let mut input = 0u64;
    let mut output = 0u64;
    for bucket in usage_buckets {
        for result in bucket.results {
            let total_in = result
                .uncached_input_tokens
                .checked_add(result.cache_read_input_tokens)
                .and_then(|v| v.checked_add(result.cache_creation.ephemeral_1h_input_tokens))
                .and_then(|v| v.checked_add(result.cache_creation.ephemeral_5m_input_tokens))
                .ok_or_else(QueryError::schema)?;
            input = input.checked_add(total_in).ok_or_else(QueryError::schema)?;
            output = output
                .checked_add(result.output_tokens)
                .ok_or_else(QueryError::schema)?;
        }
    }

    let mut metrics = Vec::new();
    let mut notes = Notes::new();
    notes.push(
        "note.anthropic_api.admin_key_required",
        "Requires Admin API key (sk-ant-admin01-...) or unscoped key; workspace keys do not work.",
    );
    notes.push(
        "note.anthropic_api.cost_conversion",
        "Costs come in USD cents (minor unit) and are converted to dollars; Priority Tier is not in the cost_report.",
    );
    notes.push(
        "note.anthropic_api.message_tokens",
        "Tokens cover messages (input includes read and write cache).",
    );
    notes.push(
        "note.anthropic_api.no_quota_endpoint",
        "No public balance/credit endpoint: limit and remaining remain unknown.",
    );
    if !saw_cost {
        notes.push(
            "note.anthropic_api.no_current_month_costs",
            "No costs recorded in the current month.",
        );
    }
    if !models.is_empty() {
        notes.push(
            "note.anthropic_api.model_filter_tokens_only",
            "The model filter applies only to tokens; costs continue to cover all models.",
        );
    }
    if saw_cost {
        let mut m = Metric::usage(
            "Custo no mês corrente (USD)",
            costs,
            "USD",
            MetricKind::Spend,
            Some(MetricWindow::Monthly),
        );
        m.period = Some(period.clone());
        m.resets_at = Some(reset);
        metrics.push(m);
    }
    for (label, value) in [
        ("Tokens de entrada (mensagens)", input as f64),
        ("Tokens de saída (mensagens)", output as f64),
    ] {
        let mut m = Metric::usage(
            label,
            value,
            "tokens",
            MetricKind::Usage,
            Some(MetricWindow::Monthly),
        );
        m.period = Some(period.clone());
        m.resets_at = Some(reset);
        metrics.push(m);
    }

    Ok(Data::new(
        metrics,
        notes,
        "https://platform.claude.com/docs/en/manage-claude/usage-cost-api",
    ))
}

// ------------------------------------------------------------------ testes

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Status};
    use std::time::Duration;
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct TestCredentials;
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_ADMIN_KEY").then(|| "test-secret".to_string())
        }
    }

    fn account(provider: Provider) -> Account {
        Account {
            id: None,
            name: "conta".to_string(),
            provider,
            credential_env: Some("TEST_ADMIN_KEY".to_string()),
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

    fn metric<'a>(data: &'a Data, label: &str) -> &'a Metric {
        data.metrics
            .iter()
            .find(|m| m.label == label)
            .expect("métrica esperada ausente")
    }

    // ----------------------------------------------------------- OpenAI

    fn openai_cost_page(
        results: serde_json::Value,
        has_more: bool,
        next_page: Option<&str>,
    ) -> serde_json::Value {
        serde_json::json!({
            "object": "page",
            "data": [{ "object": "bucket", "start_time": 1, "end_time": 2, "results": results }],
            "has_more": has_more,
            "next_page": next_page,
        })
    }

    fn openai_usage_page(input: u64, output: u64, requests: u64) -> serde_json::Value {
        serde_json::json!({
            "object": "page",
            "data": [{
                "object": "bucket", "start_time": 1, "end_time": 2,
                "results": [{
                    "object": "organization.usage.completions.result",
                    "input_tokens": input, "output_tokens": output,
                    "num_model_requests": requests,
                }],
            }],
            "has_more": false,
            "next_page": null,
        })
    }

    fn bucket(start: Option<i64>, end: Option<i64>) -> OpenAiCostBucket {
        OpenAiCostBucket {
            start_time: start,
            end_time: end,
            results: Vec::new(),
        }
    }

    #[test]
    fn openai_as_of_is_the_end_of_the_last_complete_daily_bucket() {
        let day = 86_400;
        let now = DateTime::from_timestamp(10 * day + 3_600, 0).unwrap();
        let buckets = [
            bucket(Some(8 * day), Some(9 * day)),
            bucket(Some(9 * day), Some(10 * day)),
            // The current day is clipped at the query time: not complete.
            bucket(Some(10 * day), Some(10 * day + 3_600)),
        ];
        assert_eq!(
            openai_costs_as_of(&buckets, now),
            DateTime::from_timestamp(10 * day, 0)
        );
        // Without stated times, nothing is claimed.
        assert_eq!(openai_costs_as_of(&[bucket(None, None)], now), None);
        assert_eq!(openai_costs_as_of(&[], now), None);
    }

    #[tokio::test]
    async fn openai_soma_paginas_e_moedas() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(query_param_is_missing("page"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_cost_page(
                serde_json::json!([
                    { "object": "organization.costs.result",
                      "amount": { "value": 0.06, "currency": "usd" } },
                    { "object": "organization.costs.result",
                      "amount": { "value": 0.02, "currency": "usd" } }
                ]),
                true,
                Some("cursor-2"),
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(query_param("page", "cursor-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_cost_page(
                serde_json::json!([
                    { "object": "organization.costs.result",
                      "amount": { "value": 0.04, "currency": "usd" } }
                ]),
                false,
                None,
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/usage/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_usage_page(1500, 500, 7)))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = openai(
            &account(Provider::OpenAiApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap();

        let custo = metric(&data, "Custo no mês corrente (USD)");
        assert_eq!(custo.used, Some(0.12));
        assert!(custo.limit.is_none() && custo.remaining.is_none());
        assert!(custo.period.is_some() && custo.resets_at.is_some());
        assert_eq!(custo.kind, MetricKind::Spend);
        assert_eq!(custo.window, Some(MetricWindow::Monthly));
        assert_eq!(
            metric(&data, "Tokens de entrada (completions)").used,
            Some(1500.0)
        );
        assert_eq!(
            metric(&data, "Tokens de saída (completions)").used,
            Some(500.0)
        );
        assert_eq!(
            metric(&data, "Requisições de modelo (completions)").used,
            Some(7.0)
        );
        for label in [
            "Tokens de entrada (completions)",
            "Tokens de saída (completions)",
            "Requisições de modelo (completions)",
        ] {
            assert_eq!(metric(&data, label).kind, MetricKind::Usage);
            assert_eq!(metric(&data, label).window, Some(MetricWindow::Monthly));
        }
    }

    #[tokio::test]
    async fn openai_has_more_sem_cursor_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_cost_page(
                serde_json::json!([
                    { "object": "organization.costs.result",
                      "amount": { "value": 0.06, "currency": "usd" } }
                ]),
                true,
                None,
            )))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = openai(
            &account(Provider::OpenAiApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn openai_amount_ausente_e_erro_de_esquema() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_cost_page(
                serde_json::json!([
                    { "object": "organization.costs.result", "line_item": "x" }
                ]),
                false,
                None,
            )))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = openai(
            &account(Provider::OpenAiApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn openai_403_e_erro_de_autenticacao() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = openai(
            &account(Provider::OpenAiApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::AuthenticationError);
    }

    #[tokio::test]
    async fn openai_valor_nao_finito_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"object":"page","data":[{"object":"bucket","start_time":1,"end_time":2,"results":[{"object":"organization.costs.result","amount":{"value":1e400,"currency":"usd"}}]}],"has_more":false,"next_page":null}"#,
            ))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = openai(
            &account(Provider::OpenAiApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn credencial_ausente_requer_configuracao() {
        let server = MockServer::start().await;
        let mut acc = account(Provider::OpenAiApi);
        acc.credential_env = Some("INEXISTENTE".to_string());
        let client = reqwest::Client::new();
        let err = openai(&acc, &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("INEXISTENTE"));
    }

    // --------------------------------------------------------- Anthropic

    fn anthropic_cost_page(
        amounts: &[&str],
        has_more: bool,
        next_page: Option<&str>,
    ) -> serde_json::Value {
        let results: Vec<_> = amounts
            .iter()
            .map(|a| serde_json::json!({ "amount": a, "currency": "USD" }))
            .collect();
        serde_json::json!({
            "data": [{ "starting_at": "2026-09-01T00:00:00Z", "ending_at": "2026-09-02T00:00:00Z",
                       "results": results }],
            "has_more": has_more,
            "next_page": next_page,
        })
    }

    #[tokio::test]
    async fn anthropic_converte_centavos_e_pagina() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .and(header("x-api-key", "test-secret"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(query_param_is_missing("page"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(anthropic_cost_page(
                    &["123.45", "0.55"],
                    true,
                    Some("page_MjA"),
                )),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .and(query_param("page", "page_MjA"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(anthropic_cost_page(
                    &["100"],
                    false,
                    None,
                )),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/usage_report/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "starting_at": "2026-09-01T00:00:00Z", "ending_at": "2026-09-02T00:00:00Z",
                    "results": [{
                        "uncached_input_tokens": 1500,
                        "cache_creation": { "ephemeral_1h_input_tokens": 10, "ephemeral_5m_input_tokens": 90 },
                        "cache_read_input_tokens": 200,
                        "output_tokens": 500,
                        "server_tool_use": { "web_search_requests": 2 }
                    }]
                }],
                "has_more": false,
                "next_page": null,
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = anthropic(
            &account(Provider::AnthropicApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap();

        let custo = metric(&data, "Custo no mês corrente (USD)");
        let total = custo.used.unwrap();
        assert!(
            (total - 2.24).abs() < 1e-9,
            "224 centavos devem virar 2.24 USD, obtido {total}"
        );
        assert_eq!(custo.kind, MetricKind::Spend);
        assert_eq!(custo.window, Some(MetricWindow::Monthly));
        // 1500 + 200 + 10 + 90 = 1800 tokens de entrada
        assert_eq!(
            metric(&data, "Tokens de entrada (mensagens)").used,
            Some(1800.0)
        );
        assert_eq!(
            metric(&data, "Tokens de saída (mensagens)").used,
            Some(500.0)
        );
        for label in [
            "Tokens de entrada (mensagens)",
            "Tokens de saída (mensagens)",
        ] {
            assert_eq!(metric(&data, label).kind, MetricKind::Usage);
            assert_eq!(metric(&data, label).window, Some(MetricWindow::Monthly));
        }
    }

    #[tokio::test]
    async fn anthropic_amount_invalido_e_erro_de_esquema() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(anthropic_cost_page(
                    &["abc"],
                    false,
                    None,
                )),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = anthropic(
            &account(Provider::AnthropicApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn anthropic_cursor_repetido_interrompe_laco() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(anthropic_cost_page(
                    &["1"],
                    true,
                    Some("mesmo"),
                )),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = anthropic(
            &account(Provider::AnthropicApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn anthropic_cursor_em_branco_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(anthropic_cost_page(
                    &["1"],
                    true,
                    Some("   "),
                )),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = anthropic(
            &account(Provider::AnthropicApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn anthropic_moeda_nao_usd_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "starting_at": "2026-09-01T00:00:00Z", "ending_at": "2026-09-02T00:00:00Z",
                           "results": [{ "amount": "100", "currency": "EUR" }] }],
                "has_more": false,
                "next_page": null,
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = anthropic(
            &account(Provider::AnthropicApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn openai_moeda_vazia_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_cost_page(
                serde_json::json!([
                    { "object": "organization.costs.result",
                      "amount": { "value": 0.06, "currency": "  " } }
                ]),
                false,
                None,
            )))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = openai(
            &account(Provider::OpenAiApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn openai_overflow_de_tokens_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_cost_page(
                serde_json::json!([]),
                false,
                None,
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/usage/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "page",
                "data": [
                    { "object": "bucket", "start_time": 1, "end_time": 2,
                      "results": [{ "object": "organization.usage.completions.result",
                                    "input_tokens": 18446744073709551615u64,
                                    "output_tokens": 0, "num_model_requests": 0 }] },
                    { "object": "bucket", "start_time": 2, "end_time": 3,
                      "results": [{ "object": "organization.usage.completions.result",
                                    "input_tokens": 1,
                                    "output_tokens": 0, "num_model_requests": 0 }] }
                ],
                "has_more": false,
                "next_page": null,
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = openai(
            &account(Provider::OpenAiApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }
}
