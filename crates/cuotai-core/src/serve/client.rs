//! PRD 0031 RF2.1: read a running local server's `/v1/limits` instead of
//! querying the providers again (`cuotai --status-line --from-server`).
//!
//! Only loopback `http://` addresses are accepted: the bearer token never
//! leaves the computer, and the server never listens elsewhere by default.
//! Answers are read with the core's HTTP client settings (no redirects, a
//! short timeout) and bounded; errors name neither the token nor the body.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{Token, DEFAULT_PORT};
use crate::text::Localized;
use crate::tray::AccountView;
use crate::Provider;

/// How long the whole request may take.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// The most `/v1/limits` may weigh.
const MAX_BYTES: usize = 4 * 1024 * 1024;

/// `http://127.0.0.1:<DEFAULT_PORT>`, where `cuotai --serve` listens by default.
pub fn default_url() -> String {
    format!("http://{}:{DEFAULT_PORT}", Ipv4Addr::LOCALHOST)
}

/// Why the server could not be read. Messages never carry the token, the
/// answer's body or more of the address than the user typed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    #[error("Invalid local server address: use http://127.0.0.1:PORT.")]
    InvalidUrl,
    #[error("The local server is only read over loopback (127.0.0.1, ::1 or localhost).")]
    NotLoopback,
    #[error("The local server did not answer; is it running (cuotai --serve)?")]
    Unreachable,
    #[error(
        "The local server refused the token (use --token-file with the same file as --serve)."
    )]
    Unauthorized,
    #[error("The local server has no readings yet; try again in a moment.")]
    NotReady,
    #[error("The local server's answer is not a compatible /v1/limits.")]
    Incompatible,
}

impl ClientError {
    pub fn l10n(&self) -> Localized {
        let key = match self {
            Self::InvalidUrl => "serve.client.invalid_url",
            Self::NotLoopback => "serve.client.not_loopback",
            Self::Unreachable => "serve.client.unreachable",
            Self::Unauthorized => "serve.client.unauthorized",
            Self::NotReady => "serve.client.not_ready",
            Self::Incompatible => "serve.client.incompatible",
        };
        Localized::fixed(key, self.to_string())
    }
}

/// The `/v1/limits` address of the server at `base` (`http://host:port`,
/// optionally with a trailing `/`), refusing anything but loopback `http`.
pub fn limits_url(base: &str) -> Result<url::Url, ClientError> {
    let url = url::Url::parse(base.trim()).map_err(|_| ClientError::InvalidUrl)?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(ClientError::InvalidUrl);
    }
    let loopback = match url.host() {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        None => return Err(ClientError::InvalidUrl),
    };
    if !loopback {
        return Err(ClientError::NotLoopback);
    }
    url.join("/v1/limits").map_err(|_| ClientError::InvalidUrl)
}

/// Read `/v1/limits` of the server at `base` with `token` (when the server
/// requires one) and turn each account into the view `tray::summary` takes.
pub async fn limits_views(
    base: &str,
    token: Option<&Token>,
) -> Result<Vec<AccountView>, ClientError> {
    let url = limits_url(base)?;
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .user_agent(concat!("cuotai/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| ClientError::Unreachable)?;
    let mut request = client.get(url).header("Accept", "application/json");
    if let Some(token) = token {
        request = request.bearer_auth(token.expose());
    }
    let mut response = request.send().await.map_err(|_| ClientError::Unreachable)?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err(ClientError::Unauthorized),
        503 => return Err(ClientError::NotReady),
        _ => return Err(ClientError::Incompatible),
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ClientError::Unreachable)?
    {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err(ClientError::Incompatible);
        }
        body.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&body).map_err(|_| ClientError::Incompatible)?;
    views_from_limits(&value)
}

/// The views of a `/v1/limits` body (schema 1), in its order. Unknown
/// fields and values are ignored (open enums, PRD 0031 RF5); a body
/// without `accounts` is incompatible.
pub fn views_from_limits(limits: &Value) -> Result<Vec<AccountView>, ClientError> {
    if limits.get("schema").and_then(Value::as_u64) != Some(1) {
        return Err(ClientError::Incompatible);
    }
    let accounts = limits
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or(ClientError::Incompatible)?;
    Ok(accounts.iter().map(view).collect())
}

