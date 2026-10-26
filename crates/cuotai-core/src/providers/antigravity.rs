//! Read-only quota queries for Antigravity (Google IDE) — same
//! fingerprint as the official client, same URLs and same semantics as
//! `open-sse/services/usage/google.js` + `antigravity-weekly.js` in
//! 9router f01fb909e37189008080632ddaf404f096345cde.
//!
//! Flow (one quota call per account; shared timeout):
//!   1. `POST cloudcode-pa.googleapis.com/v1internal:loadCodeAssist`
//!      → discovers `cloudaicompanionProject` (string or object with `id`)
//!      and exposes `paidTier` for the free/paid decision. Overridden if
//!      `params.project_id` is supplied manually.
//!   2. `POST daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels`
//!      → per-model quotas (short 5h window). Paid only.
//!   3. `POST daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary`
//!      → weekly buckets (`window=weekly`) and session buckets (`window=5h`).
//!
//! Behavior mirrored from 9router:
//!
//! - **Fixed headers**: `User-Agent: antigravity/ide/<VER> darwin/arm64`,
//!   `X-Client-Name: antigravity`, `X-Client-Version: <VER>`.
//! - **Free tier**: accounts with `paidTier.id` absent or equal to
//!   `"free-tier"` do NOT show per-model quotas. Only
//!   `retrieveUserQuotaSummary` matters.
//! - **Paid tier**: processes only models on the allow-list (Gemini
//!   3.8/3.7/3.6/3.5 Flash in the 3/4 tiers, Gemini Pro agent/3.1 Pro,
//!   Claude Sonnet 4.6, Claude Opus 4.6 Thinking, GPT-OSS 120B Medium,
//!   Gemini 3.1 Flash Image). Models with `isInternal=true` are skipped;
//!   without `quotaInfo`/`remainingFraction` they stay with unknown quota.
//! - **Weekly**: parsing tolerant of `groups` or `quotaSummary.groups`,
//!   `weekly` and `5h` buckets (and aliases `daily`/`five hour`/`5h`).
//!   `disabled=true` skips `weekly`; for `5h` it keeps it at `0%`.
//! - **Paid-tier reconciliation**: if ALL Gemini models (non-image)
//!   have `remainingFraction == 0` and `gemini_session` still
//!   shows a balance > 0, the session is reset to `0%` with the family's
//!   furthest `resetAt` (the weekly line stays intact).
//!   Same rule for Claude/GPT.
//! - **Partial failure**: `PARCIAL:` notes state explicitly whether the
//!   error came from `modelos` or `resumo` (it does not claim completion).
//! - **Reset**: `resetTime` may arrive as an RFC 3339 string, an epoch
//!   number in seconds OR milliseconds, or `NaiveDateTime`. Absent or
//!   malformed becomes `None`.
//! - **Raw percentages**: the fraction is multiplied by `100` without
//!   `clamp` — quotas above 100% (excess usage) are preserved.
//!
//! Credential:
//!   - Manual: env var `ANTIGRAVITY_ACCESS_TOKEN` (the IDE's Bearer).
//!   - OAuth: the dispatcher in `crate::lib` replaces `credential_env`
//!     with `__CUOTAI_OAUTH_ACCESS` when `account.oauth` is present.
//!
//! Parameters in `Account.params` (all optional):
//!   - `project_id`: forces the `project` sent to `:fetchAvailableModels`
//!     and `:retrieveUserQuotaSummary`. When absent, the adapter
//!     queries `:loadCodeAssist` to discover it (no fallback if the
//!     discovery call fails — in that case the quotas are
//!     sent without `project` and a note is recorded).

use super::{json, secret, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

// ─── Fixed endpoints (same base as the 9router registry) ──────────────────
/// `loadCodeAssist` lives at `cloudcode-pa.googleapis.com` (no `daily-`
/// prefix); the other routes are at `daily-cloudcode-pa.googleapis.com`.
pub(crate) const LOAD_CODE_ASSIST_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
pub(crate) const FETCH_AVAILABLE_MODELS_URL: &str =
    "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";
const RETRIEVE_USER_QUOTA_SUMMARY_URL: &str =
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary";

/// User-agent oficial do Antigravity IDE Desktop 2.11.0 (macOS arm64).
const ANTIGRAVITY_IDE_VERSION: &str = "2.11.0";
const ANTIGRAVITY_USER_AGENT: &str = "antigravity/ide/2.11.0 darwin/arm64";

/// Default environment variable for a manual token imported from the IDE.
const DEFAULT_CREDENTIAL_ENV: &str = "ANTIGRAVITY_ACCESS_TOKEN";

/// Recognized key in `Account.params` for a manual `project_id`.
const PARAM_PROJECT_ID: &str = "project_id";

/// `paidTier.id` sent by the server for accounts without a paid subscription.
const FREE_TIER_ID: &str = "free-tier";

/// Allow-list de modelos cujas quotas valem a pena exibir.
const IMPORTANT_MODELS: &[&str] = &[
    // Gemini 3.8 Flash tiered
    "gemini-3.8-flash-high",
    "gemini-3.8-flash-medium",
    "gemini-3.8-flash-low",
    // Gemini 3.7 Flash tiered
    "gemini-3.7-flash-high",
    "gemini-3.7-flash-medium",
    "gemini-3.7-flash-low",
    // Gemini 3.6 Flash tiered
    "gemini-3.6-flash-high",
    "gemini-3.6-flash-medium",
    "gemini-3.6-flash-low",
    // Gemini 3.5 Flash tiered
    "gemini-3.5-flash-low",
    "gemini-3.5-flash-extra-low",
    // Gemini Pro
    "gemini-pro-agent",
    "gemini-3.1-pro-low",
    // Claude & GPT
    "claude-sonnet-4-6",
    "claude-opus-4-6-thinking",
    "gpt-oss-120b-medium",
    // Image
    "gemini-3.1-flash-image",
];

// ─── Families for reconciliation / weekly grouping ────────────────
struct GroupConfig {
    pattern: &'static str,
    weekly_key: &'static str,
    weekly_label: &'static str,
    session_key: &'static str,
    session_label: &'static str,
}

const GROUP_CONFIGS: &[GroupConfig] = &[
    GroupConfig {
        pattern: "gemini",
        weekly_key: "gemini_weekly",
        weekly_label: "Gemini (Semanal)",
        session_key: "gemini_session",
        session_label: "Gemini (5h)",
    },
    GroupConfig {
        pattern: "claude",
        weekly_key: "claude_gpt_weekly",
        weekly_label: "Claude & GPT (Semanal)",
        session_key: "claude_gpt_session",
        session_label: "Claude & GPT (5h)",
    },
    GroupConfig {
        pattern: "gpt",
        weekly_key: "claude_gpt_weekly",
        weekly_label: "Claude & GPT (Semanal)",
        session_key: "claude_gpt_session",
        session_label: "Claude & GPT (5h)",
    },
];

/// Matches group names case-insensitively without regex: the set of patterns
/// is static, finite and trivial (`gemini`, `claude`, `gpt`).
fn group_matches(pattern: &str, display_name: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let needle = pattern.to_ascii_lowercase();
    let haystack = display_name.to_ascii_lowercase();
    haystack.contains(&needle)
}

/// Free/paid decision replicated from 9router: `paidTier.id` absent or
/// equal to `"free-tier"` indicates an account without a paid subscription.
fn is_free_tier_from(paid_tier_id: Option<&str>) -> bool {
    paid_tier_id.is_none() || paid_tier_id == Some(FREE_TIER_ID)
}

// ─── Public entry (dispatcher contract) ────────────────────────

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::Antigravity => fetch_antigravity(account, ctx).await,
        other => Err(QueryError::unsupported_params(
            "query.antigravity.unsupported_provider",
            params([(
                "provider",
                ParamValue::L10n(Box::new(crate::text::provider_l10n(other))),
            )]),
            "{provider} is handled by another adapter.",
        )),
    }
}

