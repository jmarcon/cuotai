//! Read-only Meta Model API (Muse models) key check (PRD 0049).
//!
//! Meta documents no usage, spend, credit or balance endpoint for the
//! Model API (<https://dev.meta.ai/docs/pricing-rate-limits>); usage is only
//! shown in the dev.meta.ai console. The adapter therefore makes one
//! documented, token-free read:
//!
//! - `GET https://api.meta.ai/v1/models` with `Authorization: Bearer <key>`
//!   (<https://dev.meta.ai/docs/api-reference/models/list-models>);
//! - the four documented `x-ratelimit-{limit,remaining}-{requests,tokens}`
//!   headers become rate-limit metrics when the response carries them. Meta
//!   documents them on token-based responses, so the model listing may omit
//!   them: an absent header is "unknown", never zero.
//!
//! Limits are per team, shared by every key of the team. No inference,
//! purchase or billing endpoint is called, and the body is read only to check
//! its shape: model ids are never copied into notes or metrics.

use super::{secret, send, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Status};
use serde_json::Value;

pub(crate) const META_MODELS_URL: &str = "https://api.meta.ai/v1/models";
/// Largest model listing read; a bigger body is a schema error.
const BODY_LIMIT: usize = 1024 * 1024;

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    meta_at(account, ctx, META_MODELS_URL).await
}

pub(crate) async fn meta_at(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "META_API_KEY")?;
    let request = ctx
        .http
        .get(url)
        .bearer_auth(key.trim())
        .header("Accept", "application/json")
        .timeout(ctx.timeout);
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
                "query.meta.http",
                params([("code", ParamValue::from(code.as_u16()))]),
                "Meta Model API returned HTTP {code}.",
            ),
        }
        .with_retry_after(retry_after(response.headers())));
    }
    let headers = response.headers().clone();
    let bytes = response
        .bytes()
        .await
        .map_err(|_| QueryError::transport())?;
    if bytes.len() > BODY_LIMIT {
        return Err(QueryError::schema());
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| QueryError::schema())?;
    if !body.get("data").is_some_and(Value::is_array) {
        return Err(QueryError::schema_at(&body, &["data"], "array"));
    }

    let mut metrics = Vec::new();
    for (label, unit, limit_h, remaining_h) in [
        (
            "Requisições",
            "requisições",
            "x-ratelimit-limit-requests",
            "x-ratelimit-remaining-requests",
        ),
        (
            "Tokens",
            "tokens",
            "x-ratelimit-limit-tokens",
            "x-ratelimit-remaining-tokens",
        ),
    ] {
        let (Some(limit), Some(remaining)) = (
            header_number(&headers, limit_h),
            header_number(&headers, remaining_h),
        ) else {
            continue;
        };
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label: format!("{label} (rate limit)"),
            used: Some((limit - remaining).max(0.0)),
            limit: Some(limit),
            remaining: Some(remaining.max(0.0)),
            unit: unit.into(),
            kind: MetricKind::RateLimit,
            window: Some(MetricWindow::Other),
            period: Some("janela corrente do rate limit".into()),
            resets_at: None,
        });
    }

    let mut notes = Notes::new();
    notes.push(
        "note.meta.key_valid",
        "Key accepted by the Meta Model API model listing; no tokens are used.",
    );
    if metrics.is_empty() {
        notes.push(
            "note.meta.rate_limit_headers_missing",
            "This response carried no `x-ratelimit-*` headers; the team's rate-limit window is unknown.",
        );
    } else {
        notes.push(
            "note.meta.rate_limit_team",
            "Rate limits are per team and shared by every key of the team.",
        );
    }
    notes.push(
        "note.meta.no_usage_api",
        "Meta documents no usage, spend or balance API; see usage and billing in the dev.meta.ai console.",
    );
    Ok(Data::new(metrics, notes, META_MODELS_URL))
}

