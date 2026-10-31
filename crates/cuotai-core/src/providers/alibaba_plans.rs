//! Read-only queries for the Alibaba Model Studio and Coding Plan providers.
//!
//! Model Studio still has no quota API authenticable with the inference
//! key. Coding Plan, on the other hand, is queried through the console gateway
//! used by the Alibaba panel. There are two mutually exclusive modes:
//!
//! - web session `session_env` (PRD 0016): pasted into the account wizard and
//!   kept in the system vault or, alternatively, in a variable
//!   named by `params.session_env`. When it exists, it always takes
//!   precedence and no API key is required. The adapter obtains `sec_token`
//!   with a GET on the dashboard and makes the console RPC with the cookie.
//! - API key: `Account.credential_env` (default `BAILIAN_CODING_PLAN_API_KEY`) is sent to the
//!   regional gateway in the three headers the console accepts.
//!
//! The region is fixed per account (`params.region = "intl" | "cn"`, default
//! `intl`). Credentials are never tried automatically in the other region and
//! there is no parameter to override production endpoints.
//!
//! The Coding Plan RPC is an internal console interface, not a stable
//! public API. The implementation follows the protocol observed in CodexBar:
//! <https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift>.
//!
//! Optional parameters:
//! - `region`: `intl` (default) or `cn`;
//! - `session_env`: name of the variable holding the full Cookie header
//!   (alternative to the vault session).

use super::{json as fetch_json, paginate, secret, send as secure_send, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider, Status};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Value};

const CODING_PLAN_BASE_CN: &str = "https://coding.dashscope.aliyuncs.com/v1";
const CODING_PLAN_BASE_INTL: &str = "https://coding-intl.dashscope.aliyuncs.com/v1";

const CODING_PLAN_GATEWAY_CN: &str = "https://bailian.console.aliyun.com/data/api.json";
const CODING_PLAN_GATEWAY_INTL: &str = "https://modelstudio.console.alibabacloud.com/data/api.json";
const CODING_PLAN_RPC_CN: &str = "https://bailian-cs.console.aliyun.com/data/api.json";
const CODING_PLAN_RPC_INTL: &str = "https://bailian-singapore-cs.alibabacloud.com/data/api.json";
const CODING_PLAN_USER_INFO_CN: &str = "https://bailian.console.aliyun.com/tool/user/info.json";
const CODING_PLAN_USER_INFO_INTL: &str =
    "https://modelstudio.console.alibabacloud.com/tool/user/info.json";

const CODING_PLAN_DASHBOARD_CN: &str =
    "https://bailian.console.aliyun.com/cn-beijing/?tab=model#/efm/coding_plan";
const CODING_PLAN_DASHBOARD_INTL: &str =
    "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=coding-plan#/efm/coding_plan";
const MODEL_STUDIO_FREE_QUOTA_CN: &str =
    "https://bailian.console.aliyun.com/cn-beijing/costing-balance/free-quota";
const MODEL_STUDIO_BILLING_CN: &str =
    "https://bailian.console.aliyun.com/cn-beijing/costing-balance/usage-statistics";
const MODEL_STUDIO_FREE_QUOTA_INTL: &str =
    "https://modelstudio.console.alibacloud.com/ap-southeast-1/costing-balance/free-quota";
const MODEL_STUDIO_USAGE_INTL: &str =
    "https://modelstudio.console.alibacloud.com/ap-southeast-1/costing-balance/usage-statistics";
const DASHSCOPE_INFERENCE_DOCS: &str =
    "https://help.aliyun.com/zh/model-studio/qwen-api-via-dashscope";
const ALIBABA_CLOUD_BILL_DOCS: &str =
    "https://www.alibabacloud.com/help/en/finance/account-management/latest/api-bill-overview";
/// Model list of the international (Singapore) region, the only one without
/// a `{WorkspaceId}` host (<https://www.alibabacloud.com/help/en/model-studio/list-models>).
pub(crate) const MODEL_STUDIO_MODELS_URL: &str =
    "https://dashscope-intl.aliyuncs.com/api/v1/models";
/// Documented default; no maximum is published.
const MODEL_STUDIO_PAGE_SIZE: u64 = 20;

/// Whether this Model Studio account selects the region the catalog covers.
pub(crate) fn model_studio_lists(account: &Account) -> bool {
    matches!(parse_region(account), Ok(Region::Intl))
}

/// Model Studio catalog with the account's DashScope key, following
/// `page_no` until `output.total` models were read. A short page before the
/// total, a mismatched page number or `success = false` rejects the sync.
pub(crate) async fn model_studio_models(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
) -> Result<Vec<crate::models::ModelInfo>, QueryError> {
    #[derive(serde::Deserialize)]
    struct Body {
        success: bool,
        output: Output,
    }
    #[derive(serde::Deserialize)]
    struct Output {
        total: u64,
        page_no: u64,
        models: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        model: String,
        name: Option<String>,
        description: Option<String>,
    }
    let key = secret(account, ctx, "DASHSCOPE_API_KEY")?;
    // The token carries `page_no:read` so each page future owns its state.
    paginate(|token: Option<String>| {
        let (page_no, read) = token
            .as_deref()
            .and_then(|t| t.split_once(':'))
            .and_then(|(p, r)| Some((p.parse::<u64>().ok()?, r.parse::<u64>().ok()?)))
            .unwrap_or((1, 0));
        let request = ctx
            .http
            .get(url)
            .bearer_auth(key.trim())
            .query(&[
                ("page_no", page_no.to_string()),
                ("page_size", MODEL_STUDIO_PAGE_SIZE.to_string()),
            ])
            .timeout(ctx.timeout);
        async move {
            let body: Body = fetch_json(request).await?;
            let output = body.output;
            let read = read + output.models.len() as u64;
            if !body.success
                || output.page_no != page_no
                || read > output.total
                || (output.models.is_empty() && read < output.total)
            {
                return Err(QueryError::schema());
            }
            let next = (read < output.total).then(|| format!("{}:{read}", page_no + 1));
            let models = output.models.into_iter().map(|m| crate::models::ModelInfo {
                id: m.model,
                name: m.name,
                description: m.description,
            });
            Ok((models.collect(), next))
        }
    })
    .await
}

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::AlibabaModelStudio => alibaba_model_studio(account, ctx).await,
        Provider::AlibabaCodingPlan => alibaba_coding_plan(account, ctx).await,
        Provider::AlibabaTokenPlan => alibaba_token_plan(account, ctx).await,
        other => Err(QueryError::unsupported_params(
            "query.alibaba_plans.unsupported_provider",
            params([(
                "provider",
                ParamValue::L10n(Box::new(crate::text::provider_l10n(other))),
            )]),
            "{provider} is handled by another adapter.",
        )),
    }
}

// --------------------------------------------------------- helpers locais

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Intl,
    Cn,
}

fn parse_region(account: &Account) -> Result<Region, QueryError> {
    let raw = account
        .params
        .get("region")
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("intl");
    match raw.to_ascii_lowercase().as_str() {
        "intl" | "international" | "global" | "sgp" | "i18n" => Ok(Region::Intl),
        "cn" | "china" | "zh" | "prc" | "beijing" | "bj" => Ok(Region::Cn),
        other => Err(QueryError::config_params(
            "query.alibaba_plans.invalid_region",
            params([("region", ParamValue::from(other.to_string()))]),
            "params.region '{region}' invalid for Alibaba; use 'intl' (default) or 'cn'.",
        )),
    }
}

fn infer_base(region: Region) -> &'static str {
    match region {
        Region::Cn => CODING_PLAN_BASE_CN,
        Region::Intl => CODING_PLAN_BASE_INTL,
    }
}

fn coding_plan_dashboard(region: Region) -> &'static str {
    match region {
        Region::Cn => CODING_PLAN_DASHBOARD_CN,
        Region::Intl => CODING_PLAN_DASHBOARD_INTL,
    }
}

// ---------------------------------------- Alibaba Model Studio / DashScope

async fn alibaba_model_studio(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    // The `DASHSCOPE_API_KEY` is required only to signal that the
    // user configured the account; it will **not** be used for any
    // request. Validating guarantees a clear error if the user forgot the
    // env var; the value is never sent to any server.
    let _unused_key = secret(account, ctx, "DASHSCOPE_API_KEY")?;
    let region = parse_region(account)?;
    let free_quota_url = match region {
        Region::Cn => MODEL_STUDIO_FREE_QUOTA_CN,
        Region::Intl => MODEL_STUDIO_FREE_QUOTA_INTL,
    };
    let usage_url = match region {
        Region::Cn => MODEL_STUDIO_BILLING_CN,
        Region::Intl => MODEL_STUDIO_USAGE_INTL,
    };
    Err(QueryError::unsupported_params(
        "query.alibaba_plans.model_studio_no_public_quota",
        params([
            ("infer_base", ParamValue::from(infer_base(region))),
            ("free_quota_url", ParamValue::from(free_quota_url)),
            ("usage_url", ParamValue::from(usage_url)),
            ("dashscope_docs", ParamValue::from(DASHSCOPE_INFERENCE_DOCS)),
            ("billing_docs", ParamValue::from(ALIBABA_CLOUD_BILL_DOCS)),
        ]),
        "Alibaba Model Studio / DashScope (DASHSCOPE_API_KEY or QWEN_API_KEY) does not grant access to a public balance or billing quota endpoint. The inference key (sk-…) only authorises POST to {infer_base} and OpenAI/Anthropic-compatible routes; the real Alibaba Cloud account balance requires the master account's AccessKey ID/Secret via QueryAccountBalance (Financial Management API) and is handled by the AlibabaCloud adapter. The free-tier limits and Model Studio consumption are only available in the authenticated console: free tier at {free_quota_url} and detailed usage at {usage_url}. Primary sources: {dashscope_docs} (inference) and {billing_docs} (account balance).",
    ))
}

// ------------------------------------------------------- Alibaba Coding Plan

const CODING_PLAN_ACTION: &str =
    "zeldaEasy.broadscope-bailian.codingPlan.queryCodingPlanInstanceInfoV2";
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

#[derive(Clone, Copy)]
struct CodingPlanEndpoints<'a> {
    gateway: &'a str,
    dashboard: &'a str,
    rpc: &'a str,
    user_info: &'a str,
}

fn production_endpoints(region: Region) -> CodingPlanEndpoints<'static> {
    match region {
        Region::Intl => CodingPlanEndpoints {
            gateway: CODING_PLAN_GATEWAY_INTL,
            dashboard: CODING_PLAN_DASHBOARD_INTL,
            rpc: CODING_PLAN_RPC_INTL,
            user_info: CODING_PLAN_USER_INFO_INTL,
        },
        Region::Cn => CodingPlanEndpoints {
            gateway: CODING_PLAN_GATEWAY_CN,
            dashboard: CODING_PLAN_DASHBOARD_CN,
            rpc: CODING_PLAN_RPC_CN,
            user_info: CODING_PLAN_USER_INFO_CN,
        },
    }
}

fn region_id(region: Region) -> &'static str {
    match region {
        Region::Intl => "ap-southeast-1",
        Region::Cn => "cn-beijing",
    }
}

fn commodity_code(region: Region) -> &'static str {
    match region {
        Region::Intl => "sfm_codingplan_public_intl",
        Region::Cn => "sfm_codingplan_public_cn",
    }
}

fn console_rpc_action(region: Region) -> &'static str {
    match region {
        Region::Intl => "IntlBroadScopeAspnGateway",
        Region::Cn => "BroadScopeAspnGateway",
    }
}

fn console_domain(region: Region) -> &'static str {
    match region {
        Region::Intl => "modelstudio.console.alibabacloud.com",
        Region::Cn => "bailian.console.aliyun.com",
    }
}

fn console_origin(region: Region) -> &'static str {
    match region {
        Region::Intl => "https://modelstudio.console.alibabacloud.com",
        Region::Cn => "https://bailian.console.aliyun.com",
    }
}

fn console_site(region: Region) -> &'static str {
    match region {
        Region::Intl => "MODELSTUDIO_ALIBABACLOUD",
        Region::Cn => "BAILIAN_ALIYUN",
    }
}

