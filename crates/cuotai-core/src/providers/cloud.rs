//! Cloud providers: Gemini API, Vertex AI, AWS Bedrock and Azure OpenAI.
//!
//! Official sources used:
//! - Google (Gemini API / Vertex AI): Cloud Monitoring REST `projects.timeSeries.list`,
//!   quota metrics `serviceruntime.googleapis.com/quota/limit` (GAUGE, sampled once a day)
//!   e `serviceruntime.googleapis.com/quota/rate/net_usage` (DELTA, 60 s) no recurso
//!   `consumer_quota`.
//! - AWS Bedrock: AWS CLI `aws ce get-cost-and-usage` (Cost Explorer), dimension filter
//!   `SERVICE = Amazon Bedrock`, credentials exclusively through a named profile (`--profile`).
//! - Azure OpenAI: Cost Management REST `Query - Usage`, api-version 2025-03-01, filtro
//!   `ResourceId In [params.resource_id]`, `ActualCost`/`PreTaxCost`, subscription scope.
//!   There is no official evidence of an "Azure OpenAI" billing dimension; the conservative
//!   attribution is per resource explicitly configured by the user.

use super::{json, param, secret, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider, Status};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, TimeZone, Utc};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

const GOOGLE_MONITORING: &str = "https://monitoring.googleapis.com";
const AZURE_MANAGEMENT: &str = "https://management.azure.com";
const AZURE_API_VERSION: &str = "2025-03-01";
const BEDROCK_SERVICE: &str = "Amazon Bedrock";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::GeminiApi => {
            fetch_google(
                GOOGLE_MONITORING,
                "generativelanguage.googleapis.com",
                "Gemini API",
                account,
                ctx,
            )
            .await
        }
        Provider::VertexAi => {
            fetch_google(
                GOOGLE_MONITORING,
                "aiplatform.googleapis.com",
                "Vertex AI",
                account,
                ctx,
            )
            .await
        }
        Provider::AwsBedrock => fetch_bedrock(account, ctx).await,
        Provider::AzureOpenAi => fetch_azure(AZURE_MANAGEMENT, account, ctx).await,
        _ => Err(QueryError::unsupported(
            "query.cloud.unsupported_provider",
            "Provider not handled by the cloud adapter.",
        )),
    }
}

/// Path identifiers (project, subscription, resource group) must never
/// contain separators or query characters: this prevents path injection in the URL.
fn path_id<'a>(value: &'a str, key: &str) -> Result<&'a str, QueryError> {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        Ok(value)
    } else {
        Err(QueryError::config_params(
            "query.cloud.invalid_resource_identifier",
            params([("key", ParamValue::from(key.to_string()))]),
            format!("params.{key} contains invalid characters for a resource identifier."),
        ))
    }
}

// ---------------------------------------------------------------------------
// Google Cloud Monitoring (Gemini API e Vertex AI)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TsList {
    #[serde(default)]
    time_series: Vec<Ts>,
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Ts {
    #[serde(default)]
    metric: Labels,
    #[serde(default)]
    resource: Labels,
    #[serde(default)]
    points: Vec<Point>,
}

