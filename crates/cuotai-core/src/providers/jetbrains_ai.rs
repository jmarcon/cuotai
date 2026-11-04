//! Read-only JetBrains AI organization credits query (PRD 0055).
//!
//! Contracts documented by JetBrains:
//!
//! - token: <https://www.jetbrains.com/help/air/exchange-client-id-and-secret-for-an-access-token.html>,
//!   `POST https://oauth.account.jetbrains.com/api/rest/oauth2/token` with HTTP
//!   Basic `client_id:client_secret` and the form body
//!   `grant_type=client_credentials&scope=jcp-public`; the answer carries
//!   `access_token` and `expires_in` (3600 s), with no refresh token, and the
//!   documentation asks clients to cache the token;
//! - usage: <https://www.jetbrains.com/help/air/analytics-api-v2.html>,
//!   `POST https://api.jetbrains.cloud/metrics/api/v2/credits-usage/group-by/tool`
//!   with `{fromDate, toDate}` (UTC dates) answers
//!   `{"items":[{"tool","tariff","topup"}]}`, the AI credits consumed from the
//!   built-in quota (`tariff`) and from top-up credits (`topup`) per tool.
//!
//! The service account is the organization's own integration credential
//! (client ID and secret), never a person's password. The token lives only in
//! this process's memory, keyed by an HMAC under a random per-process key, and
//! never reaches a note, a snapshot or the disk. No endpoint that runs AI
//! Assistant, Junie or another agent is called.

use super::{retry_after_seconds, send, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Status};
use chrono::{Datelike, NaiveDate, Utc};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const TOKEN_URL: &str = "https://oauth.account.jetbrains.com/api/rest/oauth2/token";
const API_BASE: &str = "https://api.jetbrains.cloud/metrics/api/v2";
const USAGE_PATH: &str = "/credits-usage/group-by/tool";
const SOURCE: &str = "https://api.jetbrains.cloud/metrics/api/v2/credits-usage/group-by/tool";
const DEFAULT_ID_ENV: &str = "JETBRAINS_CLIENT_ID";
const DEFAULT_SECRET_ENV: &str = "JETBRAINS_CLIENT_SECRET";
const CREDITS_UNIT: &str = "créditos";
const PERIOD: &str = "mês corrente";
/// Documented lifetime, used when the answer omits `expires_in`.
const DEFAULT_LIFETIME: u64 = 3600;
/// A cached token is renewed this long before it expires.
const RENEW_MARGIN: u64 = 60;

/// In-memory tokens per (token URL, client ID, secret) fingerprint.
static TOKENS: LazyLock<tokio::sync::Mutex<HashMap<String, (String, Instant)>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

/// A random key drawn once per process for [`fingerprint`].
static FINGERPRINT_KEY: LazyLock<[u8; 32]> = LazyLock::new(|| {
    let mut key = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut key);
    key
});

/// The cache key: an HMAC-SHA256 under a random key of this process, so the
/// map never holds the client secret or a plain hash of it.
fn fingerprint(token_url: &str, id: &str, secret: &str) -> String {
    use hmac::{Hmac, Mac as _};
    let mut mac = Hmac::<Sha256>::new_from_slice(FINGERPRINT_KEY.as_slice())
        .expect("HMAC takes a key of any length");
    for part in [token_url, id, secret] {
        mac.update(part.as_bytes());
        mac.update(&[0]);
    }
    let tag = mac.finalize().into_bytes();
    tag.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    fetch_at(account, ctx, TOKEN_URL, API_BASE, Utc::now().date_naive()).await
}

struct Client<'a> {
    id: String,
    secret: String,
    token_url: &'a str,
    key: String,
}

fn client<'a>(
    account: &Account,
    ctx: &Context<'_>,
    token_url: &'a str,
) -> Result<Client<'a>, QueryError> {
    let id = super::secret(account, ctx, DEFAULT_ID_ENV)?;
    let secret_env = account
        .params
        .get("client_secret_env")
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_SECRET_ENV);
    let secret = ctx
        .credentials
        .get(secret_env)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            QueryError::config_params(
                "query.jetbrains_ai.missing_secret",
                params([("env", ParamValue::from(secret_env.to_string()))]),
                "Missing credential: {env} (the service account's client secret).",
            )
        })?;
    let (id, secret) = (id.trim().to_string(), secret.trim().to_string());
    let key = fingerprint(token_url, &id, &secret);
    Ok(Client {
        id,
        secret,
        token_url,
        key,
    })
}

