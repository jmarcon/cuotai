//! Usage and cost queries through official organization APIs (read-only).
//!
//! OpenAI (Admin API, administration key `OPENAI_ADMIN_KEY`):
//!   - GET https://api.openai.com/v1/organization/costs
//!   - GET https://api.openai.com/v1/organization/usage/completions
//!
//!   Reference: https://developers.openai.com/api/reference/resources/admin/subresources/organization
//!
//! Anthropic (Usage & Cost Admin API, key `sk-ant-admin01-...` in `ANTHROPIC_ADMIN_KEY`):
//!   - GET https://api.anthropic.com/v1/organizations/cost_report
//!   - GET https://api.anthropic.com/v1/organizations/usage_report/messages
//!
//!   Reference: https://platform.claude.com/docs/en/manage-claude/usage-cost-api
//!
//! Neither provider offers an official remaining balance or credit
//! endpoint; `limit` and `remaining` are always unknown (None).
//!
//! Optional parameters in `Account.params` (comma-separated lists):
//! - OpenAI: `project_ids` (costs and usage), `models` (usage)
//! - Anthropic: `workspace_ids` (costs and usage), `models` (usage)
//!
//! PRD 0045 RF4: with `breakdown = ["projects"]` (or `["keys"]`) the OpenAI
//! account also reads the costs grouped by `project_id` and the project
//! list (or the completions usage grouped by `api_key_id`), and with
//! `breakdown = ["workspaces"]` the Anthropic account reads the cost report
//! grouped by `workspace_id` and the workspace list; at most three pages
//! each, never filtered by `project_ids`/`workspace_ids`.

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
/// Daily with 31 buckets per page: the current month never needs 2 pages.
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

/// Start of the current month (UTC) and the reset instant (next month).
fn month_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = now
        .date_naive()
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
        .expect("o primeiro dia de qualquer mês é válido");
    (start, start + Months::new(1))
}

/// Optional comma-separated list in `Account.params`.
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

/// Pagination envelope identical in both providers.
#[derive(Deserialize)]
struct Page<B> {
    data: Vec<B>,
    has_more: bool,
    next_page: Option<String>,
}

/// Consumes every page through the cursor; never returns a partial total.
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
    // Optional in the schema, but mandatory here: absence cannot become zero.
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

    // PRD 0045 RF4: projects or keys, opt-in; never fails the account.
    let mut rows = Vec::new();
    for kind in [crate::BreakdownKind::Projects, crate::BreakdownKind::Keys] {
        if !account.breakdown.contains(&kind) {
            continue;
        }
        let window = Window {
            start,
            now,
            reset,
            period: &period,
        };
        let fetched = match kind {
            crate::BreakdownKind::Projects => openai_projects(ctx, base, &key, window).await,
            _ => openai_keys(ctx, base, &key, window).await,
        };
        match fetched {
            Ok((found, extra)) => {
                rows.extend(found);
                crate::breakdown::push_notes(&mut notes, extra);
            }
            Err(error) => notes.push_note(crate::breakdown::unavailable_note(&error)),
        }
    }

    let mut data = Data::new(
        metrics,
        notes,
        "https://developers.openai.com/api/reference/resources/admin/subresources/organization",
    )
    .with_as_of(as_of);
    data.breakdown = crate::breakdown::finish(rows);
    Ok(data)
}

// ------------------------------------------------- PRD 0045 RF4: breakdown

/// The UTC month a breakdown covers, shared with the account total.
#[derive(Clone, Copy)]
struct Window<'a> {
    start: DateTime<Utc>,
    now: DateTime<Utc>,
    reset: DateTime<Utc>,
    period: &'a str,
}

impl Window<'_> {
    /// A month-to-date metric of a breakdown row, like the account's own.
    fn metric(&self, label: &str, value: f64, unit: &str, kind: MetricKind) -> Metric {
        let mut m = Metric::usage(label, value, unit, kind, Some(MetricWindow::Monthly));
        m.period = Some(self.period.to_string());
        m.resets_at = Some(self.reset);
        m
    }
}

