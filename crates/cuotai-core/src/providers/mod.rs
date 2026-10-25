pub(crate) mod alibaba_billing;
pub(crate) mod alibaba_plans;
pub(crate) mod antigravity;
pub(crate) mod api;
pub(crate) mod asia;
pub(crate) mod capture;
pub(crate) mod catalog;
pub(crate) mod cloud;
pub(crate) mod cloud_keys;
pub(crate) mod coding;
pub(crate) mod cursor_personal;
pub(crate) mod eleven_labs;
pub(crate) mod inference;
pub(crate) mod kimchi;
#[cfg(test)]
mod label_snapshot;
pub(crate) mod litellm;
pub(crate) mod mimo_platform;
pub(crate) mod nine_router;
pub(crate) mod nine_router_tls;
pub(crate) mod open_router;
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
    /// PRD 0036: the sub-accounts an aggregator relays; empty for every
    /// ordinary provider.
    pub sub_reports: Vec<crate::SubReport>,
    /// PRD 0033 RF3.1: the provider-stated time of the data, if any.
    pub as_of: Option<chrono::DateTime<chrono::Utc>>,
    /// Overage state not visible in a metric (PRD 0023 RF3).
    pub overage_hint: Option<crate::OverageHint>,
}

impl Data {
    /// Record the provider-stated time of the data (RF3.1).
    pub(crate) fn with_as_of(mut self, as_of: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        self.as_of = as_of;
        self
    }

    pub(crate) fn with_overage_hint(mut self, hint: Option<crate::OverageHint>) -> Self {
        self.overage_hint = hint;
        self
    }

