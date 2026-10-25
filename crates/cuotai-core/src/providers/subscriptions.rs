//! Consultas de uso e cota de assinaturas (somente leitura, credencial fornecida pelo usuário).
//!
//! ChatGPT / Codex (token OAuth da sessão ChatGPT em `CHATGPT_ACCESS_TOKEN`; NÃO extraímos
//! credenciais locais automaticamente):
//!   - GET https://chatgpt.com/backend-api/wham/usage — janelas de limite do Codex/agente
//!     (não expõe a cota de mensagens do chat web do ChatGPT, que não tem endpoint legível).
//!
//!   Interface interna, não documentada e sujeita a mudanças sem aviso.
//!   Evidência: cliente oficial de código aberto do Codex CLI,
//!   https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs
//!   e https://github.com/openai/codex/tree/main/codex-rs/codex-backend-openapi-models
//!
//! Claude (token OAuth do Claude Code em `CLAUDE_OAUTH_TOKEN`; NÃO extraímos do keychain/CLI):
//!   - GET https://api.anthropic.com/api/oauth/usage — janelas de 5 horas e 7 dias do plano
//!     Pro/Max. Endpoint no host oficial api.anthropic.com, evidenciado por clientes de
//!     código aberto da comunidade (não há documentação oficial confirmando o contrato);
//!     pode mudar sem aviso.
//!
//!   Evidência: https://github.com/loocor/codmate/blob/main/services/ClaudeUsageAPIClient.swift
//!
//! Cursor (Admin API oficial, chave de administrador em `CURSOR_ADMIN_API_KEY`, Basic auth):
//!   - POST https://api.cursor.com/teams/spend — gasto do ciclo de cobrança da equipe.
//!
//!   Requer plano Teams/Enterprise com chave de administrador.
//!   Referência: https://cursor.com/docs/account/teams/admin-api.md#get-spending-data
//!
//! GitHub Copilot (PAT em `GITHUB_TOKEN`):
//!   - modo `user` (padrão): GET /user + GET /users/{login}/settings/billing/{produto}/usage
//!   - modo `org` (`params.mode=org`, `params.org`): GET /organizations/{org}/settings/billing/{produto}/usage
//!   - produto: `params.billing=premium_requests` (padrão) | `ai_credits`
//!
//!   Referência: https://docs.github.com/en/rest/billing/usage
//!
//! Windsurf (API oficial Enterprise, chave de serviço em `WINDSURF_SERVICE_KEY`):
//!   - POST https://server.codeium.com/api/v1/GetTeamCreditBalance (permissão Billing Read)
//!   - POST https://server.codeium.com/api/v1/UserPageAnalytics (permissão Teams Read-Only)
//!
//!   Requer plano Enterprise; em planos sem acesso a API responde 403 (AuthenticationError).
//!   Referência: https://docs.windsurf.com/windsurf/accounts/api-reference/api-introduction
//!
//! Parâmetros em `Account.params`:
//! - ChatGpt: `account_id` (opcional, cabeçalho chatgpt-account-id para conta de equipe)
//! - Copilot: `mode` = `user` (padrão) | `org`; `org` = slug da organização (modo org);
//!   `billing` = `premium_requests` (padrão) | `ai_credits`

use super::{json, paginate, param, secret, Context, Data, QueryError};
use crate::models::ModelInfo;
use crate::text::{params, Notes, ParamValue};
use crate::{Account, Billing, Metric, MetricKind, MetricWindow, Provider, Scope};
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;

pub(crate) const CHATGPT_BASE: &str = "https://chatgpt.com/backend-api";
pub(crate) const CLAUDE_BASE: &str = "https://api.anthropic.com";
const CURSOR_BASE: &str = "https://api.cursor.com";
const GITHUB_BASE: &str = "https://api.github.com";
const WINDSURF_BASE: &str = "https://server.codeium.com/api/v1";
const GITHUB_API_VERSION: &str = "2026-03-10";
const CLAUDE_OAUTH_BETA: &str = "oauth-2025-04-20";
/// Teto de segurança para laços de paginação; nunca devolve total parcial.
const MAX_PAGES: u32 = 1000;
/// Tamanho de página do /teams/spend.
const CURSOR_PAGE_SIZE: u32 = 200;

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::ChatGpt => chatgpt(account, ctx, CHATGPT_BASE).await,
        Provider::Claude => claude(account, ctx, CLAUDE_BASE).await,
        Provider::Cursor => cursor(account, ctx, CURSOR_BASE).await,
        Provider::Copilot => copilot(account, ctx, GITHUB_BASE).await,
        Provider::Windsurf => windsurf(account, ctx, WINDSURF_BASE).await,
        other => Err(QueryError::unsupported_params(
            "query.subscriptions.unsupported_provider",
            params([(
                "provider",
                ParamValue::from(crate::text::provider_l10n(other)),
            )]),
            format!("{} é consultado por outro adaptador.", other.label()),
        )),
    }
}

// ------------------------------------------------------- catálogo de modelos

/// Versão enviada em `client_version`: o endpoint do Codex exige o parâmetro e filtra modelos por
/// `minimal_client_version`. Acompanha uma versão pública recente do Codex CLI.
const CODEX_CLIENT_VERSION: &str = "0.163.0";

/// `GET {base}/v1/models` da Anthropic, paginado por `after_id` enquanto `has_more`. `auth` aplica a
/// credencial (chave de API, chave administrativa ou token OAuth) a cada página.
pub(crate) async fn anthropic_models(
    ctx: &Context<'_>,
    base: &str,
    auth: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> Result<Vec<ModelInfo>, QueryError> {
    #[derive(Deserialize)]
    struct Page {
        data: Vec<Entry>,
        #[serde(default)]
        has_more: bool,
        last_id: Option<String>,
    }
    #[derive(Deserialize)]
    struct Entry {
        id: String,
        display_name: Option<String>,
    }
    let url = format!("{base}/v1/models");
    paginate(|after: Option<String>| {
        let mut query = vec![("limit", "1000".to_string())];
        query.extend(after.map(|id| ("after_id", id)));
        let request = auth(ctx.http.get(&url))
            .header("anthropic-version", "2023-06-01")
            .query(&query)
            .timeout(ctx.timeout);
        async move {
            let page: Page = json(request).await?;
            // `has_more` sem cursor seria uma página incompleta: rejeita em vez de truncar.
            let next = match (page.has_more, page.last_id) {
                (false, _) => None,
                (true, Some(id)) if !id.is_empty() => Some(id),
                (true, _) => return Err(QueryError::schema()),
            };
            let models = page.data.into_iter().map(|m| ModelInfo {
                id: m.id,
                name: m.display_name,
                description: None,
            });
            Ok((models.collect(), next))
        }
    })
    .await
}

/// Modelos do Claude com o mesmo token OAuth da assinatura: `GET /v1/models` com os cabeçalhos do
/// OAuth. O contrato público é de chave de API; o uso com o token da assinatura segue o mesmo
/// caminho do Claude Code e não é documentado, podendo mudar sem aviso.
pub(crate) async fn claude_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let token = secret(account, ctx, "CLAUDE_OAUTH_TOKEN")?;
    anthropic_models(ctx, base, move |request| {
        request
            .bearer_auth(token.trim())
            .header("anthropic-beta", CLAUDE_OAUTH_BETA)
    })
    .await
}