/// Like [`buckets`], but stops after [`crate::breakdown::MAX_PAGES`] pages
/// and says whether pages were left unread (a breakdown may be partial; an
/// account total never is).
async fn capped_buckets<B: serde::de::DeserializeOwned>(
    build: impl Fn(Option<String>) -> RequestBuilder,
) -> Result<(Vec<B>, bool), QueryError> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..crate::breakdown::MAX_PAGES {
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
            _ => return Ok((out, false)),
        }
    }
    Ok((out, true))
}

/// `id → name` from a list endpoint (`{ data: [{ id, name }] }`, one page
/// of 100); `None` when the key may not read it, so rows keep their ids.
async fn names(request: RequestBuilder) -> Option<BTreeMap<String, String>> {
    let page: serde_json::Value = json(request).await.ok()?;
    let items = page.get("data")?.as_array()?;
    Some(
        items
            .iter()
            .filter_map(|item| {
                Some((
                    item.get("id")?.as_str()?.to_string(),
                    item.get("name")?.as_str()?.to_string(),
                ))
            })
            .collect(),
    )
}

/// `note.breakdown.names_unavailable`.
fn names_note() -> crate::text::Localized {
    crate::text::Localized::fixed(
        "note.breakdown.names_unavailable",
        "The key may not read the names, so the breakdown shows ids instead.",
    )
}

/// Sums `amount` per id into rows labelled by `names` (redacted), else by
/// a generic label with the id's last characters.
fn sum_rows(
    sums: BTreeMap<String, BTreeMap<String, f64>>,
    names: Option<&BTreeMap<String, String>>,
    scope: crate::ScopeKind,
    window: Window<'_>,
    label: impl Fn(&str) -> String,
) -> Vec<crate::BreakdownRow> {
    use crate::breakdown::{fallback_label, opaque_id, redact_label};
    sums.into_iter()
        .map(|(raw, by_currency)| {
            let id = opaque_id(&raw);
            let name = names.and_then(|names| names.get(&raw));
            let row_label = match name {
                Some(name) => redact_label(name, scope, &id),
                None if raw == DEFAULT_GROUP => DEFAULT_WORKSPACE_LABEL.to_string(),
                None => fallback_label(scope, &raw),
            };
            let metrics = by_currency
                .into_iter()
                .map(|(unit, total)| window.metric(&label(&unit), total, &unit, MetricKind::Spend))
                .collect();
            crate::BreakdownRow {
                scope,
                id,
                label: row_label,
                metrics,
                truncated: false,
                notes: Vec::new(),
            }
        })
        .collect()
}

/// The group of results without an id (Anthropic's default workspace).
const DEFAULT_GROUP: &str = "default";
const DEFAULT_WORKSPACE_LABEL: &str = "Workspace padrão";

#[derive(Deserialize)]
struct OpenAiGroupedCostBucket {
    results: Vec<OpenAiGroupedCost>,
}

#[derive(Deserialize)]
struct OpenAiGroupedCost {
    amount: OpenAiAmount,
    #[serde(default)]
    project_id: Option<String>,
}