fn console_referer(region: Region) -> &'static str {
    match region {
        Region::Intl => {
            "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=coding-plan"
        }
        Region::Cn => "https://bailian.console.aliyun.com/cn-beijing/?tab=model",
    }
}

fn http_error(code: reqwest::StatusCode, session: bool) -> QueryError {
    let status = match code.as_u16() {
        401 | 403 => Status::AuthenticationError,
        429 => Status::RateLimited,
        _ => Status::Error,
    };
    let note = match (session, code.as_u16()) {
        (true, 401 | 403) => Localized::with_params(
            "query.alibaba_plans.session_http_auth",
            params([("code", ParamValue::from(code.as_u16()))]),
            "Alibaba Coding Plan authentication failed: refresh the API key or session cookie.",
        ),
        (false, 401 | 403) => Localized::with_params(
            "query.alibaba_plans.api_key_http_auth",
            params([("code", ParamValue::from(code.as_u16()))]),
            "Alibaba Coding Plan authentication failed: refresh the API key.",
        ),
        (_, code) => Localized::with_params(
            "query.alibaba_plans.coding_plan_http",
            params([("code", ParamValue::from(code))]),
            "Alibaba Coding Plan returned an HTTP {code} error.",
        ),
    };
    QueryError {
        diagnostic: None,
        status,
        note,
    }
}

async fn response_json(
    request: reqwest::RequestBuilder,
    session: bool,
) -> Result<Value, QueryError> {
    // Only the Coding Plan console RPC sends the session here.
    let response = if session {
        super::send_session(request, crate::Provider::AlibabaCodingPlan, "session_env").await?
    } else {
        secure_send(request).await?
    };
    let status = response.status();
    let payload = super::read_json::<Value>(response).await;
    if !status.is_success() {
        if let Ok(payload) = payload {
            let expanded = expand_embedded_json(payload);
            if find_string(&expanded, &["code"])
                .is_some_and(|code| contains_ascii_case_insensitive(&code, "needlogin"))
            {
                return Err(login_required(session));
            }
        }
        return Err(http_error(status, session));
    }
    payload.map_err(|_| QueryError::schema())
}

fn login_required(session: bool) -> QueryError {
    if session {
        QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.alibaba_plans.console_login_required",
                "The API key did not surface the Coding Plan quota for this account/region; configure params.session_env with a session cookie from the Alibaba console.",
            ),
        }
    } else {
        QueryError::config(
            "query.alibaba_plans.api_key_requires_session",
            "The API key did not surface the Coding Plan quota for this account/region; paste the Alibaba console session in the account wizard (or set params.session_env to the variable holding the cookie).",
        )
    }
}

fn expand_embedded_json(value: Value) -> Value {
    match value {
        Value::String(text) => match serde_json::from_str::<Value>(text.trim()) {
            Ok(Value::Object(map)) => expand_embedded_json(Value::Object(map)),
            Ok(Value::Array(items)) => expand_embedded_json(Value::Array(items)),
            _ => Value::String(text),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(expand_embedded_json).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, expand_embedded_json(value)))
                .collect(),
        ),
        primitive => primitive,
    }
}

fn find_value<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key) {
                    return Some(value);
                }
            }
            map.values().find_map(|value| find_value(value, keys))
        }
        Value::Array(items) => items.iter().find_map(|value| find_value(value, keys)),
        _ => None,
    }
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    find_value(value, keys).and_then(scalar_string)
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn direct_value<'a>(map: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| map.get(*key))
}

fn finite_number(value: Option<&Value>) -> Option<f64> {
    let number = match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => {
            let text = text.trim();
            if text.contains(',') {
                text.replace(',', "").parse::<f64>().ok()
            } else {
                text.parse::<f64>().ok()
            }
        }
        _ => None,
    }?;
    number.is_finite().then_some(number)
}

fn epoch_date(raw: f64) -> Option<DateTime<Utc>> {
    if !raw.is_finite() || raw <= 0.0 {
        return None;
    }
    let millis = if raw.abs() < 1e12 { raw * 1000.0 } else { raw };
    DateTime::<Utc>::from_timestamp_millis(millis as i64)
}

fn parse_date(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = value?;
    if let Some(number) = finite_number(Some(value)) {
        return epoch_date(number);
    }
    let Value::String(text) = value else {
        return None;
    };
    let text = text.trim();
    DateTime::parse_from_rfc3339(text)
        .map(|date| date.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
                .iter()
                .find_map(|format| NaiveDateTime::parse_from_str(text, format).ok())
                .map(|date| DateTime::from_naive_utc_and_offset(date, Utc))
        })
        .or_else(|| {
            NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .map(|date| DateTime::from_naive_utc_and_offset(date, Utc))
        })
}

fn direct_bool(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    match direct_value(map, keys)? {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => value.as_i64().map(|value| value != 0),
        Value::String(value) => {
            let value = value.trim();
            if ["true", "1", "yes", "active", "valid"]
                .iter()
                .any(|candidate| value.eq_ignore_ascii_case(candidate))
            {
                Some(true)
            } else if ["false", "0", "no", "inactive", "invalid", "expired"]
                .iter()
                .any(|candidate| value.eq_ignore_ascii_case(candidate))
            {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn active_score(map: &serde_json::Map<String, Value>, now: DateTime<Utc>) -> i8 {
    if let Some(status) = direct_value(map, &["status", "instanceStatus"]).and_then(Value::as_str) {
        let status = status.trim();
        if ["VALID", "ACTIVE"]
            .iter()
            .any(|candidate| status.eq_ignore_ascii_case(candidate))
        {
            return 3;
        }
        if [
            "EXPIRED",
            "INVALID",
            "INACTIVE",
            "DISABLED",
            "TERMINATED",
            "STOPPED",
        ]
        .iter()
        .any(|candidate| status.eq_ignore_ascii_case(candidate))
        {
            return -1;
        }
    }
    if let Some(active) = direct_bool(map, &["isActive", "active"]) {
        return if active { 3 } else { -1 };
    }
    if parse_date(direct_value(
        map,
        &["endTime", "periodEndTime", "expireTime", "expirationTime"],
    ))
    .is_some_and(|end| end > now)
    {
        return 1;
    }
    0
}

fn find_instances(value: &Value) -> Option<&Vec<Value>> {
    find_value(
        value,
        &["codingPlanInstanceInfos", "coding_plan_instance_infos"],
    )?
    .as_array()
}

const QUOTA_KEYS: [&str; 12] = [
    "per5HourUsedQuota",
    "perFiveHourUsedQuota",
    "per5HourTotalQuota",
    "perFiveHourTotalQuota",
    "perWeekUsedQuota",
    "perWeekTotalQuota",
    "perBillMonthUsedQuota",
    "perMonthUsedQuota",
    "perBillMonthTotalQuota",
    "perMonthTotalQuota",
    "per5HourQuotaNextRefreshTime",
    "perWeekQuotaNextRefreshTime",
];

fn find_quota_in_map(
    map: &serde_json::Map<String, Value>,
) -> Option<&serde_json::Map<String, Value>> {
    for key in ["codingPlanQuotaInfo", "coding_plan_quota_info"] {
        if let Some(Value::Object(quota)) = map.get(key) {
            return Some(quota);
        }
    }
    if QUOTA_KEYS.iter().any(|key| map.contains_key(*key)) {
        return Some(map);
    }
    map.values()
        .filter(|value| !value.is_array())
        .find_map(find_quota_without_arrays)
}

fn find_quota_without_arrays(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value.as_object().and_then(find_quota_in_map)
}

fn selected_instance(
    instances: &[Value],
    now: DateTime<Utc>,
) -> Option<(&serde_json::Map<String, Value>, i8)> {
    instances
        .iter()
        .filter_map(Value::as_object)
        .map(|instance| (instance, active_score(instance, now)))
        .max_by_key(|(_, score)| *score)
}

fn window_metric(
    quota: &serde_json::Map<String, Value>,
    label: &str,
    period: &str,
    window: MetricWindow,
    used_keys: &[&str],
    total_keys: &[&str],
    reset_keys: &[&str],
) -> Option<Metric> {
    let used = finite_number(direct_value(quota, used_keys)).filter(|value| *value >= 0.0);
    let limit = finite_number(direct_value(quota, total_keys)).filter(|value| *value > 0.0);
    if used.is_none() && limit.is_none() {
        return None;
    }
    let remaining = used.zip(limit).map(|(used, limit)| (limit - used).max(0.0));
    Some(Metric {
        billing: Default::default(),
        scope: None,
        label: label.into(),
        used,
        limit,
        remaining,
        unit: "requisições".into(),
        kind: MetricKind::Quota,
        window: Some(window),
        period: Some(period.into()),
        resets_at: parse_date(direct_value(quota, reset_keys)),
    })
}

/// Which credential a Coding Plan query used, for the note on its report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueriedWith {
    ApiKey,
    /// The session a `params.session_env` variable holds.
    Variable,
    /// The session kept in the system vault (PRD 0016).
    Vault,
}

impl QueriedWith {
    fn session(session: &super::WebSession) -> Self {
        if session.vault {
            Self::Vault
        } else {
            Self::Variable
        }
    }
}

fn parse_coding_plan_payload(
    payload: Value,
    region: Region,
    via: QueriedWith,
    now: DateTime<Utc>,
) -> Result<Data, QueryError> {
    let session = via != QueriedWith::ApiKey;
    let payload = expand_embedded_json(payload);
    let code = find_string(&payload, &["code", "statusCode", "status_code"]);
    let message = find_string(&payload, &["message", "msg", "statusMessage", "status_msg"]);
    let login = code
        .as_deref()
        .is_some_and(|value| contains_ascii_case_insensitive(value, "needlogin"))
        || message
            .as_deref()
            .is_some_and(|value| contains_ascii_case_insensitive(value, "log in"))
        || message
            .as_deref()
            .is_some_and(|value| contains_ascii_case_insensitive(value, "login"));
    if login {
        return Err(login_required(session));
    }
    if let Some(code) = code.as_deref().and_then(|value| value.parse::<i64>().ok()) {
        if code == 401 || code == 403 {
            return Err(http_error(
                reqwest::StatusCode::from_u16(code as u16)
                    .unwrap_or(reqwest::StatusCode::UNAUTHORIZED),
                session,
            ));
        }
        if code != 0 && code != 200 {
            return Err(QueryError {
                diagnostic: None,
                status: Status::Error,
                note: Localized::fixed(
                    "query.alibaba_plans.rpc_quota",
                    "Alibaba Coding Plan returned an error in the quota RPC.",
                ),
            });
        }
    }

    let instances = find_instances(&payload);
    let selected = instances.and_then(|items| selected_instance(items, now));
    let multiple_instances =
        instances.is_some_and(|items| items.iter().filter(|value| value.is_object()).count() > 1);

    let quota = match selected {
        Some((_, score)) if score < 0 => None,
        Some((instance, score)) if multiple_instances && score > 0 => find_quota_in_map(instance),
        Some((instance, _)) => {
            find_quota_in_map(instance).or_else(|| find_quota_without_arrays(&payload))
        }
        None => find_quota_without_arrays(&payload),
    }
    .ok_or_else(QueryError::schema)?;

    let mut metrics = Vec::with_capacity(3);
    if let Some(metric) = window_metric(
        quota,
        "Coding Plan: 5h",
        "janela de 5h",
        MetricWindow::Window5h,
        &["per5HourUsedQuota", "perFiveHourUsedQuota"],
        &["per5HourTotalQuota", "perFiveHourTotalQuota"],
        &[
            "per5HourQuotaNextRefreshTime",
            "perFiveHourQuotaNextRefreshTime",
        ],
    ) {
        metrics.push(metric);
    }
    if let Some(metric) = window_metric(
        quota,
        "Coding Plan: semanal",
        "janela semanal",
        MetricWindow::Weekly,
        &["perWeekUsedQuota"],
        &["perWeekTotalQuota"],
        &["perWeekQuotaNextRefreshTime"],
    ) {
        metrics.push(metric);
    }
    if let Some(metric) = window_metric(
        quota,
        "Coding Plan: mensal",
        "ciclo mensal",
        MetricWindow::Monthly,
        &["perBillMonthUsedQuota", "perMonthUsedQuota"],
        &["perBillMonthTotalQuota", "perMonthTotalQuota"],
        &[
            "perBillMonthQuotaNextRefreshTime",
            "perMonthQuotaNextRefreshTime",
        ],
    ) {
        metrics.push(metric);
    }
    if metrics.is_empty() {
        return Err(QueryError::schema_named(
            &payload,
            "perMonthUsedQuota",
            None,
            "number",
        ));
    }

    let plan_name = selected
        .and_then(|(instance, _)| {
            direct_value(
                instance,
                &[
                    "planName",
                    "plan_name",
                    "instanceName",
                    "instance_name",
                    "packageName",
                    "package_name",
                ],
            )
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let mut notes = Notes::new();
    if let Some(plan_name) = plan_name {
        notes.push_params(
            "note.alibaba_plans.plan",
            params([("plan", ParamValue::from(plan_name.clone()))]),
            "Plan: {plan}.",
        );
    }
    match via {
        QueriedWith::Vault => notes.push(
            "note.alibaba_plans.vault_session_query",
            "Quota queried with the web session kept in the system vault.",
        ),
        QueriedWith::Variable => notes.push(
            "note.alibaba_plans.session_query",
            "Quota queried with the session indicated by params.session_env.",
        ),
        QueriedWith::ApiKey => notes.push(
            "note.alibaba_plans.api_key_query",
            "Quota queried with the API key indicated by credential_env.",
        ),
    }
    Ok(Data::new(metrics, notes, coding_plan_dashboard(region)))
}

async fn query_with_api_key(
    ctx: &Context<'_>,
    key: &str,
    region: Region,
    endpoints: CodingPlanEndpoints<'_>,
) -> Result<Data, QueryError> {
    let payload = response_json(
        ctx.http
            .post(endpoints.gateway)
            .query(&[
                ("action", CODING_PLAN_ACTION),
                ("product", "broadscope-bailian"),
                ("api", "queryCodingPlanInstanceInfoV2"),
                ("currentRegionId", region_id(region)),
            ])
            .bearer_auth(key)
            .header("x-api-key", key)
            .header("X-DashScope-API-Key", key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Origin", console_origin(region))
            .header("Referer", coding_plan_dashboard(region))
            .timeout(ctx.timeout)
            .json(&json!({
                "queryCodingPlanInstanceInfoRequest": {
                    "commodityCode": commodity_code(region)
                }
            })),
        false,
    )
    .await?;
    parse_coding_plan_payload(payload, region, QueriedWith::ApiKey, Utc::now())
}

fn cookie_value<'a>(cookie: &'a str, name: &str) -> Option<&'a str> {
    cookie.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key.trim() == name)
            .then_some(value.trim())
            .filter(|value| !value.is_empty())
    })
}

fn sec_token_from_html(html: &str) -> Option<String> {
    for marker in ["SEC_TOKEN", "secToken", "sec_token"] {
        let mut remaining = html;
        while let Some(offset) = remaining.find(marker) {
            let after_marker = &remaining[offset + marker.len()..];
            let separator = after_marker.find(':').or_else(|| after_marker.find('='));
            if let Some(separator) = separator {
                let candidate = after_marker[separator + 1..].trim_start();
                if let Some(quote) = candidate.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    let value = &candidate[quote.len_utf8()..];
                    if let Some(end) = value.find(quote) {
                        let value = value[..end].trim();
                        if !value.is_empty() {
                            return Some(value.to_string());
                        }
                    }
                }
            }
            remaining = after_marker;
        }
    }
    None
}