    pub(crate) fn new(metrics: Vec<Metric>, notes: Notes, source: impl Into<String>) -> Self {
        let (notes, notes_l10n) = notes.finish();
        Self {
            metrics,
            notes,
            notes_l10n,
            source: source.into(),
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
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
    /// PRD 0028 RF1: what drifted, when a decode failed (value-free).
    pub diagnostic: Option<Box<crate::diagnostic::Diagnostic>>,
}

impl QueryError {
    /// Configuration-time mistake. The id distinguishes each callsite so
    /// the frontend can render a specific message instead of a generic one.
    pub fn config(id: &'static str, message: impl Into<String>) -> Self {
        Self {
            diagnostic: None,
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
            diagnostic: None,
            status: Status::NeedsConfiguration,
            note: Localized::with_params(id, params, message),
        }
    }
    pub fn unsupported(id: &'static str, message: impl Into<String>) -> Self {
        Self {
            diagnostic: None,
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
            diagnostic: None,
            status: Status::Unsupported,
            note: Localized::with_params(id, params, message),
        }
    }
    pub fn schema() -> Self {
        Self {
            diagnostic: None,
            status: Status::Error,
            note: Localized::fixed(
                SCHEMA_NOTE,
                "Resposta incompatível com a interface de consulta; nenhum valor foi estimado.",
            ),
        }
    }
    /// [`QueryError::schema`] with the redacted description of the drift.
    pub fn schema_drift(drift: crate::diagnostic::SchemaDrift) -> Self {
        Self {
            diagnostic: Some(Box::new(crate::diagnostic::Diagnostic::SchemaDrift(drift))),
            ..Self::schema()
        }
    }
    /// [`QueryError::schema`] for a field an adapter reads by hand: names
    /// `path` (see [`crate::diagnostic::field_drift`]) and the type it needed.
    pub fn schema_at(root: &serde_json::Value, path: &[&str], expected: &str) -> Self {
        Self::schema_drift(crate::diagnostic::field_drift(root, path, expected))
    }
    /// [`QueryError::schema_at`] for a field found by name anywhere in the
    /// response (see [`crate::diagnostic::named_drift`]).
    pub fn schema_named(
        root: &serde_json::Value,
        name: &str,
        found: Option<&serde_json::Value>,
        expected: &str,
    ) -> Self {
        Self::schema_drift(crate::diagnostic::named_drift(root, name, found, expected))
    }
    pub fn transport() -> Self {
        Self {
            diagnostic: None,
            status: Status::Error,
            note: Localized::fixed(TRANSPORT_NOTE, "Falha de rede ou tempo limite na consulta."),
        }
    }
    /// The account deadline (or the HTTP client's) ran out before an answer.
    pub fn timeout() -> Self {
        Self {
            diagnostic: None,
            status: Status::Error,
            note: Localized::fixed(TIMEOUT_NOTE, "Tempo limite da consulta esgotado."),
        }
    }
    /// Attach a provider's `Retry-After` (whole seconds) to this error so the
    /// refresh cycle delays only this account (PRD 0038 RF7.1).
    pub fn with_retry_after(mut self, seconds: Option<u64>) -> Self {
        if let Some(seconds) = seconds {
            self.note.params.insert(
                crate::Report::RETRY_AFTER_PARAM.to_string(),
                ParamValue::from(seconds),
            );
        }
        self
    }

    /// The request would carry credentials over plain HTTP to a non-loopback
    /// host; nothing was sent.
    pub fn insecure() -> Self {
        Self {
            diagnostic: None,
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
        return Err(QueryError { diagnostic: None,
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
        return Err(QueryError { diagnostic: None,
            status: Status::AuthenticationError,
            note: Localized::fixed(
                "query.session_redirected",
                "O serviço redirecionou a consulta (sessão expirada ou revogada); o redirecionamento não foi seguido.",
            ),
        });
    }
    Ok(response)
}

/// PRD 0016 RF5.1: the web session an adapter sends, read through the
/// slot's descriptor. `vault` tells a session pasted into (or moved to) the
/// system vault from one an environment variable holds. Never
/// `Debug`-printed; dropping it overwrites the value.
pub(crate) struct WebSession {
    pub cookie: String,
    pub vault: bool,
}

impl Drop for WebSession {
    fn drop(&mut self) {
        crate::user_session::wipe(&mut self.cookie);
    }
}

/// PRD 0016 RF5.1: the session of `slot` (a [`crate::user_session`]
/// descriptor of the account's provider), whether the vault holds it or the
/// variable named by `params.<slot>` does; `None` when the account has
/// neither. The value is sent as stored, but a session without the
/// descriptor's required cookies is refused before any request, naming only
/// the cookies (and the variable) that need fixing.
pub(crate) fn web_session(
    account: &Account,
    ctx: &Context<'_>,
    slot: &'static str,
) -> Result<Option<WebSession>, QueryError> {
    let Some(name) = account
        .params
        .get(slot)
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
    else {
        return Ok(None);
    };
    let vault = name.starts_with(crate::user_session::VAULT_NAME);
    let Some(mut cookie) = ctx.credentials.get(name) else {
        return Err(session_unset(name, slot, vault));
    };
    if cookie.trim().is_empty() {
        crate::user_session::wipe(&mut cookie);
        return Err(session_unset(name, slot, vault));
    }
    let session = WebSession {
        cookie: cookie.trim().to_string(),
        vault,
    };
    crate::user_session::wipe(&mut cookie);
    let missing = crate::user_session::missing_required(account.provider, slot, &session.cookie);
    if missing.is_empty() {
        return Ok(Some(session));
    }
    let names = missing.join(", ");
    let note = if vault {
        Localized::with_param(
            "query.session.missing_cookies",
            "names",
            names.as_str(),
            format!("A sessão guardada no cofre do sistema não tem os cookies {names}; cole uma nova no assistente da conta."),
        )
    } else {
        Localized::with_params(
            "query.session.missing_cookies_env",
            params([
                ("names", ParamValue::from(names.clone())),
                ("name", ParamValue::from(name.to_string())),
                ("slot", ParamValue::from(slot)),
            ]),
            format!("A sessão da variável {name} (params.{slot}) não tem os cookies {names}; exporte-a de novo ou cole a sessão no assistente da conta."),
        )
    };
    Err(QueryError {
        diagnostic: None,
        status: Status::NeedsConfiguration,
        note,
    })
}

/// The variable `params.<slot>` names is unset or empty (or, for a vault
/// session, the entry came back empty).
fn session_unset(name: &str, slot: &str, vault: bool) -> QueryError {
    let note = if vault {
        crate::user_session::not_found()
    } else {
        Localized::with_params(
            "query.session.variable_missing",
            params([
                ("name", ParamValue::from(name.to_string())),
                ("slot", ParamValue::from(slot.to_string())),
            ]),
            format!("Sessão ausente: a variável {name} (params.{slot}) não está definida; defina-a ou cole a sessão no assistente da conta."),
        )
    };
    QueryError {
        diagnostic: None,
        status: Status::NeedsConfiguration,
        note,
    }
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
            diagnostic: None,
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
    let bytes = response.bytes().await.map_err(|_| QueryError::schema())?;
    decode_bytes(&bytes)
}

/// Parse `bytes` as JSON and decode them as `T` with the shared decoder, so
/// a failure carries the redacted drift (PRD 0028 RF1).
pub(crate) fn decode_bytes<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, QueryError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| QueryError::schema())?;
    capture::record(&value);
    decode_value(&value)
}

/// Decode a parsed value with the shared decoder.
pub(crate) fn decode_value<T: DeserializeOwned>(
    value: &serde_json::Value,
) -> Result<T, QueryError> {
    crate::diagnostic::decode(value).map_err(QueryError::schema_drift)
}

/// A field an adapter needed and did not find in the shape it expected
/// (PRD 0028 RF1). Helpers that only see a nested object return it; the
/// caller that owns the root turns it into a [`QueryError`].
#[derive(Debug)]
pub(crate) struct Miss {
    path: Vec<String>,
    expected: &'static str,
}

impl Miss {
    pub(crate) fn new(key: &str, expected: &'static str) -> Self {
        Self {
            path: vec![key.to_string()],
            expected,
        }
    }

    /// The response root itself had the wrong type.
    pub(crate) fn root(expected: &'static str) -> Self {
        Self {
            path: Vec::new(),
            expected,
        }
    }

    /// The same miss, one level down from `steps` (`"[]"` for an array).
    pub(crate) fn under(mut self, steps: &[&str]) -> Self {
        let mut path: Vec<String> = steps.iter().map(|s| s.to_string()).collect();
        path.append(&mut self.path);
        self.path = path;
        self
    }

    pub(crate) fn into_error(self, root: &serde_json::Value) -> QueryError {
        let path: Vec<&str> = self.path.iter().map(String::as_str).collect();
        QueryError::schema_at(root, &path, self.expected)
    }
}

/// A JSON object body, or a schema failure naming the root.
pub(crate) fn object_of(
    value: &serde_json::Value,
) -> Result<&serde_json::Map<String, serde_json::Value>, QueryError> {
    value
        .as_object()
        .ok_or_else(|| QueryError::schema_at(value, &[], "object"))
}

/// A body parsed from a response, bounded and without keeping its text.
pub(crate) async fn body_value(
    response: reqwest::Response,
) -> Result<serde_json::Value, QueryError> {
    let bytes = response.bytes().await.map_err(|_| QueryError::schema())?;
    let value = serde_json::from_slice(&bytes).map_err(|_| QueryError::schema())?;
    capture::record(&value);
    Ok(value)
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
        diagnostic: None,
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

/// Parse a `Retry-After` header: delta-seconds or an HTTP date (RFC 9110
/// 10.2.3). A date in the past yields `None`.
pub(crate) fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let raw = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(seconds);
    }
    let date = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
    let delta = (date.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds();
    u64::try_from(delta).ok().filter(|seconds| *seconds > 0)
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
        for (scheme, host) in [
            ("http", "api.example.invalid"),
            ("http", "10.0.0.5"),
            ("ftp", "api.example.invalid"),
        ] {
            let url = format!("{scheme}://{host}/v1/usage");
            let error = send(client.get(&url).bearer_auth("secret-token"))
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

    struct Vars(Vec<(String, &'static str)>);
    impl Credentials for Vars {
        fn get(&self, name: &str) -> Option<String> {
            self.0
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    fn session_of(
        provider: crate::Provider,
        params: &[(&str, &str)],
        vars: &[(&str, &'static str)],
        slot: &'static str,
    ) -> Result<Option<(String, bool)>, QueryError> {
        let account = Account {
            id: None,
            provider,
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        };
        let credentials = Vars(vars.iter().map(|(k, v)| (k.to_string(), *v)).collect());
        let http = Client::new();
        let ctx = Context {
            http: &http,
            credentials: &credentials,
            timeout: Duration::from_secs(1),
        };
        web_session(&account, &ctx, slot)
            .map(|session| session.map(|s| (s.cookie.clone(), s.vault)))
    }

    /// PRD 0016 RF5.1: the three migrated providers read their sessions
    /// through the descriptor, from the vault or from `session_env`.
    #[test]
    fn web_sessions_come_from_the_vault_or_the_variable_and_need_the_required_cookies() {
        use crate::Provider::{AlibabaTokenPlan, XiaomiMimo};
        const SECRET: &str = "SECRET-VALUE";
        let vault = format!("{}platform_session_env", crate::user_session::VAULT_NAME);
        let full = "api-platform_serviceToken=SECRET-VALUE; userId=7";
        // No session configured: the adapter takes its other route.
        assert!(matches!(
            session_of(XiaomiMimo, &[], &[], "platform_session_env"),
            Ok(None)
        ));
        // A variable is sent as stored (trimmed), not reduced.
        let from_env = session_of(
            XiaomiMimo,
            &[("platform_session_env", "MIMO_COOKIE")],
            &[(
                "MIMO_COOKIE",
                " api-platform_serviceToken=SECRET-VALUE; userId=7; extra=1 ",
            )],
            "platform_session_env",
        );
        assert_eq!(from_env.unwrap(), Some((format!("{full}; extra=1"), false)));
        let from_vault = session_of(
            XiaomiMimo,
            &[("platform_session_env", vault.as_str())],
            &[(vault.as_str(), full)],
            "platform_session_env",
        );
        assert_eq!(from_vault.unwrap(), Some((full.to_string(), true)));
        // Alibaba declares no required cookie: any session goes through.
        assert!(session_of(
            AlibabaTokenPlan,
            &[("session_env", "QWEN_CLOUD_COOKIE")],
            &[("QWEN_CLOUD_COOKIE", "session=x")],
            "session_env",
        )
        .unwrap()
        .is_some());

        let unset = session_of(
            XiaomiMimo,
            &[("platform_session_env", "MIMO_COOKIE")],
            &[("MIMO_COOKIE", "  ")],
            "platform_session_env",
        )
        .expect_err("unset variable");
        assert_eq!(unset.status, Status::NeedsConfiguration);
        assert_eq!(&*unset.note.id, "query.session.variable_missing");
        assert!(unset
            .note
            .resolve("en-US")
            .contains("MIMO_COOKIE (params.platform_session_env)"));

        let lacking = session_of(
            XiaomiMimo,
            &[("session_env", "MIMO_SESSION")],
            &[("MIMO_SESSION", "serviceToken=; userId=SECRET-VALUE")],
            "session_env",
        )
        .expect_err("empty serviceToken");
        assert_eq!(lacking.status, Status::NeedsConfiguration);
        assert_eq!(&*lacking.note.id, "query.session.missing_cookies_env");
        let vault_lacking = session_of(
            XiaomiMimo,
            &[("platform_session_env", vault.as_str())],
            &[(vault.as_str(), "api-platform_serviceToken=SECRET-VALUE")],
            "platform_session_env",
        )
        .expect_err("userId missing");
        assert_eq!(&*vault_lacking.note.id, "query.session.missing_cookies");
        for (error, names) in [(&lacking, "serviceToken"), (&vault_lacking, "userId")] {
            for locale in ["pt-BR", "en-US", "es", "it"] {
                let text = error.note.resolve(locale);
                assert!(text.contains(names), "{locale}: {text}");
                assert!(!text.contains(SECRET), "{locale}: {text}");
                assert!(!text.contains(crate::user_session::VAULT_NAME), "{text}");
            }
        }
        assert!(lacking.note.resolve("es").contains("MIMO_SESSION"));
    }
}

#[cfg(test)]
mod retry_after_tests {
    use super::retry_after_seconds;
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};

    fn parse(raw: &str) -> Option<u64> {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_str(raw).unwrap());
        retry_after_seconds(&headers)
    }

    #[test]
    fn seconds_dates_and_garbage() {
        assert_eq!(parse(" 120 "), Some(120));
        assert_eq!(parse("soon"), None);
        assert_eq!(parse("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        let future = (chrono::Utc::now() + chrono::Duration::seconds(600)).to_rfc2822();
        assert!(parse(&future).is_some_and(|s| (590..=600).contains(&s)));
        assert_eq!(retry_after_seconds(&HeaderMap::new()), None);
    }
}