/// RF4: month-to-date cost per project (`group_by=project_id`), named from
/// the project list when the key may read it.
async fn openai_projects(
    ctx: &Context<'_>,
    base: &str,
    key: &str,
    window: Window<'_>,
) -> Result<(Vec<crate::BreakdownRow>, Vec<crate::text::Localized>), QueryError> {
    let query = vec![
        (
            "start_time".to_string(),
            window.start.timestamp().to_string(),
        ),
        ("end_time".to_string(), window.now.timestamp().to_string()),
        ("bucket_width".to_string(), "1d".to_string()),
        ("limit".to_string(), PAGE_LIMIT.to_string()),
        ("group_by".to_string(), "project_id".to_string()),
    ];
    let (pages, more): (Vec<OpenAiGroupedCostBucket>, bool) = capped_buckets(|cursor| {
        let mut q = query.clone();
        if let Some(c) = cursor {
            q.push(("page".to_string(), c));
        }
        ctx.http
            .get(format!("{base}/v1/organization/costs"))
            .bearer_auth(key)
            .query(&q)
            .timeout(ctx.timeout)
    })
    .await?;
    let mut sums: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for result in pages.into_iter().flat_map(|bucket| bucket.results) {
        let value = finite(result.amount.value)?;
        let currency = result.amount.currency.trim().to_uppercase();
        if currency.is_empty() {
            return Err(QueryError::schema());
        }
        let Some(project) = result.project_id.filter(|id| !id.trim().is_empty()) else {
            continue;
        };
        let entry = sums
            .entry(project)
            .or_default()
            .entry(currency)
            .or_insert(0.0);
        *entry = finite(*entry + value)?;
    }
    let mut notes = Vec::new();
    let names = if sums.is_empty() {
        None
    } else {
        names(
            ctx.http
                .get(format!("{base}/v1/organization/projects"))
                .bearer_auth(key)
                .query(&[("limit", "100"), ("include_archived", "true")])
                .timeout(ctx.timeout),
        )
        .await
    };
    if names.is_none() && !sums.is_empty() {
        notes.push(names_note());
    }
    if more {
        notes.push(crate::breakdown::paged_note());
    }
    let rows = sum_rows(
        sums,
        names.as_ref(),
        crate::ScopeKind::Project,
        window,
        |currency| format!("Custo no mês corrente ({currency})"),
    );
    Ok((rows, notes))
}

#[derive(Deserialize)]
struct OpenAiGroupedUsageBucket {
    results: Vec<OpenAiGroupedUsage>,
}

#[derive(Deserialize)]
struct OpenAiGroupedUsage {
    input_tokens: u64,
    output_tokens: u64,
    num_model_requests: u64,
    #[serde(default)]
    api_key_id: Option<String>,
}

/// RF4: month-to-date completion tokens and requests per API key
/// (`group_by=api_key_id`); the costs endpoint does not group by key, so a
/// key row has no money figure. Keys are labelled by their id's last
/// characters (the id is not a secret).
async fn openai_keys(
    ctx: &Context<'_>,
    base: &str,
    key: &str,
    window: Window<'_>,
) -> Result<(Vec<crate::BreakdownRow>, Vec<crate::text::Localized>), QueryError> {
    let query = vec![
        (
            "start_time".to_string(),
            window.start.timestamp().to_string(),
        ),
        ("end_time".to_string(), window.now.timestamp().to_string()),
        ("bucket_width".to_string(), "1d".to_string()),
        ("limit".to_string(), PAGE_LIMIT.to_string()),
        ("group_by".to_string(), "api_key_id".to_string()),
    ];
    let (pages, more): (Vec<OpenAiGroupedUsageBucket>, bool) = capped_buckets(|cursor| {
        let mut q = query.clone();
        if let Some(c) = cursor {
            q.push(("page".to_string(), c));
        }
        ctx.http
            .get(format!("{base}/v1/organization/usage/completions"))
            .bearer_auth(key)
            .query(&q)
            .timeout(ctx.timeout)
    })
    .await?;
    let mut sums: BTreeMap<String, [u64; 3]> = BTreeMap::new();
    for result in pages.into_iter().flat_map(|bucket| bucket.results) {
        let Some(id) = result.api_key_id.filter(|id| !id.trim().is_empty()) else {
            continue;
        };
        let entry = sums.entry(id).or_insert([0; 3]);
        for (slot, value) in entry.iter_mut().zip([
            result.input_tokens,
            result.output_tokens,
            result.num_model_requests,
        ]) {
            *slot = slot.checked_add(value).ok_or_else(QueryError::schema)?;
        }
    }
    let rows = sums
        .into_iter()
        .map(|(raw, [input, output, requests])| {
            let id = crate::breakdown::opaque_id(&raw);
            crate::BreakdownRow {
                scope: crate::ScopeKind::Key,
                label: crate::breakdown::fallback_label(crate::ScopeKind::Key, &raw),
                id,
                metrics: vec![
                    window.metric(
                        "Requisições de modelo (completions)",
                        requests as f64,
                        "requisições",
                        MetricKind::Usage,
                    ),
                    window.metric(
                        "Tokens de entrada (completions)",
                        input as f64,
                        "tokens",
                        MetricKind::Usage,
                    ),
                    window.metric(
                        "Tokens de saída (completions)",
                        output as f64,
                        "tokens",
                        MetricKind::Usage,
                    ),
                ],
                truncated: false,
                notes: Vec::new(),
            }
        })
        .collect();
    let notes = if more {
        vec![crate::breakdown::paged_note()]
    } else {
        Vec::new()
    };
    Ok((rows, notes))
}

