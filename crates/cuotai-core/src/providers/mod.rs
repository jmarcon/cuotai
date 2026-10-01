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

use crate::{Account, Credentials, Metric, Status};
use reqwest::{Client, RequestBuilder};
use serde::de::DeserializeOwned;
use std::time::Duration;

pub(crate) struct Context<'a> {
    pub http: &'a Client,
    pub credentials: &'a dyn Credentials,
    pub timeout: Duration,
}

#[derive(Debug)]
pub(crate) struct Data {
    pub metrics: Vec<Metric>,
    pub notes: Vec<String>,
    pub source: String,
}

#[derive(Debug)]
pub(crate) struct QueryError {
    pub status: Status,
    pub message: String,
}

impl QueryError {
    pub fn config(message: impl Into<String>) -> Self {
        Self {
            status: Status::NeedsConfiguration,
            message: message.into(),
        }
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            status: Status::Unsupported,
            message: message.into(),
        }
    }
    pub fn schema() -> Self {
        Self {
            status: Status::Error,
            message:
                "Resposta incompatível com a interface de consulta; nenhum valor foi estimado."
                    .into(),
        }
    }
    pub fn transport() -> Self {
        Self {
            status: Status::Error,
            message: "Falha de rede ou tempo limite na consulta.".into(),
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
        .ok_or_else(|| QueryError::config(format!("Credencial ausente: {name}")))
}

pub(crate) fn param<'a>(account: &'a Account, key: &str) -> Result<&'a str, QueryError> {
    account
        .params
        .get(key)
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| QueryError::config(format!("Configure params.{key}")))
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
            message: format!(
                "Consulta retornou HTTP {}. Verifique acesso e permissões.",
                code.as_u16()
            ),
        });
    }
    response.json().await.map_err(|_| QueryError::schema())
}