/// Modelos do Codex com o token da sessão ChatGPT: `GET /codex/models?client_version=…`, a lista
/// que o próprio Codex CLI lê. Interface interna e não documentada. Modelos marcados como
/// ocultos (`visibility` diferente de `list`) não aparecem no seletor do Codex e ficam de fora.
pub(crate) async fn chatgpt_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    #[derive(Deserialize)]
    struct List {
        models: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        slug: String,
        display_name: Option<String>,
        description: Option<String>,
        visibility: Option<String>,
    }
    let token = secret(account, ctx, "CHATGPT_ACCESS_TOKEN")?;
    let mut request = ctx
        .http
        .get(format!("{base}/codex/models"))
        .query(&[("client_version", CODEX_CLIENT_VERSION)])
        .bearer_auth(token.trim())
        .timeout(ctx.timeout);
    if let Some(id) = account
        .params
        .get("account_id")
        .filter(|s| !s.trim().is_empty())
    {
        request = request.header("chatgpt-account-id", id.trim());
    }
    let list: List = json(request).await?;
    Ok(list
        .models
        .into_iter()
        .filter(|m| m.visibility.as_deref().is_none_or(|v| v == "list"))
        .map(|m| ModelInfo {
            id: m.slug,
            name: m.display_name,
            description: m.description,
        })
        .collect())
}

/// Rejeita valores não finitos: NaN/infinito jamais entram em métricas.
fn finite(value: f64) -> Result<f64, QueryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(QueryError::schema())
    }
}

/// Percentuais de janela não podem ser negativos.
fn percent_metric(
    label: impl Into<String>,
    used_percent: f64,
    resets_at: Option<DateTime<Utc>>,
    window: MetricWindow,
) -> Result<Metric, QueryError> {
    let used = finite(used_percent)?;
    if used < 0.0 {
        return Err(QueryError::schema());
    }
    let mut m = Metric::usage(label, used, "%", MetricKind::Quota, Some(window));
    m.limit = Some(100.0);
    m.remaining = Some(finite(100.0 - used)?);
    m.resets_at = resets_at;
    Ok(m)
}

fn epoch_seconds(ts: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(ts, 0).single()
}

fn epoch_millis(ts: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(ts).single()
}

/// RFC 3339 estrito: presente mas malformado é erro de esquema, nunca silêncio.
fn iso(value: &Option<String>) -> Result<Option<DateTime<Utc>>, QueryError> {
    match value {
        None => Ok(None),
        Some(raw) => DateTime::parse_from_rfc3339(raw)
            .map(|d| Some(d.with_timezone(&Utc)))
            .map_err(|_| QueryError::schema()),
    }
}

fn require_metrics(data: Data) -> Result<Data, QueryError> {
    if data.metrics.is_empty() {
        Err(QueryError::schema())
    } else {
        Ok(data)
    }
}

// ----------------------------------------------------------- ChatGPT / Codex

#[derive(Deserialize)]
struct WhamWindow {
    used_percent: f64,
    limit_window_seconds: Option<i64>,
    reset_at: Option<i64>,
}

#[derive(Deserialize)]
struct WhamRateLimit {
    primary_window: Option<WhamWindow>,
    secondary_window: Option<WhamWindow>,
}

#[derive(Deserialize)]
struct WhamCredits {
    has_credits: Option<bool>,
    unlimited: Option<bool>,
    balance: Option<String>,
}

#[derive(Deserialize)]
struct WhamUsage {
    plan_type: Option<String>,
    rate_limit: Option<WhamRateLimit>,
    credits: Option<WhamCredits>,
}

fn window_label(prefix: &str, seconds: Option<i64>) -> String {
    match seconds {
        Some(s) if s % 604800 == 0 => format!("{prefix} — janela de {} semana(s) (%)", s / 604800),
        Some(s) if s % 3600 == 0 => format!("{prefix} — janela de {} hora(s) (%)", s / 3600),
        Some(s) => format!("{prefix} — janela de {s}s (%)"),
        None => format!("{prefix} — janela (%)"),
    }
}

fn codex_window(seconds: Option<i64>) -> MetricWindow {
    match seconds {
        Some(18_000) => MetricWindow::Window5h,
        Some(86_400) => MetricWindow::Daily,
        Some(604_800) => MetricWindow::Weekly,
        _ => MetricWindow::Other,
    }
}

async fn chatgpt(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let token = secret(account, ctx, "CHATGPT_ACCESS_TOKEN")?;
    let mut req = ctx
        .http
        .get(format!("{base}/wham/usage"))
        .bearer_auth(token)
        .timeout(ctx.timeout);
    if let Some(id) = account
        .params
        .get("account_id")
        .filter(|s| !s.trim().is_empty())
    {
        req = req.header("chatgpt-account-id", id.trim());
    }
    let body: WhamUsage = json(req).await?;

    let mut metrics = Vec::new();
    let mut notes = Notes::new();
    notes.push(
        "note.subscriptions.chatgpt_codex.endpoint",
        "Interface interna do Codex CLI (não documentada); pode mudar sem aviso.",
    );
    notes.push(
        "note.subscriptions.chatgpt_codex.coverage",
        "Cobre as janelas do Codex/agente; a cota de mensagens do chat web do ChatGPT não é exposta por endpoint legível.",
    );
    if let Some(plan) = body.plan_type {
        notes.push_params(
            "note.subscriptions.chatgpt_codex.plan",
            params([("plan", ParamValue::from(plan.clone()))]),
            format!("Plano: {plan}"),
        );
    }
    if let Some(rl) = body.rate_limit {
        if let Some(w) = rl.primary_window {
            metrics.push(percent_metric(
                window_label("Codex", w.limit_window_seconds),
                w.used_percent,
                w.reset_at.and_then(epoch_seconds),
                codex_window(w.limit_window_seconds),
            )?);
        }
        if let Some(w) = rl.secondary_window {
            metrics.push(percent_metric(
                window_label("Codex", w.limit_window_seconds),
                w.used_percent,
                w.reset_at.and_then(epoch_seconds),
                codex_window(w.limit_window_seconds),
            )?);
        }
    }
    if let Some(credits) = body.credits {
        if credits.unlimited == Some(true) {
            notes.push(
                "note.subscriptions.chatgpt_codex.unlimited_credits",
                "Créditos Codex ilimitados.",
            );
        } else if let Some(balance) = credits.balance {
            let value: f64 = balance.trim().parse().map_err(|_| QueryError::schema())?;
            let mut m = Metric::usage(
                "Créditos Codex — saldo restante",
                0.0,
                "créditos",
                MetricKind::Credits,
                None,
            );
            // Saldo é o que RESTA: used fica desconhecido, remaining carrega o valor.
            m.used = None;
            m.remaining = Some(finite(value)?);
            metrics.push(m);
        } else if credits.has_credits == Some(false) {
            notes.push(
                "note.subscriptions.chatgpt_codex.no_extra_credits",
                "Sem créditos Codex adicionais.",
            );
        }
    }
    require_metrics(Data::new(
        metrics,
        notes,
        "https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs",
    ))
}

