//! Alibaba Model Studio free-tier quota and recent usage through the
//! official Bailian CLI: `bl usage summary --output json` on the account's
//! dedicated console profile (`params.cli_profile`, written by the same
//! `bl auth login --console` flow the Token Plan uses).
//!
//! Contract read from the CLI source
//! (`packages/commands/src/commands/usage/summary.ts`, bl 2.8.1), never from
//! a real account:
//!
//! ```json
//! {"period": {"start": "YYYY-MM-DD", "end": "YYYY-MM-DD", "days": 7},
//!  "freeTier": [{"model", "type", "remaining", "total", "remainingPercent", "expires"}],
//!  "usage": null | {"modelsCalled", "successfulCalls",
//!                   "usages": [{"key", "value", "unit", "label"}]}}
//! ```
//!
//! `freeTier` lists only valid quotas with a positive total, lowest remaining
//! first; `expires` is a local calendar date. `usage` needs a workspace: the
//! account's `params.workspace_id` is passed as `--workspace-id` and
//! `BAILIAN_WORKSPACE_ID` is removed from the child's environment, so a slow
//! telemetry poll only runs when the account asks for it.
//!
//! Presentation: a free-tier list may name hundreds of models, so the report
//! keeps the [`SHOWN_MODELS`] most urgent ones (one model-scoped `%` quota
//! each, the expiry date as `resets_at`), the count of models with a quota,
//! and the period totals (successful calls and tokens) as usage metrics.
//! An absent value stays absent: nothing is turned into zero.

use super::{parse_region, Region, MODEL_STUDIO_FREE_QUOTA_INTL};
use crate::providers::{Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Scope, Status};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;

/// Free-tier models shown as metrics, the least quota left first.
const SHOWN_MODELS: usize = 3;
/// Usage overview period passed as `--days` (the CLI's own default).
const USAGE_DAYS: u64 = 7;
/// Console region of the international site (Singapore).
const CONSOLE_REGION: &str = "ap-southeast-1";
/// Longest workspace id accepted as an argv value.
const WORKSPACE_MAX: usize = 64;

/// Usage keys reported as metrics, with their adapter label.
const USAGE_KEYS: [(&str, &str); 3] = [
    ("total_token", "Tokens totais"),
    ("input_token", "Tokens de entrada"),
    ("output_token", "Tokens de saída"),
];

/// The account's `bl` profile, when it reads the console through the CLI.
pub(super) fn profile(account: &Account) -> Option<&str> {
    account
        .params
        .get("cli_profile")
        .map(|profile| profile.trim())
        .filter(|profile| !profile.is_empty())
}

/// Free tier and usage of a Model Studio account with a `bl` profile.
pub(super) async fn query(
    account: &Account,
    ctx: &Context<'_>,
    profile: &str,
) -> Result<Data, QueryError> {
    // PRD 0054 RF6.1: the validations run first, so a bad profile or region
    // is reported even when `bl` is missing.
    query_with(account, ctx, profile, None).await
}

async fn query_with(
    account: &Account,
    ctx: &Context<'_>,
    profile: &str,
    binary: Option<&std::path::Path>,
) -> Result<Data, QueryError> {
    if parse_region(account)? != Region::Intl {
        return Err(QueryError::unsupported(
            "query.alibaba_plans.model_studio_cli_intl_only",
            "The Model Studio query through `bl` covers only the intl (Singapore) region.",
        ));
    }
    if !crate::external_tool::valid_bl_profile(profile) {
        return Err(QueryError::config(
            "query.alibaba_plans.invalid_cli_profile",
            "The `bl` profile name is invalid: letters, digits, '-' or '_', up to 32 characters.",
        ));
    }
    let workspace = workspace_id(account)?;
    let program = match binary {
        Some(binary) => binary.to_path_buf(),
        None => {
            crate::external_tool::resolve_for(crate::external_tool::Tool::Bailian, &account.params)?
                .program
        }
    };
    let payload = run(ctx, &program, profile, workspace).await?;
    summary_data(payload, profile, workspace.is_some())
}

