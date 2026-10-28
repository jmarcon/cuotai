//! Adapter for third-party inference providers (read-only).
//!
//! This adapter's policy:
//!
//! 1. The credential names exposed in the wizard are exactly
//!    `GROQ_API_KEY`, `PERPLEXITY_API_KEY`, `MISTRAL_API_KEY`,
//!    `X_GROK_API_KEY`, `NVIDIA_NIM_API_KEY`, `POOLSIDE_API_KEY`. The
//!    user may override the env via `Account.credential_env`.
//! 2. When the official endpoint requires a credential class different
//!    from the generation one (admin, analytics, management), the user gives the alternative
//!    env in `params.admin_key_env`,
//!    `params.analytics_key_env` or `params.management_key_env` — these
//!    params store only the env **name**; the value is still
//!    read through `Context.credentials`.
//! 3. Credential precedence: configured `params.<env_ref>` →
//!    `credential_env`/`default`. We never read two credentials for the
//!    same provider.
//! 4. When the endpoint requires an alternative class and no env points
//!    to a key of the correct class, we return `NeedsConfiguration`
//!    with the instruction to use `params.<env_ref>`.
//! 5. Providers without a public per-key balance/usage endpoint return
//!    `Unsupported` with the primary source URL. No probing.
//!
//! Official sources consulted:
//!
//! - Mistral (admin): `beta.admin.billing` endpoints require an Admin API Key.
//!   `GET /v1/admin/spend-limit` returns usage/limit/currency of the current
//!   cycle.
//!   Ref.: <https://docs.mistral.ai/api/endpoint/beta/admin/billing>
//!   OpenAPI: <https://docs.mistral.ai/openapi.yaml>
//!
//! - Perplexity: the main API (Sonar/Ask) has no public per-key balance/usage
//!   endpoint. The Analytics API
//!   (`GET /v1/analytics/computer/usage`) covers the Perplexity
//!   Computer product within Enterprise organizations; it is opt-in through
//!   `params.product=computer` so as not to silently switch
//!   product.
//!   Ref.: <https://docs.perplexity.ai/docs/admin/computer-analytics-api.md>
//!   Ref.: <https://docs.perplexity.ai/api-reference/computer-analytics-usage-get.md>
//!
//! - Grok (xAI): Management API at `https://management-api.x.ai` (a host
//!   distinct from `https://api.x.ai`, inference, and from `https://api.x.com`,
//!   the X social API). Documented endpoints: `GET /prepaid/balance` for
//!   prepaid credit balance and `GET /postpaid/invoice/preview` for
//!   the forecast in the current cycle. `effectiveSpendingLimit` is the
//!   monthly ceiling configured by the user (postpaid soft spending limit), not
//!   a platform quota.
//!   Ref.: <https://docs.x.ai/developers/management-api-guide.md>
//!   Ref.: <https://docs.x.ai/developers/rest-api-reference/management/billing.md>
//!
//! - NvidiaNim: `https://integrate.api.nvidia.com` is generation only; billing
//!   is through the NVIDIA Cloud account.
//!   Ref.: <https://docs.api.nvidia.com/nim/reference/llm-apis>
//!
//! - Poolside: hosted API (`https://inference.poolside.ai/v1`) exposes
//!   only `/v1/models` and `/v1/chat/completions`; self-managed
//!   deployments publish Prometheus metrics, not per-key balance.
//!   Ref.: <https://docs.poolside.ai/api/overview.md>

use super::{json, Context, Data, QueryError};
use crate::text::{params, provider_l10n, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider};
use chrono::{DateTime, Datelike, Months, Utc};
use reqwest::RequestBuilder;
use serde::Deserialize;