// ------------------------------------------------------------------- Claude

#[derive(Deserialize)]
struct ClaudeWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeExtraUsage {
    is_enabled: Option<bool>,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
}

#[derive(Deserialize)]
struct ClaudeUsage {
    five_hour: Option<ClaudeWindow>,
    seven_day: Option<ClaudeWindow>,
    seven_day_opus: Option<ClaudeWindow>,
    seven_day_sonnet: Option<ClaudeWindow>,
    extra_usage: Option<ClaudeExtraUsage>,
}

fn claude_window(
    metrics: &mut Vec<Metric>,
    label: &str,
    window: &Option<ClaudeWindow>,
    metric_window: MetricWindow,
    model: Option<&str>,
) -> Result<(), QueryError> {
    if let Some(w) = window {
        if let Some(u) = w.utilization {
            let mut metric =
                percent_metric(format!("{label} (%)"), u, iso(&w.resets_at)?, metric_window)?;
            // Per-model weekly limits only bind their own model.
            metric.scope = model.map(|name| Scope::model(name.to_lowercase(), name));
            metrics.push(metric);
        }
    }
    Ok(())
}

async fn claude(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let token = secret(account, ctx, "CLAUDE_OAUTH_TOKEN")?;
    let body: ClaudeUsage = json(
        ctx.http
            .get(format!("{base}/api/oauth/usage"))
            .bearer_auth(token)
            .header("anthropic-beta", CLAUDE_OAUTH_BETA)
            .header("anthropic-version", "2023-06-01")
            .timeout(ctx.timeout),
    )
    .await?;

    let mut metrics = Vec::new();
    claude_window(
        &mut metrics,
        "Sessão (5 horas)",
        &body.five_hour,
        MetricWindow::Window5h,
        None,
    )?;
    claude_window(
        &mut metrics,
        "Semana (7 dias)",
        &body.seven_day,
        MetricWindow::Weekly,
        None,
    )?;
    claude_window(
        &mut metrics,
        "Semana — Opus (7 dias)",
        &body.seven_day_opus,
        MetricWindow::Weekly,
        Some("Opus"),
    )?;
    claude_window(
        &mut metrics,
        "Semana — Sonnet (7 dias)",
        &body.seven_day_sonnet,
        MetricWindow::Weekly,
        Some("Sonnet"),
    )?;
    if let Some(extra) = body.extra_usage {
        if extra.is_enabled == Some(true) {
            if let Some(used) = extra.used_credits {
                // Unidade exata (créditos vs. centavos) não é documentada: rotulamos como
                // unidades brutas do provedor.
                let mut m = Metric::usage(
                    "Uso extra — utilizado",
                    finite(used)?,
                    "unid. provedor",
                    MetricKind::Quota,
                    Some(MetricWindow::Monthly),
                );
                if let Some(limit) = extra.monthly_limit {
                    m.limit = Some(finite(limit)?);
                    // `used` keeps the provider's value; remaining never goes negative.
                    m.remaining = Some(finite(limit - used)?.max(0.0));
                }
                m.period = Some("mês corrente".into());
                // Paid extra usage, outside the plan limits.
                m.billing = Billing::Overage;
                metrics.push(m);
            }
        }
    }
    let mut notes = Notes::new();
    notes.push(
        "note.subscriptions.anthropic_session.endpoint",
        "Endpoint no host oficial api.anthropic.com, evidenciado por clientes de código aberto da comunidade; sem documentação oficial, pode mudar sem aviso.",
    );
    notes.push(
        "note.subscriptions.anthropic_session.coverage",
        "Utilização em percentual da janela; limites absolutos não são divulgados.",
    );
    require_metrics(Data::new(
        metrics,
        notes,
        "https://api.anthropic.com/api/oauth/usage (evidência comunitária: github.com/loocor/codmate)",
    ))
}

// ------------------------------------------------------------------- Cursor

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpendMember {
    spend_cents: f64,
    overall_spend_cents: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpendResponse {
    team_member_spend: Vec<SpendMember>,
    total_pages: u32,
    total_members: Option<u32>,
    subscription_cycle_start: Option<i64>,
}

async fn cursor(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "CURSOR_ADMIN_API_KEY")?;
    let mut page = 1u32;
    let mut expected_pages: Option<u32> = None;
    let mut members_seen = 0u32;
    let mut spend_cents = 0.0f64;
    let mut overall_cents = 0.0f64;
    let mut overall_known = true;
    let mut cycle_start = None;
    let mut total_members = None;
    loop {
        let response: SpendResponse = json(
            ctx.http
                .post(format!("{base}/teams/spend"))
                .basic_auth(&key, Some(""))
                .json(&serde_json::json!({ "page": page, "pageSize": CURSOR_PAGE_SIZE }))
                .timeout(ctx.timeout),
        )
        .await?;
        if response.total_pages > MAX_PAGES {
            return Err(QueryError::schema());
        }
        // O total de páginas deve ser estável entre páginas; caso contrário o
        // conjunto mudou durante a leitura e o total seria parcial.
        match expected_pages {
            None => expected_pages = Some(response.total_pages),
            Some(expected) if expected != response.total_pages => return Err(QueryError::schema()),
            _ => {}
        }
        members_seen +=
            u32::try_from(response.team_member_spend.len()).map_err(|_| QueryError::schema())?;
        for member in &response.team_member_spend {
            spend_cents = finite(spend_cents + finite(member.spend_cents)?)?;
            match member.overall_spend_cents {
                Some(overall) => overall_cents = finite(overall_cents + finite(overall)?)?,
                None => overall_known = false,
            }
        }
        cycle_start = cycle_start.or(response.subscription_cycle_start);
        total_members = total_members.or(response.total_members);
        if page >= response.total_pages {
            break;
        }
        page += 1;
    }
    // Com totalMembers oficial, a contagem de membros lidos precisa conferir.
    if let Some(total) = total_members {
        if total != members_seen {
            return Err(QueryError::schema());
        }
    }
    let mut notes = Notes::new();
    notes.push(
        "note.subscriptions.cursor_admin.endpoint",
        "Admin API oficial; requer chave de administrador de equipe (planos Teams/Enterprise).",
    );
    if let Some(start) = cycle_start.and_then(epoch_millis) {
        let start = start.to_rfc3339();
        notes.push_params(
            "note.subscriptions.cursor_admin.cycle_start",
            params([("start", ParamValue::from(start.clone()))]),
            format!("Ciclo de cobrança iniciado em {start}."),
        );
    }
    let mut demanda = Metric::usage(
        "Gasto sob demanda no ciclo",
        finite(spend_cents / 100.0)?,
        "USD",
        MetricKind::Spend,
        Some(MetricWindow::Monthly),
    );
    demanda.period = Some("ciclo de cobrança".into());
    let mut total = Metric::usage(
        "Gasto total no ciclo (inclui uso incluído)",
        0.0,
        "USD",
        MetricKind::Spend,
        Some(MetricWindow::Monthly),
    );
    total.used = if overall_known {
        Some(finite(overall_cents / 100.0)?)
    } else {
        notes.push(
            "note.subscriptions.cursor_admin.unknown_total",
            "Gasto total desconhecido: overallSpendCents ausente em algum membro.",
        );
        None
    };
    total.period = Some("ciclo de cobrança".into());
    let mut metrics = vec![demanda, total];
    if let Some(members) = total_members {
        metrics.push(Metric::usage(
            "Membros na equipe",
            f64::from(members),
            "membros",
            MetricKind::Usage,
            None,
        ));
    }
    require_metrics(Data::new(
        metrics,
        notes,
        "https://cursor.com/docs/account/teams/admin-api.md#get-spending-data",
    ))
}