/// The optional workspace, validated before it reaches argv.
fn workspace_id(account: &Account) -> Result<Option<&str>, QueryError> {
    let Some(raw) = account
        .params
        .get("workspace_id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let valid = raw.len() <= WORKSPACE_MAX
        && !raw.starts_with('-')
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(Some(raw))
    } else {
        Err(QueryError::config(
            "query.alibaba_plans.model_studio_invalid_workspace",
            "The Model Studio workspace ID is invalid: letters, digits, '-' or '_', up to 64 characters.",
        ))
    }
}

/// Runs `bl usage summary` and returns its JSON object. Neither stdout nor
/// stderr ever reaches a note: failures are classified by exit code and by
/// the CLI's JSON error code only.
async fn run(
    ctx: &Context<'_>,
    binary: &std::path::Path,
    profile: &str,
    workspace: Option<&str>,
) -> Result<Value, QueryError> {
    let days = USAGE_DAYS.to_string();
    let mut args = vec![
        "usage",
        "summary",
        "--days",
        days.as_str(),
        "--console-site",
        "international",
        "--console-region",
        CONSOLE_REGION,
        "--config",
        profile,
    ];
    if let Some(workspace) = workspace {
        args.extend(["--workspace-id", workspace]);
    }
    args.extend(["--output", "json", "--quiet"]);
    let child = tokio::process::Command::new(binary)
        .args(&args)
        .env_remove("BAILIAN_WORKSPACE_ID")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| {
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
        Err(_) => return Err(cli_timeout()),
    };
    let payload = parse_object(&output.stdout);
    if !output.status.success() {
        let code = payload
            .as_ref()
            .and_then(|payload| payload.pointer("/error/code"))
            .and_then(Value::as_i64)
            .or_else(|| output.status.code().map(i64::from));
        return Err(failure(code, &output.stderr));
    }
    let payload = payload.ok_or_else(QueryError::schema)?;
    if payload.get("error").is_some_and(|error| !error.is_null()) {
        let code = payload.pointer("/error/code").and_then(Value::as_i64);
        return Err(failure(code, &[]));
    }
    Ok(payload)
}

/// The JSON object on stdout (leading whitespace ignored), if any.
fn parse_object(stdout: &[u8]) -> Option<Value> {
    let start = stdout.iter().position(|byte| !byte.is_ascii_whitespace())?;
    serde_json::from_slice::<Value>(&stdout[start..])
        .ok()
        .filter(Value::is_object)
}

/// The CLI's exit codes (`packages/core/src/errors/codes.ts`): 3 auth,
/// 5 timeout, 6 network. Without a code, stderr is only scanned for the
/// login wording; its text is never kept.
fn failure(code: Option<i64>, stderr: &[u8]) -> QueryError {
    match code {
        Some(3) => cli_auth(),
        Some(5) => cli_timeout(),
        Some(6) => QueryError::transport(),
        _ if mentions_login(stderr) => cli_auth(),
        _ => QueryError {
            diagnostic: None,
            status: Status::Error,
            note: Localized::fixed(
                "query.alibaba_plans.model_studio_cli_failure",
                "`bl usage summary` failed. Check the CLI and try again.",
            ),
        },
    }
}