// ------------------------------------------------------------- Anthropic

#[derive(Deserialize)]
struct AnthropicCostBucket {
    results: Vec<AnthropicCostResult>,
}

#[derive(Deserialize)]
struct AnthropicCostResult {
    /// Decimal string in the currency's smallest unit (cents for USD).
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
            // Cents documented only for USD ("currently always USD").
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

    // PRD 0045 RF4: workspaces, opt-in; never fails the account.
    let mut rows = Vec::new();
    if account
        .breakdown
        .contains(&crate::BreakdownKind::Workspaces)
    {
        let window = Window {
            start,
            now,
            reset,
            period: &period,
        };
        match anthropic_workspaces(ctx, base, &key, window).await {
            Ok((found, extra)) => {
                rows = found;
                crate::breakdown::push_notes(&mut notes, extra);
            }
            Err(error) => notes.push_note(crate::breakdown::unavailable_note(&error)),
        }
    }

    let mut data = Data::new(
        metrics,
        notes,
        "https://platform.claude.com/docs/en/manage-claude/usage-cost-api",
    );
    data.breakdown = crate::breakdown::finish(rows);
    Ok(data)
}

#[derive(Deserialize)]
struct AnthropicGroupedCostBucket {
    results: Vec<AnthropicGroupedCost>,
}

#[derive(Deserialize)]
struct AnthropicGroupedCost {
    amount: String,
    currency: String,
    #[serde(default)]
    workspace_id: Option<String>,
}

