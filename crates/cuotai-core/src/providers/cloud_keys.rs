//! Provedores de chave única de IA: Cloudflare Workers AI, Ollama Cloud e Gemini AI.
//!
//! Cada adaptador é registrado de forma transparente como não suportado quando
//! a API pública do provedor não expõe leitura de quota/uso para a chave
//! enviada. Nenhum cabeçalho é inferido a partir de chamadas a `models/generate`.
//!
//! Fontes oficiais consultadas (consultadas em 2026-09-28):
//! - Cloudflare:
//!   - [Workers AI docs index](https://developers.cloudflare.com/workers-ai/llms.txt)
//!     — não lista nenhuma página de analytics/uso por chave pública; a
//!     contabilidade de Neurons é exibida apenas no painel.
//!   - [Pricing](https://developers.cloudflare.com/workers-ai/platform/pricing/)
//!     — "monitor your Neuron usage in the Cloudflare Workers AI dashboard".
//!   - [Workers AI via REST API](https://developers.cloudflare.com/api/resources/ai/methods/run/)
//!     — única rota pública de `/accounts/{id}/ai/run/{model}`.
//!   - [AI Gateway Analytics](https://developers.cloudflare.com/ai-gateway/observability/analytics/)
//!     — único dataset GraphQL público da Cloudflare para consumo de IA
//!     (`aiGatewayRequestsAdaptiveGroups`); pertence ao AI Gateway, produto
//!     distinto de Workers AI.
//!
//!   O dataset `workersInvocationsAdaptive` do GraphQL Analytics agrega
//!   invocações de qualquer Worker (`requests`, `errors`, `cpuTime`); não é
//!   métrica de Workers AI nem expõe Neurons por chamada, portanto não serve
//!   como base de quota de Workers AI.
//! - Gemini AI: [Gemini API rate limits](https://ai.google.dev/gemini-api/docs/rate-limits)
//!   e [All methods](https://ai.google.dev/api/all-methods). A chave de API do
//!   AI Studio (`AIza...`) não tem endpoint público de uso/cota residual; o
//!   dashboard [AI Studio](https://aistudio.google.com/rate-limit) é a única
//!   visualização. O caminho programático equivalente é
//!   [Cloud Monitoring timeSeries.list](https://cloud.google.com/monitoring/api/ref_v3/rest/v3/projects.timeSeries/list)
//!   via OAuth, atendido pelo adaptador `Gemini (Cloud Monitoring)`.

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
            format!("{} é consultado por outro adaptador.", other.label()),
        )),
    }
}

fn cloudflare_unsupported() -> QueryError {
    let source = "https://developers.cloudflare.com/workers-ai/platform/pricing + https://developers.cloudflare.com/workers-ai/llms.txt + https://developers.cloudflare.com/ai-gateway/observability/analytics/";
    QueryError::unsupported_params(
        "query.cloudflare.unsupported",
        params([("source", ParamValue::from(source))]),
        format!(
            "Cloudflare Workers AI não expõe rota pública de leitura de uso/cota por chave de API. A contabilidade de Neurons é exibida apenas no painel (https://dash.cloudflare.com/?to=/:account/ai/workers-ai). O dataset GraphQL público `workersInvocationsAdaptive` agrega invocações genéricas do Worker (requests, errors, cpuTime) e não expõe Neurons nem custo por modelo, portanto não pode ser apresentado como consumo de Workers AI. Para métricas de IA roteada via AI Gateway, utilize um adaptador dedicado; este provedor refere-se apenas a Workers AI direto. Fonte: {source}."
        ),
    )
}

fn gemini_unsupported() -> QueryError {
    let source = "https://ai.google.dev/gemini-api/docs/rate-limits + https://aistudio.google.com/rate-limit + https://cloud.google.com/monitoring/api/ref_v3/rest/v3/projects.timeSeries.list";
    QueryError::unsupported_params(
        "query.gemini_ai.unsupported",
        params([("source", ParamValue::from(source))]),
        format!(
            "Gemini AI (chave de API do AI Studio, `generativelanguage.googleapis.com`) não expõe endpoint público de uso ou cota residual. Os valores são exibidos apenas no painel AI Studio (https://aistudio.google.com/rate-limit). Para consulta programática, registre a mesma conta via OAuth em 'Gemini (Cloud Monitoring)' (monitoring.timeSeries.list sobre `serviceruntime.googleapis.com/quota/limit` e `quota/rate/net_usage`). Fonte: {source}."
        ),
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