fn header_number(headers: &reqwest::header::HeaderMap, name: &str) -> Option<f64> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Provider};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Key(&'static str);
    impl Credentials for Key {
        fn get(&self, name: &str) -> Option<String> {
            (name == "META_API_KEY").then(|| self.0.to_string())
        }
    }

    fn account() -> Account {
        Account {
            id: None,
            name: "muse".into(),
            provider: Provider::Meta,
            credential_env: Some("META_API_KEY".into()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    async fn run(template: ResponseTemplate) -> Result<Data, QueryError> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(template)
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let creds = Key("test-key");
        let ctx = Context {
            http: &http,
            credentials: &creds,
            timeout: Duration::from_secs(5),
        };
        meta_at(&account(), &ctx, &format!("{}/v1/models", server.uri())).await
    }

    fn models() -> serde_json::Value {
        serde_json::json!({ "object": "list", "data": [{ "id": "example-model" }] })
    }

    #[tokio::test]
    async fn rate_limit_headers_become_metrics() {
        let data = run(ResponseTemplate::new(200)
            .set_body_json(models())
            .insert_header("x-ratelimit-limit-requests", "3000")
            .insert_header("x-ratelimit-remaining-requests", "2990")
            .insert_header("x-ratelimit-limit-tokens", "1000000")
            .insert_header("x-ratelimit-remaining-tokens", "999000"))
        .await
        .expect("valid");
        assert_eq!(data.metrics.len(), 2);
        assert!(data.metrics.iter().all(|m| m.kind == MetricKind::RateLimit));
        assert_eq!(data.metrics[0].used, Some(10.0));
        assert_eq!(data.metrics[0].limit, Some(3000.0));
        assert_eq!(data.metrics[1].remaining, Some(999_000.0));
        assert!(data.notes.iter().all(|n| !n.contains("example-model")));
    }

    #[tokio::test]
    async fn absent_headers_are_unknown_not_zero() {
        let data = run(ResponseTemplate::new(200).set_body_json(models()))
            .await
            .expect("valid key");
        assert!(data.metrics.is_empty());
        assert!(data.notes.iter().any(|n| n.contains("x-ratelimit")));
    }

    #[tokio::test]
    async fn partial_headers_skip_the_bucket() {
        let data = run(ResponseTemplate::new(200)
            .set_body_json(models())
            .insert_header("x-ratelimit-limit-requests", "3000")
            .insert_header("x-ratelimit-limit-tokens", "1000")
            .insert_header("x-ratelimit-remaining-tokens", "abc"))
        .await
        .expect("valid key");
        assert!(data.metrics.is_empty());
    }

    #[tokio::test]
    async fn unauthorized_is_an_authentication_error() {
        for code in [401, 403] {
            let err = run(ResponseTemplate::new(code).set_body_string("{\"error\":\"secret\"}"))
                .await
                .expect_err("rejected");
            assert_eq!(err.status, Status::AuthenticationError);
            assert!(!err.note.text().contains("secret"));
        }
    }

    #[tokio::test]
    async fn too_many_requests_is_rate_limited() {
        let err = run(ResponseTemplate::new(429).insert_header("retry-after", "30"))
            .await
            .expect_err("429");
        assert_eq!(err.status, Status::RateLimited);
    }

    #[tokio::test]
    async fn unexpected_body_is_a_schema_error() {
        for body in ["not json", "{\"models\":[]}", "{\"data\":{}}"] {
            let err = run(ResponseTemplate::new(200).set_body_string(body))
                .await
                .expect_err("schema");
            assert_eq!(err.status, Status::Error, "{body}");
        }
    }

    #[tokio::test]
    async fn missing_key_makes_no_request() {
        let http = reqwest::Client::new();
        struct None_;
        impl Credentials for None_ {
            fn get(&self, _: &str) -> Option<String> {
                None
            }
        }
        let ctx = Context {
            http: &http,
            credentials: &None_,
            timeout: Duration::from_secs(1),
        };
        let err = meta_at(&account(), &ctx, "https://127.0.0.1:1/v1/models")
            .await
            .expect_err("no key");
        assert_ne!(err.status, Status::Ready);
    }
}
