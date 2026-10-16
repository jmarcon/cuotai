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

use super::{Context, Data, QueryError};
use crate::text::{params, ParamValue};
use crate::{Account, Provider};

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
