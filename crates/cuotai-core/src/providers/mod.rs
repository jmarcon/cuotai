pub(crate) mod alibaba_billing;
pub(crate) mod alibaba_plans;
pub(crate) mod antigravity;
pub(crate) mod api;
pub(crate) mod asia;
pub(crate) mod builtin_models;
pub(crate) mod catalog;
pub(crate) mod cloud;
pub(crate) mod cloud_keys;
pub(crate) mod coding;
pub(crate) mod cursor_personal;
pub(crate) mod inference;
pub(crate) mod kimchi;
pub(crate) mod mimo_platform;
pub(crate) mod router_accounts;
pub(crate) mod router_existing;
pub(crate) mod router_grok;
pub(crate) mod router_keys;
pub(crate) mod router_minimax;
pub(crate) mod router_sessions;
pub(crate) mod subscriptions;

use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Credentials, Metric, Status};
use reqwest::{Client, RequestBuilder};
use serde::de::DeserializeOwned;
use std::time::Duration;

pub(crate) struct Context<'a> {
    pub http: &'a Client,
    pub credentials: &'a dyn Credentials,
    pub timeout: Duration,
}

/// Result of a provider fetch. `notes` is the legacy Portuguese text vector
/// (preserved for [`crate::Report::notes`] and the CLI JSON); `notes_l10n`
/// is the parallel [`crate::text::Localized`] sidecar the desktop resolves
/// via its catalog. The two have the same length and order — providers use
/// [`crate::text::Notes`] to build them in lockstep, then destructure with
/// [`Notes::finish`] into the two fields.
#[derive(Debug)]
pub(crate) struct Data {
    pub metrics: Vec<Metric>,
    pub notes: Vec<String>,
    pub notes_l10n: Vec<Localized>,
    pub source: String,
}

impl Data {
    pub(crate) fn new(metrics: Vec<Metric>, notes: Notes, source: impl Into<String>) -> Self {
        let (notes, notes_l10n) = notes.finish();
        Self {
            metrics,
            notes,
            notes_l10n,
            source: source.into(),
        }
    }
}

/// Note id of [`QueryError::schema`]: the response did not match the
/// interface the adapter knows (read by `access` for PRD 0005 RF4.2).
pub(crate) const SCHEMA_NOTE: &str = "query.schema";
/// Note id of [`QueryError::transport`]: the provider could not be reached.
pub(crate) const TRANSPORT_NOTE: &str = "query.transport";
/// Note id of [`QueryError::timeout`]: the query ran out of time.
pub(crate) const TIMEOUT_NOTE: &str = "query.timeout";

/// RF8 structured error: a [`Status`] plus a [`Localized`] note the
/// frontend translates via the stable id. The Portuguese fallback is what
/// pre-RF8 callers used to receive as `message`.
#[derive(Debug)]
pub(crate) struct QueryError {
    pub status: Status,
    pub note: Localized,
}

impl QueryError {
    /// Configuration-time mistake. The id distinguishes each callsite so
    /// the frontend can render a specific message instead of a generic one.
    pub fn config(id: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: Status::NeedsConfiguration,
            note: Localized::fixed(id, message),
        }
    }
    pub fn config_params(
        id: &'static str,
        params: std::collections::BTreeMap<String, ParamValue>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status: Status::NeedsConfiguration,
            note: Localized::with_params(id, params, message),
        }
    }
    pub fn unsupported(id: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: Status::Unsupported,
            note: Localized::fixed(id, message),
        }
    }
    pub fn unsupported_params(
        id: &'static str,
        params: std::collections::BTreeMap<String, ParamValue>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status: Status::Unsupported,
            note: Localized::with_params(id, params, message),
        }
    }
    pub fn schema() -> Self {
        Self {
            status: Status::Error,
            note: Localized::fixed(
                SCHEMA_NOTE,
                "Resposta incompatível com a interface de consulta; nenhum valor foi estimado.",
            ),
        }
    }
    pub fn transport() -> Self {
        Self {
            status: Status::Error,
            note: Localized::fixed(TRANSPORT_NOTE, "Falha de rede ou tempo limite na consulta."),
        }
    }
    /// The account deadline (or the HTTP client's) ran out before an answer.
    pub fn timeout() -> Self {
        Self {
            status: Status::Error,
            note: Localized::fixed(TIMEOUT_NOTE, "Tempo limite da consulta esgotado."),
        }
    }
    /// The request would carry credentials over plain HTTP to a non-loopback
    /// host; nothing was sent.
    pub fn insecure() -> Self {
        Self {
            status: Status::Error,
            note: Localized::fixed(
                "query.insecure_transport",
                "Endpoint recusado: credenciais só trafegam por HTTPS. Nada foi enviado.",
            ),
        }
    }
}