// ------------------------------------------------------------ GitHub Copilot

#[derive(Deserialize)]
struct GitHubUser {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingUsageItem {
    product: String,
    unit_type: String,
    gross_quantity: f64,
    net_amount: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingUsageReport {
    usage_items: Vec<BillingUsageItem>,
}

/// Slugs de usuário/organização: somente caracteres seguros em path de URL fixa.
fn slug(value: &str) -> Result<&str, QueryError> {
    if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Ok(value)
    } else {
        Err(QueryError::config(
            "query.subscriptions.invalid_slug",
            "Identificador de usuário/organização inválido.",
        ))
    }
}

async fn copilot(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let token = secret(account, ctx, "GITHUB_TOKEN")?;
    let get = |url: String| {
        ctx.http
            .get(url)
            .bearer_auth(&token)
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", GITHUB_API_VERSION)
            .header("user-agent", "cuotai")
            .timeout(ctx.timeout)
    };
    let (product_path, usage_label, cost_label) =
        match account.params.get("billing").map(String::as_str) {
            None | Some("premium_requests") => (
                "premium_request",
                "Solicitações premium utilizadas (mês corrente)",
                "Custo líquido de solicitações premium (mês corrente)",
            ),
            Some("ai_credits") => (
                "ai_credit",
                "Créditos de IA utilizados (mês corrente)",
                "Custo líquido de créditos de IA (mês corrente)",
            ),
            Some(_) => {
                return Err(QueryError::config(
                    "query.subscriptions.copilot_billing",
                    "params.billing deve ser \"premium_requests\" ou \"ai_credits\"",
                ));
            }
        };
    let now = Utc::now();
    let (year, month) = (now.format("%Y").to_string(), now.format("%m").to_string());
    let mut notes = Notes::new();
    let scope = match account.params.get("mode").map(String::as_str) {
        None | Some("user") => {
            let me: GitHubUser = json(get(format!("{base}/user"))).await?;
            let login = slug(&me.login).map_err(|_| QueryError::schema())?;
            notes.push(
                "note.subscriptions.copilot_user.personal_scope",
                "Cobrança pessoal do usuário autenticado.",
            );
            format!("{base}/users/{login}/settings/billing/{product_path}/usage")
        }
        Some("org") => {
            let org = slug(param(account, "org")?)?;
            notes.push(
                "note.subscriptions.copilot_user.organization_scope",
                "Cobrança da organização (requer admin da organização).",
            );
            format!("{base}/organizations/{org}/settings/billing/{product_path}/usage")
        }
        Some(_) => {
            return Err(QueryError::config(
                "query.subscriptions.copilot_mode",
                "params.mode deve ser \"user\" ou \"org\"",
            ));
        }
    };
    let report: BillingUsageReport =
        json(get(scope).query(&[("year", year.as_str()), ("month", month.as_str())])).await?;

    // grossQuantity = uso bruto antes de descontos; netAmount = custo após descontos.
    let mut used = 0.0f64;
    let mut amount = 0.0f64;
    let mut unit: Option<&str> = None;
    for item in report.usage_items.iter().filter(|i| i.product == "Copilot") {
        finite(item.gross_quantity)?;
        finite(item.net_amount)?;
        match unit {
            None => unit = Some(item.unit_type.as_str()),
            Some(u) if u != item.unit_type => return Err(QueryError::schema()),
            _ => {}
        }
        used = finite(used + item.gross_quantity)?;
        amount = finite(amount + item.net_amount)?;
    }
    let unit = unit.unwrap_or("unid.").to_string();
    let mut usage = Metric::usage(
        usage_label,
        used,
        unit,
        MetricKind::Usage,
        Some(MetricWindow::Monthly),
    );
    usage.period = Some(format!("{year}-{month}"));
    let mut cost = Metric::usage(
        cost_label,
        amount,
        "USD",
        MetricKind::Spend,
        Some(MetricWindow::Monthly),
    );
    cost.period = Some(format!("{year}-{month}"));
    notes.push(
        "note.subscriptions.copilot_user.coverage",
        "API oficial de cobrança do GitHub; o limite mensal incluído no plano não é exposto pela API.",
    );
    Ok(Data::new(
        vec![usage, cost],
        notes,
        "https://docs.github.com/en/rest/billing/usage",
    ))
}

// ----------------------------------------------------------------- Windsurf

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreditBalance {
    prompt_credits_per_seat: f64,
    num_seats: f64,
    add_on_credits_available: f64,
    add_on_credits_used: f64,
    billing_cycle_start: Option<String>,
    billing_cycle_end: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserStats {
    prompt_credits_used: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPageAnalytics {
    user_table_stats: Vec<UserStats>,
}

async fn windsurf(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "WINDSURF_SERVICE_KEY")?;
    let balance: CreditBalance = json(
        ctx.http
            .post(format!("{base}/GetTeamCreditBalance"))
            .json(&serde_json::json!({ "service_key": key }))
            .timeout(ctx.timeout),
    )
    .await?;

    let per_seat = finite(balance.prompt_credits_per_seat)?;
    let seats = finite(balance.num_seats)?;
    let add_available = finite(balance.add_on_credits_available)?;
    let add_used = finite(balance.add_on_credits_used)?;
    // Janela malformada é erro de esquema: nunca consultamos com strings inválidas.
    let cycle_start = iso(&balance.billing_cycle_start)?;
    let cycle_end = iso(&balance.billing_cycle_end)?;
    if matches!((cycle_start, cycle_end), (Some(start), Some(end)) if start >= end) {
        return Err(QueryError::schema());
    }

    let mut metrics = Vec::new();
    let mut notes = Notes::new();
    notes.push(
        "note.subscriptions.windsurf.endpoint",
        "API oficial Enterprise (chave de serviço com Billing Read / Teams Read-Only); planos sem acesso recebem erro de autenticação.",
    );
    notes.push_params(
        "note.subscriptions.windsurf.allocation",
        params([
            ("per_seat", ParamValue::from(per_seat.to_string())),
            ("seats", ParamValue::from(seats.to_string())),
        ]),
        format!("Alocação de créditos de prompt: {per_seat} por assento × {seats} assentos; é uma alocação da equipe, não uma cota rígida por usuário."),
    );

    // Soma de créditos por usuário no ciclo corrente (UserPageAnalytics não é paginado;
    // promptCreditsUsed vem em centésimos de crédito e é obrigatório por usuário).
    if let (Some(start), Some(end)) = (&balance.billing_cycle_start, &balance.billing_cycle_end) {
        let analytics: UserPageAnalytics = json(
            ctx.http
                .post(format!("{base}/UserPageAnalytics"))
                .json(&serde_json::json!({
                    "service_key": key,
                    "start_timestamp": start,
                    "end_timestamp": end,
                }))
                .timeout(ctx.timeout),
        )
        .await?;
        let mut used = 0.0f64;
        for user in &analytics.user_table_stats {
            used = finite(used + finite(user.prompt_credits_used)?)?;
        }
        let used = finite(used / 100.0)?;
        let mut m = Metric::usage(
            "Créditos de prompt usados no ciclo",
            used,
            "créditos",
            MetricKind::Credits,
            Some(MetricWindow::Monthly),
        );
        m.period = Some("ciclo de cobrança".into());
        m.resets_at = cycle_end;
        metrics.push(m);
    } else {
        notes.push(
            "note.subscriptions.windsurf.missing_cycle",
            "Período do ciclo ausente na resposta; total de créditos de prompt não calculado.",
        );
    }
    metrics.push(Metric {
        billing: Default::default(),
        scope: None,
        label: "Alocação de prompt da equipe (por assento × assentos)".into(),
        used: None,
        limit: Some(finite(per_seat * seats)?),
        remaining: None,
        unit: "créditos".into(),
        kind: MetricKind::Credits,
        window: Some(MetricWindow::Monthly),
        period: Some("ciclo de cobrança".into()),
        resets_at: cycle_end,
    });

    // addOnCreditsAvailable é a alocação do ciclo (recalculada no início de cada ciclo),
    // não necessariamente o restante em tempo real.
    let mut add = Metric::usage(
        "Créditos adicionais usados no ciclo",
        add_used,
        "créditos",
        MetricKind::Credits,
        Some(MetricWindow::Monthly),
    );
    add.limit = Some(add_available);
    add.period = Some("ciclo de cobrança".into());
    add.resets_at = cycle_end;
    metrics.push(add);
    metrics.push(Metric::usage(
        "Assentos",
        seats,
        "membros",
        MetricKind::Usage,
        None,
    ));
    notes.push(
        "note.subscriptions.windsurf.balance_semantics",
        "Uso de prompt pode incluir créditos adicionais; não inferimos saldo do conjunto. addOnCreditsAvailable é a alocação informada para o ciclo, não um saldo vitalício; o restante permanece desconhecido.",
    );

    require_metrics(Data::new(
        metrics,
        notes,
        "https://docs.windsurf.com/windsurf/accounts/api-reference/api-introduction",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Status};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct TestCredentials;
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_TOKEN").then(|| "test-secret".to_string())
        }
    }

    fn account(provider: Provider) -> Account {
        Account {
            id: None,
            name: "conta".to_string(),
            provider,
            credential_env: Some("TEST_TOKEN".to_string()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
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

    // ------------------------------------------------------- ChatGPT / Codex

    fn wham_body() -> serde_json::Value {
        serde_json::json!({
            "plan_type": "plus",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": { "used_percent": 42, "limit_window_seconds": 18000, "reset_after_seconds": 3600, "reset_at": 1790000000 },
                "secondary_window": { "used_percent": 10, "limit_window_seconds": 604800, "reset_after_seconds": 86400, "reset_at": 1790500000 }
            },
            "credits": { "has_credits": true, "unlimited": false, "balance": "12.5" }
        })
    }

    #[tokio::test]
    async fn chatgpt_mapeia_janelas_e_saldo_do_codex() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wham/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(wham_body()))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = chatgpt(
            &account(Provider::ChatGpt),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap();

        let primaria = metric(&data, "Codex — janela de 5 hora(s) (%)");
        assert_eq!(primaria.used, Some(42.0));
        assert_eq!(primaria.limit, Some(100.0));
        assert_eq!(primaria.remaining, Some(58.0));
        assert_eq!(primaria.resets_at, epoch_seconds(1790000000));
        assert_eq!(primaria.kind, MetricKind::Quota);
        assert_eq!(primaria.window, Some(MetricWindow::Window5h));
        let semanal = metric(&data, "Codex — janela de 1 semana(s) (%)");
        assert_eq!(semanal.used, Some(10.0));
        assert_eq!(semanal.kind, MetricKind::Quota);
        assert_eq!(semanal.window, Some(MetricWindow::Weekly));
        let saldo = metric(&data, "Créditos Codex — saldo restante");
        assert_eq!(saldo.used, None);
        assert_eq!(saldo.remaining, Some(12.5));
        assert_eq!(saldo.kind, MetricKind::Credits);
        assert_eq!(saldo.window, None);
    }

    #[tokio::test]
    async fn chatgpt_sem_janelas_e_erro_de_esquema() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wham/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "plan_type": "free", "rate_limit": null, "credits": null
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = chatgpt(
            &account(Provider::ChatGpt),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn chatgpt_percentual_negativo_e_erro() {
        let server = MockServer::start().await;
        let mut body = wham_body();
        body["rate_limit"]["primary_window"]["used_percent"] = serde_json::json!(-5.0);
        Mock::given(method("GET"))
            .and(path("/wham/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = chatgpt(
            &account(Provider::ChatGpt),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    // ------------------------------------------------------------- Claude

    #[tokio::test]
    async fn claude_mapeia_janelas_e_uso_extra() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "five_hour": { "utilization": 25.5, "resets_at": "2026-09-28T12:00:00Z" },
                "seven_day": { "utilization": 3.0, "resets_at": "2026-10-01T00:00:00Z" },
                "extra_usage": { "is_enabled": true, "monthly_limit": 50.0, "used_credits": 12.5 }
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = claude(&account(Provider::Claude), &context(&client), &server.uri())
            .await
            .unwrap();

        let sessao = metric(&data, "Sessão (5 horas) (%)");
        assert_eq!(sessao.used, Some(25.5));
        assert_eq!(sessao.remaining, Some(74.5));
        assert_eq!(sessao.kind, MetricKind::Quota);
        assert_eq!(sessao.window, Some(MetricWindow::Window5h));
        let semana = metric(&data, "Semana (7 dias) (%)");
        assert_eq!(semana.kind, MetricKind::Quota);
        assert_eq!(semana.window, Some(MetricWindow::Weekly));
        assert_eq!(
            sessao.resets_at,
            iso(&Some("2026-09-28T12:00:00Z".to_string())).unwrap()
        );
        let extra = metric(&data, "Uso extra — utilizado");
        assert_eq!(extra.used, Some(12.5));
        assert_eq!(extra.limit, Some(50.0));
        assert_eq!(extra.remaining, Some(37.5));
        assert_eq!(extra.unit, "unid. provedor");
        assert_eq!(extra.kind, MetricKind::Quota);
        assert_eq!(extra.window, Some(MetricWindow::Monthly));
        assert_eq!(extra.billing, Billing::Overage);
        assert_eq!(sessao.billing, Billing::Included);
        assert!(sessao.scope.is_none());
    }

    async fn claude_extra(extra: serde_json::Value) -> Vec<Metric> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "five_hour": { "utilization": 60.0, "resets_at": "2026-09-28T12:00:00Z" },
                "seven_day_opus": { "utilization": 10.0, "resets_at": "2026-10-01T00:00:00Z" },
                "extra_usage": extra
            })))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        claude(&account(Provider::Claude), &context(&client), &server.uri())
            .await
            .unwrap()
            .metrics
    }

