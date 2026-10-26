//! Single-key AI providers: Cloudflare Workers AI, Ollama Cloud and Gemini AI.
//!
//! Each adapter is transparently registered as unsupported when
//! the provider's public API does not expose quota/usage reading for the key
//! sent. No header is inferred from calls to `models/generate`.
//!
//! Official sources consulted (consulted on 2026-09-28):
//! - Cloudflare:
//!   - [Workers AI docs index](https://developers.cloudflare.com/workers-ai/llms.txt)
//!     — lists no per-key public analytics/usage page; Neuron
//!     accounting is shown only in the dashboard.
//!   - [Pricing](https://developers.cloudflare.com/workers-ai/platform/pricing/)
//!     — "monitor your Neuron usage in the Cloudflare Workers AI dashboard".
//!   - [Workers AI via REST API](https://developers.cloudflare.com/api/resources/ai/methods/run/)
//!     — the only public route of `/accounts/{id}/ai/run/{model}`.
//!   - [AI Gateway Analytics](https://developers.cloudflare.com/ai-gateway/observability/analytics/)
//!     — Cloudflare's only public GraphQL dataset for AI consumption
//!     (`aiGatewayRequestsAdaptiveGroups`); it belongs to AI Gateway, a product
//!     distinct from Workers AI.
//!
//!   The GraphQL Analytics dataset `workersInvocationsAdaptive` aggregates
//!   invocations of any Worker (`requests`, `errors`, `cpuTime`); it is not a
//!   Workers AI metric nor does it expose Neurons per call, so it does not serve
//!   as a Workers AI quota basis.
//! - Gemini AI: [Gemini API rate limits](https://ai.google.dev/gemini-api/docs/rate-limits)
//!   and [All methods](https://ai.google.dev/api/all-methods). The AI Studio
//!   API key (`AIza...`) has no public usage/remaining-quota endpoint; the
//!   [AI Studio](https://aistudio.google.com/rate-limit) dashboard is the only
//!   view. The equivalent programmatic path is
//!   [Cloud Monitoring timeSeries.list](https://cloud.google.com/monitoring/api/ref_v3/rest/v3/projects.timeSeries/list)
//!   via OAuth, served by the `Gemini (Cloud Monitoring)` adapter.

use super::{json, paginate, secret, Context, Data, QueryError};
use crate::models::ModelInfo;
use crate::text::{params, ParamValue};
use crate::{Account, Provider};

/// `models.list` of the Gemini API (<https://ai.google.dev/api/models>):
/// AI Studio key in `x-goog-api-key`, paginated by `nextPageToken`.
pub(crate) const GEMINI_MODELS_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models";

/// Gemini AI catalog with the AI Studio key (never the CLI/OAuth session).
/// `name` (`models/...`) is the resource id the API returns, kept verbatim.
pub(crate) async fn gemini_models(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Page {
        // Google omits empty repeated fields.
        #[serde(default)]
        models: Vec<Entry>,
        next_page_token: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        name: String,
        display_name: Option<String>,
        description: Option<String>,
    }
    let key = secret(account, ctx, "GEMINI_API_KEY")?;
    paginate(|token: Option<String>| {
        let mut query = vec![("pageSize", "1000".to_string())];
        query.extend(token.map(|t| ("pageToken", t)));
        let request = ctx
            .http
            .get(url)
            .header("x-goog-api-key", key.trim())
            .query(&query)
            .timeout(ctx.timeout);
        async move {
            let page: Page = json(request).await?;
            let models = page.models.into_iter().map(|m| ModelInfo {
                id: m.name,
                name: m.display_name,
                description: m.description,
            });
            Ok((models.collect(), page.next_page_token))
        }
    })
    .await
}

pub(crate) async fn fetch(account: &Account, _ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::CloudflareAi => Err(cloudflare_unsupported()),
        Provider::GeminiAi => Err(gemini_unsupported()),
        other => Err(QueryError::unsupported_params(
            "query.cloud_keys.other_provider",
            params([(
                "provider",
                ParamValue::from(crate::text::provider_l10n(other)),
            )]),
            "{provider} is handled by another adapter.",
        )),
    }
}

fn cloudflare_unsupported() -> QueryError {
    let source = "https://developers.cloudflare.com/workers-ai/platform/pricing + https://developers.cloudflare.com/workers-ai/llms.txt + https://developers.cloudflare.com/ai-gateway/observability/analytics/";
    QueryError::unsupported_params(
        "query.cloudflare.unsupported",
        params([("source", ParamValue::from(source))]),
        "Cloudflare Workers AI does not expose a public per-API-key usage/quota read route. Neuron accounting is only shown in the dashboard (https://dash.cloudflare.com/?to=/:account/ai/workers-ai). The public GraphQL dataset `workersInvocationsAdaptive` aggregates generic Worker invocations (requests, errors, cpuTime) and exposes neither Neurons nor per-model cost, so it cannot be presented as Workers AI consumption. For AI metrics routed through AI Gateway, use a dedicated adapter; this provider covers direct Workers AI only. Source: {source}.",
    )
}