async fn fetch_at(
    account: &Account,
    ctx: &Context<'_>,
    token_url: &str,
    api_base: &str,
    today: NaiveDate,
) -> Result<Data, QueryError> {
    let client = client(account, ctx, token_url)?;
    let url = format!("{}{USAGE_PATH}", api_base.trim_end_matches('/'));
    let first = today.with_day(1).unwrap_or(today);
    let body = json!({
        "fromDate": first.format("%Y-%m-%d").to_string(),
        "toDate": today.format("%Y-%m-%d").to_string(),
    });

    let (token, cached) = token(&client, ctx).await?;
    let mut response = usage(ctx, &url, &token, &body).await?;
    if response.status().as_u16() == 401 && cached {
        // The cached token was revoked or rotated: exchange once more.
        TOKENS.lock().await.remove(&client.key);
        let (token, _) = token_fresh(&client, ctx).await?;
        response = usage(ctx, &url, &token, &body).await?;
    }
    let code = response.status();
    if !code.is_success() {
        if code.as_u16() == 401 {
            TOKENS.lock().await.remove(&client.key);
        }
        let retry = retry_after_seconds(response.headers());
        return Err(classify_usage(code.as_u16()).with_retry_after(retry));
    }
    let value = super::body_value(response).await?;
    parse(&value)
}

async fn usage(
    ctx: &Context<'_>,
    url: &str,
    token: &str,
    body: &Value,
) -> Result<reqwest::Response, QueryError> {
    send(
        ctx.http
            .post(url)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .json(body)
            .timeout(ctx.timeout),
    )
    .await
}

/// A cached token when one is still valid (`true`), or a fresh one.
async fn token(client: &Client<'_>, ctx: &Context<'_>) -> Result<(String, bool), QueryError> {
    if let Some((token, until)) = TOKENS.lock().await.get(&client.key) {
        if Instant::now() < *until {
            return Ok((token.clone(), true));
        }
    }
    token_fresh(client, ctx).await
}

async fn token_fresh(client: &Client<'_>, ctx: &Context<'_>) -> Result<(String, bool), QueryError> {
    let response = send(
        ctx.http
            .post(client.token_url)
            .basic_auth(&client.id, Some(&client.secret))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body("grant_type=client_credentials&scope=jcp-public")
            .timeout(ctx.timeout),
    )
    .await?;
    let code = response.status();
    if !code.is_success() {
        let retry = retry_after_seconds(response.headers());
        return Err(classify_token(code.as_u16()).with_retry_after(retry));
    }
    let value = super::body_value(response).await?;
    let token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| QueryError::schema_at(&value, &["access_token"], "string"))?
        .to_string();
    let lifetime = value
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_LIFETIME);
    let until = Instant::now() + Duration::from_secs(lifetime.saturating_sub(RENEW_MARGIN));
    TOKENS
        .lock()
        .await
        .insert(client.key.clone(), (token.clone(), until));
    Ok((token, false))
}

fn failure(status: Status, id: &'static str, text: &'static str) -> QueryError {
    QueryError {
        status,
        note: Localized::fixed(id, text),
        diagnostic: None,
    }
}

fn http(code: u16) -> QueryError {
    QueryError {
        status: Status::Error,
        note: Localized::with_params(
            "query.http",
            params([("code", ParamValue::from(code))]),
            "Query returned HTTP {code}. Check access and permissions.",
        ),
        diagnostic: None,
    }
}

fn rate_limited() -> QueryError {
    failure(
        Status::RateLimited,
        "note.jetbrains_ai.rate_limited",
        "JetBrains rate-limited the queries; the interval will be increased automatically.",
    )
}

/// Token endpoint answers (RF4): the OAuth error body is never read.
fn classify_token(code: u16) -> QueryError {
    match code {
        400 | 401 => failure(
            Status::AuthenticationError,
            "note.jetbrains_ai.client_rejected",
            "JetBrains rejected the service account client ID or secret; check both variables or rotate the secret in the JetBrains Central Console.",
        ),
        429 => rate_limited(),
        _ => http(code),
    }
}

