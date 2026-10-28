//! Read-only internal quota interfaces observed in 9router f01fb909e37189008080632ddaf404f096345cde.
//! See THIRD_PARTY_NOTICES for MIT attribution. No token generation/reset calls.
use super::{json, secret, Context, Data, QueryError};
use crate::text::{params, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::Copilot => copilot(account, ctx, "https://api.github.com").await,
        Provider::GeminiAi => gemini_cli(account, ctx, "https://cloudcode-pa.googleapis.com").await,
        _ => Err(QueryError::unsupported(
            "query.router_existing.unsupported_provider",
            "Provider has no query in this adapter.",
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
fn reset(value: Option<&str>) -> Result<Option<DateTime<Utc>>, QueryError> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|t| t.with_timezone(&Utc))
                .or_else(|_| {
                    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                        .map(|d| d.and_hms_opt(0, 0, 0).expect("midnight").and_utc())
                })
                .map_err(|_| QueryError::schema())
        })
        .transpose()
}

#[derive(Deserialize)]
struct CopilotResponse {
    copilot_plan: Option<String>,
    quota_reset_date: Option<String>,
    limited_user_reset_date: Option<String>,
    quota_snapshots: Option<BTreeMap<String, CopilotQuota>>,
    monthly_quotas: Option<BTreeMap<String, f64>>,
    limited_user_quotas: Option<BTreeMap<String, f64>>,
}
#[derive(Deserialize)]
struct CopilotQuota {
    entitlement: Option<f64>,
    remaining: Option<f64>,
    unlimited: Option<bool>,
}

async fn copilot(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "GITHUB_TOKEN")?;
    let response: CopilotResponse = json(
        ctx.http
            .get(format!("{base}/copilot_internal/user"))
            .header("Authorization", format!("token {key}"))
            .header("Accept", "application/json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "GitHubCopilotChat/0.26.7")
            .header("Editor-Version", "vscode/1.100.0")
            .header("Editor-Plugin-Version", "copilot-chat/0.26.7"),
    )
    .await?;
    let mut notes = Notes::new();
    notes.push(
        "note.router_existing.copilot_internal_endpoint",
        "Internal copilot_internal/user endpoint, as in 9router; requires the account's GitHub token (not the inference copilotToken). Subject to change and provider restrictions.",
    );
    if let Some(plan) = response.copilot_plan {
        notes.push_params(
            "note.router_existing.copilot_plan",
            params([("plan", ParamValue::from(plan))]),
            "Plan: {plan}.",
        );
    }
    let mut metrics = Vec::new();
    if let Some(snapshots) = response.quota_snapshots {
        let resets_at = reset(response.quota_reset_date.as_deref())?;
        for (name, quota) in snapshots {
            if quota.unlimited == Some(true) {
                notes.push_params(
                    "note.router_existing.copilot_unlimited_quota",
                    params([("name", ParamValue::from(name))]),
                    "{name}: unlimited per the provider response.",
                );
                continue;
            }
            let limit = quota.entitlement.map(finite).transpose()?;
            let remaining = quota.remaining.map(finite).transpose()?;
            let used = limit
                .zip(remaining)
                .map(|(l, r)| finite(l - r))
                .transpose()?;
            if limit.is_none() && remaining.is_none() {
                continue;
            }
            metrics.push(Metric {
                billing: Default::default(),
                scope: None,
                label: format!("Copilot: {}", copilot_quota_name(&name)),
                used,
                limit,
                remaining,
                unit: "solicitações".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Other),
                period: Some("janela do plano".into()),
                resets_at,
            });
        }
    } else if response.monthly_quotas.is_some() || response.limited_user_quotas.is_some() {
        let resets_at = reset(response.limited_user_reset_date.as_deref())?;
        let totals = response.monthly_quotas.unwrap_or_default();
        let used_quotas = response.limited_user_quotas.unwrap_or_default();
        for name in ["chat", "completions"] {
            let limit = totals.get(name).copied().map(finite).transpose()?;
            // 9router treats limited_user_quotas as used; keep that interpretation
            // explicitly scoped to this internal response, not billing netQuantity.
            let used = used_quotas.get(name).copied().map(finite).transpose()?;
            if limit.is_none() && used.is_none() {
                continue;
            }
            let remaining = limit.zip(used).map(|(l, u)| finite(l - u)).transpose()?;
            metrics.push(Metric {
                billing: Default::default(),
                scope: None,
                label: format!("Copilot: {}", copilot_quota_name(name)),
                used,
                limit,
                remaining,
                unit: "solicitações".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Monthly),
                period: Some("janela mensal do plano".into()),
                resets_at,
            });
        }
    } else {
        return Err(QueryError::schema());
    }
    if metrics.is_empty() {
        notes.push(
            "note.router_existing.copilot_no_numeric_quota",
            "No numeric quota returned; absent does not mean unlimited.",
        );
    }
    Ok(Data::new(
        metrics,
        notes,
        super::router_ref!("usage/github.js"),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiQuota {
    buckets: Vec<GeminiBucket>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiBucket {
    model_id: String,
    remaining_fraction: Option<Value>,
    reset_time: Option<String>,
}

async fn gemini_cli(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let env = account
        .params
        .get("session_env")
        .map(String::as_str)
        .unwrap_or("GEMINI_CLI_ACCESS_TOKEN");
    let token = ctx
        .credentials
        .get(env)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            QueryError::config_params(
                "query.router_existing.missing_gemini_session",
                params([("env", ParamValue::from(env.to_string()))]),
                "Missing Gemini CLI OAuth token in {env}. GEMINI_API_KEY does not authenticate Code Assist; configure params.session_env.",
            )
        })?;
    let project = match account
        .params
        .get("project_id")
        .filter(|s| !s.trim().is_empty())
    {
        Some(project) => project.clone(),
        None => {
            let platform = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
                2
            } else if cfg!(target_os = "macos") {
                1
            } else if cfg!(target_os = "windows") {
                5
            } else if cfg!(target_arch = "aarch64") {
                4
            } else {
                3
            };
            let info: Value = json(ctx.http.post(format!("{base}/v1internal:loadCodeAssist"))
                .bearer_auth(&token).json(&serde_json::json!({"metadata":{"ideType":9,"platform":platform,"pluginType":2}}))).await?;
            let project = &info["cloudaicompanionProject"];
            project
                .as_str()
                .or_else(|| project.get("id").and_then(Value::as_str))
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    QueryError::config(
                        "query.router_existing.missing_gemini_project",
                        "Gemini CLI did not return a Code Assist project; configure params.project_id.",
                    )
                })?
        }
    };
    let data: GeminiQuota = json(
        ctx.http
            .post(format!("{base}/v1internal:retrieveUserQuota"))
            .bearer_auth(token)
            .json(&serde_json::json!({"project":project})),
    )
    .await?;
    let mut metrics = Vec::new();
    for bucket in data.buckets {
        let Some(fraction) = bucket.remaining_fraction else {
            continue;
        };
        let fraction = match fraction {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
        .ok_or_else(QueryError::schema)?;
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(QueryError::schema());
        }
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label: bucket.model_id,
            used: Some((1.0 - fraction) * 100.0),
            limit: Some(100.0),
            remaining: Some(fraction * 100.0),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Other),
            period: None,
            resets_at: reset(bucket.reset_time.as_deref())?,
        });
    }
    let mut notes = Notes::new();
    notes.push(
        "note.router_existing.gemini_code_assist_quota",
        "Gemini CLI/Code Assist quota, NOT the AI Studio API key quota. Internal endpoint used by 9router.",
    );
    notes.push(
        "note.router_existing.gemini_normalized_percentage",
        "`remainingFraction` is reported as a percentage; the normalised 1000 total from 9router is not a real request cap.",
    );
    Ok(Data::new(
        metrics,
        notes,
        super::router_ref!("usage/google.js"),
    ))
}

