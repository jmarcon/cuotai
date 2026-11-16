//! PRD 0031 RF2.1: read a running local server's `/v1/limits` instead of
//! querying the providers again (`cuotai --status-line --from-server`).
//!
//! Only loopback `http://` addresses are accepted: the bearer token never
//! leaves the computer, and the server never listens elsewhere by default.
//! Answers are read with the core's HTTP client settings (no redirects, a
//! short timeout) and bounded; errors name neither the token nor the body.
//!
//! Re-validation S-03: before a token is sent, the program on the port must
//! prove it holds it. The client sends `GET /health` with a fresh
//! `Cuotai-Challenge` nonce and no `Authorization`; only an answer with
//! `"auth":"token"` and a valid proof (HMAC-SHA256 under the token over the
//! label, the nonce and the port dialled) gets the token. `"auth":"none"`
//! (a `--no-token` server) is read without sending it. Anything else stops
//! with [`ClientError::Unproven`]: there is no fallback.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{Token, CHALLENGE_HEADER, DEFAULT_PORT};
use crate::text::Localized;
use crate::tray::AccountView;
use crate::Provider;

/// How long the whole request may take.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// The most `/v1/limits` may weigh.
const MAX_BYTES: usize = 4 * 1024 * 1024;

/// The most a `/health` answer may weigh.
const HEALTH_MAX_BYTES: usize = 16 * 1024;

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
    #[error("The local server is only read over loopback (127.0.0.1 or ::1; localhost is read as 127.0.0.1).")]
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
    /// Re-validation S-03: the program on the port did not prove it holds
    /// the token, so the token was not sent.
    #[error("The program on port {port} did not prove it is your Cuotai server; update Cuotai on the server side, or check what holds the port.")]
    Unproven { port: u16 },
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
            Self::Unproven { port } => {
                return Localized::with_param(
                    "serve.client.unproven",
                    "port",
                    port.to_string(),
                    "The program on port {port} did not prove it is your Cuotai server; update Cuotai on the server side, or check what holds the port.",
                );
            }
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
    let mut url = url;
    let loopback = match url.host() {
        Some(url::Host::Domain(name)) if name.eq_ignore_ascii_case("localhost") => {
            // PRD 0012 RF3.1: `localhost` is read as `127.0.0.1` and never
            // resolved, so the token goes to the address the server holds
            // and not to whatever the resolver lists first.
            url.set_ip_host(IpAddr::V4(Ipv4Addr::LOCALHOST))
                .map_err(|_| ClientError::InvalidUrl)?;
            true
        }
        Some(url::Host::Domain(_)) => false,
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
    Ok(limits_accounts(base, token)
        .await?
        .into_iter()
        .map(|account| account.view)
        .collect())
}

/// One account of a server's `/v1/limits`: the view `tray::summary` takes
/// (its `key` is the stable id) and the legacy `key`, which
/// `--account` also accepts.
#[derive(Debug, Clone)]
pub struct ServerAccount {
    pub view: AccountView,
    pub key: String,
}

/// [`limits_views`] with each account's legacy key.
pub async fn limits_accounts(
    base: &str,
    token: Option<&Token>,
) -> Result<Vec<ServerAccount>, ClientError> {
    let url = limits_url(base)?;
    let client =
        crate::network::builder_with_own_roots(crate::network::ClientPurpose::LocalServer, TIMEOUT)
            .build()
            .map_err(|_| ClientError::Unreachable)?;
    // S-03: a token goes only to a server that proved it holds it.
    let token = match token {
        Some(token) => prove(&client, &url, token).await?.then_some(token),
        None => None,
    };
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
    let body = read_bounded(&mut response, MAX_BYTES).await?;
    let value: Value = serde_json::from_slice(&body).map_err(|_| ClientError::Incompatible)?;
    accounts_from_limits(&value)
}

/// S-03: challenge the server at `limits` (its `/health`) without sending
/// `token`. `Ok(true)`: it proved it holds `token`, send it. `Ok(false)`:
/// it serves without a token, read without sending it. Anything else is
/// [`ClientError::Unproven`] (or [`ClientError::Unreachable`]).
async fn prove(
    client: &reqwest::Client,
    limits: &url::Url,
    token: &Token,
) -> Result<bool, ClientError> {
    let port = limits.port_or_known_default().unwrap_or(80);
    let unproven = ClientError::Unproven { port };
    let health = limits
        .join("/health")
        .map_err(|_| ClientError::InvalidUrl)?;
    let nonce = super::token::new_challenge().map_err(|_| unproven.clone())?;
    let mut response = client
        .get(health)
        .header("Accept", "application/json")
        .header(CHALLENGE_HEADER, &nonce)
        .send()
        .await
        .map_err(|_| ClientError::Unreachable)?;
    if response.status().as_u16() != 200 {
        return Err(unproven);
    }
    let body = read_bounded(&mut response, HEALTH_MAX_BYTES)
        .await
        .map_err(|_| unproven.clone())?;
    let value: Value = serde_json::from_slice(&body).map_err(|_| unproven.clone())?;
    match value.get("auth").and_then(Value::as_str) {
        Some("none") => Ok(false),
        Some("token")
            if value
                .get("proof")
                .and_then(Value::as_str)
                .is_some_and(|proof| token.proof_matches(&nonce, port, proof)) =>
        {
            Ok(true)
        }
        _ => Err(unproven),
    }
}