/// Send a request after checking that its URL may carry credentials
/// ([`crate::transport::is_secure`]). Never reports the URL or the body.
pub(crate) async fn send(request: RequestBuilder) -> Result<reqwest::Response, QueryError> {
    let (client, request) = request.build_split();
    let request = request.map_err(|_| QueryError::transport())?;
    if !crate::transport::is_secure(request.url()) {
        return Err(QueryError::insecure());
    }
    client.execute(request).await.map_err(|error| {
        if error.is_timeout() {
            QueryError::timeout()
        } else {
            QueryError::transport()
        }
    })
}

/// PRD 0016 RF3.3: send a request that carries the web session of `slot`
/// (in its `Cookie` header) only over HTTPS to a host the slot's
/// descriptor declares; anything else is refused before the network. The
/// client never follows redirects, and a redirect answer (the provider
/// sending the session to its login page) is a rejected session, so the
/// session never reaches another host.
pub(crate) async fn send_session(
    request: RequestBuilder,
    provider: crate::Provider,
    slot: &str,
) -> Result<reqwest::Response, QueryError> {
    let (client, request) = request.build_split();
    let request = request.map_err(|_| QueryError::transport())?;
    if !crate::user_session::may_receive(provider, slot, request.url()) {
        return Err(QueryError {
            status: Status::Error,
            note: Localized::fixed(
                "query.session_host_refused",
                "Endereço recusado: a sessão só é enviada por HTTPS aos hosts declarados para ela. Nada foi enviado.",
            ),
        });
    }
    let response = client.execute(request).await.map_err(|error| {
        if error.is_timeout() {
            QueryError::timeout()
        } else {
            QueryError::transport()
        }
    })?;
    if response.status().is_redirection() {
        return Err(QueryError {
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.session_redirected",
                "O serviço redirecionou a consulta (sessão expirada ou revogada); o redirecionamento não foi seguido.",
            ),
        });
    }
    Ok(response)
}

pub(crate) fn secret(
    account: &Account,
    ctx: &Context<'_>,
    default: &str,
) -> Result<String, QueryError> {
    let name = account.credential_env.as_deref().unwrap_or(default);
    ctx.credentials
        .get(name)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            QueryError::config_params(
                "query.missing_credential",
                params([("name", ParamValue::from(name.to_string()))]),
                format!("Credencial ausente: {name}"),
            )
        })
}

pub(crate) fn param<'a>(account: &'a Account, key: &str) -> Result<&'a str, QueryError> {
    account
        .params
        .get(key)
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            QueryError::config_params(
                "query.missing_param",
                params([("key", ParamValue::from(key.to_string()))]),
                format!("Configure params.{key}"),
            )
        })
}

/// Never return response bodies or request URLs: both can contain credentials.
pub(crate) async fn json<T: DeserializeOwned>(request: RequestBuilder) -> Result<T, QueryError> {
    let response = send(request).await?;
    let code = response.status();
    if !code.is_success() {
        return Err(QueryError {
            status: match code.as_u16() {
                401 | 403 => Status::AuthenticationError,
                429 => Status::RateLimited,
                _ => Status::Error,
            },
            note: Localized::with_params(
                "query.http",
                params([("code", ParamValue::from(code.as_u16()))]),
                format!(
                    "Consulta retornou HTTP {}. Verifique acesso e permissões.",
                    code.as_u16()
                ),
            ),
        });
    }
    response.json().await.map_err(|_| QueryError::schema())
}