/// The display name of a Copilot quota key: GitHub's product names for the
/// known keys (`premium_interactions` is "Premium requests" in GitHub's own
/// UI), the key with spaces and a capital otherwise. Never the raw key.
fn copilot_quota_name(key: &str) -> String {
    match key {
        "premium_interactions" => "Premium requests".into(),
        "chat" => "Chat".into(),
        "completions" => "Completions".into(),
        other => {
            let spaced = other.replace('_', " ");
            let mut chars = spaced.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => spaced,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn copilot_quota_keys_get_display_names() {
        assert_eq!(
            super::copilot_quota_name("premium_interactions"),
            "Premium requests"
        );
        assert_eq!(super::copilot_quota_name("chat"), "Chat");
        assert_eq!(super::copilot_quota_name("code_review"), "Code review");
    }

    use super::*;
    use crate::Credentials;
    use std::time::Duration;
    use wiremock::{
        matchers::{body_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    struct Creds;
    impl Credentials for Creds {
        fn get(&self, _: &str) -> Option<String> {
            Some("dummy".into())
        }
    }
    fn account(provider: Provider) -> Account {
        Account {
            id: None,
            name: "test".into(),
            provider,
            credential_env: None,
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }
    #[tokio::test]
    async fn paid_copilot_preserves_remaining_and_does_not_invent_missing_quota() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/copilot_internal/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "copilot_plan":"pro","quota_reset_date":"2026-10-01","quota_snapshots":{
                    "premium_interactions":{"entitlement":300,"remaining":75,"unlimited":false},
                    "chat":{"unlimited":true},"completions":{}}
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds,
            timeout: Duration::from_secs(2),
        };
        let data = copilot(&account(Provider::Copilot), &ctx, &server.uri())
            .await
            .unwrap();
        assert_eq!(data.metrics.len(), 1);
        let metric = &data.metrics[0];
        assert_eq!(metric.used, Some(225.0));
        assert_eq!(metric.remaining, Some(75.0));
        assert_eq!(metric.limit, Some(300.0));
        assert_eq!(metric.kind, MetricKind::Quota);
        assert_eq!(metric.window, Some(MetricWindow::Other));
        assert_eq!(
            metric.resets_at.unwrap().to_rfc3339(),
            "2026-10-01T00:00:00+00:00"
        );
    }
    #[tokio::test]
    async fn gemini_resolves_object_project_and_reports_fraction_as_percentage() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"cloudaicompanionProject":{"id":"project-one"}}),
                ),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/v1internal:retrieveUserQuota")).and(body_json(serde_json::json!({"project":"project-one"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"buckets":[{"modelId":"gemini","remainingFraction":0.75,"resetTime":"2026-10-01T00:00:00Z"}]}))).mount(&server).await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds,
            timeout: Duration::from_secs(2),
        };
        let data = gemini_cli(&account(Provider::GeminiAi), &ctx, &server.uri())
            .await
            .unwrap();
        assert_eq!(data.metrics[0].used, Some(25.0));
        assert_eq!(data.metrics[0].remaining, Some(75.0));
        assert_eq!(data.metrics[0].unit, "%");
        assert_eq!(data.metrics[0].kind, MetricKind::Quota);
        assert_eq!(data.metrics[0].window, Some(MetricWindow::Other));
    }
}
