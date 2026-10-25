//! Adaptador para provedores de inferência de terceiros (somente leitura).
//!
//! Política deste adaptador:
//!
//! 1. Os nomes de credencial expostos no wizard são exatamente
//!    `GROQ_API_KEY`, `PERPLEXITY_API_KEY`, `MISTRAL_API_KEY`,
//!    `X_GROK_API_KEY`, `NVIDIA_NIM_API_KEY`, `POOLSIDE_API_KEY`. O
//!    usuário pode sobrescrever a env via `Account.credential_env`.
//! 2. Quando o endpoint oficial exige uma classe de credencial diferente
//!    da geração (admin, analytics, management), o usuário indica a env
//!    alternativa em `params.admin_key_env`,
//!    `params.analytics_key_env` ou `params.management_key_env` — esses
//!    params armazenam apenas o **nome** da env; o valor continua sendo
//!    lido via `Context.credentials`.
//! 3. Precedência de credencial: `params.<env_ref>` configurado →
//!    `credential_env`/`default`. Nunca lemos duas credenciais para o
//!    mesmo provedor.
//! 4. Quando o endpoint exige classe alternativa e nenhuma env aponta
//!    para uma chave da classe correta, retornamos `NeedsConfiguration`
//!    com a instrução de usar `params.<env_ref>`.
//! 5. Provedores sem endpoint público de saldo/uso por chave retornam
//!    `Unsupported` com a URL da fonte primária. Nada de probing.
//!
//! Fontes oficiais consultadas:
//!
//! - Mistral (admin): endpoints `beta.admin.billing` exigem Admin API Key.
//!   `GET /v1/admin/spend-limit` retorna uso/limite/moeda do ciclo
//!   corrente.
//!   Ref.: <https://docs.mistral.ai/api/endpoint/beta/admin/billing>
//!   OpenAPI: <https://docs.mistral.ai/openapi.yaml>
//!
//! - Perplexity: a API principal (Sonar/Ask) não tem endpoint público de
//!   saldo/uso por chave. A Analytics API
//!   (`GET /v1/analytics/computer/usage`) cobre o produto Perplexity
//!   Computer dentro de organizações Enterprise; é opt-in via
//!   `params.product=computer` para não silenciosamente trocar de
//!   produto.
//!   Ref.: <https://docs.perplexity.ai/docs/admin/computer-analytics-api.md>
//!   Ref.: <https://docs.perplexity.ai/api-reference/computer-analytics-usage-get.md>
//!
//! - Grok (xAI): Management API em `https://management-api.x.ai` (host
//!   distinto de `https://api.x.ai`, inferência, e de `https://api.x.com`,
//!   API social X). Endpoints documentados: `GET /prepaid/balance` para
//!   saldo de créditos pré-pagos e `GET /postpaid/invoice/preview` para
//!   o previsto no ciclo corrente. O `effectiveSpendingLimit` é o teto
//!   mensal configurado pelo usuário (soft spending limit pós-pago), não
//!   uma cota da plataforma.
//!   Ref.: <https://docs.x.ai/developers/management-api-guide.md>
//!   Ref.: <https://docs.x.ai/developers/rest-api-reference/management/billing.md>
//!
//! - NvidiaNim: `https://integrate.api.nvidia.com` é só geração; cobrança
//!   é pela conta NVIDIA Cloud.
//!   Ref.: <https://docs.api.nvidia.com/nim/reference/llm-apis>
//!
//! - Poolside: API hospedada (`https://inference.poolside.ai/v1`) expõe
//!   somente `/v1/models` e `/v1/chat/completions`; implantações
//!   auto-gerenciadas publicam métricas Prometheus, não saldo por chave.
//!   Ref.: <https://docs.poolside.ai/api/overview.md>

use super::{json, Context, Data, QueryError};
use crate::text::{params, provider_l10n, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider};
use chrono::{DateTime, Datelike, Months, Utc};
use reqwest::RequestBuilder;
use serde::Deserialize;

/// Limite padrão de iteração para paginação cursor-based.
const MAX_PAGES: usize = 8;

const MISTRAL_BASE: &str = "https://api.mistral.ai";
const PERPLEXITY_BASE: &str = "https://api.perplexity.ai";
const XAI_MANAGEMENT_BASE: &str = "https://management-api.x.ai";
/// OpenAI-shaped model list for the inference key (`docs.poolside.ai/api/overview.md`).
pub(crate) const POOLSIDE_MODELS_URL: &str = "https://inference.poolside.ai/v1/models";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::Perplexity => perplexity(account, ctx, PERPLEXITY_BASE).await,
        Provider::Mistral => mistral(account, ctx, MISTRAL_BASE).await,
        Provider::Grok => grok(account, ctx, XAI_MANAGEMENT_BASE).await,
        Provider::NvidiaNim => Err(QueryError::unsupported_params(
            "query.inference.nvidia_nim_no_public_balance",
            params([(
                "source",
                ParamValue::from("https://docs.api.nvidia.com/nim/reference/llm-apis"),
            )]),
            "NVIDIA NIM não expõe endpoint público de saldo por chave de \
             inferência: a cobrança é controlada pela conta NVIDIA Cloud, \
             fora do escopo desta chave. Fonte: {source}.",
        )),
        Provider::Poolside => Err(QueryError::unsupported_params(
            "query.inference.poolside_no_public_usage",
            params([(
                "source",
                ParamValue::from("https://docs.poolside.ai/api/overview.md"),
            )]),
            "Poolside não publica endpoint de uso/saldo por chave: a API \
             hospedada expõe apenas /v1/models e /v1/chat/completions. \
             Fonte: {source}.",
        )),
        other => Err(QueryError::unsupported_params(
            "query.inference.provider_handled_elsewhere",
            params([("provider", ParamValue::from(provider_l10n(other)))]),
            format!("{} é consultado por outro adaptador.", other.label()),
        )),
    }
}

// =================================================================== helpers

fn finite(value: f64) -> Result<f64, QueryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(QueryError::schema())
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