    #[tokio::test]
    async fn claude_extra_usage_variants_never_invent_zero() {
        // Disabled, absent flag and a missing amount emit no extra metric.
        for extra in [
            serde_json::json!({ "is_enabled": false, "monthly_limit": 50.0, "used_credits": 5.0 }),
            serde_json::json!({ "monthly_limit": 50.0, "used_credits": 5.0 }),
            serde_json::json!({ "is_enabled": true, "monthly_limit": 50.0 }),
            serde_json::json!({ "is_enabled": true, "monthly_limit": null, "used_credits": null }),
            serde_json::Value::Null,
        ] {
            let metrics = claude_extra(extra).await;
            assert!(
                metrics.iter().all(|m| m.billing == Billing::Included),
                "{metrics:?}"
            );
        }
        // Enabled with nothing spent: a real zero, still overage.
        let armed = claude_extra(
            serde_json::json!({ "is_enabled": true, "monthly_limit": 50.0, "used_credits": 0.0 }),
        )
        .await;
        let extra = armed
            .iter()
            .find(|m| m.billing == Billing::Overage)
            .unwrap();
        assert_eq!(extra.used, Some(0.0));
        assert_eq!(extra.remaining, Some(50.0));
        // Enabled without a cap: no limit and no remaining.
        let uncapped =
            claude_extra(serde_json::json!({ "is_enabled": true, "used_credits": 3.0 })).await;
        let extra = uncapped
            .iter()
            .find(|m| m.billing == Billing::Overage)
            .unwrap();
        assert_eq!((extra.limit, extra.remaining), (None, None));
    }

