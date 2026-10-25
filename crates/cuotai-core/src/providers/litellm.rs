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
//! Nothing else is requested: no `/v1` inference route, no key or team
//! management route. No LiteLLM code is copied; the fixtures below are
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
            "Endereço base do LiteLLM inválido: informe http(s)://host[:porta][/prefixo].",
        ),
        BaseUrlError::Forbidden => (
            "query.litellm.base_url_forbidden",
            "O endereço base do LiteLLM não pode conter usuário, senha, consulta ou fragmento.",
        ),
        BaseUrlError::Insecure => (
            "query.litellm.base_url_insecure",
            "Fora desta máquina o LiteLLM exige HTTPS (HTTP só em rede privada, com a opção explícita). Nada foi enviado.",
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
        format!("{} provedores com orçamento no LiteLLM.", sub_reports.len()),
    );
    if sub_reports.is_empty() {
        notes.push(
            "note.litellm.no_budgets",
            "O LiteLLM não tem orçamentos por provedor (provider_budget_config); nada para mostrar.",
        );
    }
    let mut data = Data::new(Vec::new(), notes, "LiteLLM: provider/budgets");
    data.sub_reports = sub_reports;
    Ok(data)
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
}