#[derive(Debug, Default, Deserialize)]
struct Labels {
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Point {
    interval: PointInterval,
    value: PointValue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PointInterval {
    end_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PointValue {
    int64_value: Option<String>,
    double_value: Option<f64>,
}

impl PointValue {
    fn finite(&self) -> Option<f64> {
        let v = match (&self.int64_value, self.double_value) {
            (Some(s), _) => s.parse::<f64>().ok()?,
            (None, Some(d)) => d,
            (None, None) => return None,
        };
        v.is_finite().then_some(v)
    }
}

/// Paginated time-series query; any missing page, repeated token
/// or invalid point is a schema error, never a partial total presented as complete.
async fn list_series(
    base: &str,
    ctx: &Context<'_>,
    token: &str,
    project: &str,
    filter: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<Ts>, QueryError> {
    let url = format!("{base}/v1/projects/{project}/timeSeries");
    let start = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut all = Vec::new();
    let mut page: Option<String> = None;
    let mut seen: HashSet<String> = HashSet::new();
    loop {
        let mut query = vec![
            ("filter", filter.to_string()),
            ("interval.startTime", start.clone()),
            ("interval.endTime", end.clone()),
        ];
        if let Some(token) = &page {
            query.push(("pageToken", token.clone()));
        }
        let list: TsList = json(ctx.http.get(&url).bearer_auth(token).query(&query)).await?;
        all.extend(list.time_series);
        match list.next_page_token.filter(|t| !t.is_empty()) {
            Some(next) if seen.insert(next.clone()) => page = Some(next),
            Some(_) => return Err(QueryError::schema()),
            None => return Ok(all),
        }
    }
}

fn google_metrics(label: &str, limits: &[Ts], usage: &[Ts]) -> Result<Vec<Metric>, QueryError> {
    let mut metrics = Vec::new();
    for ts in limits {
        if ts.points.is_empty() {
            continue;
        }
        let latest = ts
            .points
            .iter()
            .max_by(|a, b| a.interval.end_time.cmp(&b.interval.end_time))
            .ok_or_else(QueryError::schema)?;
        let value = latest.value.finite().ok_or_else(QueryError::schema)?;
        let quota = ts
            .metric
            .labels
            .get("quota_metric")
            .map(String::as_str)
            .unwrap_or("cota");
        let limit_name = ts
            .metric
            .labels
            .get("limit_name")
            .map(String::as_str)
            .unwrap_or("limite");
        let location = ts
            .resource
            .labels
            .get("location")
            .map(String::as_str)
            .unwrap_or("global");
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label: format!("{label}: {quota} / {limit_name} ({location})"),
            used: None,
            limit: Some(value),
            remaining: None,
            unit: "unidades".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Other),
            period: None,
            resets_at: None,
        });
    }
    for ts in usage {
        if ts.points.is_empty() {
            continue;
        }
        let mut total = 0.0;
        for p in &ts.points {
            total += p.value.finite().ok_or_else(QueryError::schema)?;
        }
        if !total.is_finite() {
            return Err(QueryError::schema());
        }
        let quota = ts
            .metric
            .labels
            .get("quota_metric")
            .map(String::as_str)
            .unwrap_or("cota");
        let location = ts
            .resource
            .labels
            .get("location")
            .map(String::as_str)
            .unwrap_or("global");
        let mut m = Metric::usage(
            format!("{label}: uso recente {quota} ({location})"),
            total,
            "unidades",
            MetricKind::Usage,
            Some(MetricWindow::Other),
        );
        m.period = Some("últimos 10 min".into());
        metrics.push(m);
    }
    Ok(metrics)
}

async fn fetch_google(
    base: &str,
    service: &str,
    label: &str,
    account: &Account,
    ctx: &Context<'_>,
) -> Result<Data, QueryError> {
    let project = path_id(param(account, "project_id")?, "project_id")?;
    let token = secret(account, ctx, "GOOGLE_CLOUD_ACCESS_TOKEN")?;
    let now = Utc::now();
    let limits = list_series(
        base, ctx, &token, project,
        &format!("metric.type = \"serviceruntime.googleapis.com/quota/limit\" AND resource.labels.service = \"{service}\""),
        now - ChronoDuration::hours(26), now,
    ).await?;
    let usage = list_series(
        base, ctx, &token, project,
        &format!("metric.type = \"serviceruntime.googleapis.com/quota/rate/net_usage\" AND resource.labels.service = \"{service}\""),
        now - ChronoDuration::minutes(10), now,
    ).await?;
    let metrics = google_metrics(label, &limits, &usage)?;
    let mut notes = Notes::new();
    notes.push_params(
        "note.cloud.google_scope",
        params([
            ("project", ParamValue::from(project.to_string())),
            ("service", ParamValue::from(service.to_string())),
        ]),
        "Scope: project {project}, service {service}. Limits are current quotas (sampled 1x/day); usage is the sum of net_usage over the last 10 min (~4 min lag).",
    );
    notes.push(
        "note.cloud.google_monitoring_cost",
        "Each dashboard refresh executes 2 read calls to the Cloud Monitoring API (timeSeries.list), which may be billed per the Monitoring API pricing, unlike generation calls.",
    );
    if metrics.is_empty() {
        notes.push_params(
            "note.cloud.google_no_quota_series",
            params([("service", ParamValue::from(service.to_string()))]),
            "No quota series returned. Check whether the {service} API is enabled in the project and that the token has monitoring.timeSeries.list.",
        );
    }
    Ok(Data::new(
        metrics,
        notes,
        "https://cloud.google.com/monitoring/api/ref_v3/rest/v3/projects.timeSeries/list + https://cloud.google.com/monitoring/api/metrics_gcp_p_z#serviceruntime",
    ))
}

// ---------------------------------------------------------------------------
// Azure OpenAI via Cost Management Query
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct QueryResult {
    properties: QueryProperties,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryProperties {
    #[serde(default)]
    columns: Vec<QueryColumn>,
    #[serde(default)]
    rows: Vec<Vec<serde_json::Value>>,
    next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QueryColumn {
    name: String,
}

struct AzureTotals {
    cost: f64,
    currency: String,
    rows: usize,
}

fn azure_accumulate(
    page: &QueryResult,
    totals: &mut Option<AzureTotals>,
) -> Result<(), QueryError> {
    let cost_idx = page
        .properties
        .columns
        .iter()
        .position(|c| c.name == "PreTaxCost")
        .ok_or_else(QueryError::schema)?;
    let currency_idx = page
        .properties
        .columns
        .iter()
        .position(|c| c.name == "Currency")
        .ok_or_else(QueryError::schema)?;
    for row in &page.properties.rows {
        let value = row
            .get(cost_idx)
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
            .ok_or_else(QueryError::schema)?;
        let currency = row
            .get(currency_idx)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(QueryError::schema)?;
        match totals {
            None => {
                *totals = Some(AzureTotals {
                    cost: value,
                    currency: currency.to_string(),
                    rows: 1,
                });
            }
            Some(t) => {
                if t.currency != currency {
                    return Err(QueryError::schema());
                }
                t.cost += value;
                t.rows += 1;
            }
        }
    }
    Ok(())
}

async fn fetch_azure(base: &str, account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    let subscription = path_id(param(account, "subscription_id")?, "subscription_id")?;
    let token = secret(account, ctx, "AZURE_ACCESS_TOKEN")?;
    let resource_id = azure_resource_id(param(account, "resource_id")?, subscription)?;
    let scope = format!("subscriptions/{subscription}");
    let body = serde_json::json!({
        "type": "ActualCost",
        "timeframe": "MonthToDate",
        "dataset": {
            "granularity": "None",
            "aggregation": { "totalCost": { "name": "PreTaxCost", "function": "Sum" } },
            "filter": { "dimensions": { "name": "ResourceId", "operator": "In", "values": [resource_id] } }
        }
    });
    let mut url = format!(
        "{base}/{scope}/providers/Microsoft.CostManagement/query?api-version={AZURE_API_VERSION}"
    );
    let prefix = format!("{base}/");
    let mut totals: Option<AzureTotals> = None;
    let mut seen_links: HashSet<String> = HashSet::new();
    loop {
        let page: QueryResult = json(ctx.http.post(&url).bearer_auth(&token).json(&body)).await?;
        azure_accumulate(&page, &mut totals)?;
        match page.properties.next_link.filter(|l| !l.is_empty()) {
            Some(next) if next.starts_with(&prefix) && seen_links.insert(next.clone()) => {
                url = next
            }
            Some(_) => return Err(QueryError::schema()),
            None => break,
        }
    }
    let mut notes = Notes::new();
    notes.push_params(
        "note.cloud.azure_scope",
        params([
            ("resource_id", ParamValue::from(resource_id.to_string())),
            ("scope", ParamValue::from(scope.clone())),
        ]),
        "Real cost (ActualCost/PreTaxCost) filtered by ResourceId = {resource_id} in scope /{scope}. Azure cost data updates daily and may lag by several hours; values are not for financial settlement.",
    );
    notes.push(
        "note.cloud.azure_resource_attribution",
        "Resource attribution: configure params.resource_id with the full Azure OpenAI resource ID (/subscriptions/.../providers/Microsoft.CognitiveServices/accounts/...). A shared Foundry resource includes products that are not OpenAI; it is not isolated per-model or spending quota.",
    );
    notes.push(
        "note.cloud.azure_query_cost",
        "The Cost Management Query API is not billed per request, but requires a token with cost read permission in the scope (e.g. Cost Management Reader).",
    );
    let metric = match totals {
        Some(t) => {
            if !t.cost.is_finite() {
                return Err(QueryError::schema());
            }
            let mut m = Metric::usage(
                "Custo do recurso Azure (mês corrente)",
                t.cost,
                t.currency,
                MetricKind::Spend,
                Some(MetricWindow::Monthly),
            );
            m.period = Some("mês corrente".into());
            m.resets_at = next_month_start(Utc::now());
            m
        }
        None => {
            // the value is unknown, not an assumed zero.
            notes.push(
                "note.cloud.azure_no_cost_rows",
                "The query returned no rows: the resource cost in this scope for the current month could not be confirmed.",
            );
            let mut m = Metric::usage(
                "Custo do recurso Azure (mês corrente)",
                0.0,
                "desconhecida",
                MetricKind::Spend,
                Some(MetricWindow::Monthly),
            );
            m.used = None;
            m.period = Some("mês corrente".into());
            m
        }
    };
    Ok(Data::new(
        vec![metric],
        notes,
        format!("https://learn.microsoft.com/rest/api/cost-management/query/usage (api-version {AZURE_API_VERSION})"),
    ))
}

/// Requires the full ARM ID of a Cognitive Services account inside the configured subscription:
/// /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.CognitiveServices/accounts/{name}
fn azure_resource_id<'a>(id: &'a str, subscription: &str) -> Result<&'a str, QueryError> {
    let segments: Vec<&str> = id.split('/').collect();
    let valid = id.starts_with(&format!("/subscriptions/{subscription}/resourceGroups/"))
        && id.contains("/providers/Microsoft.CognitiveServices/accounts/")
        && !id.ends_with('/')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        && segments.iter().skip(1).all(|s| !s.is_empty());
    if valid {
        Ok(id)
    } else {
        Err(QueryError::config(
            "query.cloud.invalid_azure_resource_id",
            "Set params.resource_id with the full ARM ID of the Azure OpenAI resource (/subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.CognitiveServices/accounts/{name}).",
        ))
    }
}

fn next_month_start(now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let (y, m) = (now.year(), now.month());
    let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    Utc.with_ymd_and_hms(y, m, 1, 0, 0, 0).single()
}

// ---------------------------------------------------------------------------
// AWS Bedrock via AWS CLI + Cost Explorer
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CeOutput {
    results_by_time: Vec<CeResult>,
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CeResult {
    total: BTreeMap<String, CeAmount>,
    #[serde(default)]
    estimated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CeAmount {
    amount: String,
    unit: String,
}

struct CeParsed {
    cost: f64,
    currency: String,
    estimated: bool,
}

fn parse_ce(bytes: &[u8]) -> Result<CeParsed, QueryError> {
    let out: CeOutput = super::decode_bytes(bytes)?;
    if out.results_by_time.is_empty() || out.next_page_token.is_some_and(|t| !t.is_empty()) {
        return Err(QueryError::schema());
    }
    let mut cost: Option<f64> = None;
    let mut currency: Option<String> = None;
    let mut estimated = false;
    for result in &out.results_by_time {
        estimated |= result.estimated;
        let amount = result
            .total
            .get("UnblendedCost")
            .ok_or_else(QueryError::schema)?;
        let value: f64 = amount.amount.parse().map_err(|_| QueryError::schema())?;
        if !value.is_finite() || amount.unit.is_empty() {
            return Err(QueryError::schema());
        }
        match &currency {
            None => currency = Some(amount.unit.clone()),
            Some(c) if *c != amount.unit => return Err(QueryError::schema()),
            _ => {}
        }
        cost = Some(cost.unwrap_or(0.0) + value);
    }
    match (cost, currency) {
        (Some(cost), Some(currency)) if cost.is_finite() => Ok(CeParsed {
            cost,
            currency,
            estimated,
        }),
        _ => Err(QueryError::schema()),
    }
}

/// Run the AWS CLI (`binary`, `aws` in production) with the account's named
/// profile and return stdout. Errors carry only a classification and the
/// IAM `permission` hint, never stderr, which may echo ARNs or tokens.
async fn run_aws(
    ctx: &Context<'_>,
    binary: &str,
    args: &[&str],
    profile: &str,
    permission: &str,
) -> Result<Vec<u8>, QueryError> {
    let mut cmd = tokio::process::Command::new(binary);
    cmd.args(args)
        .args(["--output", "json", "--profile", profile])
        .env("AWS_PAGER", "")
        .env("AWS_CLI_AUTO_PROMPT", "off")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            QueryError::config(
                "query.cloud.aws_cli_not_found",
                "AWS CLI not on PATH. Install AWS CLI v2 to query AWS Bedrock.",
            )
        } else {
            QueryError::transport()
        }
    })?;
    let output = match tokio::time::timeout(ctx.timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => return Err(QueryError::transport()),
        Err(_) => return Err(QueryError::timeout()),
    };
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    if stderr.contains("credential") || stderr.contains("sso") || stderr.contains("token") {
        return Err(QueryError { diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.cloud.aws_auth",
                "AWS authentication failed for the profile configured in params.profile. Verify the profile credentials (aws configure sso / aws configure).",
            ),
        });
    }
    if stderr.contains("accessdenied") || stderr.contains("not authorized") {
        return Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::with_params(
                "query.cloud.aws_access_denied",
                params([("permission", ParamValue::from(permission))]),
                "Access denied on the configured AWS profile. Grant {permission} to the profile.",
            ),
        });
    }
    if stderr.contains("throttl") || stderr.contains("too many requests") {
        return Err(QueryError {
            diagnostic: None,
            status: Status::RateLimited,
            note: Localized::fixed(
                "query.cloud.aws_throttled",
                "AWS throttled this profile's requests; try again later.",
            ),
        });
    }
    let code = output.status.code().unwrap_or(-1);
    Err(QueryError {
        diagnostic: None,
        status: Status::Error,
        note: Localized::with_params(
            "query.cloud.aws_cli",
            params([
                ("code", ParamValue::from(code.to_string())),
                ("permission", ParamValue::from(permission)),
            ]),
            "AWS CLI failed (exit {code}). Verify the {permission} permission on the profile.",
        ),
    })
}