/// Identificador de equipe/workspace: nunca pode conter separadores
/// suscetíveis a injeção em path.
fn path_id<'a>(value: &'a str, key: &str) -> Result<&'a str, QueryError> {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        Ok(value)
    } else {
        Err(QueryError::config_params(
            "query.inference.invalid_path_identifier",
            params([("key", ParamValue::from(key.to_string()))]),
            format!("params.{key} contém caracteres inválidos para identificador."),
        ))
    }
}

/// Resolve a credencial de fato usada. Política: o `param_name` opcional
/// (ex.: `params.admin_key_env`) tem prioridade quando configurado com um
/// valor não vazio; caso contrário, usamos `credential_env` ou `default`.
/// Nunca tentamos duas credenciais para o mesmo provedor. O valor nunca
/// passa pelo `Account`: é resolvido exclusivamente via
/// `Context.credentials`.
fn resolve_credential(
    account: &Account,
    ctx: &Context<'_>,
    param_name: &str,
    default: &str,
    required_class: &str,
) -> Result<String, QueryError> {
    let override_env = account
        .params
        .get(param_name)
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty());
    let env_name = match override_env {
        Some(name) => name.trim().to_string(),
        None => account
            .credential_env
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(default)
            .to_string(),
    };
    ctx.credentials
        .get(&env_name)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            let hint = if override_env.is_some() {
                format!(
                    "O parâmetro `params.{param_name}` aponta para \
                     `{env_name}`, que está vazia ou ausente."
                )
            } else {
                format!(
                    "Defina a env `{env_name}` no ambiente, ou informe \
                     `params.{param_name}` com o nome de outra env que \
                     contenha uma chave de classe {required_class}."
                )
            };
            QueryError::config_params(
                "query.inference.missing_endpoint_credential",
                params([
                    ("class", ParamValue::from(required_class.to_string())),
                    ("env", ParamValue::from(env_name.clone())),
                    ("param", ParamValue::from(param_name.to_string())),
                ]),
                format!("Chave de {required_class} ausente para este endpoint. {hint}"),
            )
        })
}

// =================================================================== Perplexity

/// `count` é obrigatório na resposta da Analytics API: sua ausência
/// significa resposta fora do contrato. Por isso NÃO usamos `default`,
/// evitando zeros silenciosos para séries quebradas.
#[derive(Deserialize)]
struct PerplexityBucket {
    count: f64,
}

#[derive(Deserialize)]
struct PerplexityPage {
    #[serde(default)]
    data: Vec<PerplexityBucket>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_page: Option<String>,
}

async fn perplexity_buckets(
    build: impl Fn(Option<String>) -> RequestBuilder,
) -> Result<Vec<PerplexityBucket>, QueryError> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let page: PerplexityPage = json(build(cursor.clone())).await?;
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

async fn perplexity(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    if !account
        .params
        .get("product")
        .map(String::as_str)
        .is_some_and(|p| p.eq_ignore_ascii_case("computer"))
    {
        return Err(QueryError::unsupported(
            "query.inference.perplexity_computer_opt_in_required",
            "Opt-in necessário: defina `params.product = computer` para \
             consultar a Analytics API do Perplexity Computer. A API \
             Sonar/Ask (restante do Perplexity) não tem endpoint público \
             de saldo/uso por chave. Fonte: \
             https://docs.perplexity.ai/docs/admin/computer-analytics-api.md"
                .to_string(),
        ));
    }

    let key = resolve_credential(
        account,
        ctx,
        "analytics_key_env",
        "PERPLEXITY_API_KEY",
        "analytics (Perplexity)",
    )?;

    let now = Utc::now();
    let (start, reset) = month_bounds(now);
    let period = start.format("%Y-%m").to_string();

    // Este adaptador consulta apenas `credit_usage`. Outros datasets
    // (`query_volume`, `daily_active_users`, `connectors`, etc.) medem
    // grandezas diferentes (consultas, membros, threads) e estão fora do
    // escopo desta consulta; nada impede uma expansão futura, mas por
    // enquanto mantemos a unidade/label sem ambiguidade.
    let dataset = "credit_usage";

    let user_email = account
        .params
        .get("user_email")
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty());

    let base_query: Vec<(String, String)> = vec![
        ("dataset".to_string(), dataset.to_string()),
        ("start_time".to_string(), start.timestamp().to_string()),
        ("end_time".to_string(), now.timestamp().to_string()),
        ("bucket_width".to_string(), "1d".to_string()),
    ];

    let buckets: Vec<PerplexityBucket> = perplexity_buckets(|cursor| {
        let mut q = base_query.clone();
        if let Some(c) = cursor {
            q.push(("page".to_string(), c));
        }
        if let Some(email) = user_email {
            q.push(("user_email".to_string(), email.to_string()));
        }
        ctx.http
            .get(format!("{base}/v1/analytics/computer/usage"))
            .bearer_auth(&key)
            .query(&q)
            .timeout(ctx.timeout)
    })
    .await?;

    let mut total = 0.0;
    for bucket in buckets {
        total = finite(total + bucket.count)?;
    }
    let total = finite(total)?;

    let mut metrics = Vec::new();
    let mut m = Metric::usage(
        "Créditos consumidos no mês (Perplexity Computer)",
        total,
        "créditos",
        MetricKind::Credits,
        Some(MetricWindow::Monthly),
    );
    m.period = Some(period.clone());
    m.resets_at = Some(reset);
    metrics.push(m);

    let mut notes = Notes::new();
    notes.push(
        "note.inference.perplexity_computer_opt_in",
        "Modo opt-in: aplicado apenas porque params.product=computer. A API \
         Sonar/Ask (restante do Perplexity) não tem endpoint público de \
         saldo/uso por chave.",
    );
    notes.push(
        "note.inference.perplexity_enterprise_analytics",
        "Analytics API exclusiva para organizações Enterprise; a chave é \
         gerada em Settings → Organization → Computer no app web.",
    );
    notes.push(
        "note.inference.perplexity_credit_usage_dataset",
        "Dataset fixado em `credit_usage` (consulta de uso da API Sonar/Ask \
         fora de escopo deste adaptador).",
    );
    notes.push(
        "note.inference.perplexity_complete_utc_days",
        "Buckets retornam dias UTC completos — o dia em andamento nunca é \
         incluído, então o total pode subir até o fechamento do próximo \
         ciclo.",
    );
    notes.push(
        "note.inference.perplexity_limit_unavailable",
        "Limite e restante do plano não são expostos: somente `used` está \
         disponível.",
    );
    if let Some(email) = user_email {
        notes.push_params(
            "note.inference.perplexity_member_filter",
            params([("email", ParamValue::from(email.to_string()))]),
            format!("Filtrado por membro da organização: {email}."),
        );
    }

    Ok(Data::new(
        metrics,
        notes,
        "https://docs.perplexity.ai/docs/admin/computer-analytics-api.md",
    ))
}