/// Model catalog of the session: the same `loadCodeAssist` discovery and
/// `fetchAvailableModels` request as the quota path, keeping every
/// non-internal entry (not only [`IMPORTANT_MODELS`]). Any failed request
/// fails the sync; a free tier or a response without `models` is no catalog.
pub(crate) async fn models(
    account: &Account,
    ctx: &Context<'_>,
    load_code_assist_url: &str,
    fetch_available_models_url: &str,
) -> Result<Vec<crate::models::ModelInfo>, QueryError> {
    let token = secret(account, ctx, DEFAULT_CREDENTIAL_ENV)?;
    let (discovered, paid_tier_id) = load_code_assist(ctx, &token, load_code_assist_url).await?;
    let no_catalog = || {
        QueryError::unsupported(
            "query.antigravity.no_model_catalog",
            "The Antigravity session returned no catalog in fetchAvailableModels (free plan or response without models).",
        )
    };
    if is_free_tier_from(paid_tier_id.as_deref()) {
        return Err(no_catalog());
    }
    let project = account
        .params
        .get(PARAM_PROJECT_ID)
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .or(discovered.as_deref());
    let response = fetch_available_models(ctx, &token, project, fetch_available_models_url).await?;
    let mut models = Vec::new();
    for (id, info) in response.models.ok_or_else(no_catalog)? {
        let info = info
            .as_object()
            .ok_or_else(|| QueryError::schema_at(&info, &[], "object"))?;
        if info.get("isInternal").and_then(serde_json::Value::as_bool) == Some(true) {
            continue;
        }
        let name = info.get("displayName").and_then(serde_json::Value::as_str);
        models.push(crate::models::ModelInfo {
            id,
            name: name.map(str::to_string),
            description: None,
        });
    }
    Ok(models)
}

// ─── Implementation ────────────────────────────────────────────────

async fn fetch_antigravity(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    fetch_antigravity_at(
        account,
        ctx,
        LOAD_CODE_ASSIST_URL,
        FETCH_AVAILABLE_MODELS_URL,
        RETRIEVE_USER_QUOTA_SUMMARY_URL,
    )
    .await
}

/// Internal envelope that accepts injected URLs (test seam). Production
/// passes the fixed constants; tests may pass `wiremock::MockServer::uri()`.
async fn fetch_antigravity_at(
    account: &Account,
    ctx: &Context<'_>,
    load_code_assist_url: &str,
    fetch_available_models_url: &str,
    retrieve_user_quota_summary_url: &str,
) -> Result<Data, QueryError> {
    let token = secret(account, ctx, DEFAULT_CREDENTIAL_ENV)?;

    let mut notes = Notes::new();
    let (project_id, paid_tier_id) = match load_code_assist(ctx, &token, load_code_assist_url).await
    {
        Ok((discovered_project, discovered_paid_tier)) => {
            (discovered_project, discovered_paid_tier)
        }
        Err(error) => {
            notes.push_params(
                "note.antigravity.load_code_assist_unavailable",
                params([("error", ParamValue::L10n(Box::new(error.note.clone())))]),
                "PARTIAL: loadCodeAssist (project_id/tier discovery) unavailable ({error}); project_id was not discovered.",
            );
            (None, None)
        }
    };

    let params_project_id = account
        .params
        .get(PARAM_PROJECT_ID)
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty());
    let effective_project = params_project_id.or(project_id.as_deref());
    if let (Some(manual), Some(discovered)) = (params_project_id, project_id.as_deref()) {
        if manual != discovered {
            notes.push(
                "note.antigravity.project_override",
                "params.project_id overrode the project_id discovered by loadCodeAssist.",
            );
        }
    }

    let is_free_tier = is_free_tier_from(paid_tier_id.as_deref());

    // 1) fetchAvailableModels (paid-tier only)
    let mut quotas = QuotaMap::new();
    let mut model_failure: Option<String> = None;
    let mut fetched_models: Option<FetchAvailableModelsResponse> = None;
    if !is_free_tier {
        match fetch_available_models(ctx, &token, effective_project, fetch_available_models_url)
            .await
        {
            Ok(data) => {
                if let Some(models) = data.models.as_ref() {
                    parse_paid_tier_models(models, &mut quotas);
                }
                fetched_models = Some(data);
            }
            Err(error) => {
                model_failure = Some(error.note.fallback.clone().into_owned());
                notes.push_params(
                    "note.antigravity.models_unavailable",
                    params([("error", ParamValue::L10n(Box::new(error.note.clone())))]),
                    "PARTIAL: fetchAvailableModels (models) unavailable ({error}); only the summary is shown.",
                );
            }
        }
    }

    // 2) retrieveUserQuotaSummary (best-effort, never blocks)
    let mut weekly_failure: Option<String> = None;
    let mut parsed_weekly = WeeklySummary::default();
    match retrieve_user_quota_summary(
        ctx,
        &token,
        effective_project,
        retrieve_user_quota_summary_url,
    )
    .await
    {
        Ok(summary) => {
            parsed_weekly = parse_weekly_summary(&summary);
        }
        Err(error) => {
            weekly_failure = Some(error.note.fallback.clone().into_owned());
            notes.push_params(
                "note.antigravity.summary_unavailable",
                params([("error", ParamValue::L10n(Box::new(error.note.clone())))]),
                "PARTIAL: retrieveUserQuotaSummary (weekly/5h summary) unavailable ({error}); only models are shown.",
            );
        }
    }

    // 3) Paid-tier reconciliation (5h session → 0% if the family is exhausted)
    if !is_free_tier && fetched_models.is_some() {
        reconcile_exhausted_family(&mut quotas, &mut parsed_weekly, "gemini");
        reconcile_exhausted_family(&mut quotas, &mut parsed_weekly, "claude");
    }

    // 4) Total failure → error. Otherwise, partial stays as Ready + PARCIAL note.
    if quotas.is_empty() && parsed_weekly.is_empty() {
        let mut detail = Vec::new();
        if let Some(msg) = model_failure {
            detail.push(format!("models (fetchAvailableModels): {msg}"));
        }
        if let Some(msg) = weekly_failure {
            detail.push(format!("summary (retrieveUserQuotaSummary): {msg}"));
        }
        let summary = if detail.is_empty() {
            "Antigravity returned no materially usable quotas.".to_string()
        } else {
            format!("Antigravity returned no data: {}.", detail.join("; "))
        };
        return Err(QueryError {
            diagnostic: None,
            status: crate::Status::Error,
            note: Localized::with_param(
                "note.antigravity.fatal_summary",
                "summary",
                summary,
                "{summary}",
            ),
        });
    }

    // ─── Building the metrics ──────────────────────────────────────
    let mut metrics = Vec::new();

    for (key, entry) in quotas.iter() {
        let label = if entry.display_name.is_empty() {
            key.clone()
        } else {
            entry.display_name.clone()
        };
        // One quota per model: exhausting a model must not block the account.
        let scope = crate::Scope::model(key.clone(), label.clone());
        metrics.push(build_model_metric(label, entry).scoped(scope));
    }

    for (_, entry) in parsed_weekly.iter_weekly() {
        if let Some(metric) = build_percent_metric(
            entry.display_name.clone(),
            entry.remaining_fraction,
            entry.reset_at,
            "semanal",
            MetricWindow::Weekly,
        ) {
            metrics.push(metric);
        }
    }

    for (_, entry) in parsed_weekly.iter_session() {
        if let Some(metric) = build_percent_metric(
            entry.display_name.clone(),
            entry.remaining_fraction,
            entry.reset_at,
            "5h",
            MetricWindow::Window5h,
        ) {
            metrics.push(metric);
        }
    }

    let source = format!("{FETCH_AVAILABLE_MODELS_URL} + {RETRIEVE_USER_QUOTA_SUMMARY_URL}");

    Ok(Data::new(metrics, notes, source))
}