    #[tokio::test]
    async fn claude_extra_past_cap_keeps_used_and_clamps_remaining() {
        let metrics = claude_extra(
            serde_json::json!({ "is_enabled": true, "monthly_limit": 50.0, "used_credits": 55.0 }),
        )
        .await;
        let extra = metrics
            .iter()
            .find(|m| m.billing == Billing::Overage)
            .unwrap();
        assert_eq!(extra.used, Some(55.0));
        assert_eq!(extra.remaining, Some(0.0));
        // The Opus weekly limit is scoped to its model.
        let opus = metrics
            .iter()
            .find(|m| m.label.starts_with("Semana — Opus"))
            .unwrap();
        assert_eq!(opus.scope.as_ref().map(|s| s.id.as_str()), Some("opus"));
        // Regression: the extra meter is not the binding limit, has no pace
        // and raises no threshold alert; the 5-hour window stays binding.
        let report = crate::Report {
            account_id: None,
            account: "a".into(),
            provider: Provider::Claude,
            checked_at: Utc::now(),
            status: crate::Status::Ready,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
        };
        let binding = crate::metrics::binding(&report).unwrap();
        assert_eq!(binding.label, "Sessão (5 horas) (%)");
        assert_eq!(
            crate::metrics::classify_report(Some(&report)),
            crate::metrics::AccountClass::Available
        );
    }