// ===================================================================== Mistral

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct MistralUsageLimits {
    #[serde(default)]
    monthly_limit_reached: bool,
    #[serde(default)]
    usage: Option<f64>,
    #[serde(default)]
    total_usage: Option<f64>,
    #[serde(default)]
    usage_limit: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct MistralLimitsContext {
    completion: MistralUsageLimits,
    currency: String,
}

#[derive(Deserialize)]
struct MistralLimitsOut {
    limits: MistralLimitsContext,
}

async fn mistral(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    // Documentação oficial: Admin API autentica via header `x-api-key`,
    // não `Authorization: Bearer`. Fonte:
    // https://docs.mistral.ai/admin/admin-api/authentication
    let key = resolve_credential(
        account,
        ctx,
        "admin_key_env",
        "MISTRAL_API_KEY",
        "admin (Mistral)",
    )?;

    let now = Utc::now();
    let (start, reset) = month_bounds(now);
    let period = start.format("%Y-%m").to_string();

    let body: MistralLimitsOut = json(
        ctx.http
            .get(format!("{base}/v1/admin/spend-limit"))
            .header("x-api-key", &key)
            .timeout(ctx.timeout),
    )
    .await?;

    let currency = body.limits.currency.trim().to_uppercase();
    if currency.is_empty() {
        return Err(QueryError::schema());
    }

    // O OpenAPI documenta o bloco como contendo apenas `monthly_limit_reached`
    // como obrigatório, mais opcionais `usage`, `vibe_usage`, `total_usage`
    // e `usage_limit`. O OpenAPI declara `usage: number` sem `format`; o
    // guia oficial `usage-metrics.md` afirma que o limite mensal é "as a
    // whole number in your Organization's billing currency (`500` means
    // 500, not 500 cents)" mas **não** confirma a mesma escala para os
    // campos numéricos de uso. Sem fonte primária explícita sobre a
    // escala desses campos, usamos a moeda declarada (`limits.currency`)
    // como rótulo genérico de unidade — `USD`, `EUR` etc. — sem assumir
    // USD direto nem cents.
    let completion = body.limits.completion;
    let used_opt = match completion.total_usage.or(completion.usage) {
        Some(v) if v.is_finite() => Some(v),
        Some(_) => return Err(QueryError::schema()),
        None => None,
    };
    let limit = completion.usage_limit.filter(|v| v.is_finite());

    let mut metrics = Vec::new();
    let mut m = Metric::usage(
        format!("Uso de completion no mês corrente ({currency})"),
        used_opt.unwrap_or(0.0),
        currency.clone(),
        MetricKind::Spend,
        Some(MetricWindow::Monthly),
    );
    m.used = used_opt;
    m.period = Some(period.clone());
    m.resets_at = Some(reset);
    if let (Some(limit), Some(used)) = (limit, used_opt) {
        m.limit = Some(limit);
        m.remaining = Some(finite(limit - used)?);
    }
    metrics.push(m);

    let mut notes = Notes::new();
    notes.push(
        "note.inference.mistral_admin_key_required",
        "Endpoint /v1/admin/spend-limit requer Admin API Key (gerada em \
         Console → Workspace → API keys → Admin keys).",
    );
    notes.push(
        "note.inference.mistral_completion_only",
        "Somente completion é contabilizado: OCR, áudio, fine-tuning e \
         Vibe não entram no total.",
    );
    notes.push(
        "note.inference.mistral_currency_scale_undocumented",
        "Unidade monetária: o OpenAPI declara `usage`/`usage_limit` como \
         `number` sem `format`, e a fonte oficial distingue apenas a \
         escala do limite mensal. A unidade exata dos campos de uso não \
         está documentada, então rotulamos com o código `currency` da \
         resposta (ex.: USD, EUR) sem assumir escala subjacente.",
    );
    if completion.monthly_limit_reached {
        notes.push(
            "note.inference.mistral_monthly_limit_reached",
            "Limite mensal atingido: a organização está bloqueada.",
        );
    }
    if used_opt.is_none() {
        notes.push(
            "note.inference.mistral_usage_unknown",
            "Resposta sem campos numéricos de uso: `used` permanece \
             desconhecido. O booleano `monthly_limit_reached` continua \
             sendo a única indicação obrigatória.",
        );
    }
    if limit.is_none() {
        notes.push(
            "note.inference.mistral_no_monthly_limit",
            "Plano sem limite mensal configurado (`no_monthly_limit=true`); \
             somente `used` está disponível.",
        );
    }

    Ok(Data::new(
        metrics,
        notes,
        "https://docs.mistral.ai/api/endpoint/beta/admin/billing",
    ))
}

// ======================================================================== Grok

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GrokUsdCents {
    #[serde(default)]
    val: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GrokBillingCycle {
    #[serde(default)]
    year: i64,
    #[serde(default)]
    month: i64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GrokInvoiceCore {
    #[serde(default)]
    amount_before_vat: String,
    #[serde(default)]
    amount_after_vat: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GrokPostpaidPreview {
    #[serde(default)]
    core_invoice: GrokInvoiceCore,
    #[serde(default)]
    effective_spending_limit: String,
    #[serde(default)]
    billing_cycle: GrokBillingCycle,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GrokPrepaidBalance {
    #[serde(default)]
    total: GrokUsdCents,
}

/// Decodifica strings de USD cents (1 USD = 100 cents) em `f64`,
/// distinguindo três casos:
/// - ausente/vazio: `Ok(None)`
/// - presente e finito: `Ok(Some(dollars))`
/// - presente mas malformado ou não finito: `Err(schema)`
///
/// A regra acima garante que campos ausentes (ex.: saldo pré-pago zerado
/// sem bloco `total`) não sejam confundidos com dados corrompidos, mas
/// qualquer string presente que não parseie vira erro de esquema — sem
/// fallback silencioso para 0.
fn parse_usd_cents(value: &str) -> Result<Option<f64>, QueryError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let cents: f64 = trimmed.parse().map_err(|_| QueryError::schema())?;
    let dollars = cents / 100.0;
    if !dollars.is_finite() {
        return Err(QueryError::schema());
    }
    Ok(Some(dollars))
}

async fn grok(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = resolve_credential(
        account,
        ctx,
        "management_key_env",
        "X_GROK_API_KEY",
        "management (xAI)",
    )?;

    let team_id_value = account
        .params
        .get("team_id")
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            QueryError::config(
                "query.inference.grok_team_id_required",
                "Configure params.team_id com o Team ID visível em \
                 console.x.ai/team/default/settings/team."
                    .to_string(),
            )
        })?;
    let team_id = path_id(team_id_value, "team_id")?;

    let now = Utc::now();
    let (start, _reset) = month_bounds(now);
    let period = start.format("%Y-%m").to_string();

    let prepaid: GrokPrepaidBalance = json(
        ctx.http
            .get(format!("{base}/v1/billing/teams/{team_id}/prepaid/balance"))
            .bearer_auth(&key)
            .timeout(ctx.timeout),
    )
    .await?;

    let preview: GrokPostpaidPreview = json(
        ctx.http
            .get(format!(
                "{base}/v1/billing/teams/{team_id}/postpaid/invoice/preview"
            ))
            .bearer_auth(&key)
            .timeout(ctx.timeout),
    )
    .await?;

    let mut metrics = Vec::new();
    let mut notes = Notes::new();
    notes.push(
        "note.inference.grok_management_api",
        "Management API (`management-api.x.ai`) é distinta da API de \
         inferência (`api.x.ai`) e da API social X (`api.x.com`); a \
         chave usada é a Management Key gerada em console.x.ai → \
         Settings → Management Keys.",
    );
    // We don't know `spending_limit` yet here; push the placeholder note
    // later when the value is known so the catalog template's `{limit}`
    // actually resolves. The placeholder-less variant stays as a
    // fallback for the rare case where preview returns without a limit.
    notes.push(
        "note.inference.grok_prepaid_no_monthly_expiration",
        "Créditos pré-pagos não têm expiração mensal documentada; não \
         inferimos reset e mantemos `resets_at = None` para esse saldo.",
    );

    // Pré-pago: o endpoint devolve `total` em USD cents (negativo quando
    // há compra, positivo quando há gasto). Tratamos o valor como saldo
    // **disponível** e o apresentamos em `remaining`; `used` permanece
    // `None` porque o campo da API é saldo, não consumo.
    match parse_usd_cents(&prepaid.total.val)? {
        Some(saldo) => {
            let mut m = Metric::usage(
                "Saldo de créditos pré-pagos",
                0.0,
                "USD",
                MetricKind::Credits,
                None,
            );
            m.used = None;
            m.remaining = Some(saldo);
            // Sem reset documentado para pré-pago: permanece None.
            metrics.push(m);
        }
        None => {
            notes.push(
                "note.inference.grok_prepaid_total_missing",
                "Sem campo `total` em /prepaid/balance: a equipe pode não \
                 ter créditos pré-pagos configurados.",
            );
        }
    }

    let cycle_total = match (
        parse_usd_cents(&preview.core_invoice.amount_after_vat),
        parse_usd_cents(&preview.core_invoice.amount_before_vat),
    ) {
        (Ok(v), _) | (_, Ok(v)) => v,
        (Err(e), _) => return Err(e),
    };
    let spending_limit = parse_usd_cents(&preview.effective_spending_limit)?;

    match (cycle_total, spending_limit) {
        (Some(total), limit_opt) => {
            let mut m = Metric::usage(
                "Uso pós-pago previsto no ciclo corrente",
                total,
                "USD",
                MetricKind::Spend,
                Some(MetricWindow::Monthly),
            );
            if preview.billing_cycle.year > 0 {
                let month = preview.billing_cycle.month.max(1) as u32;
                m.period = Some(format!("{:04}-{:02}", preview.billing_cycle.year, month));
            } else {
                m.period = Some(period.clone());
            }
            m.resets_at = Some(start + Months::new(1));
            if let Some(limit) = limit_opt {
                // Rótulo explícito: é o orçamento pós-pago que o próprio
                // usuário configurou na console (soft spending limit),
                // não uma cota da plataforma.
                m.limit = Some(limit);
                let remaining = limit - total;
                m.remaining = Some(finite(remaining)?);
            }
            metrics.push(m);
        }
        (None, _) => {
            notes.push(
                "note.inference.grok_cycle_total_unusable",
                "Preview do ciclo corrente sem total numérico utilizável.",
            );
        }
    }

    if spending_limit.is_none() {
        notes.push(
            "note.inference.grok_spending_limit_missing",
            "Sem orçamento pós-pago configurado (soft spending limit \
             ausente): o teto de gasto permanece desconhecido.",
        );
    } else if let Some(limit) = spending_limit {
        // RF8: surface the configured USD ceiling through `{limit}` so
        // the catalog template renders it. Without the placeholder the
        // catalog drops the value inside the English text.
        notes.push_params(
            "note.inference.grok_soft_spending_limit",
            params([("limit", ParamValue::from(limit.to_string()))]),
            format!(
                "`effectiveSpendingLimit` é o teto pós-pago efetivo: {limit} USD; \
                 é um orçamento (soft spending limit), não uma cota da plataforma."
            ),
        );
    }

    if metrics.is_empty() {
        // Se nenhum endpoint devolveu dados utilizáveis, retornamos erro
        // de esquema em vez de fingir sucesso com relatório vazio.
        return Err(QueryError::schema());
    }

    Ok(Data::new(
        metrics,
        notes,
        "https://docs.x.ai/developers/rest-api-reference/management/billing.md",
    ))
}

// ===================================================================== testes

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Status};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{
        bearer_token, header, method, path, query_param, query_param_is_missing,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct MapCreds(BTreeMap<String, String>);
    impl Credentials for MapCreds {
        fn get(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    fn account(provider: Provider, env: Option<&str>, params: BTreeMap<String, String>) -> Account {
        Account {
            id: None,
            name: "conta".to_string(),
            provider,
            credential_env: env.map(str::to_string),
            session: None,
            oauth: None,
            params,
            refresh_seconds: None,
            group: None,
        }
    }

    fn context<'a>(http: &'a reqwest::Client, creds: &'a MapCreds) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: Duration::from_secs(5),
        }
    }

    fn metric<'a>(data: &'a Data, label: &str) -> &'a Metric {
        data.metrics
            .iter()
            .find(|m| m.label == label)
            .expect("métrica esperada ausente")
    }

    fn approx(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "esperava {expected}, obtive {actual}"
        );
    }

    // ---------------------------------------------- defaults dos provedores

    #[tokio::test]
    async fn fetch_provider_nao_atendido_retorna_indisponivel() {
        let creds = MapCreds(BTreeMap::new());
        let http = reqwest::Client::new();
        let err = fetch(
            &account(
                Provider::OpenAiApi,
                Some("OPENAI_ADMIN_KEY"),
                BTreeMap::new(),
            ),
            &context(&http, &creds),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Unsupported);
    }

    // ------------------------------------------------------- credenciais

    #[tokio::test]
    async fn resolve_credential_prefere_param_env_quando_configurado() {
        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("M_ADMIN".to_string(), "admin-value".to_string());
        creds
            .0
            .insert("MISTRAL_API_KEY".to_string(), "ignored".to_string());
        let http = reqwest::Client::new();
        let acc = account(
            Provider::Mistral,
            Some("MISTRAL_API_KEY"),
            BTreeMap::from([("admin_key_env".to_string(), "M_ADMIN".to_string())]),
        );
        let value = resolve_credential(
            &acc,
            &context(&http, &creds),
            "admin_key_env",
            "MISTRAL_API_KEY",
            "admin (Mistral)",
        )
        .unwrap();
        assert_eq!(value, "admin-value");
    }

    #[tokio::test]
    async fn resolve_credential_sem_param_cai_em_credential_env() {
        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("MY_MGMT".to_string(), "mgmt-value".to_string());
        let http = reqwest::Client::new();
        let acc = account(Provider::Grok, Some("MY_MGMT"), BTreeMap::new());
        let value = resolve_credential(
            &acc,
            &context(&http, &creds),
            "management_key_env",
            "X_GROK_API_KEY",
            "management (xAI)",
        )
        .unwrap();
        assert_eq!(value, "mgmt-value");
    }

    #[tokio::test]
    async fn resolve_credential_sem_param_e_sem_credential_env_cai_no_default() {
        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("X_GROK_API_KEY".to_string(), "default-value".to_string());
        let http = reqwest::Client::new();
        let acc = account(Provider::Grok, None, BTreeMap::new());
        let value = resolve_credential(
            &acc,
            &context(&http, &creds),
            "management_key_env",
            "X_GROK_API_KEY",
            "management (xAI)",
        )
        .unwrap();
        assert_eq!(value, "default-value");
    }

    #[tokio::test]
    async fn resolve_credential_param_vazio_e_tratado_como_ausente() {
        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("X_GROK_API_KEY".to_string(), "default-value".to_string());
        creds.0.insert(" ".to_string(), "ignored".to_string());
        let http = reqwest::Client::new();
        let acc = account(
            Provider::Grok,
            Some("X_GROK_API_KEY"),
            BTreeMap::from([("management_key_env".to_string(), "  ".to_string())]),
        );
        let value = resolve_credential(
            &acc,
            &context(&http, &creds),
            "management_key_env",
            "X_GROK_API_KEY",
            "management (xAI)",
        )
        .unwrap();
        assert_eq!(value, "default-value");
    }

    #[tokio::test]
    async fn resolve_credential_ausente_retorna_needs_config() {
        let creds = MapCreds(BTreeMap::new());
        let http = reqwest::Client::new();
        let acc = account(Provider::Mistral, None, BTreeMap::new());
        let err = resolve_credential(
            &acc,
            &context(&http, &creds),
            "admin_key_env",
            "MISTRAL_API_KEY",
            "admin (Mistral)",
        )
        .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("admin (Mistral)"));
    }

    // ----------------------------------------------------- helpers numéricos

    #[tokio::test]
    async fn finite_aceita_valores_e_rejeita_nan_e_infinito() {
        assert!(finite(1.5).is_ok());
        assert!(finite(f64::NAN).is_err());
        assert!(finite(f64::INFINITY).is_err());
        assert!(finite(f64::NEG_INFINITY).is_err());
    }

    #[tokio::test]
    async fn path_id_recusa_caracteres_perigosos() {
        assert!(path_id("abc-123_xyz", "team_id").is_ok());
        assert_eq!(
            path_id("bad/id", "team_id").unwrap_err().status,
            Status::NeedsConfiguration
        );
        assert_eq!(
            path_id("abc def", "team_id").unwrap_err().status,
            Status::NeedsConfiguration
        );
    }

    #[tokio::test]
    async fn parse_usd_cents_decodifica_valores_e_lida_com_strings_vazias() {
        assert_eq!(parse_usd_cents("1234").unwrap(), Some(12.34));
        assert_eq!(parse_usd_cents("0").unwrap(), Some(0.0));
        assert_eq!(parse_usd_cents("").unwrap(), None);
        assert_eq!(parse_usd_cents("  50  ").unwrap(), Some(0.5));
        assert_eq!(
            parse_usd_cents("não-numérico").unwrap_err().status,
            Status::Error
        );
        assert_eq!(parse_usd_cents("1e500").unwrap_err().status, Status::Error);
    }

    // ----------------------------------------------------------- Perplexity

    #[tokio::test]
    async fn perplexity_padrao_sem_product_e_unsupported() {
        let server = MockServer::start().await;
        let creds = MapCreds(BTreeMap::new());
        let http = reqwest::Client::new();
        let err = perplexity(
            &account(
                Provider::Perplexity,
                Some("PERPLEXITY_API_KEY"),
                BTreeMap::new(),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Unsupported);
        assert!(err.note.fallback.contains("params.product = computer"));
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            0
        );
    }

    #[tokio::test]
    async fn perplexity_opt_in_sem_chave_retorna_needs_config_sem_http() {
        let server = MockServer::start().await;
        let creds = MapCreds(BTreeMap::new());
        let http = reqwest::Client::new();
        let err = perplexity(
            &account(
                Provider::Perplexity,
                Some("PERPLEXITY_API_KEY"),
                BTreeMap::from([("product".to_string(), "computer".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("analytics_key_env"));
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            0
        );
    }

    #[tokio::test]
    async fn perplexity_opt_in_com_chave_via_param_env_funciona() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/analytics/computer/usage"))
            .and(bearer_token("pplx-an"))
            .and(query_param("dataset", "credit_usage"))
            .and(query_param("bucket_width", "1d"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "categories": ["Model", "Credit Source"],
                "data": [
                    { "start_time": 1, "end_time": 2, "count": 100.0,
                      "by_categories": {
                          "Model": [{ "category": "x", "count": 60.0 }],
                          "Credit Source": [{ "category": "paid", "count": 100.0 }]
                      } },
                    { "start_time": 2, "end_time": 3, "count": 250.0,
                      "by_categories": {
                          "Model": [{ "category": "x", "count": 250.0 }],
                          "Credit Source": [{ "category": "paid", "count": 250.0 }]
                      } }
                ],
                "has_more": false,
                "next_page": null
            })))
            .mount(&server)
            .await;

        // PERPLEXITY_API_KEY intencionalmente ausente: a chave de
        // analytics é resolvida via params.analytics_key_env.
        let mut creds = MapCreds(BTreeMap::new());
        creds.0.insert("PPLX_AN".to_string(), "pplx-an".to_string());
        let http = reqwest::Client::new();
        let data = perplexity(
            &account(
                Provider::Perplexity,
                Some("PERPLEXITY_API_KEY"),
                BTreeMap::from([
                    ("product".to_string(), "computer".to_string()),
                    ("analytics_key_env".to_string(), "PPLX_AN".to_string()),
                ]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();

        let total = metric(&data, "Créditos consumidos no mês (Perplexity Computer)");
        approx(total.used.unwrap(), 350.0);
        assert_eq!(total.unit, "créditos");
        assert!(total.limit.is_none() && total.remaining.is_none());
        assert_eq!(total.kind, MetricKind::Credits);
        assert_eq!(total.window, Some(MetricWindow::Monthly));
        assert!(total.period.is_some() && total.resets_at.is_some());
    }

    #[tokio::test]
    async fn perplexity_opt_in_chave_via_credential_env_padrao_tambem_funciona() {
        // Mesmo sem params.analytics_key_env, se a env `credential_env`
        // contiver uma chave de analytics válida, usamos ela.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/analytics/computer/usage"))
            .and(bearer_token("pplx-an"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "count": 12.5 }],
                "has_more": false,
                "next_page": null
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("PERPLEXITY_API_KEY".to_string(), "pplx-an".to_string());
        let http = reqwest::Client::new();
        let data = perplexity(
            &account(
                Provider::Perplexity,
                Some("PERPLEXITY_API_KEY"),
                BTreeMap::from([("product".to_string(), "computer".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();
        approx(
            metric(&data, "Créditos consumidos no mês (Perplexity Computer)")
                .used
                .unwrap(),
            12.5,
        );
    }

    #[tokio::test]
    async fn perplexity_dataset_user_email_e_apenas_credit_usage() {
        // O dataset é fixo em `credit_usage`; este teste confirma que
        // mesmo se o usuário tentar outros nomes via params, a consulta
        // usa o dataset fixo e o filtro user_email é propagado.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/analytics/computer/usage"))
            .and(query_param("dataset", "credit_usage"))
            .and(query_param("user_email", "alice@example.com"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "count": 7.0 }],
                "has_more": false,
                "next_page": null
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("PERPLEXITY_API_KEY".to_string(), "pplx-an".to_string());
        let http = reqwest::Client::new();
        let data = perplexity(
            &account(
                Provider::Perplexity,
                Some("PERPLEXITY_API_KEY"),
                BTreeMap::from([
                    ("product".to_string(), "computer".to_string()),
                    ("user_email".to_string(), "alice@example.com".to_string()),
                    // Tentativa de override do dataset é ignorada.
                    ("dataset".to_string(), "query_volume".to_string()),
                ]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();
        approx(
            metric(&data, "Créditos consumidos no mês (Perplexity Computer)")
                .used
                .unwrap(),
            7.0,
        );
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("Dataset fixado em `credit_usage`")));
    }

    #[tokio::test]
    async fn perplexity_count_ausente_no_bucket_e_schema_nao_zero() {
        // count ausente no payload é tratado como erro de esquema
        // (não como zero silencioso).
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/analytics/computer/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{}],
                "has_more": false,
                "next_page": null
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("PERPLEXITY_API_KEY".to_string(), "pplx-an".to_string());
        let http = reqwest::Client::new();
        let err = perplexity(
            &account(
                Provider::Perplexity,
                Some("PERPLEXITY_API_KEY"),
                BTreeMap::from([("product".to_string(), "computer".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn perplexity_paginacao_soma_todas_as_paginas() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/analytics/computer/usage"))
            .and(query_param_is_missing("page"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "count": 10.0 }],
                "has_more": true,
                "next_page": "cursor-2"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/analytics/computer/usage"))
            .and(query_param("page", "cursor-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "count": 25.0 }, { "count": 5.0 }],
                "has_more": false,
                "next_page": null
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("PERPLEXITY_API_KEY".to_string(), "pplx-an".to_string());
        let http = reqwest::Client::new();
        let data = perplexity(
            &account(
                Provider::Perplexity,
                Some("PERPLEXITY_API_KEY"),
                BTreeMap::from([("product".to_string(), "computer".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();
        approx(
            metric(&data, "Créditos consumidos no mês (Perplexity Computer)")
                .used
                .unwrap(),
            40.0,
        );
        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 2);
    }

    // -------------------------------------------------------------- Mistral

    #[tokio::test]
    async fn mistral_sem_chave_retorna_needs_config_sem_http() {
        let server = MockServer::start().await;
        let creds = MapCreds(BTreeMap::new());
        let http = reqwest::Client::new();
        let err = mistral(
            &account(Provider::Mistral, None, BTreeMap::new()),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("admin (Mistral)"));
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            0
        );
    }

    #[tokio::test]
    async fn mistral_param_env_aponta_para_env_vazia_retorna_needs_config() {
        let server = MockServer::start().await;
        let creds = MapCreds(BTreeMap::new());
        let http = reqwest::Client::new();
        let err = mistral(
            &account(
                Provider::Mistral,
                Some("MISTRAL_API_KEY"),
                BTreeMap::from([("admin_key_env".to_string(), "M_ADMIN".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("M_ADMIN"));
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            0
        );
    }

    #[tokio::test]
    async fn mistral_decodifica_total_usage_e_aplica_limite() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/admin/spend-limit"))
            .and(header("x-api-key", "admin-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "limits": {
                    "completion": {
                        "monthly_limit_reached": false,
                        "usage": 0.0,
                        "vibe_usage": 0.0,
                        "total_usage": 42.5,
                        "usage_limit": 100.0
                    },
                    "last_payment_failure": false,
                    "last_payment_failure_protection": null,
                    "currency": "usd"
                }
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("M_ADMIN".to_string(), "admin-secret".to_string());
        let http = reqwest::Client::new();
        let data = mistral(
            &account(
                Provider::Mistral,
                Some("MISTRAL_API_KEY"),
                BTreeMap::from([("admin_key_env".to_string(), "M_ADMIN".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();

        let m = metric(&data, "Uso de completion no mês corrente (USD)");
        approx(m.used.unwrap(), 42.5);
        assert_eq!(m.unit, "USD");
        approx(m.limit.unwrap(), 100.0);
        assert_eq!(m.kind, MetricKind::Spend);
        assert_eq!(m.window, Some(MetricWindow::Monthly));
        approx(m.remaining.unwrap(), 57.5);
        assert!(m.period.is_some() && m.resets_at.is_some());
    }

    #[tokio::test]
    async fn mistral_caiu_em_usage_quando_total_usage_ausente() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/admin/spend-limit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "limits": {
                    "completion": {
                        "monthly_limit_reached": false,
                        "usage": 12.5
                    },
                    "currency": "EUR"
                }
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("M_ADMIN".to_string(), "admin-secret".to_string());
        let http = reqwest::Client::new();
        let data = mistral(
            &account(
                Provider::Mistral,
                Some("MISTRAL_API_KEY"),
                BTreeMap::from([("admin_key_env".to_string(), "M_ADMIN".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();
        let m = metric(&data, "Uso de completion no mês corrente (EUR)");
        approx(m.used.unwrap(), 12.5);
        assert!(m.limit.is_none() && m.remaining.is_none());
    }

    #[tokio::test]
    async fn mistral_apenas_booleano_sem_usage_mantem_used_como_none() {
        // Resposta mínima (apenas o campo obrigatório `monthly_limit_reached`):
        // mantemos `used` desconhecido e emitimos uma nota explicando.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/admin/spend-limit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "limits": {
                    "completion": { "monthly_limit_reached": false },
                    "currency": "USD"
                }
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("M_ADMIN".to_string(), "admin-secret".to_string());
        let http = reqwest::Client::new();
        let data = mistral(
            &account(
                Provider::Mistral,
                Some("MISTRAL_API_KEY"),
                BTreeMap::from([("admin_key_env".to_string(), "M_ADMIN".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();
        let m = metric(&data, "Uso de completion no mês corrente (USD)");
        assert!(m.used.is_none());
        assert!(m.limit.is_none() && m.remaining.is_none());
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("`used` permanece desconhecido")));
    }

    #[tokio::test]
    async fn mistral_currency_vazia_e_erro_de_esquema() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/admin/spend-limit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "limits": {
                    "completion": { "monthly_limit_reached": false, "usage": 1.0 },
                    "currency": "   "
                }
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("M_ADMIN".to_string(), "admin-secret".to_string());
        let http = reqwest::Client::new();
        let err = mistral(
            &account(
                Provider::Mistral,
                Some("MISTRAL_API_KEY"),
                BTreeMap::from([("admin_key_env".to_string(), "M_ADMIN".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
        // O erro é QueryError::schema — mensagem fixa do helper.
        assert!(err.note.fallback.contains("Resposta"));
    }

    #[tokio::test]
    async fn mistral_403_e_tratado_como_autenticacao() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/admin/spend-limit"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("M_ADMIN".to_string(), "admin-secret".to_string());
        let http = reqwest::Client::new();
        let err = mistral(
            &account(
                Provider::Mistral,
                Some("MISTRAL_API_KEY"),
                BTreeMap::from([("admin_key_env".to_string(), "M_ADMIN".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::AuthenticationError);
    }

    // ---------------------------------------------------------------- Grok

    #[tokio::test]
    async fn grok_sem_chave_e_sem_team_id_retorna_needs_config_sem_http() {
        let server = MockServer::start().await;
        let creds = MapCreds(BTreeMap::new());
        let http = reqwest::Client::new();
        let err = grok(
            &account(Provider::Grok, None, BTreeMap::new()),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("management (xAI)"));
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            0
        );
    }

    #[tokio::test]
    async fn grok_team_id_invalido_e_recusado() {
        let server = MockServer::start().await;
        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("X_GROK_API_KEY".to_string(), "mgmt".to_string());
        let http = reqwest::Client::new();
        let err = grok(
            &account(
                Provider::Grok,
                Some("X_GROK_API_KEY"),
                BTreeMap::from([("team_id".to_string(), "bad id!".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("team_id"));
    }

    #[tokio::test]
    async fn grok_com_team_id_valido_carrega_saldo_e_preview() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/billing/teams/abc/prepaid/balance"))
            .and(bearer_token("mgmt-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": { "val": "12345" },
                "changes": [
                    { "teamId": "abc", "changeOrigin": "PURCHASE",
                      "amount": { "val": "-1000" } }
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/billing/teams/abc/postpaid/invoice/preview"))
            .and(bearer_token("mgmt-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "coreInvoice": {
                    "amountBeforeVat": "1000",
                    "amountAfterVat": "1180",
                    "prepaidCredits": { "val": "-4500" },
                    "prepaidCreditsUsed": { "val": "0" },
                    "lines": []
                },
                "effectiveSpendingLimit": "20000",
                "defaultCredits": "0",
                "billingCycle": { "year": 2025, "month": 11 }
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("X_GROK_API_KEY".to_string(), "mgmt-secret".to_string());
        let http = reqwest::Client::new();
        let data = grok(
            &account(
                Provider::Grok,
                Some("X_GROK_API_KEY"),
                BTreeMap::from([("team_id".to_string(), "abc".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();

        let prepaid = metric(&data, "Saldo de créditos pré-pagos");
        // Pré-pago: `used` permanece None; o saldo é exposto em `remaining`.
        assert!(prepaid.used.is_none());
        approx(prepaid.remaining.unwrap(), 123.45);
        assert_eq!(prepaid.kind, MetricKind::Credits);
        assert_eq!(prepaid.window, None);
        assert_eq!(prepaid.unit, "USD");
        assert!(prepaid.period.is_none());
        // Sem reset documentado para pré-pago.
        assert!(prepaid.resets_at.is_none());

        let cycle = metric(&data, "Uso pós-pago previsto no ciclo corrente");
        approx(cycle.used.unwrap(), 11.80);
        approx(cycle.limit.unwrap(), 200.0);
        approx(cycle.remaining.unwrap(), 188.20);
        assert_eq!(cycle.kind, MetricKind::Spend);
        assert_eq!(cycle.window, Some(MetricWindow::Monthly));
        assert_eq!(cycle.period.as_deref(), Some("2025-11"));

        // Notas explicam que effectiveSpendingLimit é orçamento, não cota.
        assert!(data.notes.iter().any(|n| n.contains("orçamento")));
        assert!(data.notes.iter().any(|n| n.contains("expiração mensal")));
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn grok_prepaid_total_malformado_e_schema() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/billing/teams/abc/prepaid/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": { "val": "nao-num" },
                "changes": []
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/billing/teams/abc/postpaid/invoice/preview"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "coreInvoice": { "amountBeforeVat": "0", "amountAfterVat": "0" },
                "effectiveSpendingLimit": "0",
                "billingCycle": { "year": 2025, "month": 11 }
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("X_GROK_API_KEY".to_string(), "mgmt-secret".to_string());
        let http = reqwest::Client::new();
        let err = grok(
            &account(
                Provider::Grok,
                Some("X_GROK_API_KEY"),
                BTreeMap::from([("team_id".to_string(), "abc".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn grok_sem_metricas_e_schema_em_vez_de_sucesso_vazio() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/billing/teams/abc/prepaid/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": { "val": "" },
                "changes": []
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/billing/teams/abc/postpaid/invoice/preview"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "coreInvoice": { "amountBeforeVat": "", "amountAfterVat": "" },
                "effectiveSpendingLimit": "",
                "billingCycle": { "year": 2025, "month": 11 }
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("X_GROK_API_KEY".to_string(), "mgmt-secret".to_string());
        let http = reqwest::Client::new();
        let err = grok(
            &account(
                Provider::Grok,
                Some("X_GROK_API_KEY"),
                BTreeMap::from([("team_id".to_string(), "abc".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn grok_param_env_aponta_para_env_vazia_retorna_needs_config() {
        let server = MockServer::start().await;
        let creds = MapCreds(BTreeMap::new());
        let http = reqwest::Client::new();
        let err = grok(
            &account(
                Provider::Grok,
                Some("X_GROK_API_KEY"),
                BTreeMap::from([
                    ("management_key_env".to_string(), "XAI_MGMT".to_string()),
                    ("team_id".to_string(), "abc".to_string()),
                ]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("XAI_MGMT"));
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            0
        );
    }

    #[tokio::test]
    async fn grok_sem_prepaid_total_mas_com_preview_e_valido() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/billing/teams/abc/prepaid/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": { "val": "" },
                "changes": []
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/billing/teams/abc/postpaid/invoice/preview"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "coreInvoice": {
                    "amountBeforeVat": "0",
                    "amountAfterVat": "0"
                },
                "effectiveSpendingLimit": "0",
                "billingCycle": { "year": 2025, "month": 11 }
            })))
            .mount(&server)
            .await;

        let mut creds = MapCreds(BTreeMap::new());
        creds
            .0
            .insert("X_GROK_API_KEY".to_string(), "mgmt-secret".to_string());
        let http = reqwest::Client::new();
        let data = grok(
            &account(
                Provider::Grok,
                Some("X_GROK_API_KEY"),
                BTreeMap::from([("team_id".to_string(), "abc".to_string())]),
            ),
            &context(&http, &creds),
            &server.uri(),
        )
        .await
        .unwrap();

        // Sem métrica de saldo pré-pago (ausente).
        assert!(!data
            .metrics
            .iter()
            .any(|m| m.label == "Saldo de créditos pré-pagos"));
        // Mas a métrica do ciclo continua presente (0 USD).
        let cycle = metric(&data, "Uso pós-pago previsto no ciclo corrente");
        approx(cycle.used.unwrap(), 0.0);
        // E a nota explica a ausência.
        assert!(data.notes.iter().any(|n| n.contains("créditos pré-pagos")));
    }

    // --------------------------------------------------------------- fim
}