fn mentions_login(stderr: &[u8]) -> bool {
    let lower = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    [
        "login",
        "not logged",
        "unauthenticated",
        "session expired",
        "token expired",
        "needlogin",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn cli_auth() -> QueryError {
    QueryError {
        diagnostic: None,
        status: Status::AuthenticationError,
        note: Localized::fixed(
            "query.alibaba_plans.model_studio_cli_auth",
            "The `bl` console login of this account is missing or expired. Sign in again with the account's bl CLI profile.",
        ),
    }
}

fn cli_timeout() -> QueryError {
    QueryError {
        diagnostic: None,
        status: Status::Error,
        note: Localized::fixed(
            "query.alibaba_plans.model_studio_cli_timeout",
            "`bl usage summary` timed out; the usage overview can be slow. Try again later, or clear the workspace ID to read only the free tier.",
        ),
    }
}

/// One free-tier entry; `percent` is the remaining share, when known.
struct FreeTier {
    model: String,
    percent: Option<f64>,
    expires: Option<DateTime<Utc>>,
}

fn number(entry: &Value, key: &str, root: &Value) -> Result<Option<f64>, QueryError> {
    match entry.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(Some)
            .ok_or_else(|| QueryError::schema_named(root, key, Some(value), "number")),
    }
}

fn free_tier_entry(entry: &Value, root: &Value) -> Result<FreeTier, QueryError> {
    if !entry.is_object() {
        return Err(QueryError::schema_at(
            root,
            &["freeTier"],
            "array of objects",
        ));
    }
    let model = match entry.get("model") {
        Some(Value::String(model)) if !model.trim().is_empty() => model.trim().to_string(),
        found => return Err(QueryError::schema_named(root, "model", found, "string")),
    };
    let remaining = number(entry, "remaining", root)?;
    let total = number(entry, "total", root)?;
    let reported = number(entry, "remainingPercent", root)?;
    let percent = match (remaining, total) {
        (Some(remaining), Some(total)) if total > 0.0 && remaining >= 0.0 => {
            Some((remaining / total * 100.0).min(100.0))
        }
        _ => reported.filter(|percent| (0.0..=100.0).contains(percent)),
    };
    let expires = match entry.get("expires") {
        None | Some(Value::Null) => None,
        Some(Value::String(date)) => Some(
            NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
                .map_err(|_| {
                    QueryError::schema_named(root, "expires", entry.get("expires"), "date")
                })?
                .and_hms_opt(0, 0, 0)
                .expect("midnight exists")
                .and_utc(),
        ),
        found => return Err(QueryError::schema_named(root, "expires", found, "date")),
    };
    Ok(FreeTier {
        model,
        percent,
        expires,
    })
}

fn free_tier_metric(entry: FreeTier) -> Metric {
    Metric {
        billing: Default::default(),
        scope: Some(Scope::model(entry.model.clone(), entry.model.clone())),
        label: entry.model,
        used: entry.percent.map(|percent| 100.0 - percent),
        limit: entry.percent.map(|_| 100.0),
        remaining: entry.percent,
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: None,
        period: Some("cota gratuita".into()),
        resets_at: entry.expires,
        derived_from: None,
    }
}

fn usage_metric(label: &str, value: f64, unit: &str, period: &str) -> Metric {
    Metric {
        billing: Default::default(),
        scope: None,
        label: label.into(),
        used: Some(value),
        limit: None,
        remaining: None,
        unit: unit.into(),
        kind: MetricKind::Usage,
        window: Some(MetricWindow::Other),
        period: Some(period.into()),
        resets_at: None,
        derived_from: None,
    }
}

/// The report of one `bl usage summary` answer.
fn summary_data(payload: Value, profile: &str, workspace: bool) -> Result<Data, QueryError> {
    let entries = match payload.get("freeTier") {
        Some(Value::Array(entries)) => entries,
        found => {
            return Err(QueryError::schema_named(
                &payload, "freeTier", found, "array",
            ))
        }
    };
    let mut free_tier = entries
        .iter()
        .map(|entry| free_tier_entry(entry, &payload))
        .collect::<Result<Vec<_>, _>>()?;
    // The CLI already sorts by urgency; sorting again keeps that promise
    // even if it changes, and puts unknown shares last.
    free_tier.sort_by(|left, right| match (left.percent, right.percent) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    let count = free_tier.len();
    let mut metrics: Vec<Metric> = free_tier
        .into_iter()
        .take(SHOWN_MODELS)
        .map(free_tier_metric)
        .collect();
    metrics.push(Metric {
        billing: Default::default(),
        scope: None,
        label: "Modelos com cota gratuita".into(),
        used: Some(count as f64),
        limit: None,
        remaining: None,
        unit: "modelos".into(),
        kind: MetricKind::Usage,
        window: None,
        period: None,
        resets_at: None,
        derived_from: None,
    });

    let days = payload
        .pointer("/period/days")
        .and_then(Value::as_u64)
        .filter(|days| *days > 0)
        .unwrap_or(USAGE_DAYS);
    let period = format!("últimos {days} dias");
    let usage = match payload.get("usage") {
        None | Some(Value::Null) => None,
        Some(usage @ Value::Object(_)) => Some(usage),
        found => return Err(QueryError::schema_named(&payload, "usage", found, "object")),
    };
    if let Some(usage) = usage {
        if let Some(calls) = number(usage, "successfulCalls", &payload)? {
            metrics.push(usage_metric(
                "Chamadas bem-sucedidas",
                calls,
                "chamadas",
                &period,
            ));
        }
        let rows = match usage.get("usages") {
            None | Some(Value::Null) => &[][..],
            Some(Value::Array(rows)) => rows.as_slice(),
            found => return Err(QueryError::schema_named(&payload, "usages", found, "array")),
        };
        for (key, label) in USAGE_KEYS {
            let Some(row) = rows
                .iter()
                .find(|row| row.get("key").and_then(Value::as_str) == Some(key))
            else {
                continue;
            };
            if let Some(value) = number(row, "value", &payload)? {
                metrics.push(usage_metric(label, value, "tokens", &period));
            }
        }
    }

    let mut notes = Notes::new();
    notes.push_params(
        "note.alibaba_plans.model_studio_cli_query",
        params([("profile", ParamValue::from(profile.to_string()))]),
        "Free tier read through the `bl` CLI with profile '{profile}'; the DashScope key is not sent. The CLI gives expiry as a calendar date, shown at 00:00 UTC.",
    );
    if count == 0 {
        notes.push(
            "note.alibaba_plans.model_studio_free_tier_none",
            "`bl` reported no active free-tier quota for this account.",
        );
    } else if count > SHOWN_MODELS {
        notes.push_params(
            "note.alibaba_plans.model_studio_free_tier_more",
            params([
                ("shown", ParamValue::from(SHOWN_MODELS)),
                ("count", ParamValue::from(count)),
            ]),
            "Showing the {shown} models with the least free tier left out of {count}; the Model Studio console lists all of them.",
        );
    }
    if !workspace {
        notes.push(
            "note.alibaba_plans.model_studio_usage_skipped",
            "No usage overview: set the Model Studio workspace ID on this account to add the calls and tokens of the last days.",
        );
    } else if usage.is_none() {
        notes.push_params(
            "note.alibaba_plans.model_studio_usage_empty",
            params([("days", ParamValue::from(days))]),
            "`bl` returned no usage overview for the workspace in the last {days} days.",
        );
    }
    Ok(Data::new(metrics, notes, MODEL_STUDIO_FREE_QUOTA_INTL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Provider};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::time::Duration;

    struct NoCredentials;
    impl Credentials for NoCredentials {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    fn account(params: &[(&str, &str)]) -> Account {
        Account {
            provider: Provider::AlibabaModelStudio,
            params: params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..Default::default()
        }
    }

    fn full_payload() -> Value {
        json!({
            "period": {"start": "2026-10-28", "end": "2026-11-04", "days": 7},
            "freeTier": [
                {"model": "qwen-max", "type": "LLM", "remaining": 50000, "total": 1000000,
                 "remainingPercent": 5.0, "expires": "2026-12-01"},
                {"model": "qwen-plus", "type": "LLM", "remaining": 400000, "total": 1000000,
                 "remainingPercent": 40.0, "expires": "2027-01-15"},
                {"model": "wan2.7", "type": "Image", "remaining": 45, "total": 50,
                 "remainingPercent": 90.0, "expires": null}
            ],
            "usage": {
                "modelsCalled": 4,
                "successfulCalls": 1234,
                "usages": [
                    {"key": "total_token", "value": 987654, "unit": "tokens", "label": "Total Tokens"},
                    {"key": "input_token", "value": 800000, "unit": "tokens", "label": "Input Tokens"},
                    {"key": "output_token", "value": 187654, "unit": "tokens", "label": "Output Tokens"},
                    {"key": "image_number", "value": 3, "unit": "images", "label": "Images"}
                ]
            }
        })
    }

    fn metric<'a>(data: &'a Data, label: &str) -> &'a Metric {
        data.metrics
            .iter()
            .find(|metric| metric.label == label)
            .unwrap_or_else(|| panic!("no metric {label}"))
    }

    #[test]
    fn full_summary_shows_the_urgent_models_the_count_and_the_period_totals() {
        let data = summary_data(full_payload(), "cuotai-demo", true).unwrap();
        let labels: Vec<_> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "qwen-max",
                "qwen-plus",
                "wan2.7",
                "Modelos com cota gratuita",
                "Chamadas bem-sucedidas",
                "Tokens totais",
                "Tokens de entrada",
                "Tokens de saída",
            ]
        );
        let max = metric(&data, "qwen-max");
        assert_eq!(max.remaining, Some(5.0));
        assert_eq!(max.used, Some(95.0));
        assert_eq!(max.limit, Some(100.0));
        assert_eq!(max.unit, "%");
        assert_eq!(max.kind, MetricKind::Quota);
        assert_eq!(max.scope.as_ref().map(|s| s.id.as_str()), Some("qwen-max"));
        assert_eq!(
            max.resets_at,
            Some(
                NaiveDate::from_ymd_opt(2026, 12, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            )
        );
        assert_eq!(metric(&data, "wan2.7").resets_at, None);
        assert_eq!(metric(&data, "Modelos com cota gratuita").used, Some(3.0));
        let calls = metric(&data, "Chamadas bem-sucedidas");
        assert_eq!(calls.used, Some(1234.0));
        assert_eq!(calls.period.as_deref(), Some("últimos 7 dias"));
        assert_eq!(calls.kind, MetricKind::Usage);
        assert_eq!(metric(&data, "Tokens totais").used, Some(987654.0));
        assert!(data
            .notes
            .iter()
            .any(|note| note.contains("profile 'cuotai-demo'")));
        assert!(!data.notes.iter().any(|note| note.contains("workspace ID")));
    }

    #[test]
    fn usage_null_keeps_the_free_tier_and_says_why() {
        let mut payload = full_payload();
        payload["usage"] = Value::Null;
        let skipped = summary_data(payload.clone(), "p", false).unwrap();
        assert_eq!(skipped.metrics.len(), 4);
        assert!(skipped
            .notes
            .iter()
            .any(|note| note.contains("workspace ID")));
        let empty = summary_data(payload, "p", true).unwrap();
        assert_eq!(empty.metrics.len(), 4);
        assert!(empty
            .notes
            .iter()
            .any(|note| note.contains("no usage overview for the workspace in the last 7 days")));
    }

    #[test]
    fn empty_free_tier_reports_zero_models_and_a_note() {
        let payload = json!({"period": {"days": 7}, "freeTier": [], "usage": null});
        let data = summary_data(payload, "p", false).unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].used, Some(0.0));
        assert!(data
            .notes
            .iter()
            .any(|note| note.contains("no active free-tier quota")));
    }

    #[test]
    fn many_models_show_only_the_most_urgent_ones() {
        let entries: Vec<Value> = (0..185)
            .rev()
            .map(|i| {
                json!({"model": format!("model-{i:03}"), "remaining": i, "total": 200,
                       "remainingPercent": null, "expires": "2027-01-01"})
            })
            .collect();
        let payload = json!({"freeTier": entries, "usage": null});
        let data = summary_data(payload, "p", false).unwrap();
        let labels: Vec<_> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "model-000",
                "model-001",
                "model-002",
                "Modelos com cota gratuita"
            ]
        );
        assert_eq!(data.metrics[0].remaining, Some(0.0));
        assert_eq!(data.metrics[3].used, Some(185.0));
        assert!(data
            .notes
            .iter()
            .any(|note| note.contains("Showing the 3 models") && note.contains("out of 185")));
    }

    #[test]
    fn missing_values_stay_unknown_and_wrong_types_are_schema_errors() {
        let payload = json!({
            "freeTier": [
                {"model": "no-numbers"},
                {"model": "percent-only", "remainingPercent": 12.5},
                {"model": "qwen-max", "remaining": 10, "total": 100}
            ],
            "usage": {"usages": [{"key": "total_token", "value": null}]}
        });
        let data = summary_data(payload, "p", true).unwrap();
        let labels: Vec<_> = data.metrics.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "qwen-max",
                "percent-only",
                "no-numbers",
                "Modelos com cota gratuita"
            ]
        );
        let unknown = metric(&data, "no-numbers");
        assert_eq!(
            (unknown.used, unknown.limit, unknown.remaining),
            (None, None, None)
        );
        assert_eq!(metric(&data, "percent-only").remaining, Some(12.5));
        // No successfulCalls and a null token value: no usage metric at all.
        assert!(!data
            .metrics
            .iter()
            .any(|m| m.kind == MetricKind::Usage && m.label != "Modelos com cota gratuita"));

        for broken in [
            json!({"usage": null}),
            json!({"freeTier": {}, "usage": null}),
            json!({"freeTier": [{"remaining": 1, "total": 2}]}),
            json!({"freeTier": [{"model": "m", "remaining": "SECRET-VALUE", "total": 2}]}),
            json!({"freeTier": [{"model": "m", "expires": "soon"}]}),
            json!({"freeTier": [], "usage": {"successfulCalls": "many"}}),
            json!({"freeTier": [], "usage": []}),
        ] {
            let err = summary_data(broken.clone(), "p", true).unwrap_err();
            assert_eq!(err.status, Status::Error, "{broken}");
            assert_eq!(err.note.id, crate::providers::SCHEMA_NOTE, "{broken}");
            assert!(!err.note.fallback.contains("SECRET-VALUE"));
        }
    }

    #[tokio::test]
    async fn the_query_checks_region_profile_and_workspace_before_running() {
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &NoCredentials,
            timeout: Duration::from_secs(5),
        };
        let missing = Some(std::path::Path::new("/nonexistent/cuotai-test/bl"));
        let cn = account(&[("region", "cn"), ("cli_profile", "p")]);
        let err = query_with(&cn, &ctx, "p", missing).await.unwrap_err();
        assert_eq!(err.status, Status::Unsupported);
        let intl = account(&[("cli_profile", "p")]);
        let err = query_with(&intl, &ctx, "-rf", missing).await.unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
        for bad in ["--help", "with space", "a;b", &"x".repeat(65)] {
            let acc = account(&[("cli_profile", "p"), ("workspace_id", bad)]);
            let err = query_with(&acc, &ctx, "p", missing).await.unwrap_err();
            assert_eq!(
                err.note.id,
                "query.alibaba_plans.model_studio_invalid_workspace"
            );
        }
        let err = query_with(&intl, &ctx, "p", missing).await.unwrap_err();
        assert_eq!(err.note.id, "query.alibaba_plans.cli_not_found");
        assert_eq!(err.status, Status::NeedsConfiguration);
    }

    #[test]
    fn the_profile_comes_from_the_account() {
        assert_eq!(
            profile(&account(&[("cli_profile", " cuotai-1 ")])),
            Some("cuotai-1")
        );
        assert_eq!(profile(&account(&[("cli_profile", "  ")])), None);
        assert_eq!(profile(&account(&[])), None);
    }

    /// A fake `bl` that records its argv and environment, prints `stdout`
    /// and exits with `code` (after `sleep` seconds).
    #[cfg(unix)]
    struct FakeCli {
        dir: tempfile::TempDir,
    }

    #[cfg(unix)]
    impl FakeCli {
        fn new(stdout: &str, stderr: &str, code: i32, sleep: u32) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("out.json"), stdout).unwrap();
            std::fs::write(dir.path().join("err.txt"), stderr).unwrap();
            let bin = dir.path().join("bl");
            std::fs::write(
                &bin,
                format!(
                    "#!/bin/sh\n\
                     d=$(dirname \"$0\")\n\
                     printf '%s\\n' \"$@\" > \"$d/argv.txt\"\n\
                     printf '%s' \"${{BAILIAN_WORKSPACE_ID-unset}}\" > \"$d/env.txt\"\n\
                     sleep {sleep}\n\
                     cat \"$d/out.json\"\n\
                     cat \"$d/err.txt\" >&2\n\
                     exit {code}\n"
                ),
            )
            .unwrap();
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
            Self { dir }
        }

        fn path(&self) -> String {
            self.dir.path().join("bl").to_string_lossy().into_owned()
        }

        fn argv(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.path().join("argv.txt"))
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect()
        }

        fn env(&self) -> String {
            std::fs::read_to_string(self.dir.path().join("env.txt")).unwrap()
        }

        async fn query(&self, account: &Account, timeout: Duration) -> Result<Data, QueryError> {
            let http = reqwest::Client::new();
            let ctx = Context {
                http: &http,
                credentials: &NoCredentials,
                timeout,
            };
            query_with(
                account,
                &ctx,
                "cuotai-demo",
                Some(std::path::Path::new(&self.path())),
            )
            .await
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_fake_cli_gets_a_fixed_argv_and_its_summary_becomes_metrics() {
        let fake = FakeCli::new(&full_payload().to_string(), "", 0, 0);
        let acc = account(&[
            ("cli_profile", "cuotai-demo"),
            ("workspace_id", "llm-abc_123"),
        ]);
        let data = fake.query(&acc, Duration::from_secs(5)).await.unwrap();
        assert_eq!(data.metrics.len(), 8);
        assert_eq!(data.source, MODEL_STUDIO_FREE_QUOTA_INTL);
        assert_eq!(
            fake.argv(),
            [
                "usage",
                "summary",
                "--days",
                "7",
                "--console-site",
                "international",
                "--console-region",
                "ap-southeast-1",
                "--config",
                "cuotai-demo",
                "--workspace-id",
                "llm-abc_123",
                "--output",
                "json",
                "--quiet",
            ]
        );
        // The shell's workspace never reaches the CLI: only the account's.
        assert_eq!(fake.env(), "unset");

        let without = account(&[("cli_profile", "cuotai-demo")]);
        let mut payload = full_payload();
        payload["usage"] = Value::Null;
        let fake = FakeCli::new(&payload.to_string(), "", 0, 0);
        let data = fake.query(&without, Duration::from_secs(5)).await.unwrap();
        assert_eq!(data.metrics.len(), 4);
        assert!(!fake.argv().iter().any(|arg| arg == "--workspace-id"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cli_failures_are_classified_without_leaking_output() {
        let acc = account(&[("cli_profile", "cuotai-demo")]);
        let auth_json = r#"{"error":{"code":3,"message":"No console access token LEAK-1"}}"#;
        let cases: [(&str, &str, i32, Status, &str); 6] = [
            (
                auth_json,
                "",
                3,
                Status::AuthenticationError,
                "model_studio_cli_auth",
            ),
            (
                "",
                "Error: please login first LEAK-2",
                1,
                Status::AuthenticationError,
                "model_studio_cli_auth",
            ),
            (
                r#"{"error":{"code":5,"message":"Request timed out. LEAK-3"}}"#,
                "",
                5,
                Status::Error,
                "model_studio_cli_timeout",
            ),
            ("", "socket hang up LEAK-4", 6, Status::Error, "transport"),
            (
                "",
                "boom LEAK-5",
                1,
                Status::Error,
                "model_studio_cli_failure",
            ),
            ("not json LEAK-6", "", 0, Status::Error, "schema"),
        ];
        for (stdout, stderr, code, status, id) in cases {
            let fake = FakeCli::new(stdout, stderr, code, 0);
            let err = fake.query(&acc, Duration::from_secs(5)).await.unwrap_err();
            assert_eq!(err.status, status, "{stdout} {stderr}");
            assert!(
                err.note.id.contains(id),
                "{} for {stdout} {stderr}",
                err.note.id
            );
            let rendered = format!("{} {:?}", err.note.fallback, err.note.params);
            assert!(!rendered.contains("LEAK"), "{rendered}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_slow_cli_is_killed_at_the_account_deadline() {
        let acc = account(&[("cli_profile", "cuotai-demo")]);
        let fake = FakeCli::new(&full_payload().to_string(), "", 0, 5);
        let started = std::time::Instant::now();
        let err = fake
            .query(&acc, Duration::from_millis(300))
            .await
            .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(err.note.id, "query.alibaba_plans.model_studio_cli_timeout");
    }
}