    #[tokio::test]
    async fn claude_utilizacao_nao_finita_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"five_hour":{"utilization":1e400,"resets_at":null}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = claude(&account(Provider::Claude), &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    // ------------------------------------------------------------- Cursor

    #[tokio::test]
    async fn cursor_soma_todas_as_paginas() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/teams/spend"))
            .and(body_json(
                serde_json::json!({ "page": 1, "pageSize": CURSOR_PAGE_SIZE }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "teamMemberSpend": [{ "spendCents": 2450.5, "overallSpendCents": 3000.0 }],
                "totalPages": 2,
                "totalMembers": 2,
                "subscriptionCycleStart": 1790000000000i64,
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/teams/spend"))
            .and(body_json(
                serde_json::json!({ "page": 2, "pageSize": CURSOR_PAGE_SIZE }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "teamMemberSpend": [{ "spendCents": 49.5, "overallSpendCents": 100.0 }],
                "totalPages": 2,
                "totalMembers": 2,
                "subscriptionCycleStart": 1790000000000i64,
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = cursor(&account(Provider::Cursor), &context(&client), &server.uri())
            .await
            .unwrap();

        assert_eq!(metric(&data, "Gasto sob demanda no ciclo").used, Some(25.0));
        assert_eq!(
            metric(&data, "Gasto total no ciclo (inclui uso incluído)").used,
            Some(31.0)
        );
        assert_eq!(metric(&data, "Membros na equipe").used, Some(2.0));
        for label in [
            "Gasto sob demanda no ciclo",
            "Gasto total no ciclo (inclui uso incluído)",
        ] {
            assert_eq!(metric(&data, label).kind, MetricKind::Spend);
            assert_eq!(metric(&data, label).window, Some(MetricWindow::Monthly));
        }
        assert_eq!(metric(&data, "Membros na equipe").kind, MetricKind::Usage);
        assert_eq!(metric(&data, "Membros na equipe").window, None);
    }

    #[tokio::test]
    async fn cursor_overall_ausente_torna_total_desconhecido() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/teams/spend"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "teamMemberSpend": [
                    { "spendCents": 100.0, "overallSpendCents": 100.0 },
                    { "spendCents": 100.0 }
                ],
                "totalPages": 1,
                "totalMembers": 2,
                "subscriptionCycleStart": 1790000000000i64,
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = cursor(&account(Provider::Cursor), &context(&client), &server.uri())
            .await
            .unwrap();

        assert_eq!(metric(&data, "Gasto sob demanda no ciclo").used, Some(2.0));
        assert_eq!(
            metric(&data, "Gasto total no ciclo (inclui uso incluído)").used,
            None
        );
    }

    #[tokio::test]
    async fn cursor_total_de_paginas_instavel_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/teams/spend"))
            .and(body_json(
                serde_json::json!({ "page": 1, "pageSize": CURSOR_PAGE_SIZE }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "teamMemberSpend": [{ "spendCents": 1.0, "overallSpendCents": 1.0 }],
                "totalPages": 2,
                "totalMembers": 2,
                "subscriptionCycleStart": 1790000000000i64,
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/teams/spend"))
            .and(body_json(
                serde_json::json!({ "page": 2, "pageSize": CURSOR_PAGE_SIZE }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "teamMemberSpend": [{ "spendCents": 1.0, "overallSpendCents": 1.0 }],
                "totalPages": 3,
                "totalMembers": 2,
                "subscriptionCycleStart": 1790000000000i64,
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = cursor(&account(Provider::Cursor), &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn cursor_contagem_de_membros_inconsistente_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/teams/spend"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "teamMemberSpend": [{ "spendCents": 1.0, "overallSpendCents": 1.0 }],
                "totalPages": 1,
                "totalMembers": 5,
                "subscriptionCycleStart": 1790000000000i64,
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = cursor(&account(Provider::Cursor), &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    // ------------------------------------------------------ GitHub Copilot

    fn premium_report() -> serde_json::Value {
        serde_json::json!({
            "timePeriod": { "year": 2026, "month": 9 },
            "usageItems": [
                { "product": "Copilot", "sku": "copilot_premium_request", "model": "gpt-5",
                  "unitType": "requests", "pricePerUnit": 0.04,
                  "grossQuantity": 120.0, "grossAmount": 4.8,
                  "discountQuantity": 20.0, "discountAmount": 0.8,
                  "netQuantity": 100.0, "netAmount": 4.0 },
                { "product": "Copilot", "sku": "copilot_premium_request", "model": "claude-sonnet",
                  "unitType": "requests", "pricePerUnit": 0.04,
                  "grossQuantity": 55.0, "grossAmount": 2.2,
                  "discountQuantity": 5.0, "discountAmount": 0.2,
                  "netQuantity": 50.0, "netAmount": 2.0 },
                { "product": "Actions", "sku": "actions_linux", "model": "",
                  "unitType": "minutes", "pricePerUnit": 0.008,
                  "grossQuantity": 10.0, "grossAmount": 0.08,
                  "discountQuantity": 0.0, "discountAmount": 0.0,
                  "netQuantity": 10.0, "netAmount": 0.08 }
            ]
        })
    }

    #[tokio::test]
    async fn copilot_usuario_soma_uso_bruto_e_custo_liquido() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "octocat"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/users/octocat/settings/billing/premium_request/usage",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(premium_report()))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = copilot(
            &account(Provider::Copilot),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap();

        let uso = metric(&data, "Solicitações premium utilizadas (mês corrente)");
        assert_eq!(uso.used, Some(175.0));
        assert_eq!(uso.unit, "requests");
        assert_eq!(uso.kind, MetricKind::Usage);
        assert_eq!(uso.window, Some(MetricWindow::Monthly));
        assert_eq!(
            metric(
                &data,
                "Custo líquido de solicitações premium (mês corrente)"
            )
            .used,
            Some(6.0)
        );
        let custo = metric(
            &data,
            "Custo líquido de solicitações premium (mês corrente)",
        );
        assert_eq!(custo.kind, MetricKind::Spend);
        assert_eq!(custo.window, Some(MetricWindow::Monthly));
    }

    #[tokio::test]
    async fn copilot_modo_org_usa_endpoint_da_organizacao() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/organizations/minha-org/settings/billing/premium_request/usage",
            ))
            .and(query_param("year", "2026"))
            .respond_with(ResponseTemplate::new(200).set_body_json(premium_report()))
            .mount(&server)
            .await;

        let mut acc = account(Provider::Copilot);
        acc.params.insert("mode".into(), "org".into());
        acc.params.insert("org".into(), "minha-org".into());
        let client = reqwest::Client::new();
        let data = copilot(&acc, &context(&client), &server.uri())
            .await
            .unwrap();
        assert_eq!(
            metric(&data, "Solicitações premium utilizadas (mês corrente)").used,
            Some(175.0)
        );
    }

    #[tokio::test]
    async fn copilot_ai_credits_usa_endpoint_de_creditos() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "octocat"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/users/octocat/settings/billing/ai_credit/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "timePeriod": { "year": 2026, "month": 9 },
                "usageItems": [
                    { "product": "Copilot", "sku": "copilot_ai_credit", "model": "gpt-5",
                      "unitType": "Copilot Credits", "pricePerUnit": 0.01,
                      "grossQuantity": 300.0, "grossAmount": 3.0,
                      "discountQuantity": 0.0, "discountAmount": 0.0,
                      "netQuantity": 300.0, "netAmount": 3.0 }
                ]
            })))
            .mount(&server)
            .await;

        let mut acc = account(Provider::Copilot);
        acc.params.insert("billing".into(), "ai_credits".into());
        let client = reqwest::Client::new();
        let data = copilot(&acc, &context(&client), &server.uri())
            .await
            .unwrap();
        let uso = metric(&data, "Créditos de IA utilizados (mês corrente)");
        assert_eq!(uso.used, Some(300.0));
        assert_eq!(uso.unit, "Copilot Credits");
    }

    #[tokio::test]
    async fn copilot_slug_invalido_requer_configuracao() {
        let server = MockServer::start().await;
        let mut acc = account(Provider::Copilot);
        acc.params.insert("mode".into(), "org".into());
        acc.params.insert("org".into(), "../evil".into());
        let client = reqwest::Client::new();
        let err = copilot(&acc, &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
    }

    #[tokio::test]
    async fn copilot_modo_org_sem_org_requer_configuracao() {
        let server = MockServer::start().await;
        let mut acc = account(Provider::Copilot);
        acc.params.insert("mode".into(), "org".into());
        let client = reqwest::Client::new();
        let err = copilot(&acc, &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("org"));
    }

    // ------------------------------------------------------------ Windsurf

    #[tokio::test]
    async fn windsurf_combina_saldo_e_uso_do_ciclo() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/GetTeamCreditBalance"))
            .and(body_json(
                serde_json::json!({ "service_key": "test-secret" }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "promptCreditsPerSeat": 500,
                "numSeats": 4,
                "addOnCreditsAvailable": 6500,
                "addOnCreditsUsed": 3500,
                "billingCycleStart": "2026-09-01T00:00:00Z",
                "billingCycleEnd": "2026-10-01T00:00:00Z"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/UserPageAnalytics"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "userTableStats": [
                    { "promptCreditsUsed": 12500 },
                    { "promptCreditsUsed": 7500 },
                    { "promptCreditsUsed": 0 }
                ],
                "billingCycleStart": "2026-09-01T00:00:00Z",
                "billingCycleEnd": "2026-10-01T00:00:00Z"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = windsurf(
            &account(Provider::Windsurf),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap();

        let creditos = metric(&data, "Créditos de prompt usados no ciclo");
        assert_eq!(creditos.used, Some(200.0));
        assert_eq!(creditos.limit, None);
        assert_eq!(creditos.remaining, None);
        assert_eq!(creditos.kind, MetricKind::Credits);
        assert_eq!(creditos.window, Some(MetricWindow::Monthly));
        assert_eq!(
            creditos.resets_at,
            iso(&Some("2026-10-01T00:00:00Z".to_string())).unwrap()
        );
        let adicionais = metric(&data, "Créditos adicionais usados no ciclo");
        assert_eq!(adicionais.used, Some(3500.0));
        assert_eq!(adicionais.limit, Some(6500.0));
        assert_eq!(adicionais.remaining, None);
        assert_eq!(adicionais.kind, MetricKind::Credits);
        assert_eq!(adicionais.window, Some(MetricWindow::Monthly));
        let alocacao = metric(
            &data,
            "Alocação de prompt da equipe (por assento × assentos)",
        );
        assert_eq!(alocacao.kind, MetricKind::Credits);
        assert_eq!(alocacao.window, Some(MetricWindow::Monthly));
        assert_eq!(metric(&data, "Assentos").kind, MetricKind::Usage);
        assert_eq!(metric(&data, "Assentos").window, None);
    }

    #[tokio::test]
    async fn windsurf_creditos_ausentes_em_usuario_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/GetTeamCreditBalance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "promptCreditsPerSeat": 500,
                "numSeats": 1,
                "addOnCreditsAvailable": 0,
                "addOnCreditsUsed": 0,
                "billingCycleStart": "2026-09-01T00:00:00Z",
                "billingCycleEnd": "2026-10-01T00:00:00Z"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/UserPageAnalytics"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "userTableStats": [{}],
                "billingCycleStart": "2026-09-01T00:00:00Z",
                "billingCycleEnd": "2026-10-01T00:00:00Z"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = windsurf(
            &account(Provider::Windsurf),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn windsurf_janela_malformada_e_erro() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/GetTeamCreditBalance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "promptCreditsPerSeat": 500,
                "numSeats": 1,
                "addOnCreditsAvailable": 0,
                "addOnCreditsUsed": 0,
                "billingCycleStart": "não-é-data",
                "billingCycleEnd": "2026-10-01T00:00:00Z"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = windsurf(
            &account(Provider::Windsurf),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    // --------------------------------------------------------------- Geral

    #[tokio::test]
    async fn credencial_ausente_requer_configuracao() {
        let server = MockServer::start().await;
        let mut acc = account(Provider::ChatGpt);
        acc.credential_env = Some("INEXISTENTE".to_string());
        let client = reqwest::Client::new();
        let err = chatgpt(&acc, &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        assert!(err.note.fallback.contains("INEXISTENTE"));
    }

    // ------------------------------------------------ catálogo de modelos

    #[tokio::test]
    async fn claude_models_follow_after_id_with_oauth_headers() {
        use wiremock::matchers::header;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer test-secret"))
            .and(header("anthropic-beta", CLAUDE_OAUTH_BETA))
            .and(header("anthropic-version", "2023-06-01"))
            .and(query_param("limit", "1000"))
            .and(query_param("after_id", "claude-b"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "claude-a", "display_name": "Claude A", "type": "model" }],
                "has_more": false, "first_id": "claude-a", "last_id": "claude-a"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "claude-b", "display_name": "Claude B", "type": "model" }],
                "has_more": true, "first_id": "claude-b", "last_id": "claude-b"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let models = claude_models(&account(Provider::Claude), &context(&client), &server.uri())
            .await
            .unwrap();
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["claude-b", "claude-a"]);
        assert_eq!(models[0].name.as_deref(), Some("Claude B"));
    }

    #[tokio::test]
    async fn claude_models_reject_incomplete_pages_and_classify_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "claude-a" }], "has_more": true, "last_id": null
            })))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let err = claude_models(&account(Provider::Claude), &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(&*err.note.id, "query.schema");

        for (code, status) in [
            (401, Status::AuthenticationError),
            (429, Status::RateLimited),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(code).set_body_string("segredo-no-corpo"))
                .mount(&server)
                .await;
            let err = claude_models(&account(Provider::Claude), &context(&client), &server.uri())
                .await
                .unwrap_err();
            assert_eq!(err.status, status);
            assert!(!err.note.fallback.contains("segredo"));
        }
    }

    #[tokio::test]
    async fn chatgpt_models_send_client_version_and_hide_unlisted() {
        use wiremock::matchers::header;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/codex/models"))
            .and(query_param("client_version", CODEX_CLIENT_VERSION))
            .and(header("authorization", "Bearer test-secret"))
            .and(header("chatgpt-account-id", "acc-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [
                    { "slug": "gpt-b", "display_name": "GPT B", "description": "d", "visibility": "list" },
                    { "slug": "gpt-a" },
                    { "slug": "gpt-hidden", "visibility": "hide" },
                    { "slug": "gpt-none", "visibility": "none" }
                ]
            })))
            .mount(&server)
            .await;
        let mut acc = account(Provider::ChatGpt);
        acc.params.insert("account_id".into(), "acc-1".into());
        let client = reqwest::Client::new();
        let models = chatgpt_models(&acc, &context(&client), &server.uri())
            .await
            .unwrap();
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["gpt-b", "gpt-a"]);
        assert_eq!(models[0].description.as_deref(), Some("d"));
    }

    #[tokio::test]
    async fn chatgpt_models_reject_entries_without_slug() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [{ "display_name": "sem slug" }]
            })))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let err = chatgpt_models(
            &account(Provider::ChatGpt),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(&*err.note.id, "query.schema");
    }
}