async fn resolve_sec_token(
    ctx: &Context<'_>,
    cookie: &str,
    region: Region,
    endpoints: CodingPlanEndpoints<'_>,
) -> Result<String, QueryError> {
    if let Ok(response) = super::send_session(
        ctx.http
            .get(endpoints.dashboard)
            .header("Cookie", cookie)
            .header("User-Agent", BROWSER_USER_AGENT)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .timeout(ctx.timeout),
        crate::Provider::AlibabaCodingPlan,
        "session_env",
    )
    .await
    {
        if response.status().is_success() {
            if let Ok(html) = super::read_body(response, super::BODY_LIMIT)
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            {
                if let Some(token) = sec_token_from_html(&html) {
                    return Ok(token);
                }
            }
        }
    }

    // The last page decides: one that could not answer says nothing about
    // the session, so its failure is reported instead of a login request.
    let outage = match super::send_session(
        ctx.http
            .get(endpoints.user_info)
            .header("Cookie", cookie)
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .header("Referer", console_origin(region))
            .timeout(ctx.timeout),
        crate::Provider::AlibabaCodingPlan,
        "session_env",
    )
    .await
    {
        Ok(response) if response.status().is_success() => {
            if let Ok(payload) = super::read_json::<Value>(response).await {
                let expanded = expand_embedded_json(payload);
                if let Some(token) = find_string(&expanded, &["secToken", "sec_token"]) {
                    return Ok(token);
                }
            }
            None
        }
        other => console_outage(other),
    };

    cookie_value(cookie, "sec_token")
        .map(str::to_string)
        .ok_or_else(|| outage.unwrap_or_else(|| login_required(true)))
}

/// The failure of a console page that did not answer the question: a request
/// that never got through, a rate limit or a server failure. `None` when the
/// answer refused the session (a redirect, a 401).
fn console_outage(answer: Result<reqwest::Response, QueryError>) -> Option<QueryError> {
    match answer {
        Ok(response) => {
            let code = response.status();
            (code.as_u16() == 429 || code.is_server_error()).then(|| http_error(code, true))
        }
        Err(error) => (error.status != Status::AuthenticationError).then_some(error),
    }
}

async fn query_with_session(
    ctx: &Context<'_>,
    session: &super::WebSession,
    region: Region,
    endpoints: CodingPlanEndpoints<'_>,
) -> Result<Data, QueryError> {
    let cookie = session.cookie.as_str();
    let sec_token = resolve_sec_token(ctx, cookie, region, endpoints).await?;
    let trace_id = uuid::Uuid::new_v4().to_string();
    let mut cornerstone = json!({
        "feTraceId": trace_id,
        "feURL": coding_plan_dashboard(region),
        "protocol": "V2",
        "console": "ONE_CONSOLE",
        "productCode": "p_efm",
        "domain": console_domain(region),
        "consoleSite": console_site(region),
        "userNickName": "",
        "userPrincipalName": "",
        "xsp_lang": "en-US"
    });
    if let Some(anonymous) = cookie_value(cookie, "cna") {
        cornerstone
            .as_object_mut()
            .expect("cornerstone is an object")
            .insert("X-Anonymous-Id".into(), Value::String(anonymous.into()));
    }
    let params = json!({
        "Api": CODING_PLAN_ACTION,
        "V": "1.0",
        "Data": {
            "queryCodingPlanInstanceInfoRequest": {
                "commodityCode": commodity_code(region),
                "onlyLatestOne": true
            },
            "cornerstoneParam": cornerstone
        }
    })
    .to_string();
    let form = [
        ("params", params.as_str()),
        ("region", region_id(region)),
        ("sec_token", sec_token.as_str()),
    ];
    let mut request = ctx
        .http
        .post(endpoints.rpc)
        .query(&[
            ("action", console_rpc_action(region)),
            ("product", "sfm_bailian"),
            ("api", CODING_PLAN_ACTION),
            ("_v", "undefined"),
        ])
        .header("Cookie", cookie)
        .header("Accept", "*/*")
        .header("X-Requested-With", "XMLHttpRequest")
        .header("User-Agent", BROWSER_USER_AGENT)
        .header("Origin", console_origin(region))
        .header("Referer", console_referer(region))
        .timeout(ctx.timeout)
        .form(&form);
    if let Some(csrf) =
        cookie_value(cookie, "login_aliyunid_csrf").or_else(|| cookie_value(cookie, "csrf"))
    {
        request = request
            .header("x-xsrf-token", csrf)
            .header("x-csrf-token", csrf);
    }
    let payload = response_json(request, true).await?;
    parse_coding_plan_payload(payload, region, QueriedWith::session(session), Utc::now())
}

async fn fetch_coding_plan_at(
    account: &Account,
    ctx: &Context<'_>,
    endpoints: CodingPlanEndpoints<'_>,
) -> Result<Data, QueryError> {
    let region = parse_region(account)?;
    if let Some(session) = super::web_session(account, ctx, "session_env")? {
        return query_with_session(ctx, &session, region, endpoints).await;
    }
    let key = secret(account, ctx, "BAILIAN_CODING_PLAN_API_KEY")?;
    query_with_api_key(ctx, &key, region, endpoints).await
}

async fn alibaba_coding_plan(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    let region = parse_region(account)?;
    fetch_coding_plan_at(account, ctx, production_endpoints(region)).await
}

// Token Plan quota is a console session query, not an inference API-key call.
const TOKEN_PLAN_USAGE_API: &str = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage";
const TOKEN_PLAN_SUBSCRIPTION_API: &str =
    "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/subscription";
const TOKEN_PLAN_QUOTA_CONFIG_API: &str =
    "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/quota-config";
const TOKEN_PLAN_DASHBOARD: &str =
    "https://home.qwencloud.com/billing/subscription/token-plan-individual";
const TOKEN_PLAN_COMMODITY_CODE: &str = "sfm_tokenplansolo_public_intl";
const TOKEN_PLAN_CONSOLE_PRODUCT: &str = "sfm_bailian";
const TOKEN_PLAN_CONSOLE_ACTION: &str = "IntlBroadScopeAspnGateway";
const TOKEN_PLAN_REGION: &str = "ap-southeast-1";
const TOKEN_PLAN_LANGUAGE: &str = "en-US";
const TOKEN_PLAN_CONSOLE_SITE: &str = "QWENCLOUD";
const TOKEN_PLAN_DOMAIN: &str = "home.qwencloud.com";

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct TokenPlanQuotaTotals {
    five_hour: Option<f64>,
    weekly: Option<f64>,
    monthly: Option<f64>,
}

fn token_plan_metrics(
    payload: Value,
    quota: TokenPlanQuotaTotals,
) -> Result<Vec<Metric>, QueryError> {
    let payload = expand_embedded_json(payload);
    if find_string(&payload, &["code"])
        .is_some_and(|code| contains_ascii_case_insensitive(&code, "needlogin"))
    {
        return Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.alibaba_plans.token_plan_session_expired",
                "Alibaba Token Plan session expired; renew params.session_env.",
            ),
        });
    }
    let mut metrics = Vec::new();
    for (label, period, window, percentage, reset, total) in [
        (
            "Token Plan: 5h",
            "janela de 5h",
            MetricWindow::Window5h,
            "per5HourPercentage",
            "per5HourResetTime",
            quota.five_hour,
        ),
        (
            "Token Plan: semanal",
            "janela semanal",
            MetricWindow::Weekly,
            "per1WeekPercentage",
            "per1WeekResetTime",
            quota.weekly,
        ),
        (
            "Token Plan: mensal",
            "ciclo mensal",
            MetricWindow::Monthly,
            "per1MonthPercentage",
            "per1MonthResetTime",
            quota.monthly,
        ),
    ] {
        if let Some(raw) = find_value(&payload, &[percentage]).filter(|raw| !raw.is_null()) {
            let fraction = raw.as_f64().ok_or_else(|| {
                QueryError::schema_named(&payload, percentage, Some(raw), "number")
            })?;
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err(QueryError::schema_named(
                    &payload,
                    percentage,
                    Some(raw),
                    "number",
                ));
            }
            let resets_at = find_value(&payload, &[reset])
                .filter(|value| !value.is_null())
                .map(|value| {
                    value
                        .as_i64()
                        .and_then(chrono::DateTime::from_timestamp_millis)
                        .ok_or_else(|| {
                            QueryError::schema_named(&payload, reset, Some(value), "number")
                        })
                })
                .transpose()?;
            let used_percent = fraction * 100.0;
            if let Some(total) = total.filter(|value| value.is_finite() && *value > 0.0) {
                let used = total * fraction;
                metrics.push(Metric {
                    billing: Default::default(),
                    scope: None,
                    label: label.into(),
                    used: Some(used),
                    limit: Some(total),
                    remaining: Some((total - used).max(0.0)),
                    unit: "tokens".into(),
                    kind: MetricKind::Quota,
                    window: Some(window),
                    period: Some(period.into()),
                    resets_at,
                });
            } else {
                metrics.push(Metric {
                    billing: Default::default(),
                    scope: None,
                    label: label.into(),
                    used: Some(used_percent),
                    limit: Some(100.0),
                    remaining: Some(100.0 - used_percent),
                    kind: MetricKind::Quota,
                    window: Some(window),
                    unit: "%".into(),
                    period: Some(period.into()),
                    resets_at,
                });
            }
        }
    }
    if metrics.is_empty() {
        return Err(QueryError::schema_named(
            &payload,
            "per5HourPercentage",
            None,
            "number",
        ));
    }
    Ok(metrics)
}