/// RF4: month-to-date cost per workspace (`group_by[]=workspace_id`; the
/// default workspace comes without an id), named from the workspace list.
async fn anthropic_workspaces(
    ctx: &Context<'_>,
    base: &str,
    key: &str,
    window: Window<'_>,
) -> Result<(Vec<crate::BreakdownRow>, Vec<crate::text::Localized>), QueryError> {
    let query = vec![
        ("starting_at".to_string(), window.start.to_rfc3339()),
        ("ending_at".to_string(), window.now.to_rfc3339()),
        ("bucket_width".to_string(), "1d".to_string()),
        ("limit".to_string(), PAGE_LIMIT.to_string()),
        ("group_by[]".to_string(), "workspace_id".to_string()),
    ];
    let (pages, more): (Vec<AnthropicGroupedCostBucket>, bool) = capped_buckets(|cursor| {
        let mut q = query.clone();
        if let Some(c) = cursor {
            q.push(("page".to_string(), c));
        }
        ctx.http
            .get(format!("{base}/v1/organizations/cost_report"))
            .header("x-api-key", key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .query(&q)
            .timeout(ctx.timeout)
    })
    .await?;
    let mut sums: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for result in pages.into_iter().flat_map(|bucket| bucket.results) {
        if !result.currency.trim().eq_ignore_ascii_case("usd") {
            return Err(QueryError::schema());
        }
        let cents: f64 = result.amount.parse().map_err(|_| QueryError::schema())?;
        let workspace = result
            .workspace_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_GROUP.to_string());
        let entry = sums
            .entry(workspace)
            .or_default()
            .entry("USD".to_string())
            .or_insert(0.0);
        *entry = finite(*entry + finite(cents)? / 100.0)?;
    }
    let mut notes = Vec::new();
    let named = sums.keys().any(|id| id != DEFAULT_GROUP);
    let names = if named {
        names(
            ctx.http
                .get(format!("{base}/v1/organizations/workspaces"))
                .header("x-api-key", key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .query(&[("limit", "100"), ("include_archived", "true")])
                .timeout(ctx.timeout),
        )
        .await
    } else {
        None
    };
    if named && names.is_none() {
        notes.push(names_note());
    }
    if more {
        notes.push(crate::breakdown::paged_note());
    }
    let rows = sum_rows(
        sums,
        names.as_ref(),
        crate::ScopeKind::Workspace,
        window,
        |_| "Custo no mês corrente (USD)".to_string(),
    );
    Ok((rows, notes))
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
            budgets: Vec::new(),
            floors: Vec::new(),
            breakdown: Vec::new(),
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

    // ------------------------------------------------ PRD 0045 RF4

    fn with_breakdown(provider: Provider, kinds: &[crate::BreakdownKind]) -> Account {
        let mut account = account(provider);
        account.breakdown = kinds.to_vec();
        account
    }

    async fn mount_openai_totals(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(query_param_is_missing("group_by"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_cost_page(
                serde_json::json!([{ "amount": { "value": 10.0, "currency": "usd" } }]),
                false,
                None,
            )))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/usage/completions"))
            .and(query_param_is_missing("group_by"))
            .respond_with(ResponseTemplate::new(200).set_body_json(openai_usage_page(1, 1, 1)))
            .mount(server)
            .await;
    }

    fn project_costs(count: usize) -> serde_json::Value {
        let results: Vec<_> = (0..count)
            .map(|n| {
                serde_json::json!({
                    "object": "organization.costs.result",
                    "amount": { "value": n as f64 + 1.0, "currency": "usd" },
                    "project_id": format!("proj_{n:03}"),
                })
            })
            .chain([serde_json::json!({
                "amount": { "value": 0.5, "currency": "usd" }, "project_id": null
            })])
            .collect();
        openai_cost_page(serde_json::json!(results), false, None)
    }

    #[tokio::test]
    async fn openai_projects_are_named_from_the_project_list() {
        let server = MockServer::start().await;
        mount_openai_totals(&server).await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(query_param("group_by", "project_id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(project_costs(2)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "id": "proj_000", "name": "Proj-A", "status": "active" },
                    { "id": "proj_001", "name": "owner@example.com", "status": "active" }
                ]
            })))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let account = with_breakdown(Provider::OpenAiApi, &[crate::BreakdownKind::Projects]);
        let data = openai(&account, &context(&client), &server.uri())
            .await
            .unwrap();
        // The account total is unchanged by the breakdown.
        assert_eq!(
            metric(&data, "Custo no mês corrente (USD)").used,
            Some(10.0)
        );
        let rows = &data.breakdown;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].id.as_str(), rows[0].label.as_str()),
            ("proj_001", "Project …j001")
        );
        assert_eq!(rows[1].label, "Proj-A");
        assert_eq!(rows[1].metrics[0].used, Some(1.0));
        assert_eq!(rows[1].metrics[0].window, Some(MetricWindow::Monthly));
        assert!(
            rows[1].metrics[0].resets_at.is_some(),
            "the UTC month is reused"
        );
        // Two extra requests: the grouped costs and the project names.
        let extra = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| {
                r.url.path() == "/v1/organization/projects"
                    || r.url.query().is_some_and(|q| q.contains("group_by"))
            })
            .count();
        assert_eq!(extra, 2);
    }

    #[tokio::test]
    async fn openai_projects_keep_their_ids_without_names_and_cap_at_25() {
        let server = MockServer::start().await;
        mount_openai_totals(&server).await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(query_param("group_by", "project_id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(project_costs(30)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/projects"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let account = with_breakdown(Provider::OpenAiApi, &[crate::BreakdownKind::Projects]);
        let data = openai(&account, &context(&client), &server.uri())
            .await
            .unwrap();
        assert_eq!(data.breakdown.len(), crate::breakdown::MAX_ROWS);
        assert_eq!(data.breakdown[0].label, "Project …j029");
        assert_eq!(data.breakdown.last().unwrap().label, "Others (6)");
        assert!(data
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.breakdown.names_unavailable"));
    }

    #[tokio::test]
    async fn openai_keys_report_tokens_and_requests_per_key() {
        let server = MockServer::start().await;
        mount_openai_totals(&server).await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/usage/completions"))
            .and(query_param("group_by", "api_key_id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "page",
                "data": [{ "start_time": 1, "end_time": 2, "results": [
                    { "input_tokens": 10, "output_tokens": 5, "num_model_requests": 3, "api_key_id": "key_abc1" },
                    { "input_tokens": 1, "output_tokens": 1, "num_model_requests": 1, "api_key_id": "key_zz99" },
                    { "input_tokens": 7, "output_tokens": 7, "num_model_requests": 2, "api_key_id": "key_abc1" }
                ]}],
                "has_more": false, "next_page": null
            })))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let account = with_breakdown(Provider::OpenAiApi, &[crate::BreakdownKind::Keys]);
        let data = openai(&account, &context(&client), &server.uri())
            .await
            .unwrap();
        let rows = &data.breakdown;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "Key …abc1");
        assert_eq!(rows[0].scope, crate::ScopeKind::Key);
        assert_eq!(
            rows[0].metrics[0].used,
            Some(5.0),
            "requests order the rows"
        );
        assert_eq!(rows[0].metrics[1].used, Some(17.0));
    }

    #[tokio::test]
    async fn a_failing_breakdown_leaves_the_openai_total_standing() {
        let server = MockServer::start().await;
        mount_openai_totals(&server).await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(query_param("group_by", "project_id"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let account = with_breakdown(Provider::OpenAiApi, &[crate::BreakdownKind::Projects]);
        let data = openai(&account, &context(&client), &server.uri())
            .await
            .unwrap();
        assert!(data.breakdown.is_empty());
        assert_eq!(
            metric(&data, "Custo no mês corrente (USD)").used,
            Some(10.0)
        );
        assert!(data
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.breakdown.unavailable"));
    }

    #[tokio::test]
    async fn anthropic_workspaces_are_named_and_the_default_one_labelled() {
        let server = MockServer::start().await;
        let cost_page = |results: serde_json::Value| serde_json::json!({ "data": [{ "results": results }], "has_more": false, "next_page": null });
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .and(query_param("group_by[]", "workspace_id"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(cost_page(serde_json::json!([
                    { "amount": "41200", "currency": "USD", "workspace_id": "wrkspc_01A" },
                    { "amount": "29800", "currency": "USD", "workspace_id": "wrkspc_01B" },
                    { "amount": "150", "currency": "USD", "workspace_id": null }
                ]))),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .and(query_param_is_missing("group_by[]"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(cost_page(serde_json::json!([
                    { "amount": "71150", "currency": "USD" }
                ]))),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/usage_report/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [], "has_more": false, "next_page": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/workspaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [ { "id": "wrkspc_01A", "name": "Proj-A" }, { "id": "wrkspc_01B", "name": "Proj-B" } ]
            })))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let account = with_breakdown(Provider::AnthropicApi, &[crate::BreakdownKind::Workspaces]);
        let data = anthropic(&account, &context(&client), &server.uri())
            .await
            .unwrap();
        let rows = &data.breakdown;
        assert_eq!(
            rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            ["Proj-A", "Proj-B", "Workspace padrão"]
        );
        assert_eq!(rows[0].metrics[0].used, Some(412.0));
        assert_eq!(rows[0].scope, crate::ScopeKind::Workspace);
        assert_eq!(rows[2].label_text().resolve("en-US"), "Default workspace");
        assert_eq!(
            crate::breakdown::top_line(rows, crate::breakdown::CARD_ROWS)
                .unwrap()
                .text(),
            "Top: Proj-A $412 · Proj-B $298"
        );
    }

    #[tokio::test]
    async fn without_the_opt_in_no_grouped_query_is_made() {
        let server = MockServer::start().await;
        mount_openai_totals(&server).await;
        let client = reqwest::Client::new();
        let data = openai(
            &account(Provider::OpenAiApi),
            &context(&client),
            &server.uri(),
        )
        .await
        .unwrap();
        assert!(data.breakdown.is_empty());
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}