/// Analytics API answers (RF4).
fn classify_usage(code: u16) -> QueryError {
    match code {
        401 => failure(
            Status::AuthenticationError,
            "note.jetbrains_ai.token_rejected",
            "The Analytics API refused the service account token; check the service account in the JetBrains Central Console.",
        ),
        403 => failure(
            Status::AuthenticationError,
            "note.jetbrains_ai.forbidden",
            "The service account may not read AI analytics: give it a role with the View AI analytics permission (Business or Enterprise plan).",
        ),
        429 => rate_limited(),
        400 | 422 => QueryError::schema(),
        _ => http(code),
    }
}

/// Display name of a documented tool id; an unknown, well-formed id stays as
/// it is, anything else is `None`.
fn tool_name(id: &str) -> Option<String> {
    let known = match id {
        "aia" => Some("AI Assistant"),
        "junie" => Some("Junie"),
        "claude_agent" => Some("Claude Agent"),
        "codex_agent" => Some("Codex"),
        "gemini_agent" => Some("Gemini CLI"),
        "opencode_agent" => Some("OpenCode"),
        "pi" => Some("Pi"),
        "copilot" => Some("GitHub Copilot"),
        "other" => Some("Other"),
        _ => None,
    };
    if let Some(name) = known {
        return Some(name.to_string());
    }
    let valid = (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    valid.then(|| id.to_string())
}

fn credits(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64().filter(|v| v.is_finite() && *v >= 0.0),
        _ => None,
    }
}

fn metric(label: String, used: f64) -> Metric {
    Metric {
        label,
        used: Some(used),
        limit: None,
        remaining: None,
        unit: CREDITS_UNIT.into(),
        kind: MetricKind::Usage,
        window: Some(MetricWindow::Monthly),
        period: Some(PERIOD.into()),
        resets_at: None,
        billing: Default::default(),
        scope: None,
    }
}

fn parse(value: &Value) -> Result<Data, QueryError> {
    let body = super::object_of(value)?;
    let items = body
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| QueryError::schema_at(value, &["items"], "array"))?;

    let mut notes = Notes::new();
    notes.push(
        "note.jetbrains_ai.source",
        "AI credits consumed by the organization this month (UTC), read from the JetBrains Analytics API with the service account; reading spends no credits.",
    );
    if items.is_empty() {
        notes.push(
            "note.jetbrains_ai.no_usage",
            "JetBrains reported no AI credit consumption this month.",
        );
        return Ok(Data::new(Vec::new(), notes, SOURCE));
    }

    let mut tools = Vec::new();
    let (mut tariff_total, mut topup_total) = (Some(0.0), Some(0.0));
    let (mut partial, mut skipped) = (false, false);
    for item in items {
        let tool = item.get("tool").and_then(Value::as_str).and_then(tool_name);
        let tariff = credits(item.get("tariff"));
        let topup = credits(item.get("topup"));
        tariff_total = tariff_total.zip(tariff).map(|(a, b)| a + b);
        topup_total = topup_total.zip(topup).map(|(a, b)| a + b);
        let used = match (tariff, topup) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(v), None) | (None, Some(v)) => {
                partial = true;
                Some(v)
            }
            (None, None) => None,
        };
        match (tool, used) {
            (Some(tool), Some(used)) => {
                tools.push(metric(format!("Créditos de IA — {tool}"), used))
            }
            _ => skipped = true,
        }
    }

    let mut metrics = Vec::new();
    match tariff_total {
        Some(total) => metrics.push(metric(
            "Créditos de IA da organização (cota incluída)".into(),
            total,
        )),
        None => notes.push(
            "note.jetbrains_ai.tariff_total_missing",
            "A tool came without its built-in quota consumption; the organization total for the built-in quota was omitted.",
        ),
    }
    match topup_total {
        Some(total) => metrics.push(metric(
            "Créditos de IA da organização (top-up)".into(),
            total,
        )),
        None => notes.push(
            "note.jetbrains_ai.topup_total_missing",
            "A tool came without its top-up consumption; the organization top-up total was omitted.",
        ),
    }
    metrics.extend(tools);
    if partial {
        notes.push(
            "note.jetbrains_ai.tool_partial",
            "Some per-tool values are partial: JetBrains reported only the built-in quota or only the top-up part.",
        );
    }
    if skipped {
        notes.push(
            "note.jetbrains_ai.tool_skipped",
            "Some tools were left out because their name or values were unreadable.",
        );
    }
    notes.push(
        "note.jetbrains_ai.no_limit",
        "The Analytics API reports consumption only: the organization's credit pool and each member's 30-day quota are not exposed.",
    );
    Ok(Data::new(metrics, notes, SOURCE))
}

