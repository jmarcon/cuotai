//! PRD 0036 RF10.1: read-only provider budgets of a user's own LiteLLM
//! proxy, an aggregator whose sub-accounts are the upstream providers.
//!
//! Contract (public documentation, read 2026-10-25:
//! <https://docs.litellm.ai/docs/proxy/provider_budget_routing>, section
//! "Monitoring Provider Remaining Budget"):
//!
//! - `GET {base}/provider/budgets` with `Authorization: Bearer <key>` (the
//!   proxy's master key or a virtual key);
//! - the body is `{ "providers": { "<provider>": { "budget_limit": number,
//!   "time_period": "1d" | "10d" | …, "spend": number, "budget_reset_at":
//!   string | null } } }`, one entry per provider listed in the proxy's
//!   `provider_budget_config`.
//!
//! Nothing else is requested by default: no `/v1` inference route. With
//! `breakdown = ["keys"]` or `["teams"]` (PRD 0045 RF3) the read-only
//! `GET /key/list` (paged) and `GET /team/list` are read too
//! (<https://docs.litellm.ai/docs/proxy/virtual_keys>,
//! <https://docs.litellm.ai/docs/proxy/team_budgets>). No LiteLLM code is copied; the fixtures below are
//! written from the documented example. A live instance has not been
//! checked yet (owner verification), so the access level is `Official` (a documented route).

use super::nine_router::{parse_base, sub_id, BaseUrlError};
use super::{json, secret, Context, Data, QueryError};
use crate::text::{params, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider, Status, SubReport};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// LiteLLM's documented default listen address.
pub(crate) const DEFAULT_BASE_URL: &str = "http://127.0.0.1:4000";
const BUDGETS_PATH: &str = "/provider/budgets";
const VIA: &str = "docs.litellm.ai/docs/proxy/provider_budget_routing";
/// Cap on the providers read from one answer.
const MAX_PROVIDERS: usize = 200;

/// The validated base URL of `account` (`params.base_url`, default
/// [`DEFAULT_BASE_URL`]), with the same host rules as 9router.
pub(crate) fn account_base(account: &Account) -> Result<url::Url, BaseUrlError> {
    let raw = account
        .params
        .get("base_url")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_BASE_URL);
    let allow = account
        .params
        .get("allow_private_http")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"));
    parse_base(raw, allow)
}

/// LiteLLM provider name → Cuotai provider; `None` for the others.
pub(crate) fn map_upstream(name: &str) -> Option<Provider> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "openai" => Provider::OpenAiApi,
        "anthropic" => Provider::AnthropicApi,
        "azure" => Provider::AzureOpenAi,
        "vertex_ai" => Provider::VertexAi,
        "gemini" => Provider::GeminiAi,
        "bedrock" => Provider::AwsBedrock,
        "groq" => Provider::Groq,
        "mistral" => Provider::Mistral,
        "perplexity" => Provider::Perplexity,
        "xai" => Provider::Grok,
        _ => return None,
    })
}