/// Bedrock catalog: `aws bedrock list-foundation-models` (control plane,
/// read-only, no Cost Explorer charge; the API has no pagination). Region
/// is `params.region` when set, otherwise the profile's default region.
pub(crate) async fn bedrock_models(
    account: &Account,
    ctx: &Context<'_>,
    binary: &str,
) -> Result<Vec<crate::models::ModelInfo>, QueryError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct List {
        model_summaries: Vec<Summary>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Summary {
        model_id: String,
        model_name: Option<String>,
        provider_name: Option<String>,
    }
    let profile = param(account, "profile")?;
    let mut args = vec!["bedrock", "list-foundation-models"];
    if let Some(region) = account
        .params
        .get("region")
        .filter(|r| !r.trim().is_empty())
    {
        let valid = region.starts_with(|c: char| c.is_ascii_lowercase())
            && region
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !valid {
            return Err(QueryError::config_params(
                "query.cloud.invalid_resource_identifier",
                params([("key", ParamValue::from("region"))]),
                "params.{key} contains invalid characters for a resource identifier.",
            ));
        }
        args.extend(["--region", region]);
    }
    let stdout = run_aws(ctx, binary, &args, profile, "bedrock:ListFoundationModels").await?;
    let list: List = super::decode_bytes(&stdout)?;
    Ok(list
        .model_summaries
        .into_iter()
        .map(|m| crate::models::ModelInfo {
            id: m.model_id,
            name: m.model_name,
            description: m.provider_name,
        })
        .collect())
}