// ─── Helpers compartilhados ──────────────────────────────────────────
fn build_model_metric(label: String, entry: &QuotaEntry) -> Metric {
    if let Some(fraction) = entry.remaining_fraction {
        if let Some(metric) = build_percent_metric(
            label.clone(),
            fraction,
            entry.reset_at,
            "5h",
            MetricWindow::Window5h,
        ) {
            return metric;
        }
    }
    Metric {
        billing: Default::default(),
        scope: None,
        label,
        used: None,
        limit: None,
        remaining: None,
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Window5h),
        period: Some("5h".into()),
        resets_at: entry.reset_at,
    }
}

fn build_percent_metric(
    label: String,
    remaining_fraction: f64,
    reset_at: Option<DateTime<Utc>>,
    period: &'static str,
    window: MetricWindow,
) -> Option<Metric> {
    if !remaining_fraction.is_finite() {
        return None;
    }
    // No `clamp` — quotas above 100% (excess usage) are preserved.
    let used_percent = (1.0 - remaining_fraction) * 100.0;
    let remaining_percent = remaining_fraction * 100.0;
    let mut metric = Metric::usage(label, used_percent, "%", MetricKind::Quota, Some(window));
    metric.limit = Some(100.0);
    metric.remaining = Some(remaining_percent);
    metric.period = Some(period.into());
    metric.resets_at = reset_at;
    Some(metric)
}

// ─── Chamadas HTTP ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LoadCodeAssistResponse {
    #[serde(
        rename = "cloudaicompanionProject",
        alias = "cloudaicompanion_project",
        default,
        deserialize_with = "deserialize_project_id"
    )]
    cloudaicompanion_project: Option<String>,
    #[serde(default, rename = "paidTier")]
    paid_tier: Option<PaidTier>,
}