use std::future::Future;

/// Most pages a model listing may follow (RF4.3).
const MODEL_PAGE_LIMIT: usize = 50;

/// Follow page tokens to completion. A repeated token, the page limit or
/// any page error fails the whole listing: callers never get a partial list.
/// `page` builds each request synchronously and returns an owned future: an
/// `AsyncFnMut` borrowing the context fails the higher-ranked `Send` check
/// that spawning model operations (TUI tasks, Tauri commands) needs.
pub(crate) async fn paginate<T, F>(
    mut page: impl FnMut(Option<String>) -> F,
) -> Result<Vec<T>, QueryError>
where
    F: Future<Output = Result<(Vec<T>, Option<String>), QueryError>>,
{
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut token = None;
    for _ in 0..MODEL_PAGE_LIMIT {
        let (batch, next) = page(token).await?;
        items.extend(batch);
        match next.filter(|t| !t.is_empty()) {
            None => return Ok(items),
            Some(next) if !seen.insert(next.clone()) => return Err(QueryError::schema()),
            Some(next) => token = Some(next),
        }
    }
    Err(QueryError {
        status: Status::Error,
        note: Localized::fixed(
            "query.models.page_limit",
            "A listagem de modelos excedeu o limite de páginas; nada foi salvo.",
        ),
    })
}

/// OpenAI-shaped `GET …/models` (`data[].id`) with a Bearer key. Any entry
/// without a string id rejects the whole response.
pub(crate) async fn openai_models(
    ctx: &Context<'_>,
    url: &str,
    key: &str,
) -> Result<Vec<crate::models::ModelInfo>, QueryError> {
    #[derive(serde::Deserialize)]
    struct List {
        data: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        id: String,
    }
    let list: List = json(
        ctx.http
            .get(url)
            .bearer_auth(key.trim())
            .header("Accept", "application/json")
            .timeout(ctx.timeout),
    )
    .await?;
    Ok(list
        .data
        .into_iter()
        .map(|e| crate::models::ModelInfo {
            id: e.id,
            name: None,
            description: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn paginate_stops_on_repeated_token_and_page_limit() {
        let mut calls = 0;
        let looped = paginate(|_: Option<String>| {
            calls += 1;
            std::future::ready(Ok((vec![calls], Some("same".to_string()))))
        })
        .await
        .unwrap_err();
        assert_eq!((&*looped.note.id, calls), ("query.schema", 2));

        calls = 0;
        let endless = paginate(|_: Option<String>| {
            calls += 1;
            std::future::ready(Ok((vec![calls], Some(calls.to_string()))))
        })
        .await
        .unwrap_err();
        assert_eq!(
            (&*endless.note.id, calls),
            ("query.models.page_limit", MODEL_PAGE_LIMIT)
        );

        let mut pages = vec![(vec![3], None), (vec![1, 2], Some("p2".to_string()))];
        let full = paginate(|token: Option<String>| {
            assert_eq!(token.is_some(), pages.len() == 1, "token forwarded");
            std::future::ready(Ok(pages.pop().unwrap()))
        })
        .await
        .unwrap();
        assert_eq!(full, [1, 2, 3]);
    }
    #[tokio::test]
    async fn send_refuses_plain_http_to_a_remote_host_before_sending() {
        let client = reqwest::Client::new();
        for url in [
            "http://api.example.invalid/v1/usage",
            "http://10.0.0.5/v1/usage",
            "ftp://api.example.invalid/x",
        ] {
            let error = send(client.get(url).bearer_auth("secret-token"))
                .await
                .unwrap_err();
            assert_eq!(&*error.note.id, "query.insecure_transport", "{url}");
            assert_eq!(error.status, Status::Error);
        }
    }

    #[tokio::test]
    async fn send_allows_plain_http_to_loopback_servers() {
        use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;
        let value: serde_json::Value = json(reqwest::Client::new().get(server.uri()))
            .await
            .unwrap();
        assert_eq!(value["ok"], true);
    }
}
