pub(crate) mod alibaba_billing;
pub(crate) mod alibaba_plans;
pub(crate) mod antigravity;
pub(crate) mod api;
pub(crate) mod asia;
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
                "query.schema",
                "Resposta incompatível com a interface de consulta; nenhum valor foi estimado.",
            ),
        }
    }
    pub fn transport() -> Self {
        Self {
            status: Status::Error,
            note: Localized::fixed(
                "query.transport",
                "Falha de rede ou tempo limite na consulta.",
            ),
        }
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
    let response = request.send().await.map_err(|_| QueryError::transport())?;
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