fn find_object_containing_keys<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Option<&'a serde_json::Map<String, Value>> {
    match value {
        Value::Object(map) => {
            if keys.iter().any(|key| map.contains_key(*key)) {
                return Some(map);
            }
            map.values().find_map(|value| {
                if value.is_array() {
                    None
                } else {
                    find_object_containing_keys(value, keys)
                }
            })
        }
        Value::Array(items) => items
            .iter()
            .find_map(|value| find_object_containing_keys(value, keys)),
        _ => None,
    }
}

fn plan_code_from_subscription(payload: &Value) -> Option<String> {
    let expanded = expand_embedded_json(payload.clone());
    let plan = find_object_containing_keys(
        &expanded,
        &["specCode", "spec_code", "planName", "plan_name"],
    )?;
    for key in ["specCode", "spec_code", "planName", "plan_name"] {
        if let Some(value) = plan.get(key).and_then(scalar_string) {
            let trimmed = value.trim().to_ascii_lowercase();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn finite_f64(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .and_then(|n| (n.is_finite() && n > 0.0).then_some(n))
}

fn quota_totals_from_config(payload: &Value, plan_code: &str) -> TokenPlanQuotaTotals {
    let expanded = expand_embedded_json(payload.clone());
    let Some(entry) = find_value(&expanded, &[plan_code]) else {
        return TokenPlanQuotaTotals::default();
    };
    let Some(quota) = entry.as_object() else {
        return TokenPlanQuotaTotals::default();
    };
    let five_hour = finite_f64(quota.get("five_hour").or_else(|| quota.get("fiveHour")))
        .or_else(|| finite_f64(quota.get("5hour")));
    let weekly = finite_f64(quota.get("weekly"));
    let monthly = finite_f64(quota.get("monthly"));
    TokenPlanQuotaTotals {
        five_hour,
        weekly,
        monthly,
    }
}

struct TokenPlanSession<'a> {
    cookie: &'a str,
    sec_token: &'a str,
}

fn build_token_plan_request(
    ctx: &Context<'_>,
    endpoint: &str,
    session: &TokenPlanSession<'_>,
    api: &str,
    data_params: Value,
) -> reqwest::RequestBuilder {
    let mut cornerstone = json!({
        "feTraceId": uuid::Uuid::new_v4().to_string(),
        "feURL": TOKEN_PLAN_DASHBOARD,
        "protocol": "V2",
        "console": "ONE_CONSOLE",
        "productCode": "p_efm",
        "domain": TOKEN_PLAN_DOMAIN,
        "consoleSite": TOKEN_PLAN_CONSOLE_SITE,
        "userNickName": "",
        "userPrincipalName": "",
        "xsp_lang": TOKEN_PLAN_LANGUAGE,
    });
    if let Some(cna) = cookie_value(session.cookie, "cna") {
        cornerstone["X-Anonymous-Id"] = Value::String(cna.into());
    }
    let mut data = match data_params {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("value".into(), other);
            map
        }
    };
    data.insert("cornerstoneParam".into(), cornerstone);
    let envelope = json!({"Api": api, "V": "1.0", "Data": Value::Object(data)}).to_string();
    let form = [
        ("product", TOKEN_PLAN_CONSOLE_PRODUCT),
        ("action", TOKEN_PLAN_CONSOLE_ACTION),
        ("sec_token", session.sec_token),
        ("region", TOKEN_PLAN_REGION),
        ("language", TOKEN_PLAN_LANGUAGE),
        ("params", envelope.as_str()),
    ];
    let mut request = ctx
        .http
        .post(endpoint)
        .query(&[
            ("action", TOKEN_PLAN_CONSOLE_ACTION),
            ("product", TOKEN_PLAN_CONSOLE_PRODUCT),
            ("api", api),
            ("_v", "undefined"),
        ])
        .header("Cookie", session.cookie)
        .header("Origin", format!("https://{TOKEN_PLAN_DOMAIN}"))
        .header("Referer", TOKEN_PLAN_DASHBOARD)
        .header("User-Agent", BROWSER_USER_AGENT)
        .header("X-Requested-With", "XMLHttpRequest")
        .timeout(ctx.timeout)
        .form(&form);
    if let Some(csrf) = cookie_value(session.cookie, "login_aliyunid_csrf")
        .or_else(|| cookie_value(session.cookie, "csrf"))
    {
        request = request
            .header("x-xsrf-token", csrf)
            .header("x-csrf-token", csrf);
    }
    request
}

async fn token_plan_send(
    ctx: &Context<'_>,
    endpoint: &str,
    session: &TokenPlanSession<'_>,
    api: &str,
    data_params: Value,
) -> Result<reqwest::Response, QueryError> {
    let request = build_token_plan_request(ctx, endpoint, session, api, data_params);
    let response =
        super::send_session(request, crate::Provider::AlibabaTokenPlan, "session_env").await?;
    if !response.status().is_success() {
        let code = response.status();
        return Err(QueryError {
            diagnostic: None,
            status: match code.as_u16() {
                401 | 403 => Status::AuthenticationError,
                429 => Status::RateLimited,
                _ => Status::Error,
            },
            note: Localized::with_params(
                "query.alibaba_plans.token_plan_http",
                params([
                    ("api", ParamValue::from(api.to_string())),
                    ("code", ParamValue::from(code.as_u16())),
                ]),
                "Token Plan {api} returned HTTP {code}.",
            ),
        });
    }
    Ok(response)
}

async fn token_plan_post_value(
    ctx: &Context<'_>,
    endpoint: &str,
    session: &TokenPlanSession<'_>,
    api: &str,
    data_params: Value,
) -> Result<Option<Value>, QueryError> {
    match token_plan_send(ctx, endpoint, session, api, data_params).await {
        Ok(response) => super::body_value(response).await.map(Some),
        Err(err) if matches!(err.status, Status::Error | Status::RateLimited) => Ok(None),
        Err(err) => Err(err),
    }
}

async fn fetch_token_plan_metadata(
    ctx: &Context<'_>,
    endpoint: &str,
    session: &TokenPlanSession<'_>,
) -> (Option<String>, TokenPlanQuotaTotals) {
    let subscription_params = json!({"commodityCode": TOKEN_PLAN_COMMODITY_CODE});
    let subscription = token_plan_post_value(
        ctx,
        endpoint,
        session,
        TOKEN_PLAN_SUBSCRIPTION_API,
        subscription_params,
    )
    .await
    .unwrap_or_default();
    let plan_code = subscription.as_ref().and_then(plan_code_from_subscription);
    let quota_config = token_plan_post_value(
        ctx,
        endpoint,
        session,
        TOKEN_PLAN_QUOTA_CONFIG_API,
        Value::Object(serde_json::Map::new()),
    )
    .await
    .unwrap_or_default();
    let quota = match (plan_code.as_deref(), quota_config.as_ref()) {
        (Some(code), Some(payload)) => quota_totals_from_config(payload, code),
        _ => TokenPlanQuotaTotals::default(),
    };
    (plan_code, quota)
}

/// Token Plan quota of `account`: its web session (the vault one, or the
/// variable `params.session_env` names) takes precedence over the `bl`
/// profile in `params.cli_profile`. `endpoint` and `user_info` are the
/// console URLs (the test servers in tests).
async fn token_plan_at(
    account: &Account,
    ctx: &Context<'_>,
    endpoint: &str,
    user_info: &str,
) -> Result<Data, QueryError> {
    if parse_region(account)? != Region::Intl {
        return Err(QueryError::unsupported(
            "query.alibaba_plans.token_plan_intl_only",
            "Qwen Cloud Token Plan requires the intl (Singapore) region.",
        ));
    }
    if let Some(session) = super::web_session(account, ctx, "session_env")? {
        return token_plan_with_session(ctx, &session, endpoint, user_info).await;
    }
    match account
        .params
        .get("cli_profile")
        .map(String::as_str)
        .filter(|name| !name.trim().is_empty())
    {
        Some(profile) => token_plan_via_cli(ctx, profile).await,
        None => Err(QueryError::config(
            "query.alibaba_plans.token_plan_auth_method_required",
            "Paste the Qwen Cloud session in the account wizard, or configure params.session_env (variable with the cookie) or params.cli_profile (official CLI) for the Qwen Cloud Token Plan.",
        )),
    }
}

async fn token_plan_with_session(
    ctx: &Context<'_>,
    web: &super::WebSession,
    endpoint: &str,
    user_info: &str,
) -> Result<Data, QueryError> {
    let cookie = web.cookie.as_str();
    let dashboard = TOKEN_PLAN_DASHBOARD;
    let info = super::send_session(
        ctx.http
            .get(user_info)
            .header("Cookie", cookie)
            .header("Referer", dashboard)
            .timeout(ctx.timeout),
        crate::Provider::AlibabaTokenPlan,
        "session_env",
    )
    .await?;
    let code = info.status();
    if code.as_u16() == 429 || code.is_server_error() {
        return Err(QueryError {
            diagnostic: None,
            status: if code.as_u16() == 429 {
                Status::RateLimited
            } else {
                Status::Error
            },
            note: Localized::with_params(
                "query.alibaba_plans.token_plan_http",
                params([
                    ("api", ParamValue::from("user/info")),
                    ("code", ParamValue::from(code.as_u16())),
                ]),
                "Token Plan {api} returned HTTP {code}.",
            ),
        });
    }
    if !code.is_success() {
        return Err(QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.alibaba_plans.qwen_cloud_session_invalid",
                "Invalid Qwen Cloud session; renew params.session_env.",
            ),
        });
    }
    let info = super::body_value(info).await?;
    let info = expand_embedded_json(info);
    let sec_token = find_string(&info, &["secToken", "sec_token"])
        .or_else(|| cookie_value(cookie, "sec_token").map(str::to_owned))
        .ok_or_else(|| QueryError {
            diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.alibaba_plans.qwen_cloud_missing_sec_token",
                "Qwen Cloud session without sec_token; renew params.session_env.",
            ),
        })?;
    let session = TokenPlanSession {
        cookie,
        sec_token: &sec_token,
    };
    let usage_response = token_plan_send(
        ctx,
        endpoint,
        &session,
        TOKEN_PLAN_USAGE_API,
        Value::Object(serde_json::Map::new()),
    )
    .await?;
    let usage_payload = super::body_value(usage_response).await?;
    let (_plan_code, quota) = fetch_token_plan_metadata(ctx, endpoint, &session).await;
    let metrics = token_plan_metrics(usage_payload, quota)?;
    let mut notes = Notes::new();
    if web.vault {
        notes.push(
            "note.alibaba_plans.token_plan_vault_session_query",
            "Quota queried with the web session kept in the system vault; QWEN_API_KEY is not sent.",
        );
    } else {
        notes.push(
            "note.alibaba_plans.token_plan_session_query",
            "Quota queried with the session indicated by params.session_env; QWEN_API_KEY is not sent.",
        );
    }
    if quota == TokenPlanQuotaTotals::default() {
        notes.push(
            "note.alibaba_plans.token_plan_metadata_unavailable",
            "Qwen Cloud metadata (subscription/quota-config) unavailable; the report shows percentages only.",
        );
    }
    Ok(Data::new(metrics, notes, dashboard))
}