async fn fetch_bedrock(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    let profile = param(account, "profile")?;
    let today = Utc::now().date_naive();
    let start: NaiveDate = today.with_day(1).ok_or_else(QueryError::schema)?;
    let end = today + ChronoDuration::days(1);
    let filter = serde_json::json!({"Dimensions": {"Key": "SERVICE", "Values": [BEDROCK_SERVICE]}})
        .to_string();
    let period = format!("Start={start},End={end}");
    let args = [
        "ce",
        "get-cost-and-usage",
        "--time-period",
        &period,
        "--granularity",
        "MONTHLY",
        "--metrics",
        "UnblendedCost",
        "--filter",
        &filter,
        "--no-paginate",
    ];
    let stdout = run_aws(ctx, "aws", &args, profile, "ce:GetCostAndUsage").await?;
    let parsed = parse_ce(&stdout)?;
    let mut cost = Metric::usage(
        "Custo AWS Bedrock (mês corrente)",
        parsed.cost,
        parsed.currency,
        MetricKind::Spend,
        Some(MetricWindow::Monthly),
    );
    cost.period = Some("mês corrente".into());
    cost.resets_at = next_month_start(Utc::now());
    let mut notes = Notes::new();
    notes.push(
        "note.cloud.aws_cost_scope",
        "Cost Explorer filtered by service \"Amazon Bedrock\", monthly granularity, current month. AWS cost data updates ~1x/day and may lag by up to 24h.",
    );
    notes.push(
        "note.cloud.aws_query_cost",
        "Each get-cost-and-usage call to Cost Explorer is billed by AWS (US$ 0.01 per request), unlike generation calls: every dashboard refresh incurs a query charge.",
    );
    if parsed.estimated {
        notes.push(
            "note.cloud.aws_estimated",
            "Current-period values are marked as estimated by Cost Explorer.",
        );
    }
    Ok(Data::new(
        vec![cost],
        notes,
        "https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html",
    ))
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use reqwest::Client;
    use std::time::Duration;

    struct Creds;
    impl Credentials for Creds {
        fn get(&self, name: &str) -> Option<String> {
            matches!(name, "GOOGLE_CLOUD_ACCESS_TOKEN" | "AZURE_ACCESS_TOKEN")
                .then(|| "token-teste".into())
        }
    }

    fn account(provider: Provider, params: &[(&str, &str)]) -> Account {
        Account {
            id: None,
            name: "teste".into(),
            provider,
            credential_env: None,
            session: None,
            oauth: None,
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn ctx<'a>(http: &'a Client, creds: &'a Creds) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: Duration::from_secs(5),
        }
    }

    fn point(end: &str, int: Option<&str>, dbl: Option<f64>) -> Point {
        Point {
            interval: PointInterval {
                end_time: end.into(),
            },
            value: PointValue {
                int64_value: int.map(str::to_string),
                double_value: dbl,
            },
        }
    }

    fn ts(metric: &[(&str, &str)], resource: &[(&str, &str)], points: Vec<Point>) -> Ts {
        Ts {
            metric: Labels {
                labels: metric
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            },
            resource: Labels {
                labels: resource
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            },
            points,
        }
    }

    #[test]
    fn google_limit_usa_ponto_mais_recente() {
        let limits = vec![ts(
            &[
                ("quota_metric", "generate_requests"),
                ("limit_name", "Requests per minute"),
            ],
            &[("location", "us-central1")],
            vec![
                point("2026-09-27T00:00:00Z", Some("100"), None),
                point("2026-09-28T00:00:00Z", Some("150"), None),
            ],
        )];
        let metrics = google_metrics("Vertex AI", &limits, &[]).unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].limit, Some(150.0));
        assert_eq!(metrics[0].used, None);
        assert_eq!(metrics[0].kind, MetricKind::Quota);
        assert_eq!(metrics[0].window, Some(MetricWindow::Other));
        assert!(metrics[0].label.contains("generate_requests"));
        assert!(metrics[0].label.contains("us-central1"));
    }

    #[test]
    fn google_ponto_invalido_e_erro_nao_descarte() {
        let usage = vec![ts(
            &[("quota_metric", "generate_requests")],
            &[],
            vec![
                point("2026-09-28T10:00:00Z", Some("3"), None),
                point("2026-09-28T10:01:00Z", None, Some(f64::NAN)),
            ],
        )];
        assert!(google_metrics("Gemini API", &[], &usage).is_err());
        let limits = vec![ts(
            &[],
            &[],
            vec![point("2026-09-28T00:00:00Z", Some("abc"), None)],
        )];
        assert!(google_metrics("Gemini API", &limits, &[]).is_err());
    }

    #[test]
    fn google_uso_soma_pontos() {
        let usage = vec![ts(
            &[("quota_metric", "generate_requests")],
            &[],
            vec![
                point("2026-09-28T10:00:00Z", Some("3"), None),
                point("2026-09-28T10:01:00Z", None, Some(2.5)),
            ],
        )];
        let metrics = google_metrics("Gemini API", &[], &usage).unwrap();
        assert_eq!(metrics[0].used, Some(5.5));
        assert_eq!(metrics[0].kind, MetricKind::Usage);
        assert_eq!(metrics[0].window, Some(MetricWindow::Other));
    }

    #[test]
    fn azure_acumula_custo_e_moeda() {
        let page: QueryResult = serde_json::from_value(serde_json::json!({
            "properties": {
                "columns": [{"name": "PreTaxCost", "type": "Number"}, {"name": "Currency", "type": "String"}],
                "rows": [[1.5, "USD"], [2.25, "USD"]]
            }
        })).unwrap();
        let mut totals = None;
        azure_accumulate(&page, &mut totals).unwrap();
        let t = totals.unwrap();
        assert_eq!(t.cost, 3.75);
        assert_eq!(t.currency, "USD");
    }

    #[test]
    fn azure_rejeita_moedas_mistas_ou_ausentes() {
        let mixed: QueryResult = serde_json::from_value(serde_json::json!({
            "properties": {
                "columns": [{"name": "PreTaxCost"}, {"name": "Currency"}],
                "rows": [[1.0, "USD"]]
            }
        }))
        .unwrap();
        let second: QueryResult = serde_json::from_value(serde_json::json!({
            "properties": {
                "columns": [{"name": "PreTaxCost"}, {"name": "Currency"}],
                "rows": [[1.0, "BRL"]]
            }
        }))
        .unwrap();
        let mut totals = None;
        azure_accumulate(&mixed, &mut totals).unwrap();
        assert!(azure_accumulate(&second, &mut totals).is_err());

        let missing: QueryResult = serde_json::from_value(serde_json::json!({
            "properties": {
                "columns": [{"name": "PreTaxCost"}, {"name": "Currency"}],
                "rows": [[1.0, null]]
            }
        }))
        .unwrap();
        assert!(azure_accumulate(&missing, &mut None).is_err());
    }

    const RID: &str = "/subscriptions/sub-1/resourceGroups/rg-1/providers/Microsoft.CognitiveServices/accounts/openai-1";

    #[test]
    fn azure_valida_resource_id() {
        assert!(azure_resource_id(RID, "sub-1").is_ok());
        assert!(azure_resource_id("/subscriptions/outra/resourceGroups/rg/providers/Microsoft.CognitiveServices/accounts/x", "sub-1").is_err());
        assert!(azure_resource_id(
            "/subscriptions/sub-1/resourceGroups/rg/providers/Microsoft.Storage/storageAccounts/x",
            "sub-1"
        )
        .is_err());
        assert!(azure_resource_id("openai-1", "sub-1").is_err());
    }

    #[tokio::test]
    async fn azure_busca_paginada_soma_paginas() {
        use wiremock::matchers::{body_partial_json, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let page1 = serde_json::json!({
            "properties": {
                "columns": [{"name": "PreTaxCost"}, {"name": "Currency"}],
                "rows": [[10.0, "USD"]],
                "nextLink": format!("{}/subscriptions/sub-1/providers/Microsoft.CostManagement/query?api-version={AZURE_API_VERSION}&$skiptoken=abc", server.uri())
            }
        });
        let page2 = serde_json::json!({
            "properties": {
                "columns": [{"name": "PreTaxCost"}, {"name": "Currency"}],
                "rows": [[5.5, "USD"]]
            }
        });
        Mock::given(method("POST"))
            .and(path("/subscriptions/sub-1/providers/Microsoft.CostManagement/query"))
            .and(body_partial_json(serde_json::json!({
                "dataset": {"filter": {"dimensions": {"name": "ResourceId", "operator": "In", "values": [RID]}}}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
            .up_to_n_times(1)
            .mount(&server).await;
        Mock::given(method("POST"))
            .and(path(
                "/subscriptions/sub-1/providers/Microsoft.CostManagement/query",
            ))
            .and(query_param("$skiptoken", "abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page2))
            .mount(&server)
            .await;
        let http = Client::new();
        let creds = Creds;
        let acc = account(
            Provider::AzureOpenAi,
            &[("subscription_id", "sub-1"), ("resource_id", RID)],
        );
        let data = fetch_azure(&server.uri(), &acc, &ctx(&http, &creds))
            .await
            .unwrap();
        assert_eq!(data.metrics[0].used, Some(15.5));
        assert_eq!(data.metrics[0].unit, "USD");
        assert_eq!(data.metrics[0].kind, MetricKind::Spend);
        assert_eq!(data.metrics[0].window, Some(MetricWindow::Monthly));
    }

    #[tokio::test]
    async fn azure_sem_linhas_nao_reporta_zero_falso() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/subscriptions/sub-1/providers/Microsoft.CostManagement/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "properties": {"columns": [{"name": "PreTaxCost"}, {"name": "Currency"}], "rows": []}
            })))
            .mount(&server).await;
        let http = Client::new();
        let creds = Creds;
        let acc = account(
            Provider::AzureOpenAi,
            &[("subscription_id", "sub-1"), ("resource_id", RID)],
        );
        let data = fetch_azure(&server.uri(), &acc, &ctx(&http, &creds))
            .await
            .unwrap();
        assert_eq!(data.metrics[0].used, None);
    }

    #[tokio::test]
    async fn google_busca_pagina_via_token() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let page1 = serde_json::json!({
            "timeSeries": [{
                "metric": {"labels": {"quota_metric": "q", "limit_name": "l"}},
                "resource": {"labels": {"location": "global"}},
                "points": [{"interval": {"endTime": "2026-09-28T00:00:00Z"}, "value": {"int64Value": "42"}}]
            }],
            "nextPageToken": "p2"
        });
        let page2 = serde_json::json!({"timeSeries": []});
        let base_path = "/v1/projects/proj-1/timeSeries";
        Mock::given(method("GET"))
            .and(path(base_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(base_path))
            .and(query_param("pageToken", "p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&page2))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(base_path))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"timeSeries": []})),
            )
            .mount(&server)
            .await;
        let http = Client::new();
        let creds = Creds;
        let acc = account(Provider::GeminiApi, &[("project_id", "proj-1")]);
        let data = fetch_google(
            &server.uri(),
            "generativelanguage.googleapis.com",
            "Gemini API",
            &acc,
            &ctx(&http, &creds),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].limit, Some(42.0));
    }

    #[test]
    fn bedrock_interpreta_saida_cost_explorer() {
        let sample = br#"{
            "ResultsByTime": [{
                "TimePeriod": {"Start": "2026-09-01", "End": "2026-09-28"},
                "Total": {"UnblendedCost": {"Amount": "12.34", "Unit": "USD"}},
                "Groups": [], "Estimated": true
            }],
            "DimensionValueAttributes": []
        }"#;
        let parsed = parse_ce(sample).unwrap();
        assert!((parsed.cost - 12.34).abs() < 1e-9);
        assert_eq!(parsed.currency, "USD");
        assert!(parsed.estimated);
    }

    #[test]
    fn bedrock_rejeita_saida_sem_custo_ou_moeda_mista() {
        assert!(parse_ce(b"not json").is_err());
        assert!(parse_ce(br#"{"ResultsByTime": []}"#).is_err());
        assert!(parse_ce(br#"{"ResultsByTime": [{"Total": {}, "Estimated": false}]}"#).is_err());
        assert!(parse_ce(br#"{"NextPageToken": "abc", "ResultsByTime": [{"Total": {"UnblendedCost": {"Amount": "1", "Unit": "USD"}}, "Estimated": false}]}"#).is_err());
        assert!(parse_ce(
            br#"{"ResultsByTime": [
            {"Total": {"UnblendedCost": {"Amount": "1", "Unit": "USD"}}, "Estimated": false},
            {"Total": {"UnblendedCost": {"Amount": "1", "Unit": "BRL"}}, "Estimated": false}
        ]}"#
        )
        .is_err());
        assert!(parse_ce(br#"{"ResultsByTime": [{"Total": {"UnblendedCost": {"Amount": "abc", "Unit": "USD"}}, "Estimated": false}]}"#).is_err());
    }

    #[tokio::test]
    async fn aws_sem_perfil_nao_executa_cli() {
        let http = Client::new();
        let creds = Creds;
        let acc = account(Provider::AwsBedrock, &[]);
        let err = fetch_bedrock(&acc, &ctx(&http, &creds)).await.unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
    }

    /// Fake `aws` answering by profile/region and logging its argv.
    #[cfg(unix)]
    fn fake_aws(dir: &std::path::Path) -> String {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("aws");
        let log = dir.join("argv.log");
        let script = format!(
            r#"#!/bin/sh
echo "$@" >> '{log}'
case "$*" in
  *"--region eu-west-1"*) echo '{{"modelSummaries":[{{"modelId":"eu.model","modelName":"EU"}}]}}';;
  *"--profile denied"*) echo 'AccessDeniedException: arn:aws:iam::123:user/secret' >&2; exit 254;;
  *"--profile throttled"*) echo 'ThrottlingException: Rate exceeded' >&2; exit 254;;
  *"--profile expired"*) echo 'Error loading SSO Token: AKIASECRET' >&2; exit 255;;
  *"--profile broken"*) echo '{{"modelSummaries":[{{"modelName":"no id"}}]}}';;
  *) echo '{{"modelSummaries":[{{"modelId":"b","modelName":"B","providerName":"P"}},{{"modelId":"a"}}]}}';;
esac
"#,
            log = log.display()
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin.to_string_lossy().into_owned()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bedrock_catalog_uses_profile_and_region_and_classifies_failures() {
        let dir = tempfile::tempdir().unwrap();
        let aws = fake_aws(dir.path());
        let (http, creds) = (Client::new(), Creds);
        let list = async |params: &[(&str, &str)]| {
            let acc = account(Provider::AwsBedrock, params);
            bedrock_models(&acc, &ctx(&http, &creds), &aws).await
        };
        let ids = |models: Vec<crate::models::ModelInfo>| -> Vec<String> {
            models.into_iter().map(|m| m.id).collect()
        };

        let default = list(&[("profile", "prod")]).await.unwrap();
        assert_eq!(default[0].name.as_deref(), Some("B"));
        assert_eq!(default[0].description.as_deref(), Some("P"));
        assert_eq!(ids(default), ["b", "a"]);
        let eu = list(&[("profile", "prod"), ("region", "eu-west-1")]).await;
        assert_eq!(
            ids(eu.unwrap()),
            ["eu.model"],
            "region isolates the catalog"
        );

        for (profile, status) in [
            ("denied", Status::AuthenticationError),
            ("expired", Status::AuthenticationError),
            ("throttled", Status::RateLimited),
            ("broken", Status::Error),
        ] {
            let error = list(&[("profile", profile)]).await.unwrap_err();
            assert_eq!(error.status, status, "{profile}");
            assert!(!error.note.fallback.contains("SECRET"), "no stderr echo");
            assert!(!error.note.fallback.contains("arn:"), "no stderr echo");
        }
        let denied = list(&[("profile", "denied")]).await.unwrap_err();
        assert!(denied
            .note
            .fallback
            .contains("bedrock:ListFoundationModels"));

        let bad_region = list(&[("profile", "prod"), ("region", "us;rm -rf")]).await;
        assert_eq!(bad_region.unwrap_err().status, Status::NeedsConfiguration);
        let argv = std::fs::read_to_string(dir.path().join("argv.log")).unwrap();
        assert_eq!(argv.lines().count(), 7, "invalid region never runs the CLI");
        assert!(argv
            .lines()
            .all(|l| l.starts_with("bedrock list-foundation-models")));
        assert!(argv.contains("--region eu-west-1 --output json --profile prod"));
        assert!(!argv.contains(" ce "), "never Cost Explorer");
    }

    #[test]
    fn identificadores_de_caminho_rejeitam_injecao() {
        assert!(path_id("proj-1_a.b:c", "project_id").is_ok());
        for bad in ["a/b", "a?b", "a#b", "a%2Fb", "a b"] {
            assert!(path_id(bad, "project_id").is_err());
        }
    }
}