fn gemini_unsupported() -> QueryError {
    let source = "https://ai.google.dev/gemini-api/docs/rate-limits + https://aistudio.google.com/rate-limit + https://cloud.google.com/monitoring/api/ref_v3/rest/v3/projects.timeSeries.list";
    QueryError::unsupported_params(
        "query.gemini_ai.unsupported",
        params([("source", ParamValue::from(source))]),
        "Gemini AI (AI Studio API key, `generativelanguage.googleapis.com`) does not expose a public usage or residual quota endpoint. Values are only shown in the AI Studio dashboard (https://aistudio.google.com/rate-limit). For programmatic query, register the same account via OAuth in 'Gemini (Cloud Monitoring)' (monitoring.timeSeries.list over `serviceruntime.googleapis.com/quota/limit` and `quota/rate/net_usage`). Source: {source}.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Status;
    use wiremock::matchers::{header, method, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Keys;
    impl crate::Credentials for Keys {
        fn get(&self, name: &str) -> Option<String> {
            name.strip_prefix("GEMINI_KEY_").map(|a| format!("AIza{a}"))
        }
    }

    async fn list(server: &MockServer, account: &str) -> Result<Vec<String>, QueryError> {
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Keys,
            timeout: std::time::Duration::from_secs(5),
        };
        let account = Account {
            id: None,
            name: account.into(),
            provider: Provider::GeminiAi,
            credential_env: Some(format!("GEMINI_KEY_{account}")),
            ..Default::default()
        };
        let models = gemini_models(&account, &ctx, &server.uri()).await?;
        Ok(models.into_iter().map(|m| m.id).collect())
    }

    fn page(names: &[&str], next: Option<&str>) -> ResponseTemplate {
        let models: Vec<_> = names
            .iter()
            .map(|n| serde_json::json!({ "name": n, "displayName": n.to_uppercase() }))
            .collect();
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({ "models": models, "nextPageToken": next }))
    }

    async fn reply(server: &MockServer, key: &str, token: Option<&str>, body: ResponseTemplate) {
        let mock = Mock::given(method("GET")).and(header("x-goog-api-key", key));
        let mock = match token {
            Some(token) => mock.and(query_param("pageToken", token)),
            None => mock.and(query_param_is_missing("pageToken")),
        };
        mock.respond_with(body).mount(server).await;
    }

    #[tokio::test]
    async fn follows_page_tokens_per_key_and_rejects_bad_pages() {
        let server = MockServer::start().await;
        reply(
            &server,
            "AIzaA",
            None,
            page(&["models/b", "models/a"], Some("t2")),
        )
        .await;
        reply(&server, "AIzaA", Some("t2"), page(&["models/c"], None)).await;
        reply(&server, "AIzaB", None, page(&["models/only-b"], Some(""))).await;
        assert_eq!(
            list(&server, "A").await.unwrap(),
            ["models/b", "models/a", "models/c"]
        );
        assert_eq!(
            list(&server, "B").await.unwrap(),
            ["models/only-b"],
            "another key gets its own catalog; an empty token ends the list"
        );

        server.reset().await;
        reply(&server, "AIzaA", None, page(&["models/a"], Some("t2"))).await;
        reply(
            &server,
            "AIzaA",
            Some("t2"),
            page(&["models/b"], Some("t2")),
        )
        .await;
        let repeated = list(&server, "A").await.unwrap_err();
        assert_eq!(repeated.status, Status::Error, "repeated token");

        server.reset().await;
        reply(&server, "AIzaA", None, page(&["models/a"], Some("t2"))).await;
        let incomplete = ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({ "models": [{ "displayName": "No name" }] }));
        reply(&server, "AIzaA", Some("t2"), incomplete).await;
        assert_eq!(list(&server, "A").await.unwrap_err().status, Status::Error);

        for (code, status) in [
            (401, Status::AuthenticationError),
            (403, Status::AuthenticationError),
            (429, Status::RateLimited),
        ] {
            server.reset().await;
            let body = ResponseTemplate::new(code).set_body_string("AIzaA leaked");
            reply(&server, "AIzaA", None, body).await;
            let error = list(&server, "A").await.unwrap_err();
            assert_eq!(error.status, status, "HTTP {code}");
            assert!(!error.note.fallback.contains("AIza"), "no body or key echo");
        }
    }
}