fn view(account: &Value) -> AccountView {
    let text = |key: &str| {
        account
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let provider = text("provider");
    let label =
        Provider::from_slug(&provider).map_or_else(|| provider.clone(), |p| p.label().to_owned());
    let binding = account.get("binding").filter(|b| b.is_object());
    let time = |key: &str| {
        binding
            .and_then(|b| b.get(key))
            .and_then(Value::as_str)
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
    };
    let remaining = binding
        .and_then(|b| b.get("used_ratio"))
        .and_then(Value::as_f64)
        .filter(|ratio| ratio.is_finite())
        .map(|ratio| (1.0 - ratio).clamp(0.0, 1.0));
    let incident = account
        .get("incident")
        .filter(|i| i.get("stale").and_then(Value::as_bool) != Some(true))
        .and_then(|i| i.get("state"))
        .and_then(Value::as_str)
        .is_some_and(|state| state != "operational");
    AccountView {
        key: text("id"),
        provider,
        provider_label: label,
        name: text("name"),
        remaining,
        stale: account.get("stale").and_then(Value::as_bool) == Some(true),
        exhausted: text("class") == "exhausted",
        resets_at: time("blocked_until").or_else(|| time("resets_at")),
        incident,
        ..AccountView::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_loopback_http_addresses_are_read() {
        assert_eq!(
            limits_url("http://127.0.0.1:7878").unwrap().as_str(),
            "http://127.0.0.1:7878/v1/limits"
        );
        assert!(limits_url("http://localhost:7878/").is_ok());
        assert!(limits_url("http://[::1]:7878").is_ok());
        assert!(limits_url(&default_url()).is_ok());
        assert_eq!(
            limits_url("http://192.168.0.2:7878"),
            Err(ClientError::NotLoopback)
        );
        assert_eq!(
            limits_url("http://example.com:7878"),
            Err(ClientError::NotLoopback)
        );
        for bad in [
            "https://127.0.0.1:7878",
            "http://user:pass@127.0.0.1:7878",
            "http://127.0.0.1:7878/v1/snapshot",
            "http://127.0.0.1:7878/?x=1",
            "not a url",
        ] {
            assert_eq!(limits_url(bad), Err(ClientError::InvalidUrl), "{bad}");
        }
    }

    #[test]
    fn limits_become_tray_views() {
        let body = json!({
            "schema": 1,
            "generated_at": "2026-10-25T12:00:00Z",
            "accounts": [
                {"index": 0, "id": "acc_a", "key": "claude:Main", "name": "Main",
                 "provider": "claude", "class": "available", "stale": false,
                 "binding": {"used_ratio": 0.78, "resets_at": "2026-10-25T15:00:00Z"},
                 "incident": {"state": "major_outage", "stale": false},
                 "future_field": 1},
                {"index": 1, "id": "acc_b", "key": "x:Side", "name": "Side",
                 "provider": "some_future_provider", "class": "exhausted", "stale": true,
                 "binding": null}
            ],
            "summary": {}
        });
        let views = views_from_limits(&body).unwrap();
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].key, "acc_a");
        assert_eq!(views[0].provider_label, Provider::Claude.label());
        assert!((views[0].remaining.unwrap() - 0.22).abs() < 1e-9);
        assert!(views[0].incident);
        assert!(views[0].resets_at.is_some());
        assert_eq!(views[1].remaining, None);
        assert!(views[1].exhausted && views[1].stale);
        assert_eq!(views[1].provider_label, "some_future_provider");
    }

    #[test]
    fn other_bodies_are_incompatible() {
        assert_eq!(
            views_from_limits(&json!({"schema": 2, "accounts": []})),
            Err(ClientError::Incompatible)
        );
        assert_eq!(
            views_from_limits(&json!({"schema": 1})),
            Err(ClientError::Incompatible)
        );
    }
}