/// Default iteration limit for cursor-based pagination.
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
            "NVIDIA NIM does not expose a public balance endpoint by inference key: billing is controlled by the NVIDIA Cloud account, outside the scope of this key. Source: {source}.",
        )),
        Provider::Poolside => Err(QueryError::unsupported_params(
            "query.inference.poolside_no_public_usage",
            params([(
                "source",
                ParamValue::from("https://docs.poolside.ai/api/overview.md"),
            )]),
            "Poolside does not publish a usage/balance endpoint by key: the hosted API only exposes /v1/models and /v1/chat/completions. Source: {source}.",
        )),
        other => Err(QueryError::unsupported_params(
            "query.inference.provider_handled_elsewhere",
            params([("provider", ParamValue::from(provider_l10n(other)))]),
            "{provider} is handled by another adapter.",
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

/// Start of the current month (UTC) and the reset instant (next month).
fn month_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = now
        .date_naive()
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
        .expect("the first day of any month is valid");
    (start, start + Months::new(1))
}

/// Team/workspace identifier: it must never contain separators
/// susceptible to path injection.
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
            "params.{key} contains invalid characters for an identifier.",
        ))
    }
}

/// Resolves the credential actually used. Policy: the optional `param_name`
/// (e.g. `params.admin_key_env`) takes priority when configured with a
/// non-empty value; otherwise we use `credential_env` or `default`.
/// We never try two credentials for the same provider. The value never
/// passes through `Account`: it is resolved exclusively via
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
            let values = || {
                params([
                    ("class", ParamValue::from(required_class.to_string())),
                    ("env", ParamValue::from(env_name.clone())),
                    ("param", ParamValue::from(param_name.to_string())),
                ])
            };
            let hint = if override_env.is_some() {
                Localized::with_params(
                    "query.inference.missing_endpoint_credential.hint_override",
                    values(),
                    "The parameter `params.{param}` points to `{env}`, which is empty or missing.",
                )
            } else {
                Localized::with_params(
                    "query.inference.missing_endpoint_credential.hint_default",
                    values(),
                    "Set the `{env}` environment variable, or set `params.{param}` to the name of another variable holding a {class} key.",
                )
            };
            let mut values = values();
            values.insert("hint".into(), hint.into());
            QueryError::config_params(
                "query.inference.missing_endpoint_credential",
                values,
                "Missing {class} key for this endpoint. {hint}",
            )
        })
}

// =================================================================== Perplexity