async fn alibaba_token_plan(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    token_plan_at(
        account,
        ctx,
        "https://cs-data.qwencloud.com/data/api.json",
        "https://home.qwencloud.com/tool/user/info.json",
    )
    .await
}

fn is_safe_cli_profile(profile: &str) -> bool {
    crate::account_edit::is_valid_cli_profile(profile)
}

fn classify_cli_failure(stderr: &str) -> Status {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("login")
        || lower.contains("auth")
        || lower.contains("credential")
        || lower.contains("session expired")
        || lower.contains("needlogin")
        || lower.contains("notlogin")
        || lower.contains("unauthenticated")
        || lower.contains("token expired")
        || lower.contains("token invalid")
        || lower.contains("expired")
    {
        Status::AuthenticationError
    } else {
        Status::Error
    }
}

fn token_plan_cli_metrics(payload: Value) -> Result<Vec<Metric>, QueryError> {
    let payload = expand_embedded_json(payload);
    if find_string(&payload, &["code"])
        .is_some_and(|code| contains_ascii_case_insensitive(&code, "needlogin"))
    {
        return Err(QueryError { diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.alibaba_plans.token_plan_cli_session_expired",
                "Alibaba Token Plan session expired; renew with `bl auth login --console --console-site international --config <profile>`.",
            ),
        });
    }
    let mut metrics = Vec::new();
    for (label, period, window, percentage, reset) in [
        (
            "Token Plan: 5h",
            "janela de 5h",
            MetricWindow::Window5h,
            "per5HourPercentage",
            "per5HourResetTime",
        ),
        (
            "Token Plan: semanal",
            "janela semanal",
            MetricWindow::Weekly,
            "per1WeekPercentage",
            "per1WeekResetTime",
        ),
    ] {
        if let Some(raw) = find_value(&payload, &[percentage]).filter(|raw| !raw.is_null()) {
            let fraction = raw.as_f64().ok_or_else(|| {
                QueryError::schema_named(&payload, percentage, Some(raw), "number")
            })?;
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err(QueryError::schema_named(
                    &payload,
                    percentage,
                    Some(raw),
                    "number",
                ));
            }
            let resets_at = find_value(&payload, &[reset])
                .filter(|value| !value.is_null())
                .map(|value| {
                    value
                        .as_i64()
                        .and_then(chrono::DateTime::from_timestamp_millis)
                        .ok_or_else(|| {
                            QueryError::schema_named(&payload, reset, Some(value), "number")
                        })
                })
                .transpose()?;
            metrics.push(Metric {
                billing: Default::default(),
                scope: None,
                label: label.into(),
                used: Some(fraction * 100.0),
                limit: Some(100.0),
                remaining: Some(100.0 - fraction * 100.0),
                unit: "%".into(),
                kind: MetricKind::Quota,
                window: Some(window),
                period: Some(period.into()),
                resets_at,
            });
        }
    }
    Ok(metrics)
}

async fn token_plan_via_cli(ctx: &Context<'_>, profile: &str) -> Result<Data, QueryError> {
    if !is_safe_cli_profile(profile) {
        return Err(QueryError::config(
            "query.alibaba_plans.invalid_cli_profile",
            "The `bl` profile name is invalid: letters, digits, '-' or '_', up to 32 characters.",
        ));
    }
    run_token_plan_cli(ctx, "bl", profile).await
}