/// The body of `response`, refused past `max` bytes.
async fn read_bounded(
    response: &mut reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, ClientError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ClientError::Unreachable)?
    {
        if body.len() + chunk.len() > max {
            return Err(ClientError::Incompatible);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// The views of a `/v1/limits` body (schema 1), in its order. Unknown
/// fields and values are ignored (open enums, PRD 0031 RF5); a body
/// without `accounts` is incompatible.
pub fn views_from_limits(limits: &Value) -> Result<Vec<AccountView>, ClientError> {
    Ok(accounts_from_limits(limits)?
        .into_iter()
        .map(|account| account.view)
        .collect())
}

/// [`views_from_limits`] with each account's legacy key.
pub fn accounts_from_limits(limits: &Value) -> Result<Vec<ServerAccount>, ClientError> {
    if limits.get("schema").and_then(Value::as_u64) != Some(1) {
        return Err(ClientError::Incompatible);
    }
    let accounts = limits
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or(ClientError::Incompatible)?;
    Ok(accounts
        .iter()
        .map(|account| ServerAccount {
            view: view(account),
            key: crate::text::strip_disguising(
                account
                    .get("key")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .into_owned(),
        })
        .collect())
}

fn view(account: &Value) -> AccountView {
    // S-04: the server's text never carries control or direction
    // characters to the terminal.
    let text = |key: &str| {
        crate::text::strip_disguising(account.get(key).and_then(Value::as_str).unwrap_or_default())
            .into_owned()
    };
    let provider = text("provider");
    // An unknown slug shows only when it looks like one; anything else is "?".
    let label = Provider::from_slug(&provider).map_or_else(
        || {
            let slug = !provider.is_empty()
                && provider.len() <= 32
                && provider
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if slug {
                provider.clone()
            } else {
                "?".to_owned()
            }
        },
        |p| p.label().to_owned(),
    );
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
        // V-04: a failed account counts as stale, like a failed query in
        // `cuotai --status-line` without a server (marked, exit 2).
        stale: account.get("stale").and_then(Value::as_bool) == Some(true)
            || account.get("failure").is_some_and(Value::is_object),
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
        assert_eq!(
            limits_url("http://LocalHost:7878/").unwrap().as_str(),
            "http://127.0.0.1:7878/v1/limits"
        );
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

    /// PRD 0012 RF3.1: a process holding only `[::1]:port` never receives
    /// the request (and its token) for `http://localhost:port`.
    #[tokio::test]
    async fn localhost_never_reaches_an_ipv6_only_listener() {
        let Ok(squatter) = std::net::TcpListener::bind("[::1]:0") else {
            return; // no IPv6 loopback on this host
        };
        squatter.set_nonblocking(true).unwrap();
        let port = squatter.local_addr().unwrap().port();
        let token = Token::generate().unwrap();
        let _ = limits_views(&format!("http://localhost:{port}"), Some(&token)).await;
        assert_eq!(
            squatter.accept().map(|_| ()).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "the [::1] listener must not receive the request"
        );
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

    /// S-04: OSC 0 (window title), OSC 52 (clipboard), CSI and bidi
    /// characters from the server never reach the line.
    #[test]
    fn server_text_loses_control_and_direction_characters() {
        let body = json!({
            "schema": 1,
            "accounts": [
                {"id": "a\u{1b}[2Jb", "key": "k\u{7}", "name": "\u{1b}]0;pwned\u{7}Ma\u{202e}in\u{200f}",
                 "provider": "claude", "class": "available", "stale": false, "binding": null},
                {"id": "b", "key": "b", "name": "x\u{1b}]52;c;aGk=\u{7}",
                 "provider": "Evil\u{1b}[31m", "class": "available", "stale": false, "binding": null}
            ]
        });
        let views = views_from_limits(&body).unwrap();
        assert_eq!(views[0].name, "]0;pwnedMain");
        assert_eq!(views[0].key, "a[2Jb");
        assert_eq!(views[1].name, "x]52;c;aGk=");
        assert_eq!(views[1].provider_label, "?");
        for view in &views {
            for text in [&view.name, &view.key, &view.provider, &view.provider_label] {
                assert!(
                    !text
                        .chars()
                        .any(|c| c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
                    "{text:?}"
                );
            }
        }
    }

    /// V-04: a failed account without a preserved reading counts as stale.
    #[test]
    fn a_failed_account_counts_as_stale() {
        let body = json!({"schema": 1, "accounts": [
            {"id": "a", "key": "a", "name": "A", "provider": "claude", "class": "unmeasured",
             "stale": false, "binding": null, "failure": {"category": "auth_required"}},
            {"id": "b", "key": "b", "name": "B", "provider": "claude", "class": "unmeasured",
             "stale": false, "binding": null}
        ]});
        let views = views_from_limits(&body).unwrap();
        assert!(views[0].stale);
        assert!(!views[1].stale);
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