/// `count` is required in the Analytics API response: its absence
/// means a response outside the contract. That is why we do NOT use `default`,
/// avoiding silent zeros for broken series.
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
            "Opt-in required: set `params.product = computer` to query the Perplexity Computer Analytics API. The Sonar/Ask API (rest of Perplexity) has no public endpoint."
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

    // This adapter queries only `credit_usage`. Other datasets
    // (`query_volume`, `daily_active_users`, `connectors`, etc.) measure
    // different quantities (queries, members, threads) and are out of the
    // scope of this query; nothing prevents a future expansion, but for
    // now we keep the unit/label unambiguous.
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
        "Opt-in mode: applied only because params.product=computer. The Sonar/Ask API (rest of Perplexity) has no public balance/usage endpoint by key.",
    );
    notes.push(
        "note.inference.perplexity_enterprise_analytics",
        "Computer Enterprise Analytics API. Credit usage dataset pinned to `credit_usage`; the per-user daily breakdown exposes call counts and USD.",
    );
    notes.push(
        "note.inference.perplexity_credit_usage_dataset",
        "Dataset pinned to `credit_usage` (the Sonar/Ask API usage query is out of scope for this adapter).",
    );
    notes.push(
        "note.inference.perplexity_complete_utc_days",
        "Credit usage cycles complete on calendar day boundaries in UTC; period resets at 00:00 UTC.",
    );
    notes.push(
        "note.inference.perplexity_limit_unavailable",
        "Spending limit preview without numeric total: soft/hard not exposed for this user in the current cycle.",
    );
    if user_email.is_some() {
        // PRD 0052 RF3.6: the note never carries the address.
        notes.push(
            "note.inference.perplexity_member_filtered",
            "Filtered by organisation member.",
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
    // Official documentation: the Admin API authenticates via the `x-api-key` header,
    // not `Authorization: Bearer`. Source:
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

    // The OpenAPI documents the block as having only `monthly_limit_reached`
    // as required, plus optional `usage`, `vibe_usage`, `total_usage`
    // and `usage_limit`. The OpenAPI declares `usage: number` without `format`; the
    // official guide `usage-metrics.md` states that the monthly limit is "as a
    // whole number in your Organization's billing currency (`500` means
    // 500, not 500 cents)" but does **not** confirm the same scale for the
    // numeric usage fields. Without an explicit primary source on the
    // scale of those fields, we use the declared currency (`limits.currency`)
    // as a generic unit label — `USD`, `EUR` etc. — without assuming
    // USD directly or cents.
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
        "Endpoint /v1/admin/spend-limit requires Admin API Key (generated in Console → Workspace → API keys → Admin keys).",
    );
    notes.push(
        "note.inference.mistral_completion_only",
        "Endpoint /v1/admin/spend-limit is read-only; no completions are generated by Cuotai.",
    );
    notes.push(
        "note.inference.mistral_currency_scale_undocumented",
        "Currency: USD; cost fields in this preview are reported without scale annotation.",
    );
    if completion.monthly_limit_reached {
        notes.push(
            "note.inference.mistral_monthly_limit_reached",
            "Monthly limit reached: the organisation is blocked.",
        );
    }
    if used_opt.is_none() {
        notes.push(
            "note.inference.mistral_usage_unknown",
            "Response without numeric usage fields: `used` remains unknown. The boolean `monthly_limit_reached` continues to be the only required indication.",
        );
    }
    if limit.is_none() {
        notes.push(
            "note.inference.mistral_no_monthly_limit",
            "Plan without a configured monthly limit (`no_monthly_limit=true`); only `used` is available.",
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

/// Decodes USD cents strings (1 USD = 100 cents) into `f64`,
/// distinguishing three cases:
/// - absent/empty: `Ok(None)`
/// - present and finite: `Ok(Some(dollars))`
/// - present but malformed or non-finite: `Err(schema)`
///
/// The rule above ensures that absent fields (e.g. a zeroed prepaid balance
/// without a `total` block) are not confused with corrupted data, but
/// any present string that does not parse becomes a schema error — with no silent
/// fallback to 0.
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
                "Set params.team_id with the Team ID visible at console.x.ai/team/default/settings/team.",
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
        "Management API (`management-api.x.ai`) is distinct from the inference API (`api.x.ai`) and the X social API (`api.x.com`); the key used is the Management Key generated at console.x.ai → Settings → Team.",
    );
    // We don't know `spending_limit` yet here; push the placeholder note
    // later when the value is known so the catalog template's `{limit}`
    // actually resolves. The placeholder-less variant stays as a
    // fallback for the rare case where preview returns without a limit.
    notes.push(
        "note.inference.grok_prepaid_no_monthly_expiration",
        "Prepaid credits have no documented monthly expiration; no reset is inferred and `resets_at = None` is kept for that balance.",
    );

    // Prepaid: the endpoint returns `total` in USD cents (negative when
    // there is a purchase, positive when there is spending). We treat the value as the
    // **available** balance and present it in `remaining`; `used` stays
    // `None` because the API field is a balance, not consumption.
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
            // No documented reset for prepaid: stays None.
            metrics.push(m);
        }
        None => {
            notes.push(
                "note.inference.grok_prepaid_total_missing",
                "No `total` field in /prepaid/balance: the team may not have prepaid credits configured.",
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
                // Explicit label: it is the postpaid budget that the
                // user configured in the console (soft spending limit),
                // not a platform quota.
                m.limit = Some(limit);
                let remaining = limit - total;
                m.remaining = Some(finite(remaining)?);
            }
            metrics.push(m);
        }
        (None, _) => {
            notes.push(
                "note.inference.grok_cycle_total_unusable",
                "Current cycle preview without usable numeric total.",
            );
        }
    }

    if spending_limit.is_none() {
        notes.push(
            "note.inference.grok_spending_limit_missing",
            "No postpaid budget configured (soft spending limit absent): the spending cap remains unknown.",
        );
    } else if let Some(limit) = spending_limit {
        // RF8: surface the configured USD ceiling through `{limit}` so
        // the catalog template renders it. Without the placeholder the
        // catalog drops the value inside the English text.
        notes.push_params(
            "note.inference.grok_soft_spending_limit",
            params([("limit", ParamValue::from(limit.to_string()))]),
            "`effectiveSpendingLimit` is the effective postpaid cap: {limit} USD; it is a budget (soft spending limit), not a platform quota.",
        );
    }

    if metrics.is_empty() {
        // If no endpoint returned usable data, we return a schema error
        // instead of faking success with an empty report.
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

    // ---------------------------------------------- provider defaults

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

    // ----------------------------------------------------- numeric helpers

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

        // PERPLEXITY_API_KEY intentionally absent: the analytics
        // key is resolved through params.analytics_key_env.
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
        // Even without params.analytics_key_env, if the `credential_env` env
        // holds a valid analytics key, we use it.
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
        // The dataset is fixed to `credit_usage`; this test confirms that
        // even if the user tries other names through params, the query
        // uses the fixed dataset and the user_email filter is propagated.
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
                    // Attempt to override the dataset is ignored.
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
            .any(|n| n.contains("Dataset pinned to `credit_usage`")));
        // PRD 0052 RF3.6: the filter went in the request, not in the note.
        data.assert_id_free(&["alice@example.com", &server.uri()]);
        assert!(data
            .notes
            .iter()
            .any(|n| n == "Filtered by organisation member."));
    }

    #[tokio::test]
    async fn perplexity_count_ausente_no_bucket_e_schema_nao_zero() {
        // count missing from the payload is treated as a schema error
        // (not as a silent zero).
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
        // Minimal response (only the required `monthly_limit_reached` field):
        // we keep `used` unknown and emit a note explaining it.
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
            .any(|n| n.contains("`used` remains unknown")));
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
        // The error is QueryError::schema — fixed helper message.
        assert!(err.note.fallback.contains("Response"));
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
        // Prepaid: `used` stays None; the balance is exposed in `remaining`.
        assert!(prepaid.used.is_none());
        approx(prepaid.remaining.unwrap(), 123.45);
        assert_eq!(prepaid.kind, MetricKind::Credits);
        assert_eq!(prepaid.window, None);
        assert_eq!(prepaid.unit, "USD");
        assert!(prepaid.period.is_none());
        // No documented reset for prepaid.
        assert!(prepaid.resets_at.is_none());

        let cycle = metric(&data, "Uso pós-pago previsto no ciclo corrente");
        approx(cycle.used.unwrap(), 11.80);
        approx(cycle.limit.unwrap(), 200.0);
        approx(cycle.remaining.unwrap(), 188.20);
        assert_eq!(cycle.kind, MetricKind::Spend);
        assert_eq!(cycle.window, Some(MetricWindow::Monthly));
        assert_eq!(cycle.period.as_deref(), Some("2025-11"));

        // Notes explain that effectiveSpendingLimit is a budget, not a quota.
        assert!(data.notes.iter().any(|n| n.contains("budget")));
        assert!(data.notes.iter().any(|n| n.contains("monthly expiration")));
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

        // No prepaid balance metric (absent).
        assert!(!data
            .metrics
            .iter()
            .any(|m| m.label == "Saldo de créditos pré-pagos"));
        // But the cycle metric is still present (0 USD).
        let cycle = metric(&data, "Uso pós-pago previsto no ciclo corrente");
        approx(cycle.used.unwrap(), 0.0);
        // And the note explains the absence.
        assert!(data.notes.iter().any(|n| n.contains("prepaid credits")));
    }

    // --------------------------------------------------------------- fim
}