/// `time_period` (`"1d"`, `"7d"`, `"1mo"`, `"30d"`…) as a window.
fn window_of(period: &str) -> MetricWindow {
    match period.trim().to_ascii_lowercase().as_str() {
        "1d" | "24h" => MetricWindow::Daily,
        "7d" | "1w" => MetricWindow::Weekly,
        "1mo" | "30d" => MetricWindow::Monthly,
        _ => MetricWindow::Other,
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn clean(raw: &str) -> String {
    raw.chars()
        .filter(|ch| !ch.is_control())
        .take(60)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The sub-accounts of a `/provider/budgets` answer, in name order.
fn parse_budgets(value: &Value, now: DateTime<Utc>) -> Result<Vec<SubReport>, QueryError> {
    let providers = value
        .get("providers")
        .and_then(Value::as_object)
        .ok_or_else(QueryError::schema)?;
    let mut subs = Vec::new();
    for (name, entry) in providers.iter().take(MAX_PROVIDERS) {
        let name = clean(name);
        if name.is_empty() {
            continue;
        }
        let entry = entry.as_object().ok_or_else(QueryError::schema)?;
        let limit = number(entry.get("budget_limit")) // The docs use 1e-12 to block a provider: a cap that small (or zero)
            // is shown as "no cap", never as a full meter.
            .filter(|limit| *limit >= 0.01);
        let spend = number(entry.get("spend"));
        let period = entry
            .get("time_period")
            .and_then(Value::as_str)
            .map(clean)
            .filter(|period| !period.is_empty());
        let resets_at = entry
            .get("budget_reset_at")
            .and_then(Value::as_str)
            .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
            .map(|at| at.with_timezone(&Utc));
        let mut metrics = Vec::new();
        // Absent spend is unknown, never zero.
        if spend.is_some() || limit.is_some() {
            metrics.push(Metric {
                billing: Default::default(),
                scope: None,
                label: period.as_deref().map_or_else(
                    || "Orçamento do provedor".to_string(),
                    |period| format!("Orçamento do provedor ({period})"),
                ),
                used: spend,
                limit,
                remaining: match (limit, spend) {
                    (Some(limit), Some(spend)) => Some((limit - spend).max(0.0)),
                    _ => None,
                },
                unit: "USD".into(),
                kind: MetricKind::Spend,
                window: period.as_deref().map(window_of),
                period: None,
                resets_at,
                derived_from: None,
            });
        }
        subs.push(SubReport {
            id: sub_id(&name),
            upstream: name.clone(),
            provider: map_upstream(&name),
            label: name,
            status: Status::Ready,
            metrics,
            notes_l10n: Vec::new(),
            checked_at: now,
            as_of: None,
            enabled_upstream: None,
            duplicate_of: None,
            plan: None,
            via: Some(VIA.to_string()),
            identity: None,
        });
    }
    Ok(subs)
}

fn base_error(error: BaseUrlError) -> QueryError {
    let (id, text) = match error {
        BaseUrlError::Malformed => (
            "query.litellm.base_url",
            "Invalid LiteLLM base address: enter http(s)://host[:port][/prefix].",
        ),
        BaseUrlError::Forbidden => (
            "query.litellm.base_url_forbidden",
            "The LiteLLM base address cannot contain a user, password, query or fragment.",
        ),
        BaseUrlError::Insecure => (
            "query.litellm.base_url_insecure",
            "Outside this machine LiteLLM requires HTTPS (HTTP only on a private network, with the explicit option). Nothing was sent.",
        ),
    };
    QueryError::config(id, text)
}

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    fetch_at(account, ctx, Utc::now()).await
}

pub(crate) async fn fetch_at(
    account: &Account,
    ctx: &Context<'_>,
    now: DateTime<Utc>,
) -> Result<Data, QueryError> {
    let base = account_base(account).map_err(base_error)?;
    let key = secret(account, ctx, "LITELLM_API_KEY")?;
    let url = format!("{}{BUDGETS_PATH}", base.as_str().trim_end_matches('/'));
    let value: Value = json(
        ctx.http
            .get(&url)
            .bearer_auth(key.trim())
            .header("Accept", "application/json")
            .timeout(ctx.timeout),
    )
    .await?;
    let hidden: Vec<String> = account
        .params
        .get("hidden")
        .map(|raw| {
            raw.split(',')
                .map(|id| id.trim().to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let sub_reports: Vec<SubReport> = parse_budgets(&value, now)?
        .into_iter()
        .filter(|sub| !hidden.contains(&sub.id))
        .collect();
    let mut notes = Notes::new();
    notes.push_params(
        "note.litellm.summary",
        params([("total", ParamValue::from(sub_reports.len() as u64))]),
        "{total} providers with a budget in LiteLLM.",
    );
    if sub_reports.is_empty() {
        notes.push(
            "note.litellm.no_budgets",
            "LiteLLM has no per-provider budgets (provider_budget_config); nothing to show.",
        );
    }
    // PRD 0045 RF3: virtual keys and teams, opt-in; a failure never fails
    // the account.
    let root = base.as_str().trim_end_matches('/');
    let mut rows = Vec::new();
    for kind in [crate::BreakdownKind::Keys, crate::BreakdownKind::Teams] {
        if !account.breakdown.contains(&kind) {
            continue;
        }
        let fetched = match kind {
            crate::BreakdownKind::Keys => key_rows(ctx, root, key.trim()).await,
            _ => team_rows(ctx, root, key.trim()).await,
        };
        match fetched {
            Ok((found, more)) => {
                rows.extend(found);
                if more {
                    notes.push_note(crate::breakdown::paged_note());
                }
            }
            Err(error) => notes.push_note(crate::breakdown::unavailable_note(&error)),
        }
    }
    let mut data = Data::new(Vec::new(), notes, "LiteLLM: provider/budgets");
    data.sub_reports = sub_reports;
    data.breakdown = crate::breakdown::finish(rows);
    Ok(data)
}

// ------------------------------------------------- PRD 0045 RF3: breakdown

const KEYS_PATH: &str = "/key/list";
const TEAMS_PATH: &str = "/team/list";
/// The largest page `/key/list` documents.
const KEY_PAGE_SIZE: usize = 100;

/// `GET /key/list?return_full_object=true&page=N&size=100`, at most
/// [`crate::breakdown::MAX_PAGES`] pages; whether pages were left unread.
async fn key_rows(
    ctx: &Context<'_>,
    root: &str,
    key: &str,
) -> Result<(Vec<crate::BreakdownRow>, bool), QueryError> {
    let mut rows = Vec::new();
    for page in 1..=crate::breakdown::MAX_PAGES {
        let url =
            format!("{root}{KEYS_PATH}?return_full_object=true&page={page}&size={KEY_PAGE_SIZE}");
        let value: Value = json(
            ctx.http
                .get(&url)
                .bearer_auth(key)
                .header("Accept", "application/json")
                .timeout(ctx.timeout),
        )
        .await?;
        let keys = value
            .get("keys")
            .and_then(Value::as_array)
            .ok_or_else(QueryError::schema)?;
        for item in keys {
            // A key listed as a bare token string carries no figure.
            if let Some(item) = item.as_object() {
                rows.extend(budget_row(item, crate::ScopeKind::VirtualKey)?);
            }
        }
        let total_pages = value.get("total_pages").and_then(Value::as_u64);
        let last = keys.is_empty()
            || total_pages.is_none_or(|total| page as u64 >= total)
            || keys.len() < KEY_PAGE_SIZE;
        if last {
            return Ok((rows, false));
        }
    }
    Ok((rows, true))
}

/// `GET /team/list`: one answer with every team the credential may read.
async fn team_rows(
    ctx: &Context<'_>,
    root: &str,
    key: &str,
) -> Result<(Vec<crate::BreakdownRow>, bool), QueryError> {
    let value: Value = json(
        ctx.http
            .get(format!("{root}{TEAMS_PATH}"))
            .bearer_auth(key)
            .header("Accept", "application/json")
            .timeout(ctx.timeout),
    )
    .await?;
    let teams = value.as_array().ok_or_else(QueryError::schema)?;
    let mut rows = Vec::new();
    for item in teams {
        let item = item.as_object().ok_or_else(QueryError::schema)?;
        rows.extend(budget_row(item, crate::ScopeKind::Team)?);
    }
    Ok((rows, false))
}

/// One virtual key or team: a `Quota` (`spend` of `max_budget`, reset at
/// `budget_reset_at`) when it has a budget, its `Spend` otherwise. The
/// label is the alias (redacted), else the last four characters of the
/// masked key name or of the id. Only `token`/`team_id`, the alias, the
/// masked key name, the figures and `blocked` are read: never `user_id`,
/// members or e-mails.
fn budget_row(
    item: &serde_json::Map<String, Value>,
    scope: crate::ScopeKind,
) -> Result<Option<crate::BreakdownRow>, QueryError> {
    use crate::breakdown::{digest_id, fallback_label, opaque_id, redact_label};
    let team = scope == crate::ScopeKind::Team;
    let (id_field, alias_field) = if team {
        ("team_id", "team_alias")
    } else {
        ("token", "key_alias")
    };
    let Some(raw_id) = item
        .get(id_field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return Ok(None);
    };
    // A key's token is the hash of its secret: only a digest leaves.
    let id = if team {
        opaque_id(raw_id)
    } else {
        digest_id(raw_id)
    };
    let alias = item
        .get(alias_field)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let label = if alias.is_empty() {
        let masked = item
            .get("key_name")
            .and_then(Value::as_str)
            .filter(|name| !team && !name.trim().is_empty())
            .unwrap_or(raw_id);
        fallback_label(scope, masked)
    } else {
        redact_label(alias, scope, &id)
    };
    let spend = number(item.get("spend"));
    let budget = number(item.get("max_budget")).filter(|limit| *limit > 0.0);
    let resets_at = item
        .get("budget_reset_at")
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|at| at.with_timezone(&Utc));
    let window = item
        .get("budget_duration")
        .and_then(Value::as_str)
        .map(window_of);
    let metric = match (budget, spend) {
        (Some(limit), spend) => Metric {
            label: if team {
                "Orçamento da equipe"
            } else {
                "Orçamento da chave"
            }
            .into(),
            used: spend,
            limit: Some(limit),
            remaining: spend.map(|spend| limit - spend),
            unit: "USD".into(),
            kind: MetricKind::Quota,
            window,
            period: None,
            resets_at,
            billing: Default::default(),
            scope: None,
            derived_from: None,
        },
        (None, Some(spend)) => Metric::usage("Gasto total", spend, "USD", MetricKind::Spend, None),
        (None, None) => return Ok(None),
    };
    let mut notes = Vec::new();
    if item.get("blocked") == Some(&Value::Bool(true)) {
        notes.push(crate::breakdown::disabled_note());
    }
    Ok(Some(crate::BreakdownRow {
        scope,
        id,
        label,
        metrics: vec![metric],
        truncated: false,
        notes,
    }))
}

/// PRD 0036 RF10.3 with RF2.2: Test connection of a LiteLLM proxy (the
/// budgets route, read-only), in the shared outcome vocabulary.
pub(crate) async fn test_connection(
    account: &Account,
    ctx: &Context<'_>,
) -> super::nine_router::TestOutcome {
    use super::nine_router::TestOutcome;
    match fetch(account, ctx).await {
        Ok(data) => TestOutcome::Connected {
            version: None,
            connections: data.sub_reports.len(),
        },
        Err(error) => match error.status {
            Status::AuthenticationError => TestOutcome::PasswordWrong,
            Status::NeedsConfiguration => TestOutcome::PasswordRequired,
            Status::Error if error.note.id == QueryError::schema().note.id => {
                TestOutcome::NotNineRouter
            }
            _ => TestOutcome::Unreachable,
        },
    }
}

/// PRD 0028 RF11: the fixture whose metric identities `label_snapshot`
/// records (the documented `/provider/budgets` example).
#[cfg(test)]
pub(super) fn label_fixtures() -> Vec<(&'static str, Vec<Metric>)> {
    let body = serde_json::json!({"providers": {
        "openai": {"budget_limit": 100.0, "time_period": "1d", "spend": 2.5,
                   "budget_reset_at": "2026-10-02T00:00:00Z"},
        "anthropic": {"budget_limit": 50.0, "time_period": "30d", "spend": 1.0,
                      "budget_reset_at": null}
    }});
    let subs = parse_budgets(&body, Utc::now()).expect("fixture decodes");
    vec![(
        "litellm",
        subs.into_iter().flat_map(|sub| sub.metrics).collect(),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Keys;
    impl Credentials for Keys {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_LITELLM_KEY").then(|| "sk-litellm-test".into())
        }
    }

    fn account(server: &MockServer, extra: &[(&str, &str)]) -> Account {
        let mut params: BTreeMap<String, String> = [("base_url".to_string(), server.uri())]
            .into_iter()
            .collect();
        for (key, value) in extra {
            params.insert((*key).into(), (*value).into());
        }
        Account {
            id: None,
            name: "proxy".into(),
            provider: Provider::LiteLlm,
            credential_env: Some("TEST_LITELLM_KEY".into()),
            session: None,
            oauth: None,
            params,
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
            credentials: &Keys,
            timeout: Duration::from_secs(5),
        }
    }

    /// The documented example, with one reset time and one absent spend.
    fn documented() -> Value {
        json!({ "providers": {
            "openai": { "budget_limit": 1e-12, "time_period": "1d", "spend": 0.0, "budget_reset_at": null },
            "azure": { "budget_limit": 100.0, "time_period": "1d", "spend": 25.5,
                       "budget_reset_at": "2026-10-26T00:00:00Z" },
            "anthropic": { "budget_limit": 100.0, "time_period": "10d", "spend": 0.0, "budget_reset_at": null },
            "vertex_ai": { "budget_limit": 100.0, "time_period": "12d", "budget_reset_at": null },
            "fireworks_ai": { "budget_limit": 5.0, "time_period": "1mo", "spend": 1.0, "budget_reset_at": null }
        }})
    }

    async fn mount(server: &MockServer, status: u16, body: Value) {
        Mock::given(method("GET"))
            .and(path(BUDGETS_PATH))
            .and(header("authorization", "Bearer sk-litellm-test"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn provider_budgets_become_sub_accounts() {
        let server = MockServer::start().await;
        mount(&server, 200, documented()).await;
        let http = reqwest::Client::new();
        let data = fetch_at(&account(&server, &[]), &context(&http), Utc::now())
            .await
            .unwrap();
        let subs = &data.sub_reports;
        assert_eq!(subs.len(), 5);
        let by = |name: &str| subs.iter().find(|sub| sub.upstream == name).unwrap();
        let azure = &by("azure").metrics[0];
        assert_eq!(by("azure").provider, Some(Provider::AzureOpenAi));
        assert_eq!(azure.kind, MetricKind::Spend);
        assert_eq!(azure.limit, Some(100.0));
        assert_eq!(azure.remaining, Some(74.5));
        assert_eq!(azure.window, Some(MetricWindow::Daily));
        assert!(azure.resets_at.is_some());
        assert_eq!(by("anthropic").metrics[0].window, Some(MetricWindow::Other));
        // A tiny or zero limit is "no cap", never a full meter.
        assert_eq!(by("openai").metrics[0].limit, None);
        // An absent spend stays unknown.
        assert_eq!(by("vertex_ai").metrics[0].used, None);
        assert_eq!(by("vertex_ai").metrics[0].remaining, None);
        // An upstream Cuotai does not know has no provider.
        assert_eq!(by("fireworks_ai").provider, None);
        // Only the budgets route was requested.
        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|r| r.url.path() == BUDGETS_PATH));
    }

    #[tokio::test]
    async fn hidden_providers_are_left_out() {
        let server = MockServer::start().await;
        mount(&server, 200, documented()).await;
        let http = reqwest::Client::new();
        let hidden = sub_id("azure");
        let data = fetch_at(
            &account(&server, &[("hidden", hidden.as_str())]),
            &context(&http),
            Utc::now(),
        )
        .await
        .unwrap();
        assert!(data.sub_reports.iter().all(|sub| sub.upstream != "azure"));
    }

    #[tokio::test]
    async fn errors_are_classified_without_the_body() {
        let http = reqwest::Client::new();
        for (code, status) in [
            (401, Status::AuthenticationError),
            (429, Status::RateLimited),
            (500, Status::Error),
        ] {
            let server = MockServer::start().await;
            mount(&server, code, json!({"error": "sk-secret-in-body"})).await;
            let error = fetch_at(&account(&server, &[]), &context(&http), Utc::now())
                .await
                .unwrap_err();
            assert_eq!(error.status, status, "{code}");
            assert!(!error.note.fallback.contains("sk-secret"));
        }
        let server = MockServer::start().await;
        mount(&server, 200, json!({"unexpected": true})).await;
        let error = fetch_at(&account(&server, &[]), &context(&http), Utc::now())
            .await
            .unwrap_err();
        assert_eq!(error.status, Status::Error);
        assert_eq!(error.note.id, QueryError::schema().note.id);
    }

    #[tokio::test]
    async fn test_connection_counts_the_budgets_or_names_the_failure() {
        use crate::providers::nine_router::TestOutcome;
        let http = reqwest::Client::new();
        let server = MockServer::start().await;
        mount(&server, 200, documented()).await;
        assert!(matches!(
            test_connection(&account(&server, &[]), &context(&http)).await,
            TestOutcome::Connected { connections: 5, .. }
        ));
        let server = MockServer::start().await;
        mount(&server, 401, json!({})).await;
        assert_eq!(
            test_connection(&account(&server, &[]), &context(&http)).await,
            TestOutcome::PasswordWrong
        );
    }

    #[test]
    fn remote_plain_http_is_refused_before_any_request() {
        let mut acct = Account {
            params: [(
                "base_url".to_string(),
                "http://litellm.example.com".to_string(),
            )]
            .into_iter()
            .collect(),
            ..Account::default()
        };
        acct.provider = Provider::LiteLlm;
        assert!(matches!(account_base(&acct), Err(BaseUrlError::Insecure)));
        acct.params.clear();
        assert_eq!(account_base(&acct).unwrap().port(), Some(4000));
    }

    // ------------------------------------------------ PRD 0045 RF3

    async fn mount_breakdown(server: &MockServer) {
        use wiremock::matchers::query_param;
        mount(server, 200, documented()).await;
        let key = |n: usize, alias: Option<&str>, budget: Option<f64>, reset: Option<&str>| {
            json!({
                "token": format!("88b1c0ffee{n:04}hashedtoken"),
                "key_name": format!("sk-...{n:04}"),
                "key_alias": alias,
                "spend": n as f64 + 0.5,
                "max_budget": budget,
                "budget_duration": reset.map(|_| "30d"),
                "budget_reset_at": reset,
                "user_id": "person@example.com",
                "team_id": "team-1",
                "blocked": n == 2,
            })
        };
        Mock::given(method("GET"))
            .and(path(KEYS_PATH))
            .and(query_param("page", "1"))
            .and(query_param("return_full_object", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [
                    key(1, Some("ci-pipeline"), Some(10.0), Some("2026-11-01T00:00:00Z")),
                    key(2, None, Some(5.0), None),
                    key(3, Some("sk-live-0123456789abcdef"), None, None),
                    "88b1c0ffee9999hashedtoken"
                ],
                "total_count": 4, "current_page": 1, "total_pages": 1
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(TEAMS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "team_id": "team-1", "team_alias": "Platform", "spend": 42.0, "max_budget": 100.0,
                  "budget_reset_at": "2026-11-01T00:00:00Z",
                  "members_with_roles": [{ "user_id": "u-1", "user_email": "person@example.com" }] },
                { "team_id": "team-2", "team_alias": null, "spend": 3.0 }
            ])))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn virtual_keys_and_teams_become_breakdown_rows() {
        let server = MockServer::start().await;
        mount_breakdown(&server).await;
        let http = reqwest::Client::new();
        let mut acct = account(&server, &[]);
        acct.breakdown = vec![crate::BreakdownKind::Keys, crate::BreakdownKind::Teams];
        let data = fetch_at(&acct, &context(&http), Utc::now()).await.unwrap();
        // The provider budgets stand beside the breakdown.
        assert_eq!(data.sub_reports.len(), 5);
        let rows = &data.breakdown;
        let by = |label: &str| {
            rows.iter()
                .find(|row| row.label == label)
                .unwrap_or_else(|| panic!("{label}"))
        };
        let team = by("Platform");
        assert_eq!(team.scope, crate::ScopeKind::Team);
        assert_eq!(team.id, "team-1");
        let quota = team.capped().unwrap();
        assert_eq!(
            (quota.used, quota.limit, quota.remaining),
            (Some(42.0), Some(100.0), Some(58.0))
        );
        assert!(quota.resets_at.is_some());
        let ci = by("ci-pipeline");
        assert_eq!(ci.scope, crate::ScopeKind::VirtualKey);
        assert_eq!(ci.metrics[0].kind, MetricKind::Quota);
        assert_eq!(ci.metrics[0].window, Some(MetricWindow::Monthly));
        // A key without an alias shows the last characters of its masked name.
        let unnamed = by("Virtual key …0002");
        assert_eq!(unnamed.notes[0].id, "note.breakdown.disabled");
        assert_eq!(
            unnamed.metrics[0].resets_at, None,
            "no reset without budget_reset_at"
        );
        // A secret-shaped alias is cut to its last four characters.
        let secret = by("…cdef");
        assert_eq!(secret.metrics[0].kind, MetricKind::Spend);
        assert_eq!(by("Team …eam2").metrics[0].used, Some(3.0));
        assert_eq!(rows.len(), 5);
        // Never a user id, an e-mail, a member or a key hash.
        let text = serde_json::to_string(rows).unwrap();
        for leaked in ["person@", "u-1", "c0ffee", "hashedtoken", "sk-live"] {
            assert!(!text.contains(leaked), "{leaked}");
        }
    }

    #[tokio::test]
    async fn key_lists_follow_their_pages_up_to_three() {
        use wiremock::matchers::query_param;
        let server = MockServer::start().await;
        mount(&server, 200, documented()).await;
        let page = |n: usize| {
            let keys: Vec<Value> = (0..KEY_PAGE_SIZE)
                .map(|i| json!({ "token": format!("t{n}-{i}"), "key_alias": format!("k{n}-{i}"), "spend": 1.0 }))
                .collect();
            json!({ "keys": keys, "total_count": 1000, "current_page": n, "total_pages": 10 })
        };
        for n in 1..=4 {
            Mock::given(method("GET"))
                .and(path(KEYS_PATH))
                .and(query_param("page", n.to_string()))
                .respond_with(ResponseTemplate::new(200).set_body_json(page(n)))
                .mount(&server)
                .await;
        }
        let http = reqwest::Client::new();
        let mut acct = account(&server, &[]);
        acct.breakdown = vec![crate::BreakdownKind::Keys];
        let data = fetch_at(&acct, &context(&http), Utc::now()).await.unwrap();
        assert_eq!(data.breakdown.len(), crate::breakdown::MAX_ROWS);
        assert_eq!(data.breakdown.last().unwrap().label, "Others (276)");
        assert!(data
            .notes_l10n
            .iter()
            .any(|n| n.id == "note.breakdown.paged"));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.url.path() == KEYS_PATH)
                .count(),
            3
        );
    }

    #[tokio::test]
    async fn a_refused_team_list_keeps_the_budgets() {
        let server = MockServer::start().await;
        mount(&server, 200, documented()).await;
        Mock::given(method("GET"))
            .and(path(TEAMS_PATH))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let mut acct = account(&server, &[]);
        acct.breakdown = vec![crate::BreakdownKind::Teams];
        let data = fetch_at(&acct, &context(&http), Utc::now()).await.unwrap();
        assert_eq!(data.sub_reports.len(), 5);
        assert!(data.breakdown.is_empty());
        let note = data
            .notes_l10n
            .iter()
            .find(|n| n.id == "note.breakdown.unavailable")
            .unwrap();
        assert!(note.text().contains("credential"));
    }
}