/// Accepts `cloudaicompanionProject` in three formats:
/// - plain string (`"projects/foo"`)
/// - object with an `id` field (`{"id":"projects/foo"}`)
/// - absent, null or unknown format → `None` (never an error)
fn deserialize_project_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde_json::Value;
    let value = Value::deserialize(deserializer)?;
    let normalized = match value {
        Value::Null => return Ok(None),
        Value::String(s) => {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
        }
        Value::Object(map) => match map.get("id").and_then(Value::as_str) {
            Some(id) => {
                let trimmed = id.trim().to_string();
                if trimmed.is_empty() {
                    return Ok(None);
                }
                trimmed
            }
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(Some(normalized))
}

#[derive(Debug, Deserialize)]
struct PaidTier {
    #[serde(default)]
    id: Option<String>,
}

async fn load_code_assist(
    ctx: &Context<'_>,
    token: &str,
    url: &str,
) -> Result<(Option<String>, Option<String>), QueryError> {
    let body: LoadCodeAssistResponse = json(
        ctx.http
            .post(url)
            .bearer_auth(token)
            .header("User-Agent", ANTIGRAVITY_USER_AGENT)
            .header("Content-Type", "application/json")
            .header("X-Client-Name", "antigravity")
            .header("X-Client-Version", ANTIGRAVITY_IDE_VERSION)
            .timeout(ctx.timeout)
            .body(serde_json::json!({ "metadata": {}, "mode": 1 }).to_string()),
    )
    .await?;
    let paid_tier_id = body
        .paid_tier
        .as_ref()
        .and_then(|t| t.id.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok((body.cloudaicompanion_project, paid_tier_id))
}

#[derive(Debug, Default, Deserialize)]
struct FetchAvailableModelsResponse {
    #[serde(default)]
    models: Option<serde_json::Map<String, serde_json::Value>>,
}

async fn fetch_available_models(
    ctx: &Context<'_>,
    token: &str,
    project_id: Option<&str>,
    url: &str,
) -> Result<FetchAvailableModelsResponse, QueryError> {
    let body = match project_id {
        Some(p) => serde_json::json!({ "project": p }).to_string(),
        None => "{}".into(),
    };
    json(
        ctx.http
            .post(url)
            .bearer_auth(token)
            .header("User-Agent", ANTIGRAVITY_USER_AGENT)
            .header("Content-Type", "application/json")
            .header("X-Client-Name", "antigravity")
            .header("X-Client-Version", ANTIGRAVITY_IDE_VERSION)
            .timeout(ctx.timeout)
            .body(body),
    )
    .await
}

#[derive(Debug, Default, Deserialize)]
struct QuotaSummaryEnvelope {
    #[serde(default)]
    groups: Option<Vec<QuotaGroup>>,
    #[serde(default, rename = "quotaSummary", alias = "quota_summary")]
    quota_summary: Option<NestedSummary>,
}

#[derive(Debug, Default, Deserialize)]
struct NestedSummary {
    #[serde(default)]
    groups: Option<Vec<QuotaGroup>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaGroup {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    buckets: Option<Vec<QuotaBucket>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaBucket {
    #[serde(default)]
    window: Option<String>,
    #[serde(default)]
    bucket_id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    remaining_fraction: Option<serde_json::Value>,
    #[serde(default)]
    reset_time: Option<serde_json::Value>,
    #[serde(default)]
    disabled: Option<bool>,
}

async fn retrieve_user_quota_summary(
    ctx: &Context<'_>,
    token: &str,
    project_id: Option<&str>,
    url: &str,
) -> Result<QuotaSummaryEnvelope, QueryError> {
    let body = match project_id {
        Some(p) => serde_json::json!({ "project": p }).to_string(),
        None => "{}".into(),
    };
    json(
        ctx.http
            .post(url)
            .bearer_auth(token)
            .header("User-Agent", ANTIGRAVITY_USER_AGENT)
            .header("Content-Type", "application/json")
            .header("X-Client-Name", "antigravity")
            .header("X-Client-Version", ANTIGRAVITY_IDE_VERSION)
            .timeout(ctx.timeout)
            .body(body),
    )
    .await
}

// ─── Pure parsers (testable without network) ───────────────────────

#[derive(Clone, Debug)]
struct QuotaEntry {
    remaining_fraction: Option<f64>,
    reset_at: Option<DateTime<Utc>>,
    display_name: String,
}

#[derive(Default)]
struct QuotaMap {
    inner: std::collections::BTreeMap<String, QuotaEntry>,
}

impl QuotaMap {
    fn new() -> Self {
        Self::default()
    }
    fn insert(&mut self, key: String, entry: QuotaEntry) {
        self.inner.insert(key, entry);
    }
    fn iter(&self) -> impl Iterator<Item = (&String, &QuotaEntry)> {
        self.inner.iter()
    }
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    fn get(&self, key: &str) -> Option<&QuotaEntry> {
        self.inner.get(key)
    }
}

#[derive(Clone, Debug, Default)]
struct WeeklyBucket {
    remaining_fraction: f64,
    reset_at: Option<DateTime<Utc>>,
    display_name: String,
}

#[derive(Default)]
struct WeeklySummary {
    weekly: std::collections::BTreeMap<String, WeeklyBucket>,
    session: std::collections::BTreeMap<String, WeeklyBucket>,
}

impl WeeklySummary {
    fn is_empty(&self) -> bool {
        self.weekly.is_empty() && self.session.is_empty()
    }
    fn insert_weekly(&mut self, key: String, bucket: WeeklyBucket) {
        // First occurrence wins, identical to 9router.
        self.weekly.entry(key).or_insert(bucket);
    }
    fn insert_session(&mut self, key: String, bucket: WeeklyBucket) {
        self.session.entry(key).or_insert(bucket);
    }
    fn iter_weekly(&self) -> impl Iterator<Item = (&String, &WeeklyBucket)> {
        self.weekly.iter()
    }
    fn iter_session(&self) -> impl Iterator<Item = (&String, &WeeklyBucket)> {
        self.session.iter()
    }
    #[cfg(test)]
    fn get_weekly(&self, key: &str) -> Option<&WeeklyBucket> {
        self.weekly.get(key)
    }
    fn get_session(&self, key: &str) -> Option<&WeeklyBucket> {
        self.session.get(key)
    }
    fn get_session_mut(&mut self, key: &str) -> Option<&mut WeeklyBucket> {
        self.session.get_mut(key)
    }
}

fn parse_paid_tier_models(
    models: &serde_json::Map<String, serde_json::Value>,
    quotas: &mut QuotaMap,
) {
    for (model_key, info_value) in models {
        if !IMPORTANT_MODELS.contains(&model_key.as_str()) {
            continue;
        }
        let info = match info_value.as_object() {
            Some(obj) => obj,
            None => continue,
        };
        if info
            .get("isInternal")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let quota_info = info.get("quotaInfo").and_then(serde_json::Value::as_object);
        let remaining_fraction = quota_info
            .and_then(|quota| quota.get("remainingFraction"))
            .and_then(parse_fraction);
        let reset_at = quota_info
            .and_then(|quota| quota.get("resetTime"))
            .and_then(parse_reset_value);

        let display_name = info
            .get("displayName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(model_key)
            .to_string();

        quotas.insert(
            model_key.clone(),
            QuotaEntry {
                remaining_fraction,
                reset_at,
                display_name,
            },
        );
    }
}

fn parse_fraction(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|f| f.is_finite())
}

/// Accepts an RFC 3339 string, an epoch number in seconds or milliseconds,
/// or a number in a string. `None` when malformed or absent.
fn parse_reset_value(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    match value {
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if trimmed.bytes().all(|b| b.is_ascii_digit()) {
                let numeric: f64 = trimmed.parse().ok()?;
                return epoch_to_datetime(numeric);
            }
            DateTime::parse_from_rfc3339(trimmed)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|| {
                    NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|n| n.and_utc())
                })
        }
        serde_json::Value::Number(n) => n.as_f64().and_then(epoch_to_datetime),
        _ => None,
    }
}

fn epoch_to_datetime(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() {
        return None;
    }
    let millis = if value.abs() < 1e12 {
        value * 1000.0
    } else {
        value
    };
    DateTime::from_timestamp_millis(millis as i64)
}

fn parse_weekly_summary(envelope: &QuotaSummaryEnvelope) -> WeeklySummary {
    let mut summary = WeeklySummary::default();

    let groups: &[QuotaGroup] = envelope
        .groups
        .as_deref()
        .or_else(|| {
            envelope
                .quota_summary
                .as_ref()
                .and_then(|n| n.groups.as_deref())
        })
        .unwrap_or(&[]);

    for group in groups {
        let display_name = group
            .display_name
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();

        let buckets = match group.buckets.as_deref() {
            Some(b) => b,
            None => continue,
        };

        for bucket in buckets {
            let window = bucket
                .window
                .as_deref()
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            let bucket_text = format!(
                "{} {}",
                bucket.bucket_id.as_deref().unwrap_or(""),
                bucket.display_name.as_deref().unwrap_or("")
            )
            .to_ascii_lowercase();

            let is_weekly = window == "weekly"
                || bucket_text.contains("weekly")
                || bucket_text.contains("semanal");
            let is_session = window == "5h"
                || window == "daily"
                || bucket_text.contains("five hour")
                || bucket_text.contains("5h")
                || bucket_text.contains("daily");
            if !is_weekly && !is_session {
                continue;
            }

            let disabled = bucket.disabled.unwrap_or(false);
            if disabled && is_weekly {
                continue;
            }
            let fraction = if disabled {
                0.0
            } else {
                match bucket.remaining_fraction.as_ref().and_then(parse_fraction) {
                    Some(f) if f.is_finite() => f,
                    _ => continue,
                }
            };

            let reset_at = bucket.reset_time.as_ref().and_then(parse_reset_value);

            let config = GROUP_CONFIGS
                .iter()
                .find(|c| group_matches(c.pattern, &display_name));
            let Some(config) = config else { continue };

            let key = if is_weekly {
                config.weekly_key
            } else {
                config.session_key
            };
            let label = if is_weekly {
                config.weekly_label
            } else {
                config.session_label
            };
            let bucket_entry = WeeklyBucket {
                remaining_fraction: fraction,
                reset_at,
                display_name: label.to_string(),
            };

            if is_weekly {
                summary.insert_weekly(key.to_string(), bucket_entry);
            } else {
                summary.insert_session(key.to_string(), bucket_entry);
            }
        }
    }

    summary
}

/// "gemini" or "claude" family. If all the family's models have
/// `remainingFraction == 0` and the 5h session still shows a balance > 0,
/// forces the session to `0%` with the family's furthest `resetAt`.
fn reconcile_exhausted_family(quotas: &mut QuotaMap, weekly: &mut WeeklySummary, family: &str) {
    let session_key = match family {
        "gemini" => "gemini_session",
        "claude" => "claude_gpt_session",
        _ => return,
    };

    let family_keys: Vec<String> = match family {
        "gemini" => quotas
            .inner
            .keys()
            .filter(|k| {
                let k = k.as_str();
                k.starts_with("gemini-") && !k.contains("image")
            })
            .cloned()
            .collect(),
        "claude" => quotas
            .inner
            .keys()
            .filter(|k| {
                let k = k.as_str();
                k.starts_with("claude-") || k.starts_with("gpt-")
            })
            .cloned()
            .collect(),
        _ => return,
    };

    if family_keys.is_empty() {
        return;
    }

    let all_exhausted = family_keys.iter().all(|k| {
        quotas
            .get(k)
            .is_some_and(|e| e.remaining_fraction == Some(0.0))
    });
    if !all_exhausted {
        return;
    }

    let orig_reset = match weekly.get_session(session_key) {
        Some(s) if s.remaining_fraction > 0.0 => s.reset_at,
        _ => return,
    };

    let max_reset = family_keys
        .iter()
        .filter_map(|k| quotas.get(k).and_then(|e| e.reset_at))
        .max();

    if let Some(slot) = weekly.get_session_mut(session_key) {
        slot.remaining_fraction = 0.0;
        if max_reset.is_some() {
            slot.reset_at = max_reset.or(orig_reset);
        }
    }
}

// ─── Unit tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Status};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct StaticCred(&'static str, &'static str);
    impl Credentials for StaticCred {
        fn get(&self, name: &str) -> Option<String> {
            (name == self.0).then(|| self.1.to_string())
        }
    }

    fn account() -> Account {
        Account {
            id: None,
            name: "ag".into(),
            provider: Provider::Antigravity,
            credential_env: Some(DEFAULT_CREDENTIAL_ENV.into()),
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
            timeout: Duration::from_secs(5),
        }
    }

    // ── parse_reset_value ───────────────────────────────────────────

    #[test]
    fn parse_reset_value_lida_rfc3339() {
        let v = json!("2026-09-13T12:00:00Z");
        let parsed = parse_reset_value(&v).expect("rfc3339");
        assert_eq!(parsed.to_rfc3339(), "2026-09-13T12:00:00+00:00");
    }

    #[test]
    fn parse_reset_value_lida_epoch_segundos() {
        let v = json!(1_700_000_000.0_f64);
        let parsed = parse_reset_value(&v).expect("epoch seconds");
        assert_eq!(parsed.timestamp(), 1_700_000_000);
    }

    #[test]
    fn parse_reset_value_lida_epoch_millis() {
        let v = json!(1_700_000_000_000_i64);
        let parsed = parse_reset_value(&v).expect("epoch millis");
        assert_eq!(parsed.timestamp_millis(), 1_700_000_000_000);
    }

    #[test]
    fn parse_reset_value_rejeita_infinito() {
        let v = json!(f64::INFINITY);
        assert!(parse_reset_value(&v).is_none());
    }

    #[test]
    fn parse_reset_value_rejeita_string_invalida() {
        let v = json!("ontem");
        assert!(parse_reset_value(&v).is_none());
    }

    #[test]
    fn parse_reset_value_rejeita_valor_ausente() {
        assert!(parse_reset_value(&json!(null)).is_none());
        assert!(parse_reset_value(&json!("")).is_none());
    }

    // ── parse_paid_tier_models ──────────────────────────────────────

    #[test]
    fn parse_paid_tier_models_pula_internos_e_desconhecidos() {
        let mut models = serde_json::Map::new();
        models.insert(
            "internal-model".into(),
            json!({
                "displayName": "Internal",
                "isInternal": true,
                "quotaInfo": { "remainingFraction": 0.5 }
            }),
        );
        models.insert(
            "unknown-model".into(),
            json!({
                "displayName": "Mystery",
                "quotaInfo": { "remainingFraction": 0.5 }
            }),
        );
        let mut quotas = QuotaMap::new();
        parse_paid_tier_models(&models, &mut quotas);
        assert!(quotas.is_empty());
    }

    #[test]
    fn parse_paid_tier_models_preserva_fraction_e_displayname() {
        let mut models = serde_json::Map::new();
        models.insert(
            "gemini-3.8-flash-high".into(),
            json!({
                "displayName": "Gemini 3.8 Flash (High)",
                "quotaInfo": {
                    "remainingFraction": 0.85,
                    "resetTime": "2026-08-25T12:00:00Z"
                }
            }),
        );
        let mut quotas = QuotaMap::new();
        parse_paid_tier_models(&models, &mut quotas);
        let entry = quotas.get("gemini-3.8-flash-high").expect("entry");
        assert!((entry.remaining_fraction.unwrap() - 0.85).abs() < 1e-9);
        assert_eq!(entry.display_name, "Gemini 3.8 Flash (High)");
    }

    #[test]
    fn parse_paid_tier_models_preserves_unknown_fraction() {
        let mut models = serde_json::Map::new();
        for (key, quota_info) in [
            (
                "claude-sonnet-4-6",
                json!({ "resetTime": "2026-09-13T12:00:00Z" }),
            ),
            (
                "claude-opus-4-6-thinking",
                json!({ "remainingFraction": "unknown" }),
            ),
        ] {
            models.insert(key.into(), json!({ "quotaInfo": quota_info }));
        }
        let mut quotas = QuotaMap::new();
        parse_paid_tier_models(&models, &mut quotas);
        for key in models.keys() {
            assert_eq!(quotas.get(key).unwrap().remaining_fraction, None);
        }
        let metric = build_model_metric(
            "Claude Sonnet".into(),
            quotas.get("claude-sonnet-4-6").unwrap(),
        );
        assert_eq!(
            (metric.used, metric.limit, metric.remaining),
            (None, None, None)
        );
        assert_eq!(metric.kind, MetricKind::Quota);
        assert_eq!(metric.window, Some(MetricWindow::Window5h));
    }

    #[test]
    fn unknown_model_prevents_false_session_exhaustion() {
        for unknown in [
            json!({"quotaInfo": {"remainingFraction": "unknown"}}),
            json!({}),
        ] {
            let models = serde_json::Map::from_iter([
                (
                    "claude-sonnet-4-6".into(),
                    json!({"quotaInfo": {"remainingFraction": 0.0}}),
                ),
                ("claude-opus-4-6-thinking".into(), unknown),
            ]);
            let mut quotas = QuotaMap::new();
            parse_paid_tier_models(&models, &mut quotas);
            assert_eq!(
                quotas
                    .get("claude-opus-4-6-thinking")
                    .unwrap()
                    .remaining_fraction,
                None
            );
            let mut weekly = WeeklySummary::default();
            weekly.insert_session(
                "claude_gpt_session".into(),
                WeeklyBucket {
                    remaining_fraction: 0.5,
                    reset_at: None,
                    display_name: "Claude & GPT (5h)".into(),
                },
            );
            reconcile_exhausted_family(&mut quotas, &mut weekly, "claude");
            assert_eq!(
                weekly
                    .get_session("claude_gpt_session")
                    .unwrap()
                    .remaining_fraction,
                0.5
            );
        }
    }

    #[test]
    fn parse_paid_tier_models_aceita_fraction_string() {
        let mut models = serde_json::Map::new();
        models.insert(
            "gemini-3.7-flash-low".into(),
            json!({
                "displayName": "Gemini 3.7 Flash (Low)",
                "quotaInfo": { "remainingFraction": "0.35" }
            }),
        );
        let mut quotas = QuotaMap::new();
        parse_paid_tier_models(&models, &mut quotas);
        assert_eq!(
            quotas
                .get("gemini-3.7-flash-low")
                .unwrap()
                .remaining_fraction,
            Some(0.35)
        );
    }

    #[test]
    fn parse_paid_tier_models_preserva_uso_excedente() {
        // The server may report a negative remainingFraction (usage >100%).
        let mut models = serde_json::Map::new();
        models.insert(
            "claude-opus-4-6-thinking".into(),
            json!({
                "displayName": "Claude Opus 4.6",
                "quotaInfo": { "remainingFraction": -0.10 }
            }),
        );
        let mut quotas = QuotaMap::new();
        parse_paid_tier_models(&models, &mut quotas);
        assert_eq!(
            quotas
                .get("claude-opus-4-6-thinking")
                .unwrap()
                .remaining_fraction,
            Some(-0.10)
        );
    }

    // ── parse_weekly_summary ────────────────────────────────────────

    fn sample_group() -> QuotaGroup {
        QuotaGroup {
            display_name: Some("Gemini Models".into()),
            buckets: Some(vec![
                QuotaBucket {
                    window: Some("weekly".into()),
                    bucket_id: Some("gemini-weekly-bucket".into()),
                    display_name: Some("Weekly Limit".into()),
                    remaining_fraction: Some(json!(0.75)),
                    reset_time: Some(json!("2026-09-15T00:00:00Z")),
                    disabled: None,
                },
                QuotaBucket {
                    window: Some("5h".into()),
                    bucket_id: Some("gemini-5h-bucket".into()),
                    display_name: Some("Five Hour".into()),
                    remaining_fraction: Some(json!(0.9)),
                    reset_time: Some(json!("2026-09-09T00:00:00Z")),
                    disabled: None,
                },
            ]),
        }
    }

    fn claude_group() -> QuotaGroup {
        QuotaGroup {
            display_name: Some("Claude and GPT models".into()),
            buckets: Some(vec![QuotaBucket {
                window: Some("weekly".into()),
                bucket_id: Some("claude-gpt-weekly".into()),
                display_name: Some("Weekly Quota".into()),
                remaining_fraction: Some(json!(0.5)),
                reset_time: Some(json!("2026-09-14T00:00:00Z")),
                disabled: None,
            }]),
        }
    }

    #[test]
    fn parse_weekly_summary_extrai_gemini_e_claude() {
        let envelope = QuotaSummaryEnvelope {
            groups: Some(vec![sample_group(), claude_group()]),
            quota_summary: None,
        };
        let summary = parse_weekly_summary(&envelope);
        let weekly = summary.get_weekly("gemini_weekly").expect("gemini weekly");
        assert_eq!(weekly.display_name, "Gemini (Semanal)");
        assert!((weekly.remaining_fraction - 0.75).abs() < 1e-9);
        let session = summary
            .get_session("gemini_session")
            .expect("gemini session");
        assert_eq!(session.display_name, "Gemini (5h)");
        let claude = summary
            .get_weekly("claude_gpt_weekly")
            .expect("claude weekly");
        assert_eq!(claude.display_name, "Claude & GPT (Semanal)");
        assert!((claude.remaining_fraction - 0.5).abs() < 1e-9);
    }

    #[test]
    fn parse_weekly_summary_aceita_formato_aninhado() {
        let envelope = QuotaSummaryEnvelope {
            groups: None,
            quota_summary: Some(NestedSummary {
                groups: Some(vec![sample_group()]),
            }),
        };
        let summary = parse_weekly_summary(&envelope);
        assert!(summary.get_weekly("gemini_weekly").is_some());
    }

    #[test]
    fn parse_weekly_summary_descarta_bucket_mensal() {
        let envelope = QuotaSummaryEnvelope {
            groups: Some(vec![QuotaGroup {
                display_name: Some("Gemini Models".into()),
                buckets: Some(vec![QuotaBucket {
                    window: Some("monthly".into()),
                    bucket_id: Some("g-monthly".into()),
                    display_name: Some("Monthly Limit".into()),
                    remaining_fraction: Some(json!(0.9)),
                    reset_time: None,
                    disabled: None,
                }]),
            }]),
            quota_summary: None,
        };
        let summary = parse_weekly_summary(&envelope);
        assert!(summary.is_empty());
    }

    #[test]
    fn parse_weekly_summary_descarta_weekly_disabled() {
        let envelope = QuotaSummaryEnvelope {
            groups: Some(vec![QuotaGroup {
                display_name: Some("Gemini Models".into()),
                buckets: Some(vec![QuotaBucket {
                    window: Some("weekly".into()),
                    bucket_id: Some("gemini-weekly".into()),
                    display_name: Some("Weekly".into()),
                    remaining_fraction: Some(json!(0.75)),
                    reset_time: None,
                    disabled: Some(true),
                }]),
            }]),
            quota_summary: None,
        };
        let summary = parse_weekly_summary(&envelope);
        assert!(summary.is_empty());
    }

    #[test]
    fn parse_weekly_summary_mantem_sessao_disabled_com_zero() {
        let envelope = QuotaSummaryEnvelope {
            groups: Some(vec![QuotaGroup {
                display_name: Some("Gemini Models".into()),
                buckets: Some(vec![QuotaBucket {
                    window: Some("5h".into()),
                    bucket_id: Some("gemini-5h".into()),
                    display_name: Some("Five Hour".into()),
                    remaining_fraction: Some(json!(0.5)),
                    reset_time: None,
                    disabled: Some(true),
                }]),
            }]),
            quota_summary: None,
        };
        let summary = parse_weekly_summary(&envelope);
        let session = summary.get_session("gemini_session").expect("session");
        assert_eq!(session.remaining_fraction, 0.0);
    }

    #[test]
    fn parse_weekly_summary_rejeita_fraction_nao_finita() {
        let envelope = QuotaSummaryEnvelope {
            groups: Some(vec![QuotaGroup {
                display_name: Some("Gemini Models".into()),
                buckets: Some(vec![QuotaBucket {
                    window: Some("weekly".into()),
                    bucket_id: Some("gemini-weekly".into()),
                    display_name: Some("Weekly".into()),
                    remaining_fraction: Some(json!("not-a-number")),
                    reset_time: None,
                    disabled: None,
                }]),
            }]),
            quota_summary: None,
        };
        let summary = parse_weekly_summary(&envelope);
        assert!(summary.is_empty());
    }

    #[test]
    fn parse_weekly_summary_ignora_familia_desconhecida() {
        let envelope = QuotaSummaryEnvelope {
            groups: Some(vec![QuotaGroup {
                display_name: Some("Unknown AI Provider".into()),
                buckets: Some(vec![QuotaBucket {
                    window: Some("weekly".into()),
                    bucket_id: Some("weekly".into()),
                    display_name: Some("Weekly".into()),
                    remaining_fraction: Some(json!(0.5)),
                    reset_time: None,
                    disabled: None,
                }]),
            }]),
            quota_summary: None,
        };
        let summary = parse_weekly_summary(&envelope);
        assert!(summary.is_empty());
    }

    #[test]
    fn parse_weekly_summary_rejeita_input_sem_groups() {
        let envelope = QuotaSummaryEnvelope::default();
        let summary = parse_weekly_summary(&envelope);
        assert!(summary.is_empty());
    }

    // ── reconcile_exhausted_family ──────────────────────────────────

    #[test]
    fn reconcilia_gemini_session_zero_quando_familia_exausta() {
        let mut quotas = QuotaMap::new();
        quotas.insert(
            "gemini-3.8-flash-high".into(),
            QuotaEntry {
                remaining_fraction: Some(0.0),
                reset_at: Some(
                    DateTime::parse_from_rfc3339("2026-09-13T12:00:00Z")
                        .unwrap()
                        .with_timezone(&Utc),
                ),
                display_name: "Gemini 3.8 Flash (High)".into(),
            },
        );
        quotas.insert(
            "gemini-3.7-flash-low".into(),
            QuotaEntry {
                remaining_fraction: Some(0.0),
                reset_at: Some(
                    DateTime::parse_from_rfc3339("2026-09-13T18:00:00Z")
                        .unwrap()
                        .with_timezone(&Utc),
                ),
                display_name: "Gemini 3.7 Flash (Low)".into(),
            },
        );
        let mut weekly = WeeklySummary::default();
        weekly.insert_session(
            "gemini_session".into(),
            WeeklyBucket {
                remaining_fraction: 1.0,
                reset_at: Some(
                    DateTime::parse_from_rfc3339("2026-09-13T10:00:00Z")
                        .unwrap()
                        .with_timezone(&Utc),
                ),
                display_name: "Gemini (5h)".into(),
            },
        );

        reconcile_exhausted_family(&mut quotas, &mut weekly, "gemini");

        let session = weekly.get_session("gemini_session").expect("session");
        assert_eq!(session.remaining_fraction, 0.0);
        let expected = DateTime::parse_from_rfc3339("2026-09-13T18:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(session.reset_at.unwrap(), expected);
    }

    #[test]
    fn reconciliacao_nao_toca_quando_familia_nao_exausta() {
        let mut quotas = QuotaMap::new();
        quotas.insert(
            "gemini-3.8-flash-high".into(),
            QuotaEntry {
                remaining_fraction: Some(0.9),
                reset_at: None,
                display_name: "Gemini 3.8 Flash (High)".into(),
            },
        );
        let mut weekly = WeeklySummary::default();
        weekly.insert_session(
            "gemini_session".into(),
            WeeklyBucket {
                remaining_fraction: 0.5,
                reset_at: None,
                display_name: "Gemini (5h)".into(),
            },
        );

        reconcile_exhausted_family(&mut quotas, &mut weekly, "gemini");

        let session = weekly.get_session("gemini_session").expect("session");
        assert_eq!(session.remaining_fraction, 0.5);
    }

    #[test]
    fn reconciliacao_nao_clobber_weekly_quando_sessao_ja_esta_zero() {
        let mut quotas = QuotaMap::new();
        quotas.insert(
            "claude-sonnet-4-6".into(),
            QuotaEntry {
                remaining_fraction: Some(0.0),
                reset_at: None,
                display_name: "Claude Sonnet 4.6".into(),
            },
        );
        let mut weekly = WeeklySummary::default();
        weekly.insert_session(
            "claude_gpt_session".into(),
            WeeklyBucket {
                remaining_fraction: 0.0,
                reset_at: None,
                display_name: "Claude & GPT (5h)".into(),
            },
        );
        weekly.insert_weekly(
            "claude_gpt_weekly".into(),
            WeeklyBucket {
                remaining_fraction: 0.75,
                reset_at: Some(
                    DateTime::parse_from_rfc3339("2026-09-15T00:00:00Z")
                        .unwrap()
                        .with_timezone(&Utc),
                ),
                display_name: "Claude & GPT (Semanal)".into(),
            },
        );

        reconcile_exhausted_family(&mut quotas, &mut weekly, "claude");

        let session = weekly.get_session("claude_gpt_session").expect("session");
        assert_eq!(session.remaining_fraction, 0.0);
        let wk = weekly.get_weekly("claude_gpt_weekly").expect("weekly");
        assert!((wk.remaining_fraction - 0.75).abs() < 1e-9);
    }

    // ── ProjectId normalization (string / objeto / ausente) ──────────

    #[test]
    fn project_id_aceita_string() {
        let body: LoadCodeAssistResponse = serde_json::from_value(json!({
            "cloudaicompanionProject": "projects-1"
        }))
        .unwrap();
        assert_eq!(body.cloudaicompanion_project.as_deref(), Some("projects-1"));
    }

    #[test]
    fn project_id_aceita_objeto_com_id() {
        let body: LoadCodeAssistResponse = serde_json::from_value(json!({
            "cloudaicompanionProject": { "id": "projects-2" }
        }))
        .unwrap();
        assert_eq!(body.cloudaicompanion_project.as_deref(), Some("projects-2"));
    }

    #[test]
    fn project_id_objeto_sem_id_vira_none() {
        let body: LoadCodeAssistResponse = serde_json::from_value(json!({
            "cloudaicompanionProject": { "name": "no-id" }
        }))
        .unwrap();
        assert!(body.cloudaicompanion_project.is_none());
    }

    #[test]
    fn project_id_string_vazia_vira_none() {
        let body: LoadCodeAssistResponse = serde_json::from_value(json!({
            "cloudaicompanionProject": "  "
        }))
        .unwrap();
        assert!(body.cloudaicompanion_project.is_none());
    }

    #[test]
    fn project_id_ausente_vira_none() {
        let body: LoadCodeAssistResponse = serde_json::from_value(json!({})).unwrap();
        assert!(body.cloudaicompanion_project.is_none());
    }

    // ── HTTP: wiremock do test seam fetch_antigravity_at ────────────

    fn paid_tier_payload() -> serde_json::Value {
        json!({
            "cloudaicompanionProject": "p1",
            "paidTier": { "id": "g1-pro-tier", "name": "Google AI Pro" }
        })
    }

    fn free_tier_payload() -> serde_json::Value {
        json!({
            "cloudaicompanionProject": "p-free",
            "paidTier": { "id": "free-tier" }
        })
    }

    fn paid_models_payload() -> serde_json::Value {
        json!({
            "models": {
                "gemini-3.8-flash-high": {
                    "displayName": "Gemini 3.8 Flash (High)",
                    "quotaInfo": {
                        "remainingFraction": 0.85,
                        "resetTime": "2026-09-13T12:00:00Z"
                    }
                },
                "claude-sonnet-4-6": {
                    "displayName": "Claude Sonnet 4.6",
                    "quotaInfo": {
                        "remainingFraction": 0.50,
                        "resetTime": "2026-09-13T12:00:00Z"
                    }
                }
            }
        })
    }

    fn weekly_payload_paid_reconciles_session() -> serde_json::Value {
        json!({
            "groups": [{
                "displayName": "Gemini Models",
                "buckets": [
                    {
                        "window": "weekly",
                        "remainingFraction": 0.75,
                        "resetTime": "2026-09-15T00:00:00Z"
                    },
                    {
                        "window": "5h",
                        "remainingFraction": 1.0,
                        "resetTime": "2026-09-13T10:00:00Z"
                    }
                ]
            }]
        })
    }

    fn weekly_payload_free() -> serde_json::Value {
        json!({
            "groups": [{
                "displayName": "Gemini Models",
                "buckets": [{
                    "window": "weekly",
                    "remainingFraction": 1.0,
                    "resetTime": "2026-09-15T00:00:00Z"
                }]
            }, {
                "displayName": "Claude and GPT models",
                "buckets": [{
                    "window": "weekly",
                    "remainingFraction": 0.0,
                    "resetTime": "2026-09-13T12:00:00Z"
                }]
            }]
        })
    }

    async fn run_with_mock(
        load_payload: serde_json::Value,
        models_payload: serde_json::Value,
        weekly_payload: serde_json::Value,
    ) -> Result<Data, QueryError> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(load_payload))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(models_payload))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuotaSummary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(weekly_payload))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = StaticCred(DEFAULT_CREDENTIAL_ENV, "test-token");
        let ctx = ctx(&http, &creds);
        let base = server.uri();
        fetch_antigravity_at(
            &account(),
            &ctx,
            &format!("{base}/v1internal:loadCodeAssist"),
            &format!("{base}/v1internal:fetchAvailableModels"),
            &format!("{base}/v1internal:retrieveUserQuotaSummary"),
        )
        .await
    }

    async fn catalog(
        load: serde_json::Value,
        models_response: ResponseTemplate,
    ) -> (Result<Vec<crate::models::ModelInfo>, QueryError>, usize) {
        use wiremock::matchers::body_json;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(load))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .and(body_json(json!({ "project": "p1" })))
            .respond_with(models_response)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let creds = StaticCred(DEFAULT_CREDENTIAL_ENV, "test-token");
        let base = server.uri();
        let result = models(
            &account(),
            &ctx(&http, &creds),
            &format!("{base}/v1internal:loadCodeAssist"),
            &format!("{base}/v1internal:fetchAvailableModels"),
        )
        .await;
        let calls = server.received_requests().await.unwrap().len();
        (result, calls)
    }

    #[tokio::test]
    async fn catalog_lists_every_public_model_of_paid_sessions_only() {
        let mut payload = paid_models_payload();
        payload["models"]["unlisted-model"] = json!({});
        payload["models"]["internal-model"] = json!({ "isInternal": true });
        let ok = ResponseTemplate::new(200).set_body_json(payload);
        let (models, calls) = catalog(paid_tier_payload(), ok.clone()).await;
        let models = models.unwrap();
        let mut ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                "claude-sonnet-4-6",
                "gemini-3.8-flash-high",
                "unlisted-model"
            ],
            "not limited to the quota allow-list; internal entries dropped"
        );
        assert!(models
            .iter()
            .any(|m| m.name.as_deref() == Some("Claude Sonnet 4.6")));
        assert_eq!(calls, 2);

        let (free, calls) = catalog(free_tier_payload(), ok).await;
        assert_eq!(free.unwrap_err().status, Status::Unsupported);
        assert_eq!(calls, 1, "free tier never calls fetchAvailableModels");

        let no_models = ResponseTemplate::new(200).set_body_json(json!({}));
        let (empty, _) = catalog(paid_tier_payload(), no_models).await;
        assert_eq!(empty.unwrap_err().status, Status::Unsupported);

        let bad_entry = ResponseTemplate::new(200).set_body_json(json!({ "models": { "x": 1 } }));
        let (bad, _) = catalog(paid_tier_payload(), bad_entry).await;
        assert_eq!(bad.unwrap_err().status, Status::Error);

        let denied = ResponseTemplate::new(403).set_body_string("test-token");
        let (denied, _) = catalog(paid_tier_payload(), denied).await;
        let denied = denied.unwrap_err();
        assert_eq!(denied.status, Status::AuthenticationError);
        assert!(!denied.note.fallback.contains("test-token"));
    }

    #[tokio::test]
    async fn paid_tier_emit_modelos_e_weekly_com_sessao_reconciliada() {
        let data = run_with_mock(
            paid_tier_payload(),
            paid_models_payload(),
            weekly_payload_paid_reconciles_session(),
        )
        .await
        .expect("ok");

        let labels: Vec<&str> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        assert!(labels.contains(&"Gemini 3.8 Flash (High)"));
        assert!(labels.contains(&"Claude Sonnet 4.6"));
        assert!(labels.contains(&"Gemini (Semanal)"));
        assert!(labels.contains(&"Gemini (5h)"));
        assert!(data.metrics.iter().all(|m| m.kind == MetricKind::Quota));
        // Per-model quotas are scoped; the family aggregates are not.
        let flash = data
            .metrics
            .iter()
            .find(|m| m.label == "Gemini 3.8 Flash (High)")
            .unwrap();
        assert_eq!(
            flash.scope.as_ref().map(|s| s.kind),
            Some(crate::ScopeKind::Model)
        );
        assert!(data
            .metrics
            .iter()
            .find(|m| m.label == "Gemini (5h)")
            .unwrap()
            .scope
            .is_none());

        // Reconciliation: gemini_3.8_flash_high has 0.85 (not zero),
        // so the session stays intact at 100%.
        let session = data
            .metrics
            .iter()
            .find(|m| m.label == "Gemini (5h)")
            .expect("session metric");
        assert!((session.remaining.unwrap() - 100.0).abs() < 1e-9);
        assert_eq!(session.window, Some(MetricWindow::Window5h));
        let weekly = data
            .metrics
            .iter()
            .find(|m| m.label == "Gemini (Semanal)")
            .expect("weekly metric");
        assert_eq!(weekly.window, Some(MetricWindow::Weekly));
    }

    #[tokio::test]
    async fn free_tier_apenas_weekly_sem_per_model() {
        // A mock for models also exists, but must not be called.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(free_tier_payload()))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuotaSummary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(weekly_payload_free()))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = StaticCred(DEFAULT_CREDENTIAL_ENV, "test-token");
        let ctx = ctx(&http, &creds);
        let base = server.uri();
        let data = fetch_antigravity_at(
            &account(),
            &ctx,
            &format!("{base}/v1internal:loadCodeAssist"),
            &format!("{base}/v1internal:fetchAvailableModels"),
            &format!("{base}/v1internal:retrieveUserQuotaSummary"),
        )
        .await
        .expect("ok");

        let labels: Vec<&str> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        assert!(labels.contains(&"Gemini (Semanal)"));
        assert!(labels.contains(&"Claude & GPT (Semanal)"));
        // Nenhum per-model.
        assert!(!labels.iter().any(|l| l.starts_with("Gemini 3.")));
    }

    #[tokio::test]
    async fn paid_tier_models_falham_mas_weekly_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(paid_tier_payload()))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuotaSummary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(weekly_payload_free()))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = StaticCred(DEFAULT_CREDENTIAL_ENV, "test-token");
        let ctx = ctx(&http, &creds);
        let base = server.uri();
        let data = fetch_antigravity_at(
            &account(),
            &ctx,
            &format!("{base}/v1internal:loadCodeAssist"),
            &format!("{base}/v1internal:fetchAvailableModels"),
            &format!("{base}/v1internal:retrieveUserQuotaSummary"),
        )
        .await
        .expect("ok");

        // Weekly only; PARCIAL note about models.
        let labels: Vec<&str> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        assert!(labels.contains(&"Gemini (Semanal)"));
        assert!(
            data.notes
                .iter()
                .any(|n| n.contains("PARTIAL") && n.contains("models")),
            "expected note about modelos"
        );
    }

    #[tokio::test]
    async fn paid_tier_weekly_falha_mas_models_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(paid_tier_payload()))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(paid_models_payload()))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuotaSummary"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = StaticCred(DEFAULT_CREDENTIAL_ENV, "test-token");
        let ctx = ctx(&http, &creds);
        let base = server.uri();
        let data = fetch_antigravity_at(
            &account(),
            &ctx,
            &format!("{base}/v1internal:loadCodeAssist"),
            &format!("{base}/v1internal:fetchAvailableModels"),
            &format!("{base}/v1internal:retrieveUserQuotaSummary"),
        )
        .await
        .expect("ok");

        let labels: Vec<&str> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        // Per-model still emitted.
        assert!(labels.contains(&"Gemini 3.8 Flash (High)"));
        assert!(labels.contains(&"Claude Sonnet 4.6"));
        // PARCIAL note about weekly/summary.
        assert!(
            data.notes
                .iter()
                .any(|n| n.contains("PARTIAL") && n.contains("summary")),
            "expected note about resumo"
        );
    }

    #[tokio::test]
    async fn paid_tier_ambos_falham_retorna_err_com_diagnostico() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(paid_tier_payload()))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuotaSummary"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = StaticCred(DEFAULT_CREDENTIAL_ENV, "test-token");
        let ctx = ctx(&http, &creds);
        let base = server.uri();
        let err = fetch_antigravity_at(
            &account(),
            &ctx,
            &format!("{base}/v1internal:loadCodeAssist"),
            &format!("{base}/v1internal:fetchAvailableModels"),
            &format!("{base}/v1internal:retrieveUserQuotaSummary"),
        )
        .await
        .unwrap_err();

        let msg = err.note.fallback;
        assert!(
            msg.contains("models") && msg.contains("summary"),
            "erro deve distinguir modelos vs resumo: {msg}"
        );
    }

    #[tokio::test]
    async fn free_tier_weekly_falha_retorna_err() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(free_tier_payload()))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuotaSummary"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let creds = StaticCred(DEFAULT_CREDENTIAL_ENV, "test-token");
        let ctx = ctx(&http, &creds);
        let base = server.uri();
        let err = fetch_antigravity_at(
            &account(),
            &ctx,
            &format!("{base}/v1internal:loadCodeAssist"),
            &format!("{base}/v1internal:fetchAvailableModels"),
            &format!("{base}/v1internal:retrieveUserQuotaSummary"),
        )
        .await
        .unwrap_err();
        let msg = err.note.fallback;
        assert!(msg.contains("summary"), "free sem weekly: {msg}");
    }

    // ── fetch: erros e fallbacks ────────────────────────────────────

    #[tokio::test]
    async fn fetch_falha_sem_credencial() {
        let http = reqwest::Client::new();
        struct Empty;
        impl Credentials for Empty {
            fn get(&self, _: &str) -> Option<String> {
                None
            }
        }
        let creds = Empty;
        let ctx = ctx(&http, &creds);
        let err = fetch_antigravity_at(
            &account(),
            &ctx,
            "https://unused.invalid/load",
            "https://unused.invalid/models",
            "https://unused.invalid/summary",
        )
        .await
        .unwrap_err();
        assert!(matches!(err.status, crate::Status::NeedsConfiguration));
    }
}