/// PRD 0028 RF11: the fixture whose metric identities `label_snapshot`
/// records. Add a case when the adapter learns a new metric.
#[cfg(test)]
pub(super) fn label_fixtures() -> Vec<(&'static str, Vec<Metric>)> {
    let body = json!({"items": [{"tool": "junie", "tariff": 1.5, "topup": 0.5}]});
    let data = parse(&body).expect("fixture decodes");
    vec![("jetbrains_ai", data.metrics)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Provider};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{body_json, body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ID: &str = "placeholder-client-id";
    const SECRET: &str = "placeholder-client-secret";
    const TOKEN: &str = "placeholder-access-token";

    struct TestCredentials {
        secret: Option<&'static str>,
    }
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            match name {
                "TEST_JB_ID" => Some(ID.into()),
                "TEST_JB_SECRET" => self.secret.map(Into::into),
                _ => None,
            }
        }
    }

    fn account() -> Account {
        let mut params = BTreeMap::new();
        params.insert("client_secret_env".into(), "TEST_JB_SECRET".into());
        Account {
            id: None,
            name: "JetBrains AI".into(),
            provider: Provider::JetbrainsAi,
            credential_env: Some("TEST_JB_ID".into()),
            session: None,
            oauth: None,
            params,
            refresh_seconds: None,
            group: None,
        }
    }

    fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, 17).unwrap()
    }

    fn basic() -> String {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{ID}:{SECRET}"));
        format!("Basic {encoded}")
    }

    async fn mount_token(server: &MockServer, response: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(header("authorization", basic().as_str()))
            .and(header("content-type", "application/x-www-form-urlencoded"))
            .and(body_string(
                "grant_type=client_credentials&scope=jcp-public",
            ))
            .respond_with(response)
            .mount(server)
            .await;
    }

    fn token_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": TOKEN, "token_type": "Bearer",
            "expires_in": 3600, "scope": "jcp-public"
        }))
    }

    async fn mount_usage(server: &MockServer, response: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(format!("/metrics/api/v2{USAGE_PATH}")))
            .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
            .and(body_json(
                json!({"fromDate": "2026-03-01", "toDate": "2026-03-17"}),
            ))
            .respond_with(response)
            .mount(server)
            .await;
    }

    async fn call(server: &MockServer, creds: &TestCredentials) -> Result<Data, QueryError> {
        let http = http_client();
        let ctx = Context {
            http: &http,
            credentials: creds,
            timeout: Duration::from_secs(5),
        };
        let token_url = format!("{}/token", server.uri());
        let base = format!("{}/metrics/api/v2", server.uri());
        let result = fetch_at(&account(), &ctx, &token_url, &base, today()).await;
        // Each test runs on its own port, but forget the token anyway.
        TOKENS
            .lock()
            .await
            .remove(&fingerprint(&token_url, ID, SECRET));
        result
    }

    async fn run(usage_response: ResponseTemplate) -> Result<Data, QueryError> {
        let server = MockServer::start().await;
        mount_token(&server, token_ok()).await;
        mount_usage(&server, usage_response).await;
        call(
            &server,
            &TestCredentials {
                secret: Some(SECRET),
            },
        )
        .await
    }

    fn find<'a>(data: &'a Data, label: &str) -> Option<&'a Metric> {
        data.metrics.iter().find(|m| m.label == label)
    }

    fn has_note(data: &Data, id: &str) -> bool {
        data.notes_l10n.iter().any(|n| n.id == id)
    }

    fn assert_free_of_secrets(text: &str) {
        for (index, needle) in [ID, SECRET, TOKEN, "jetbrains.cloud", "oauth.account"]
            .into_iter()
            .enumerate()
        {
            // The message names only the position, never the placeholder value.
            assert!(
                !text.contains(needle),
                "placeholder #{index} leaked into a note"
            );
        }
    }

    #[tokio::test]
    async fn documented_example_becomes_totals_and_tools() {
        let data = run(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                {"tool": "aia", "tariff": 288.47, "topup": 381.28},
                {"tool": "claude_agent", "tariff": 748.72, "topup": 790.19},
                {"tool": "junie", "tariff": 142.99, "topup": 231.72}
            ]
        })))
        .await
        .expect("ok");
        let tariff = find(&data, "Créditos de IA da organização (cota incluída)").unwrap();
        assert!((tariff.used.unwrap() - 1180.18).abs() < 1e-6);
        let topup = find(&data, "Créditos de IA da organização (top-up)").unwrap();
        assert!((topup.used.unwrap() - 1403.19).abs() < 1e-6);
        let junie = find(&data, "Créditos de IA — Junie").unwrap();
        assert!((junie.used.unwrap() - 374.71).abs() < 1e-6);
        assert_eq!(junie.limit, None);
        assert_eq!(junie.remaining, None);
        assert_eq!(junie.window, Some(MetricWindow::Monthly));
        assert_eq!(junie.unit, "créditos");
        assert!(find(&data, "Créditos de IA — AI Assistant").is_some());
        assert!(find(&data, "Créditos de IA — Claude Agent").is_some());
        assert_eq!(data.metrics.len(), 5);
        assert_eq!(data.source, SOURCE);
        assert!(!has_note(&data, "note.jetbrains_ai.tool_partial"));
        for note in &data.notes {
            assert_free_of_secrets(note);
        }
    }

    #[tokio::test]
    async fn empty_items_emit_no_metric_and_never_zero() {
        let data = run(ResponseTemplate::new(200).set_body_json(json!({"items": []})))
            .await
            .expect("ok");
        assert!(data.metrics.is_empty());
        assert!(has_note(&data, "note.jetbrains_ai.no_usage"));
    }

    #[tokio::test]
    async fn partial_values_omit_the_total_and_say_so() {
        let data = run(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                {"tool": "junie", "tariff": 2.0},
                {"tool": "aia", "tariff": 1.0, "topup": null},
                {"tool": "mystery_agent", "tariff": 0.5, "topup": 0.25},
                {"tool": "Bad Name!", "tariff": 9.0, "topup": 9.0},
                {"tool": "copilot", "tariff": -1.0, "topup": "x"}
            ]
        })))
        .await
        .expect("ok");
        let tariff = find(&data, "Créditos de IA da organização (cota incluída)");
        assert!(tariff.is_none(), "copilot's tariff is unreadable");
        assert!(find(&data, "Créditos de IA da organização (top-up)").is_none());
        assert!(has_note(&data, "note.jetbrains_ai.tariff_total_missing"));
        assert!(has_note(&data, "note.jetbrains_ai.topup_total_missing"));
        assert!(has_note(&data, "note.jetbrains_ai.tool_partial"));
        assert!(has_note(&data, "note.jetbrains_ai.tool_skipped"));
        assert_eq!(
            find(&data, "Créditos de IA — Junie").unwrap().used,
            Some(2.0)
        );
        assert_eq!(
            find(&data, "Créditos de IA — mystery_agent").unwrap().used,
            Some(0.75)
        );
        assert!(find(&data, "Créditos de IA — GitHub Copilot").is_none());
        assert!(data.metrics.iter().all(|m| !m.label.contains("Bad Name")));
    }

    #[tokio::test]
    async fn explicit_zero_is_a_real_zero() {
        let data = run(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"tool": "junie", "tariff": 0, "topup": 0}]
        })))
        .await
        .expect("ok");
        assert_eq!(
            find(&data, "Créditos de IA — Junie").unwrap().used,
            Some(0.0)
        );
    }

    #[tokio::test]
    async fn schema_drift_is_a_schema_error() {
        for body in [json!({"data": []}), json!([1, 2]), json!({"items": {}})] {
            let error = run(ResponseTemplate::new(200).set_body_json(body))
                .await
                .unwrap_err();
            assert_eq!(error.status, Status::Error);
            assert_eq!(error.note.id, super::super::SCHEMA_NOTE);
        }
    }

    #[tokio::test]
    async fn usage_errors_are_classified_without_details() {
        for (code, status, id) in [
            (
                401,
                Status::AuthenticationError,
                "note.jetbrains_ai.token_rejected",
            ),
            (
                403,
                Status::AuthenticationError,
                "note.jetbrains_ai.forbidden",
            ),
            (429, Status::RateLimited, "note.jetbrains_ai.rate_limited"),
            (500, Status::Error, "query.http"),
        ] {
            let error = run(ResponseTemplate::new(code)
                .insert_header("retry-after", "120")
                .set_body_string(format!("secret {SECRET} at https://api.jetbrains.cloud")))
            .await
            .unwrap_err();
            assert_eq!(error.status, status, "{code}");
            assert_eq!(error.note.id, id, "{code}");
            assert_free_of_secrets(error.note.text());
            if code == 429 {
                assert!(error
                    .note
                    .params
                    .contains_key(crate::Report::RETRY_AFTER_PARAM));
            }
        }
    }

    #[tokio::test]
    async fn token_errors_are_authentication_errors() {
        for code in [400, 401] {
            let server = MockServer::start().await;
            mount_token(
                &server,
                ResponseTemplate::new(code).set_body_json(json!({
                    "error": "invalid_client", "error_description": SECRET
                })),
            )
            .await;
            let error = call(
                &server,
                &TestCredentials {
                    secret: Some(SECRET),
                },
            )
            .await
            .unwrap_err();
            assert_eq!(error.status, Status::AuthenticationError);
            assert_eq!(error.note.id, "note.jetbrains_ai.client_rejected");
            assert_free_of_secrets(error.note.text());
        }
    }

    #[tokio::test]
    async fn token_without_access_token_is_a_schema_error() {
        let server = MockServer::start().await;
        mount_token(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({"token_type": "Bearer"})),
        )
        .await;
        let error = call(
            &server,
            &TestCredentials {
                secret: Some(SECRET),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.note.id, super::super::SCHEMA_NOTE);
    }

    #[tokio::test]
    async fn missing_secret_is_a_configuration_error() {
        let server = MockServer::start().await;
        let error = call(&server, &TestCredentials { secret: None })
            .await
            .unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
        assert_eq!(error.note.id, "query.jetbrains_ai.missing_secret");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_client_id_is_a_configuration_error() {
        let server = MockServer::start().await;
        let mut account = account();
        account.credential_env = Some("UNSET_JB_ID".into());
        let http = http_client();
        let creds = TestCredentials {
            secret: Some(SECRET),
        };
        let ctx = Context {
            http: &http,
            credentials: &creds,
            timeout: Duration::from_secs(5),
        };
        let error = fetch_at(&account, &ctx, &server.uri(), &server.uri(), today())
            .await
            .unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
    }

    #[tokio::test]
    async fn token_is_cached_and_renewed_after_a_401() {
        let server = MockServer::start().await;
        let exchanges = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = exchanges.clone();
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(move |_: &wiremock::Request| {
                counter.fetch_add(1, Ordering::SeqCst);
                token_ok()
            })
            .mount(&server)
            .await;
        let usage = std::sync::Arc::new(AtomicUsize::new(0));
        let calls = usage.clone();
        Mock::given(method("POST"))
            .and(path(format!("/metrics/api/v2{USAGE_PATH}")))
            .respond_with(move |_: &wiremock::Request| {
                // The third usage call (second refresh) sees a revoked token.
                if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                    ResponseTemplate::new(401)
                } else {
                    ResponseTemplate::new(200).set_body_json(json!({"items": []}))
                }
            })
            .mount(&server)
            .await;

        let http = http_client();
        let creds = TestCredentials {
            secret: Some(SECRET),
        };
        let ctx = Context {
            http: &http,
            credentials: &creds,
            timeout: Duration::from_secs(5),
        };
        let token_url = format!("{}/token", server.uri());
        let base = format!("{}/metrics/api/v2", server.uri());
        fetch_at(&account(), &ctx, &token_url, &base, today())
            .await
            .expect("first refresh");
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        fetch_at(&account(), &ctx, &token_url, &base, today())
            .await
            .expect("second refresh retries with a fresh token");
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        assert_eq!(usage.load(Ordering::SeqCst), 3);
        fetch_at(&account(), &ctx, &token_url, &base, today())
            .await
            .expect("third refresh reuses the new token");
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        TOKENS
            .lock()
            .await
            .remove(&fingerprint(&token_url, ID, SECRET));
    }

    #[test]
    fn tool_names_map_and_validate() {
        assert_eq!(tool_name("aia").as_deref(), Some("AI Assistant"));
        assert_eq!(tool_name("junie").as_deref(), Some("Junie"));
        assert_eq!(tool_name("new_agent-2").as_deref(), Some("new_agent-2"));
        assert_eq!(tool_name(""), None);
        assert_eq!(tool_name("Has Space"), None);
        assert_eq!(tool_name(&"a".repeat(41)), None);
    }

    #[test]
    fn fingerprint_hides_the_secret() {
        let key = fingerprint("u", ID, SECRET);
        assert!(!key.contains(SECRET));
        assert_ne!(key, fingerprint("u", ID, "other"));
    }
}