async fn run_token_plan_cli(
    ctx: &Context<'_>,
    binary: &str,
    profile: &str,
) -> Result<Data, QueryError> {
    let mut cmd = tokio::process::Command::new(crate::cli_path::resolve(binary));
    cmd.args([
        "usage",
        "token-plan",
        "--console-site",
        "international",
        "--console-region",
        TOKEN_PLAN_REGION,
        "--config",
        profile,
        "--output",
        "json",
        "--quiet",
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .kill_on_drop(true);
    let child = cmd.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            QueryError::config(
                "query.alibaba_plans.cli_not_found",
                "`bl` (Bailian CLI) not found. Install the CLI and try again.",
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
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status = if serde_json::from_slice::<Value>(&output.stdout)
            .ok()
            .and_then(|payload| payload.pointer("/error/code").and_then(Value::as_i64))
            == Some(3)
        {
            Status::AuthenticationError
        } else {
            classify_cli_failure(&stderr)
        };
        let note = match status {
            Status::AuthenticationError => Localized::fixed(
                "query.alibaba_plans.cli_auth",
                "`bl auth login --console` exited with an error. Check the CLI and try again.",
            ),
            _ => Localized::fixed(
                "query.alibaba_plans.cli_failure",
                "Could not start `bl auth login --console`.",
            ),
        };
        return Err(QueryError {
            diagnostic: None,
            status,
            note,
        });
    }
    let stdout = output.stdout;
    let trimmed = stdout
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|start| &stdout[start..])
        .unwrap_or(&[]);
    let payload: Value = serde_json::from_slice(trimmed).map_err(|_| QueryError::schema())?;
    if !payload.is_object() {
        return Err(QueryError::schema_at(&payload, &[], "object"));
    }
    let metrics = token_plan_cli_metrics(payload)?;
    let mut notes = Notes::new();
    let profile_param = params([("profile", ParamValue::from(profile.to_string()))]);
    if metrics.is_empty() {
        notes.push_params(
            "note.alibaba_plans.cli_quota_unknown",
            profile_param,
            "CLI `bl` queried profile '{profile}' but did not report 5h or weekly limits; quota unknown. Check the Token Plan console.",
        );
    } else {
        notes.push_params(
            "note.alibaba_plans.cli_query",
            profile_param,
            "Quota queried via CLI `bl` with profile '{profile}'; missing windows remain unknown. QWEN_API_KEY is not used.",
        );
    }
    Ok(Data::new(metrics, notes, TOKEN_PLAN_DASHBOARD))
}

#[cfg(all(test, unix))]
async fn token_plan_via_cli_with_path(
    _account: &Account,
    ctx: &Context<'_>,
    profile: &str,
    binary: &str,
) -> Result<Data, QueryError> {
    run_token_plan_cli(ctx, binary, profile).await
}

// --------------------------------------------------------------- testes

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Status};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{
        bearer_token, body_json, body_string_contains, header, method, path, query_param,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct TestCredentials(&'static str, &'static str);
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            if name == self.0 {
                Some(self.1.to_string())
            } else {
                None
            }
        }
    }

    struct MapCredentials(BTreeMap<String, String>);
    impl Credentials for MapCredentials {
        fn get(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }
    fn account(provider: Provider, env: &'static str, region: Option<&'static str>) -> Account {
        let mut params = BTreeMap::new();
        if let Some(r) = region {
            params.insert("region".into(), r.into());
        }
        Account {
            id: None,
            name: "conta".into(),
            provider,
            credential_env: Some(env.into()),
            session: None,
            oauth: None,
            params,
            refresh_seconds: None,
            group: None,
        }
    }

    fn ctx_with<'a>(http: &'a reqwest::Client, creds: &'a dyn Credentials) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: Duration::from_secs(5),
        }
    }

    #[tokio::test]
    async fn model_studio_catalog_reads_every_page_until_total() {
        let server = MockServer::start().await;
        let page = |no: u64, total: u64, ids: &[&str]| {
            let models: Vec<_> = ids
                .iter()
                .map(|id| json!({ "model": id, "name": id.to_uppercase() }))
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "output": { "total": total, "page_no": no, "page_size": 20, "models": models }
            }))
        };
        let mount = async |no: &str, response: ResponseTemplate| {
            Mock::given(method("GET"))
                .and(bearer_token("sk-ms"))
                .and(query_param("page_no", no))
                .respond_with(response)
                .mount(&server)
                .await;
        };
        let creds = TestCredentials("DASHSCOPE_API_KEY", "sk-ms");
        let http = reqwest::Client::new();
        let ctx = ctx_with(&http, &creds);
        let acc = account(Provider::AlibabaModelStudio, "DASHSCOPE_API_KEY", None);
        let list = async || model_studio_models(&acc, &ctx, &server.uri()).await;

        mount("1", page(1, 3, &["qwen-b", "qwen-a"])).await;
        mount("2", page(2, 3, &["qwen-c"])).await;
        let ids: Vec<String> = list().await.unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["qwen-b", "qwen-a", "qwen-c"]);

        // Total says more but the next page is empty: incomplete listing.
        server.reset().await;
        mount("1", page(1, 5, &["qwen-a"])).await;
        mount("2", page(2, 5, &[])).await;
        assert_eq!(list().await.unwrap_err().status, Status::Error);

        // The server echoes another page number: never accepted.
        server.reset().await;
        mount("1", page(3, 1, &["qwen-a"])).await;
        assert_eq!(list().await.unwrap_err().status, Status::Error);

        server.reset().await;
        mount("1", ResponseTemplate::new(401).set_body_string("sk-ms")).await;
        let denied = list().await.unwrap_err();
        assert_eq!(denied.status, Status::AuthenticationError);
        assert!(!denied.note.fallback.contains("sk-ms"));

        assert!(model_studio_lists(&acc));
        let cn = account(
            Provider::AlibabaModelStudio,
            "DASHSCOPE_API_KEY",
            Some("cn"),
        );
        assert!(!model_studio_lists(&cn), "cn needs a WorkspaceId host");
    }

    async fn fetch_on(
        account: &Account,
        ctx: &Context<'_>,
        server: &MockServer,
    ) -> Result<Data, QueryError> {
        let gateway = format!("{}/api", server.uri());
        let dashboard = format!("{}/dashboard", server.uri());
        let rpc = format!("{}/rpc", server.uri());
        let user_info = format!("{}/user-info", server.uri());
        fetch_coding_plan_at(
            account,
            ctx,
            CodingPlanEndpoints {
                gateway: &gateway,
                dashboard: &dashboard,
                rpc: &rpc,
                user_info: &user_info,
            },
        )
        .await
    }

    fn metric<'a>(data: &'a Data, label: &str) -> &'a Metric {
        data.metrics
            .iter()
            .find(|metric| metric.label == label)
            .expect("métrica esperada")
    }
    #[test]
    fn parse_region_aceita_aliases_e_normaliza_case() {
        for (raw, expected) in [
            ("intl", Region::Intl),
            ("INTL", Region::Intl),
            ("international", Region::Intl),
            ("sgp", Region::Intl),
            ("cn", Region::Cn),
            ("CN", Region::Cn),
            ("beijing", Region::Cn),
        ] {
            let mut acc = account(Provider::AlibabaCodingPlan, "QWEN_API_KEY", Some(raw));
            assert_eq!(parse_region(&acc).unwrap(), expected, "alias {raw}");
            acc.params.remove("region");
            assert_eq!(parse_region(&acc).unwrap(), Region::Intl, "padrão intl");
        }
    }

    #[test]
    fn parse_region_rejeita_valores_desconhecidos() {
        let acc = account(Provider::AlibabaCodingPlan, "QWEN_API_KEY", Some("eu"));
        let err = parse_region(&acc).unwrap_err();
        assert!(matches!(err.status, Status::NeedsConfiguration));
        assert!(err.note.fallback.contains("params.region"));
        assert!(err.note.fallback.contains("'eu'"));
    }

    #[tokio::test]
    async fn alibaba_model_studio_sem_chave_devolve_needs_configuration() {
        let http = reqwest::Client::new();
        let acc = account(
            Provider::AlibabaModelStudio,
            "DASHSCOPE_API_KEY",
            Some("intl"),
        );
        let creds = TestCredentials("OTHER_KEY", "x");
        let ctx = ctx_with(&http, &creds);
        let err = alibaba_model_studio(&acc, &ctx).await.unwrap_err();
        assert!(matches!(err.status, Status::NeedsConfiguration));
        assert!(err.note.fallback.contains("DASHSCOPE_API_KEY"));
    }

    #[tokio::test]
    async fn alibaba_model_studio_com_chave_e_region_invalida_rejeita() {
        let http = reqwest::Client::new();
        let acc = account(
            Provider::AlibabaModelStudio,
            "DASHSCOPE_API_KEY",
            Some("eu"),
        );
        let creds = TestCredentials("DASHSCOPE_API_KEY", "sk-test");
        let ctx = ctx_with(&http, &creds);
        let err = alibaba_model_studio(&acc, &ctx).await.unwrap_err();
        assert!(matches!(err.status, Status::NeedsConfiguration));
        assert!(err.note.fallback.contains("'eu'"));
    }

    #[tokio::test]
    async fn alibaba_model_studio_cn_e_intl_devolve_unsupported_com_dashboard_correto() {
        let http = reqwest::Client::new();
        let creds = TestCredentials("DASHSCOPE_API_KEY", "sk-test");
        let ctx = ctx_with(&http, &creds);

        let acc_cn = account(
            Provider::AlibabaModelStudio,
            "DASHSCOPE_API_KEY",
            Some("cn"),
        );
        let err_cn = alibaba_model_studio(&acc_cn, &ctx).await.unwrap_err();
        assert!(matches!(err_cn.status, Status::Unsupported));
        assert!(err_cn.note.fallback.contains(MODEL_STUDIO_BILLING_CN));
        assert!(err_cn.note.fallback.contains(MODEL_STUDIO_FREE_QUOTA_CN));
        assert!(err_cn.note.fallback.contains(DASHSCOPE_INFERENCE_DOCS));
        assert!(err_cn.note.fallback.contains(CODING_PLAN_BASE_CN));

        let acc_intl = account(
            Provider::AlibabaModelStudio,
            "DASHSCOPE_API_KEY",
            Some("intl"),
        );
        let err_intl = alibaba_model_studio(&acc_intl, &ctx).await.unwrap_err();
        assert!(matches!(err_intl.status, Status::Unsupported));
        assert!(err_intl.note.fallback.contains(MODEL_STUDIO_USAGE_INTL));
        assert!(err_intl
            .note
            .fallback
            .contains(MODEL_STUDIO_FREE_QUOTA_INTL));
        assert!(err_intl.note.fallback.contains(DASHSCOPE_INFERENCE_DOCS));
        assert!(err_intl.note.fallback.contains(CODING_PLAN_BASE_INTL));
    }

    #[tokio::test]
    async fn api_key_customizada_retorna_tres_janelas_com_fracoes_e_resets() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .and(query_param("action", CODING_PLAN_ACTION))
            .and(query_param("product", "broadscope-bailian"))
            .and(query_param("api", "queryCodingPlanInstanceInfoV2"))
            .and(query_param("currentRegionId", "ap-southeast-1"))
            .and(bearer_token("custom-secret"))
            .and(header("x-api-key", "custom-secret"))
            .and(header("X-DashScope-API-Key", "custom-secret"))
            .and(body_json(json!({
                "queryCodingPlanInstanceInfoRequest": {
                    "commodityCode": "sfm_codingplan_public_intl"
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status_code": 0,
                "data": {
                    "codingPlanInstanceInfos": [{"planName": "Pro", "status": "VALID"}],
                    "codingPlanQuotaInfo": {
                        "per5HourUsedQuota": 12.5,
                        "per5HourTotalQuota": 100.25,
                        "per5HourQuotaNextRefreshTime": 1_700_000_300_000_i64,
                        "perWeekUsedQuota": "2.25",
                        "perWeekTotalQuota": "10.5",
                        "perWeekQuotaNextRefreshTime": "2026-09-30T12:30:00Z",
                        "perBillMonthTotalQuota": 90
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let credentials = MapCredentials(BTreeMap::from([
            ("MY_QWEN_KEY".into(), "custom-secret".into()),
            ("QWEN_API_KEY".into(), "wrong-secret".into()),
        ]));
        let http = reqwest::Client::new();
        let data = fetch_on(
            &account(Provider::AlibabaCodingPlan, "MY_QWEN_KEY", Some("intl")),
            &ctx_with(&http, &credentials),
            &server,
        )
        .await
        .unwrap();

        let five_hour = metric(&data, "Coding Plan: 5h");
        assert_eq!(five_hour.used, Some(12.5));
        assert_eq!(five_hour.limit, Some(100.25));
        assert_eq!(five_hour.remaining, Some(87.75));
        assert_eq!(five_hour.kind, MetricKind::Quota);
        assert_eq!(five_hour.window, Some(MetricWindow::Window5h));
        assert_eq!(
            five_hour.resets_at,
            DateTime::<Utc>::from_timestamp_millis(1_700_000_300_000)
        );
        let weekly = metric(&data, "Coding Plan: semanal");
        assert_eq!(weekly.used, Some(2.25));
        assert_eq!(weekly.limit, Some(10.5));
        assert_eq!(weekly.kind, MetricKind::Quota);
        assert_eq!(weekly.window, Some(MetricWindow::Weekly));
        assert_eq!(
            weekly.resets_at,
            Some(
                DateTime::parse_from_rfc3339("2026-09-30T12:30:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );
        let monthly = metric(&data, "Coding Plan: mensal");
        assert_eq!(monthly.used, None);
        assert_eq!(monthly.limit, Some(90.0));
        assert_eq!(monthly.remaining, None);
        assert_eq!(monthly.kind, MetricKind::Quota);
        assert_eq!(monthly.window, Some(MetricWindow::Monthly));
    }

    #[tokio::test]
    async fn json_aninhado_serializado_nao_mistura_instancia_expirada() {
        let server = MockServer::start().await;
        let inner = json!({
            "statusCode": 200,
            "data": {
                "codingPlanInstanceInfos": [
                    {
                        "planName": "Expired",
                        "status": "EXPIRED",
                        "codingPlanQuotaInfo": {
                            "per5HourUsedQuota": 99,
                            "per5HourTotalQuota": 100
                        }
                    },
                    {
                        "planName": "Active",
                        "status": "VALID",
                        "codingPlanQuotaInfo": {
                            "per5HourUsedQuota": 4,
                            "per5HourTotalQuota": 40,
                            "perWeekUsedQuota": 5,
                            "perWeekTotalQuota": 50,
                            "perBillMonthUsedQuota": 6,
                            "perBillMonthTotalQuota": 60
                        }
                    }
                ]
            }
        });
        Mock::given(method("POST"))
            .and(path("/api"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"DataV2": {"data": inner.to_string()}}
            })))
            .mount(&server)
            .await;
        let credentials = MapCredentials(BTreeMap::from([("QWEN_API_KEY".into(), "key".into())]));
        let http = reqwest::Client::new();
        let data = fetch_on(
            &account(Provider::AlibabaCodingPlan, "QWEN_API_KEY", Some("intl")),
            &ctx_with(&http, &credentials),
            &server,
        )
        .await
        .unwrap();
        assert_eq!(metric(&data, "Coding Plan: 5h").used, Some(4.0));
        assert_eq!(metric(&data, "Coding Plan: semanal").used, Some(5.0));
        assert_eq!(metric(&data, "Coding Plan: mensal").used, Some(6.0));
        assert!(data.notes.iter().any(|note| note == "Plan: Active."));
    }

    #[tokio::test]
    async fn console_need_login_com_api_key_pede_session_sem_invalidar_chave() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "code": "ConsoleNeedLogin",
                "message": "You need to log in."
            })))
            .mount(&server)
            .await;
        let credentials = MapCredentials(BTreeMap::from([("QWEN_API_KEY".into(), "key".into())]));
        let http = reqwest::Client::new();
        let error = fetch_on(
            &account(Provider::AlibabaCodingPlan, "QWEN_API_KEY", Some("intl")),
            &ctx_with(&http, &credentials),
            &server,
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
        assert!(error.note.fallback.contains("params.session_env"));
        assert!(!error
            .note
            .fallback
            .to_ascii_lowercase()
            .contains("inválida"));
    }

    #[tokio::test]
    async fn session_env_preferida_obtem_sec_token_e_faz_rpc_com_cookie() {
        let server = MockServer::start().await;
        let cookie = "login_aliyunid_ticket=ticket; cna=anon; csrf=csrf-value";
        Mock::given(method("GET"))
            .and(path("/dashboard"))
            .and(header("Cookie", cookie))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"<script>window.cfg={SEC_TOKEN: "dashboard-token"}</script>"#,
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rpc"))
            .and(query_param("action", "IntlBroadScopeAspnGateway"))
            .and(query_param("product", "sfm_bailian"))
            .and(query_param("api", CODING_PLAN_ACTION))
            .and(header("Cookie", cookie))
            .and(header("x-xsrf-token", "csrf-value"))
            .and(body_string_contains("sec_token=dashboard-token"))
            .and(body_string_contains("region=ap-southeast-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "codingPlanQuotaInfo": {
                        "per5HourUsedQuota": 1,
                        "per5HourTotalQuota": 10
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let credentials =
            MapCredentials(BTreeMap::from([("ALIBABA_SESSION".into(), cookie.into())]));
        let http = reqwest::Client::new();
        let mut account = account(Provider::AlibabaCodingPlan, "MISSING_API_KEY", Some("intl"));
        account
            .params
            .insert("session_env".into(), "ALIBABA_SESSION".into());
        let data = fetch_on(&account, &ctx_with(&http, &credentials), &server)
            .await
            .unwrap();
        assert_eq!(metric(&data, "Coding Plan: 5h").remaining, Some(9.0));
    }

    /// `sec_token` comes from two console pages. When they are down, the
    /// session was not refused: it must not read as a login failure.
    #[tokio::test]
    async fn coding_plan_console_outage_is_not_a_login_failure() {
        for (code, expected) in [
            (Some(503), Status::Error),
            (Some(429), Status::RateLimited),
            (Some(200), Status::AuthenticationError),
            (Some(302), Status::AuthenticationError),
            (None, Status::Error),
        ] {
            let server = MockServer::start().await;
            let uri = if let Some(code) = code {
                Mock::given(method("GET"))
                    .respond_with(ResponseTemplate::new(code).set_body_string("<html></html>"))
                    .mount(&server)
                    .await;
                server.uri()
            } else {
                // A port nothing listens on: the console is unreachable.
                let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                format!("http://{}", closed.local_addr().unwrap())
            };
            let cookie = "login_aliyunid_ticket=ticket; cna=anon";
            let credentials =
                MapCredentials(BTreeMap::from([("ALIBABA_SESSION".into(), cookie.into())]));
            let http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap();
            let mut account = account(Provider::AlibabaCodingPlan, "MISSING_API_KEY", Some("intl"));
            account
                .params
                .insert("session_env".into(), "ALIBABA_SESSION".into());
            let (dashboard, rpc, user_info, gateway) = (
                format!("{uri}/dashboard"),
                format!("{uri}/rpc"),
                format!("{uri}/user-info"),
                format!("{uri}/api"),
            );
            let result = fetch_coding_plan_at(
                &account,
                &ctx_with(&http, &credentials),
                CodingPlanEndpoints {
                    gateway: &gateway,
                    dashboard: &dashboard,
                    rpc: &rpc,
                    user_info: &user_info,
                },
            )
            .await;
            let Err(error) = result else {
                panic!("{code:?} must fail the account");
            };
            assert_eq!(error.status, expected, "{code:?}");
        }
    }

    /// PRD 0016 RF5.1: a session kept in the vault (read under the name the
    /// monitor puts in `params.session_env`) takes the same console route as
    /// the variable, and the report says which one it used.
    #[tokio::test]
    async fn coding_plan_reads_the_vault_session_like_the_variable_and_says_so() {
        let server = MockServer::start().await;
        let cookie = "login_aliyunid_ticket=ticket; cna=anon";
        Mock::given(method("GET"))
            .and(path("/dashboard"))
            .and(header("Cookie", cookie))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"<script>window.cfg={SEC_TOKEN: "tok"}</script>"#),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rpc"))
            .and(header("Cookie", cookie))
            .and(body_string_contains("sec_token=tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"codingPlanQuotaInfo": {"per5HourUsedQuota": 2, "per5HourTotalQuota": 10}}
            })))
            .mount(&server)
            .await;
        let vault = format!("{}session_env", crate::user_session::VAULT_NAME);
        let http = reqwest::Client::new();
        for (name, note) in [
            (vault.as_str(), "note.alibaba_plans.vault_session_query"),
            ("ALIBABA_SESSION", "note.alibaba_plans.session_query"),
        ] {
            let credentials = MapCredentials(BTreeMap::from([(name.into(), cookie.into())]));
            let mut account = account(Provider::AlibabaCodingPlan, "MISSING_API_KEY", Some("intl"));
            account.params.insert("session_env".into(), name.into());
            let data = fetch_on(&account, &ctx_with(&http, &credentials), &server)
                .await
                .unwrap();
            assert_eq!(
                metric(&data, "Coding Plan: 5h").remaining,
                Some(8.0),
                "{name}"
            );
            let ids: Vec<_> = data.notes_l10n.iter().map(|n| n.id.to_string()).collect();
            assert!(ids.iter().any(|id| id == note), "{name}: {ids:?}");
        }
    }

    #[tokio::test]
    async fn token_plan_prefers_the_vault_session_over_the_cli_profile_and_says_so() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tool/user/info.json"))
            .and(header("Cookie", "session=vault-cookie"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"secToken": "sec"}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/data/api.json"))
            .and(query_param("api", TOKEN_PLAN_USAGE_API))
            .and(header("Cookie", "session=vault-cookie"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": "{\"per5HourPercentage\":0.5,\"per5HourResetTime\":1787001180000}"}),
            ))
            .mount(&server)
            .await;
        let vault = format!("{}session_env", crate::user_session::VAULT_NAME);
        let credentials = MapCredentials(BTreeMap::from([(
            vault.clone(),
            "session=vault-cookie".into(),
        )]));
        let none = account(Provider::AlibabaTokenPlan, "QWEN_API_KEY", None);
        let mut with_session = none.clone();
        with_session.params.insert("session_env".into(), vault);
        // The `bl` profile is never run while a session exists.
        with_session
            .params
            .insert("cli_profile".into(), "cuotai-qwen".into());
        let http = reqwest::Client::new();
        let data = token_plan_at(
            &with_session,
            &ctx_with(&http, &credentials),
            &format!("{}/data/api.json", server.uri()),
            &format!("{}/tool/user/info.json", server.uri()),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics[0].remaining, Some(50.0));
        assert_eq!(
            data.notes_l10n[0].id,
            "note.alibaba_plans.token_plan_vault_session_query"
        );

        // Neither a session nor a profile: the paste comes first in the
        // guidance, the variable and the profile stay as alternatives.
        let error = token_plan_at(
            &none,
            &ctx_with(&http, &credentials),
            &format!("{}/data/api.json", server.uri()),
            &format!("{}/tool/user/info.json", server.uri()),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
        let en = error.note.resolve("en-US");
        assert!(en.starts_with("Paste the Qwen Cloud session"), "{en}");
        assert!(en.contains("params.session_env") && en.contains("params.cli_profile"));
    }

    #[tokio::test]
    async fn quotas_invalidas_ausentes_e_http_nao_vazam_segredos() {
        for payload in [
            json!({"status_code": 0, "data": {"codingPlanQuotaInfo": {}}}),
            json!({
                "status_code": 0,
                "data": {
                    "codingPlanQuotaInfo": {
                        "per5HourUsedQuota": "NaN",
                        "per5HourTotalQuota": 0,
                        "perWeekUsedQuota": -1,
                        "perWeekTotalQuota": "Infinity"
                    },
                    "echo": "custom-secret"
                }
            }),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/api"))
                .respond_with(ResponseTemplate::new(200).set_body_json(payload))
                .mount(&server)
                .await;
            let credentials = MapCredentials(BTreeMap::from([(
                "CUSTOM_KEY".into(),
                "custom-secret".into(),
            )]));
            let http = reqwest::Client::new();
            let error = fetch_on(
                &account(Provider::AlibabaCodingPlan, "CUSTOM_KEY", Some("intl")),
                &ctx_with(&http, &credentials),
                &server,
            )
            .await
            .unwrap_err();
            assert_eq!(error.status, Status::Error);
            assert!(!error.note.fallback.contains("custom-secret"));
        }

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .respond_with(
                ResponseTemplate::new(500).set_body_string("custom-secret server diagnostic"),
            )
            .mount(&server)
            .await;
        let credentials = MapCredentials(BTreeMap::from([(
            "CUSTOM_KEY".into(),
            "custom-secret".into(),
        )]));
        let http = reqwest::Client::new();
        let error = fetch_on(
            &account(Provider::AlibabaCodingPlan, "CUSTOM_KEY", Some("intl")),
            &ctx_with(&http, &credentials),
            &server,
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::Error);
        assert!(!error.note.fallback.contains("custom-secret"));
        assert!(!error.note.fallback.contains("server diagnostic"));
    }

    #[tokio::test]
    async fn fetch_dispatch_nao_atende_outros_provedores() {
        let http = reqwest::Client::new();
        let creds = TestCredentials("DASHSCOPE_API_KEY", "sk-test");
        let ctx = ctx_with(&http, &creds);
        let acc = account(Provider::Qwen, "QWEN_API_KEY", Some("intl"));
        let err = fetch(&acc, &ctx).await.unwrap_err();
        assert!(matches!(err.status, Status::Unsupported));
        assert!(err.note.fallback.contains("Qwen"));
        assert!(err.note.fallback.contains("another adapter"));
    }
    #[test]
    fn token_plan_janelas_independentes_e_sem_quota_inventada() {
        let metrics = token_plan_metrics(serde_json::json!({"data": {"per1WeekPercentage": 0.7, "per1WeekResetTime": 1787001180000i64}}), TokenPlanQuotaTotals::default()).unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].label, "Token Plan: semanal");
        assert_eq!(metrics[0].used, Some(70.0));
        assert!((metrics[0].remaining.unwrap() - 30.0).abs() < 1e-10);
        assert_eq!(
            metrics[0].resets_at.unwrap().timestamp_millis(),
            1787001180000
        );
        assert!(token_plan_metrics(json!({}), TokenPlanQuotaTotals::default()).is_err());
        assert!(token_plan_metrics(
            json!({"per5HourPercentage": 1.4}),
            TokenPlanQuotaTotals::default()
        )
        .is_err());
    }
    #[test]
    fn token_plan_monthly_only_is_reported() {
        let metrics = token_plan_metrics(
            json!({"data": {"per1MonthPercentage": 0.4, "per1MonthResetTime": 1787001180000i64}}),
            TokenPlanQuotaTotals::default(),
        )
        .unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].label, "Token Plan: mensal");
        assert_eq!(metrics[0].used, Some(40.0));
        assert_eq!(metrics[0].unit, "%");
    }
    #[test]
    fn token_plan_quota_totals_aplicados_somente_com_plano_correspondente() {
        let payload = json!({
            "data": {
                "per5HourPercentage": 0.25,
                "per5HourResetTime": 1787001180000i64,
                "per1WeekPercentage": 0.5,
                "per1WeekResetTime": 1788001180000i64,
                "per1MonthPercentage": 0.1,
                "per1MonthResetTime": 1789001180000i64
            }
        });
        let metrics = token_plan_metrics(
            payload.clone(),
            TokenPlanQuotaTotals {
                five_hour: Some(8_000.0),
                weekly: Some(40_000.0),
                monthly: Some(10_000_000.0),
            },
        )
        .unwrap();
        assert_eq!(metrics.len(), 3);
        assert!(metrics.iter().all(|m| m.kind == MetricKind::Quota));
        let windows: Vec<_> = metrics.iter().map(|m| m.window).collect();
        assert_eq!(
            windows,
            [
                Some(MetricWindow::Window5h),
                Some(MetricWindow::Weekly),
                Some(MetricWindow::Monthly),
            ]
        );
        assert_eq!(metrics[0].unit, "tokens");
        assert_eq!(metrics[0].limit, Some(8_000.0));
        assert_eq!(metrics[0].used, Some(2_000.0));
        assert_eq!(metrics[1].limit, Some(40_000.0));
        assert_eq!(metrics[1].used, Some(20_000.0));
        assert_eq!(metrics[2].limit, Some(10_000_000.0));
        assert_eq!(metrics[2].used, Some(1_000_000.0));
    }
    #[test]
    fn token_plan_metadata_helpers_leem_apenas_campos_conhecidos() {
        let subscription = json!({
            "data": {
                "specCode": "Lite",
                "planName": "lite",
                "totals": {"ignored": true}
            }
        });
        assert_eq!(
            plan_code_from_subscription(&subscription).as_deref(),
            Some("lite")
        );
        let quotas = quota_totals_from_config(
            &json!({"data": {"lite": {"five_hour": 8000, "weekly": 40000, "monthly": 0}}}),
            "lite",
        );
        assert_eq!(quotas.five_hour, Some(8000.0));
        assert_eq!(quotas.weekly, Some(40_000.0));
        assert_eq!(quotas.monthly, None);
        assert_eq!(
            quota_totals_from_config(&json!({"data": {"max": {"weekly": 1}}}), "lite"),
            TokenPlanQuotaTotals::default()
        );
    }
    #[tokio::test]
    async fn qwen_cloud_consulta_cookie_e_sec_token_sem_api_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tool/user/info.json"))
            .and(header("Cookie", "session=test-cookie"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"secToken": "sec-test"}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/data/api.json"))
            .and(query_param("api", TOKEN_PLAN_USAGE_API))
            .and(header("Cookie", "session=test-cookie"))
            .and(header("Origin", "https://home.qwencloud.com"))
            .and(body_string_contains("sec_token=sec-test"))
            .and(body_string_contains("QWENCLOUD"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": "{\"per5HourPercentage\":0.25,\"per5HourResetTime\":1787001180000}"})))
            .mount(&server).await;
        let http = reqwest::Client::new();
        let creds = TestCredentials("QWEN_CLOUD_COOKIE", "session=test-cookie");
        let mut account = account(Provider::AlibabaTokenPlan, "QWEN_API_KEY", None);
        account
            .params
            .insert("session_env".into(), "QWEN_CLOUD_COOKIE".into());
        let data = token_plan_at(
            &account,
            &ctx_with(&http, &creds),
            &format!("{}/data/api.json", server.uri()),
            &format!("{}/tool/user/info.json", server.uri()),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics[0].remaining, Some(75.0));
        assert_eq!(
            data.metrics[0].resets_at.unwrap().timestamp_millis(),
            1787001180000
        );
    }

    /// `null` is an absent value: one window without a reset, or without a
    /// reading yet, must not fail the windows that have one. No metric is
    /// invented for the null percentage.
    #[test]
    fn token_plan_null_fields_are_absent() {
        let payload = || {
            json!({
                "per5HourPercentage": 0.25,
                "per5HourResetTime": null,
                "per1WeekPercentage": null,
                "per1WeekResetTime": null
            })
        };
        let session = token_plan_metrics(payload(), TokenPlanQuotaTotals::default())
            .unwrap_or_else(|error| panic!("session: {error:?}"));
        let cli =
            token_plan_cli_metrics(payload()).unwrap_or_else(|error| panic!("cli: {error:?}"));
        for metrics in [session, cli] {
            assert_eq!(metrics.len(), 1);
            assert_eq!(metrics[0].remaining, Some(75.0));
            assert_eq!(metrics[0].resets_at, None);
        }
    }

    /// The login and the form accept a profile with a leading underscore:
    /// the query must accept every profile they let through.
    #[test]
    fn the_query_accepts_every_profile_the_login_accepts() {
        for name in ["_work", "work", "a-b_c", "-x", "", "a b", "with.dot"] {
            assert_eq!(
                is_safe_cli_profile(name),
                crate::account_edit::is_valid_cli_profile(name),
                "{name:?}"
            );
        }
    }

    /// An outage of the user-info route says nothing about the session:
    /// reporting it as invalid drops the preserved reading and asks the user
    /// to paste a session that still works.
    #[tokio::test]
    async fn qwen_cloud_user_info_outage_is_not_an_invalid_session() {
        for (code, expected) in [
            (503, Status::Error),
            (500, Status::Error),
            (429, Status::RateLimited),
            (401, Status::AuthenticationError),
            (403, Status::AuthenticationError),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/tool/user/info.json"))
                .respond_with(ResponseTemplate::new(code))
                .mount(&server)
                .await;
            let http = reqwest::Client::new();
            let creds = TestCredentials("QWEN_CLOUD_COOKIE", "session=test-cookie");
            let mut account = account(Provider::AlibabaTokenPlan, "QWEN_API_KEY", None);
            account
                .params
                .insert("session_env".into(), "QWEN_CLOUD_COOKIE".into());
            let result = token_plan_at(
                &account,
                &ctx_with(&http, &creds),
                &format!("{}/data/api.json", server.uri()),
                &format!("{}/tool/user/info.json", server.uri()),
            )
            .await;
            let Err(error) = result else {
                panic!("HTTP {code} must fail the account");
            };
            assert_eq!(error.status, expected, "HTTP {code}");
        }
    }

    #[tokio::test]
    async fn token_plan_usage_sobrevive_a_falha_dos_metadados() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tool/user/info.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"secToken": "sec-test"}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/data/api.json"))
            .and(query_param("api", TOKEN_PLAN_USAGE_API))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"data": "{\"per1MonthPercentage\":0.4,\"per1MonthResetTime\":1787001180000}"}),
                ),
            )
            .mount(&server)
            .await;
        // Metadata endpoints respond with 500; usage must still succeed and
        // surface the percent-based monthly metric.
        for api in [TOKEN_PLAN_SUBSCRIPTION_API, TOKEN_PLAN_QUOTA_CONFIG_API] {
            Mock::given(method("POST"))
                .and(path("/data/api.json"))
                .and(query_param("api", api))
                .respond_with(
                    ResponseTemplate::new(500)
                        .set_body_string("metadata-secret custom-secret-marker"),
                )
                .mount(&server)
                .await;
        }
        let http = reqwest::Client::new();
        let creds = TestCredentials("QWEN_CLOUD_COOKIE", "session=test-cookie");
        let mut account = account(Provider::AlibabaTokenPlan, "QWEN_API_KEY", None);
        account
            .params
            .insert("session_env".into(), "QWEN_CLOUD_COOKIE".into());
        let data = token_plan_at(
            &account,
            &ctx_with(&http, &creds),
            &format!("{}/data/api.json", server.uri()),
            &format!("{}/tool/user/info.json", server.uri()),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].label, "Token Plan: mensal");
        assert_eq!(data.metrics[0].used, Some(40.0));
        assert_eq!(data.metrics[0].limit, Some(100.0));
        assert_eq!(data.metrics[0].unit, "%");
        assert!(
            data.notes.iter().any(|note| note.contains("metadata")),
            "anotação deve indicar metadata indisponível: {:?}",
            data.notes
        );
        assert!(!data
            .notes
            .iter()
            .any(|note| note.contains("custom-secret-marker")));
    }

    #[tokio::test]
    async fn token_plan_quota_totais_aplicados_apenas_quando_plano_confere() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tool/user/info.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"secToken": "sec-test"}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/data/api.json"))
            .and(query_param("api", TOKEN_PLAN_USAGE_API))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({
                    "data": "{\"per5HourPercentage\":0.5,\"per5HourResetTime\":1787001180000,\"per1WeekPercentage\":0.25,\"per1WeekResetTime\":1788001180000}"
                })),
            )
            .mount(&server)
            .await;
        // Subscription returns "lite"; quota-config brings totals only for
        // "lite" — no match for other keys must be applied.
        Mock::given(method("POST"))
            .and(path("/data/api.json"))
            .and(query_param("api", TOKEN_PLAN_SUBSCRIPTION_API))
            .and(body_string_contains("commodityCode"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"specCode": "lite", "planName": "Lite"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/data/api.json"))
            .and(query_param("api", TOKEN_PLAN_QUOTA_CONFIG_API))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "lite": {"five_hour": 8000, "weekly": 40000},
                    "max": {"five_hour": 800000, "weekly": 4000000}
                }
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let creds = TestCredentials("QWEN_CLOUD_COOKIE", "session=test-cookie");
        let mut account = account(Provider::AlibabaTokenPlan, "QWEN_API_KEY", None);
        account
            .params
            .insert("session_env".into(), "QWEN_CLOUD_COOKIE".into());
        let data = token_plan_at(
            &account,
            &ctx_with(&http, &creds),
            &format!("{}/data/api.json", server.uri()),
            &format!("{}/tool/user/info.json", server.uri()),
        )
        .await
        .unwrap();
        let five_hour = data
            .metrics
            .iter()
            .find(|metric| metric.label == "Token Plan: 5h")
            .expect("métrica 5h");
        assert_eq!(five_hour.unit, "tokens");
        assert_eq!(five_hour.limit, Some(8_000.0));
        assert_eq!(five_hour.used, Some(4_000.0));
        let weekly = data
            .metrics
            .iter()
            .find(|metric| metric.label == "Token Plan: semanal")
            .expect("métrica semanal");
        assert_eq!(weekly.unit, "tokens");
        assert_eq!(weekly.limit, Some(40_000.0));
        assert_eq!(weekly.used, Some(10_000.0));
        assert!(data.notes.iter().all(|note| !note.contains("indisponível")));
    }

    #[test]
    fn cli_profile_validador_rejeita_injecao_e_numeros() {
        assert!(is_safe_cli_profile("default"));
        assert!(is_safe_cli_profile("work-profile-1_v2"));
        assert!(!is_safe_cli_profile(""));
        assert!(!is_safe_cli_profile("-leading"));
        assert!(!is_safe_cli_profile("with space"));
        assert!(!is_safe_cli_profile("semi;colon"));
        assert!(!is_safe_cli_profile("with$shell"));
        assert!(!is_safe_cli_profile("with.dot"));
        assert!(!is_safe_cli_profile(&"x".repeat(33)));
        assert!(!is_safe_cli_profile(&"x".repeat(65)));
    }

    #[test]
    fn cli_classificacao_erros_login_e_generico() {
        assert_eq!(
            classify_cli_failure("Error: Please login first"),
            Status::AuthenticationError
        );
        assert_eq!(
            classify_cli_failure("NeedLogin: token expired"),
            Status::AuthenticationError
        );
        assert_eq!(classify_cli_failure("network unreachable"), Status::Error);
    }

    #[test]
    fn cli_parser_aceita_5h_e_semanal_com_resets() {
        let metrics = token_plan_cli_metrics(json!({
            "per5HourPercentage": 0.25,
            "per5HourResetTime": 1787001180000i64,
            "per1WeekPercentage": 0.5,
            "per1WeekResetTime": 1788001180000i64
        }))
        .unwrap();
        assert_eq!(metrics.len(), 2);
        assert!(metrics.iter().all(|m| m.kind == MetricKind::Quota));
        assert_eq!(metrics[0].window, Some(MetricWindow::Window5h));
        assert_eq!(metrics[1].window, Some(MetricWindow::Weekly));
        assert_eq!(metrics[0].label, "Token Plan: 5h");
        assert_eq!(metrics[0].used, Some(25.0));
        assert_eq!(metrics[0].unit, "%");
        assert_eq!(metrics[1].label, "Token Plan: semanal");
        assert_eq!(metrics[1].used, Some(50.0));
    }

    #[test]
    fn cli_parser_objeto_vazio_preserva_quota_desconhecida() {
        assert!(token_plan_cli_metrics(json!({})).unwrap().is_empty());
        let metrics = token_plan_cli_metrics(json!({"per1WeekPercentage": 0.25})).unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].label, "Token Plan: semanal");
        assert_eq!(metrics[0].used, Some(25.0));
    }

    #[test]
    fn cli_parser_login_required_no_payload() {
        let err = token_plan_cli_metrics(json!({"code": "NeedLogin"})).unwrap_err();
        assert_eq!(err.status, Status::AuthenticationError);
    }

    #[cfg(unix)] // fake `bl` is a `#!/bin/sh` script
    #[tokio::test]
    async fn cli_consulta_bl_e_monta_metricas_quando_resposta_valida() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bl");
        let body = serde_json::to_string(&json!({
            "per5HourPercentage": 0.4,
            "per5HourResetTime": 1787001180000i64,
            "per1WeekPercentage": 0.7,
            "per1WeekResetTime": 1788001180000i64
        }))
        .unwrap();
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\n\
                 echo '{body}'\n\
                 exit 0\n"
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let http = reqwest::Client::new();
        let creds = MapCredentials(BTreeMap::new());
        let ctx = ctx_with(&http, &creds);
        let mut account = account(Provider::AlibabaTokenPlan, "QWEN_API_KEY", Some("intl"));
        account.params.insert("cli_profile".into(), "demo".into());
        let path = bin.to_string_lossy().into_owned();
        let data = token_plan_via_cli_with_path(&account, &ctx, "demo", &path)
            .await
            .unwrap();
        assert_eq!(data.metrics.len(), 2);
        assert_eq!(data.metrics[0].label, "Token Plan: 5h");
        assert_eq!(data.metrics[0].used, Some(40.0));
        assert_eq!(data.metrics[1].label, "Token Plan: semanal");
        assert_eq!(data.metrics[1].used, Some(70.0));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cli_objeto_vazio_retorna_quota_desconhecida() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bl");
        std::fs::write(&bin, "#!/bin/sh\necho '{}'\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let http = reqwest::Client::new();
        let creds = MapCredentials(BTreeMap::new());
        let ctx = ctx_with(&http, &creds);
        let mut account = account(Provider::AlibabaTokenPlan, "QWEN_API_KEY", Some("intl"));
        account.params.insert("cli_profile".into(), "demo".into());
        let path = bin.to_string_lossy().into_owned();
        let data = token_plan_via_cli_with_path(&account, &ctx, "demo", &path)
            .await
            .unwrap();
        assert!(data.metrics.is_empty());
        assert!(data.notes.iter().any(|note| note.contains("quota unknown")));
        assert!(data.notes.iter().all(|note| !note.contains("{}")));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cli_stdout_ausente_ou_nao_objeto_e_schema_incompativel() {
        for body in ["", "[]", "\"ok\""] {
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("bl");
            std::fs::write(&bin, format!("#!/bin/sh\nprintf '%s' '{body}'\nexit 0\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&bin).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&bin, perms).unwrap();
            }
            let http = reqwest::Client::new();
            let creds = MapCredentials(BTreeMap::new());
            let ctx = ctx_with(&http, &creds);
            let path = bin.to_string_lossy();
            let err = run_token_plan_cli(&ctx, &path, "demo").await.unwrap_err();
            assert_eq!(err.status, Status::Error, "body {body:?}");
            assert!(err.note.fallback.contains("incompatible"), "body {body:?}");
            if !body.is_empty() {
                assert!(!err.note.fallback.contains(body));
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cli_login_required_classifica_autenticacao_sem_vazar_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bl");
        std::fs::write(
            &bin,
            "#!/bin/sh\necho 'Error: please login first (token leaked XYZ_NEVER_LEAK)' >&2\nexit 1\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let http = reqwest::Client::new();
        let creds = MapCredentials(BTreeMap::new());
        let ctx = ctx_with(&http, &creds);
        let mut account = account(Provider::AlibabaTokenPlan, "QWEN_API_KEY", Some("intl"));
        account.params.insert("cli_profile".into(), "demo".into());
        let path = bin.to_string_lossy().into_owned();
        let err = token_plan_via_cli_with_path(&account, &ctx, "demo", &path)
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::AuthenticationError);
        assert!(!err.note.fallback.contains("XYZ_NEVER_LEAK"));
        assert!(!err.note.fallback.contains("secret-cli-token"));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cli_login_required_on_stdout_classifies_auth_without_leaking_payload() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bl");
        std::fs::write(
            &bin,
            "#!/bin/sh\nprintf '%s\\n' '{\"error\":{\"code\":3,\"message\":\"No console access token found.\",\"hint\":\"Run `bl auth login --console`.\"}}'\nexit 3\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let http = reqwest::Client::new();
        let creds = MapCredentials(BTreeMap::new());
        let ctx = ctx_with(&http, &creds);
        let path = bin.to_string_lossy();
        let err = run_token_plan_cli(&ctx, &path, "demo").await.unwrap_err();
        assert_eq!(err.status, Status::AuthenticationError);
        assert!(!err.note.fallback.contains("No console access token found"));
        assert!(!err.note.fallback.contains("Run `bl auth login --console`."));
    }
}
