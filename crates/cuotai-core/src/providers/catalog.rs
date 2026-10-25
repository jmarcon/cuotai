//! Live model listings for the providers whose catalog is not a plain inference-key `/models`.
//!
//! Every adapter is read-only and uses the credential the account already stores (the same one
//! its quota query uses). Endpoints that are not documented for that credential come from
//! public open-source clients (the vendors' own CLIs, 9router) and are listed in
//! `docs/providers.md`; a response the credential is not accepted for surfaces as the usual
//! classified failure (authentication, rate limit, transport, schema), never as an invented list.
//! Bodies, URLs and tokens never reach errors.

use super::{json, param, secret, Context, QueryError};
use crate::models::ModelInfo;
use crate::text::{params, ParamValue};
use crate::Account;
use reqwest::RequestBuilder;
use serde_json::Value;

pub(crate) const OPENAI_BASE: &str = "https://api.openai.com";
pub(crate) const MISTRAL_BASE: &str = "https://api.mistral.ai";
pub(crate) const PERPLEXITY_BASE: &str = "https://api.perplexity.ai";
pub(crate) const NVIDIA_BASE: &str = "https://integrate.api.nvidia.com";
pub(crate) const XAI_BASE: &str = "https://api.x.ai";
pub(crate) const GROK_CLI_BASE: &str = "https://cli-chat-proxy.grok.com";
pub(crate) const CLINE_BASE: &str = "https://api.cline.bot";
pub(crate) const MINIMAX_BASE: &str = "https://api.minimax.io";
pub(crate) const MINIMAX_CN_BASE: &str = "https://api.minimaxi.com";
pub(crate) const OPENCODE_GO_BASE: &str = "https://opencode.ai/zen/go";
pub(crate) const OLLAMA_BASE: &str = "https://ollama.com";
pub(crate) const KIMI_CODING_BASE: &str = "https://api.kimi.com/coding";
pub(crate) const CURSOR_BASE: &str = "https://api2.cursor.sh";
pub(crate) const ZED_BASE: &str = "https://cloud.zed.dev";
pub(crate) const CODING_PLAN_INTL: &str = "https://coding-intl.dashscope.aliyuncs.com";
pub(crate) const CODING_PLAN_CN: &str = "https://coding.dashscope.aliyuncs.com";
pub(crate) const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com";
pub(crate) const VERTEX_BASE: &str = "https://aiplatform.googleapis.com";
pub(crate) const AZURE_BASE: &str = "https://management.azure.com";
pub(crate) const CLOUDFLARE_BASE: &str = "https://api.cloudflare.com";

fn text(item: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| item.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// One list entry: a bare string id or an object with the first present id key.
fn entry(item: &Value) -> Result<ModelInfo, QueryError> {
    if let Some(id) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(ModelInfo {
            id: id.to_owned(),
            name: None,
            description: None,
        });
    }
    let id =
        text(item, &["id", "slug", "modelId", "model", "name"]).ok_or_else(QueryError::schema)?;
    let name =
        text(item, &["display_name", "displayName", "modelName", "name"]).filter(|n| *n != id);
    Ok(ModelInfo {
        id,
        name,
        description: text(item, &["description"]),
    })
}

/// Entries of a list body: a root array or one under `data`/`models`/`results`.
/// Any entry without an id rejects the whole response.
pub(crate) fn collect(body: &Value) -> Result<Vec<ModelInfo>, QueryError> {
    let items = match body {
        Value::Array(items) => items,
        Value::Object(map) => ["data", "models", "results"]
            .iter()
            .find_map(|k| map.get(*k).and_then(Value::as_array))
            .ok_or_else(QueryError::schema)?,
        _ => return Err(QueryError::schema()),
    };
    items.iter().map(entry).collect()
}

async fn list(request: RequestBuilder, ctx: &Context<'_>) -> Result<Vec<ModelInfo>, QueryError> {
    let body: Value = json(request.timeout(ctx.timeout)).await?;
    collect(&body)
}

fn get(ctx: &Context<'_>, url: String, key: &str) -> RequestBuilder {
    ctx.http
        .get(url)
        .bearer_auth(key.trim())
        .header("Accept", "application/json")
}

// ----------------------------------------------------------- inference-key APIs

/// OpenAI-shaped `GET {base}/v1/models` with a Bearer key. The account may store an
/// administration key, which the provider can refuse: that is reported, not hidden.
pub(crate) async fn bearer_models(
    account: &Account,
    ctx: &Context<'_>,
    url: String,
    default_env: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let key = secret(account, ctx, default_env)?;
    list(get(ctx, url, &key), ctx).await
}

/// Anthropic `GET /v1/models` with an `x-api-key` key (the account may hold an Admin key).
pub(crate) async fn anthropic_key_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let key = secret(account, ctx, "ANTHROPIC_ADMIN_KEY")?;
    super::subscriptions::anthropic_models(ctx, base, move |request| {
        request.header("x-api-key", key.trim())
    })
    .await
}

/// Grok: the CLI session (`product = "cli"`, `session_env`) lists through the CLI proxy with the
/// same client headers its quota query sends; otherwise the account key goes to the xAI API.
pub(crate) async fn grok_models(
    account: &Account,
    ctx: &Context<'_>,
    cli_base: &str,
    api_base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    if let Some(env) = account.params.get("session_env") {
        let token = ctx
            .credentials
            .get(env)
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                QueryError::config_params(
                    "query.missing_credential",
                    params([("name", ParamValue::from(env.clone()))]),
                    format!("Credencial ausente: {env}"),
                )
            })?;
        let request = super::router_grok::grok_headers(
            ctx.http.get(format!("{cli_base}/models")),
            &token,
            account,
        );
        return list(request, ctx).await;
    }
    bearer_models(
        account,
        ctx,
        format!("{api_base}/v1/models"),
        "X_GROK_API_KEY",
    )
    .await
}

pub(crate) async fn cline_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    bearer_models(
        account,
        ctx,
        format!("{base}/api/v1/models"),
        "CLINE_API_KEY",
    )
    .await
}

pub(crate) async fn minimax_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    bearer_models(
        account,
        ctx,
        format!("{base}/v1/models"),
        "MINIMAX_IO_API_KEY",
    )
    .await
}

/// Kimi Code: `GET /coding/v1/models`, authenticated like the quota query (platform key as
/// `x-api-key`, or the OAuth access token as Bearer with the device headers).
pub(crate) async fn kimi_coding_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let url = format!("{base}/v1/models");
    let non_empty = |name: &str| ctx.credentials.get(name).filter(|s| !s.trim().is_empty());
    let api_key = non_empty(account.credential_env.as_deref().unwrap_or("KIMI_API_KEY"));
    let access = account
        .params
        .get("token_env")
        .and_then(|name| non_empty(name));
    let request = if let Some(key) = api_key {
        ctx.http.get(&url).header("x-api-key", key.trim())
    } else if let Some(token) = access {
        ctx.http
            .get(&url)
            .bearer_auth(token.trim())
            .header("X-Msh-Platform", "cuotai")
            .header("X-Msh-Version", env!("CARGO_PKG_VERSION"))
            .header("X-Msh-Device-Name", "cuotai-cli")
            .header(
                "X-Msh-Device-Model",
                format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            )
            .header(
                "X-Msh-Device-Id",
                account
                    .params
                    .get("device_id")
                    .filter(|v| !v.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| "cuotai".to_string()),
            )
    } else {
        // Same error the quota query raises for a missing credential.
        secret(account, ctx, "KIMI_API_KEY")?;
        return Err(QueryError::schema());
    };
    list(request.header("Accept", "application/json"), ctx).await
}

pub(crate) async fn alibaba_coding_plan_models(
    account: &Account,
    ctx: &Context<'_>,
    intl: &str,
    cn: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let base = match account.params.get("region").map(String::as_str) {
        Some("cn") => cn,
        _ => intl,
    };
    bearer_models(
        account,
        ctx,
        format!("{base}/v1/models"),
        "ALIBABA_CODING_PLAN_API_KEY",
    )
    .await
}

// ----------------------------------------------------------- session / OAuth tokens

/// Cursor: the Connect JSON form of the `GetUsableModels` RPC the editor calls, with the same
/// session token the personal quota query uses. Not documented by Cursor.
pub(crate) async fn cursor_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let token = secret(account, ctx, "CURSOR_ACCESS_TOKEN")?;
    let body: Value = json(
        ctx.http
            .post(format!("{base}/aiserver.v1.AiService/GetUsableModels"))
            .bearer_auth(token.trim())
            .header("Content-Type", "application/json")
            .header("connect-protocol-version", "1")
            .header("Accept", "application/json")
            .body("{}")
            .timeout(ctx.timeout),
    )
    .await?;
    let items = body
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(QueryError::schema)?;
    items
        .iter()
        .map(|m| {
            let id = text(m, &["modelId"]).ok_or_else(QueryError::schema)?;
            let name = text(m, &["displayName", "displayNameShort"]).filter(|n| *n != id);
            Ok(ModelInfo {
                id,
                name,
                description: None,
            })
        })
        .collect()
}

/// Kiro (AWS CodeWhisperer): `GET ListAvailableModels` on the host of the profile ARN's region,
/// with the same bearer token and auth-method headers as the quota query.
pub(crate) async fn kiro_models(
    account: &Account,
    ctx: &Context<'_>,
    host: Option<&str>,
) -> Result<Vec<ModelInfo>, QueryError> {
    let token = secret(account, ctx, "KIRO_ACCESS_TOKEN")?;
    let arn = account
        .params
        .get("profile_arn")
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty());
    let region = arn
        .and_then(|a| a.split(':').nth(3))
        .filter(|r| {
            !r.is_empty()
                && r.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
        .unwrap_or("us-east-1");
    let url = host.map_or_else(
        || format!("https://q.{region}.amazonaws.com/ListAvailableModels"),
        |h| format!("{h}/ListAvailableModels"),
    );
    let mut query = vec![("origin", "AI_EDITOR")];
    query.extend(arn.map(|a| ("profileArn", a)));
    let mut request = ctx
        .http
        .get(url)
        .query(&query)
        .bearer_auth(token.trim())
        .header("Accept", "application/json")
        .header("x-amzn-kiro-agent-mode", "vibe")
        .header("x-amz-user-agent", "aws-sdk-js/1.0.0 KiroIDE")
        .header("user-agent", "aws-sdk-js/1.0.0 KiroIDE");
    request = match account.params.get("auth_method").map(String::as_str) {
        Some("api_key") => request.header("tokentype", "API_KEY"),
        Some("external_idp") => request.header("TokenType", "EXTERNAL_IDP"),
        _ => request,
    };
    let body: Value = json(request.timeout(ctx.timeout)).await?;
    let items = body
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(QueryError::schema)?;
    items
        .iter()
        .map(|m| {
            let id = text(m, &["modelId"]).ok_or_else(QueryError::schema)?;
            Ok(ModelInfo {
                name: text(m, &["modelName"]).filter(|n| *n != id),
                id,
                description: text(m, &["description"]),
            })
        })
        .collect()
}

/// Zed: the account's organization, an LLM token for it, then the hosted `/models`.
pub(crate) async fn zed_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let token = secret(account, ctx, "ZED_ACCESS_TOKEN")?;
    let user_id = param(account, "user_id")?;
    let user_auth = format!("{user_id} {token}");
    let with_system = |request: RequestBuilder| match account
        .params
        .get("system_id")
        .filter(|s| !s.trim().is_empty())
    {
        Some(id) => request.header("x-zed-system-id", id),
        None => request,
    };
    // Entries are either a string or `[string]` / `{id}` depending on the server version.
    let id_of = |v: &Value| -> Option<String> {
        match v {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Array(a) => a.first().and_then(Value::as_str).map(str::to_owned),
            Value::Object(_) => text(v, &["id"]),
            _ => None,
        }
    };
    let me: Value = json(
        with_system(ctx.http.get(format!("{base}/client/users/me")))
            .header("Accept", "application/json")
            .header("Authorization", &user_auth)
            .timeout(ctx.timeout),
    )
    .await?;
    let organization = me
        .get("default_organization_id")
        .and_then(id_of)
        .or_else(|| {
            let orgs = me.get("organizations")?.as_array()?;
            let pick = orgs
                .iter()
                .find(|o| o.get("is_personal").and_then(Value::as_bool) == Some(true))
                .or_else(|| orgs.first())?;
            pick.get("id").and_then(id_of)
        })
        .ok_or_else(QueryError::schema)?;
    let llm: Value = json(
        with_system(ctx.http.post(format!("{base}/client/llm_tokens")))
            .header("Accept", "application/json")
            .header("Authorization", &user_auth)
            .json(&serde_json::json!({ "organization_id": organization }))
            .timeout(ctx.timeout),
    )
    .await?;
    let llm_token = llm
        .get("token")
        .and_then(|t| {
            id_of(t).or_else(|| t.get("value").and_then(Value::as_str).map(str::to_owned))
        })
        .ok_or_else(QueryError::schema)?;
    let body: Value = json(
        ctx.http
            .get(format!("{base}/models"))
            .bearer_auth(llm_token.trim())
            .header("Accept", "application/json")
            .header("x-zed-client-supports-x-ai", "true")
            .timeout(ctx.timeout),
    )
    .await?;
    let items = body
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(QueryError::schema)?;
    let mut models = Vec::new();
    for item in items {
        if item.get("is_disabled").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let id = item
            .get("id")
            .and_then(id_of)
            .ok_or_else(QueryError::schema)?;
        models.push(ModelInfo {
            name: text(item, &["display_name", "displayName"]).filter(|n| *n != id),
            id,
            description: None,
        });
    }
    Ok(models)
}

// ----------------------------------------------------------- cloud platforms

/// Gemini API (Generative Language) with the account's Google token: the Cloud Monitoring
/// scope the quota query needs usually is not accepted here, and that is what gets reported.
pub(crate) async fn gemini_api_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let token = secret(account, ctx, "GOOGLE_CLOUD_ACCESS_TOKEN")?;
    let body: Value =
        json(get(ctx, format!("{base}/v1beta/models?pageSize=1000"), &token).timeout(ctx.timeout))
            .await?;
    let items = body
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(QueryError::schema)?;
    items
        .iter()
        .map(|m| {
            let id = text(m, &["name"]).ok_or_else(QueryError::schema)?;
            Ok(ModelInfo {
                name: text(m, &["displayName"]),
                id,
                description: text(m, &["description"]),
            })
        })
        .collect()
}

/// Vertex AI publisher models with the account's Google token (`publishers/google`).
pub(crate) async fn vertex_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let token = secret(account, ctx, "GOOGLE_CLOUD_ACCESS_TOKEN")?;
    let body: Value = json(
        get(
            ctx,
            format!("{base}/v1beta1/publishers/google/models?pageSize=1000"),
            &token,
        )
        .timeout(ctx.timeout),
    )
    .await?;
    let items = body
        .get("publisherModels")
        .and_then(Value::as_array)
        .ok_or_else(QueryError::schema)?;
    items
        .iter()
        .map(|m| {
            let name = text(m, &["name"]).ok_or_else(QueryError::schema)?;
            let id = name.rsplit('/').next().unwrap_or(&name).to_owned();
            Ok(ModelInfo {
                id,
                name: None,
                description: None,
            })
        })
        .collect()
}

/// Azure OpenAI: the models the configured resource can deploy
/// (`Microsoft.CognitiveServices/accounts/{name}/models`), with the account's ARM token.
pub(crate) async fn azure_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let token = secret(account, ctx, "AZURE_ACCESS_TOKEN")?;
    let resource = param(account, "resource_id")?;
    let resource = resource.trim_matches('/');
    if resource
        .split('/')
        .any(|part| part.is_empty() || part == "..")
    {
        return Err(QueryError::schema());
    }
    #[derive(serde::Deserialize)]
    struct Page {
        value: Vec<Value>,
        #[serde(rename = "nextLink")]
        next_link: Option<String>,
    }
    let prefix = format!("{base}/");
    let mut url = format!("{base}/{resource}/models?api-version=2024-10-01");
    let mut seen = std::collections::HashSet::new();
    let mut models: Vec<ModelInfo> = Vec::new();
    for _ in 0..50 {
        let page: Page = json(get(ctx, url.clone(), &token).timeout(ctx.timeout)).await?;
        for item in &page.value {
            let name = item
                .pointer("/model/name")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(QueryError::schema)?;
            models.push(ModelInfo {
                id: name.to_owned(),
                name: None,
                description: None,
            });
        }
        match page.next_link.filter(|l| !l.is_empty()) {
            None => return Ok(models),
            // Never follow a link off the ARM host (the token would travel with it) or a repeat.
            Some(next) if !next.starts_with(&prefix) || !seen.insert(next.clone()) => {
                return Err(QueryError::schema())
            }
            Some(next) => url = next,
        }
    }
    Err(QueryError::schema())
}

/// Cloudflare Workers AI catalog of the account (`/accounts/{id}/ai/models/search`).
pub(crate) async fn cloudflare_models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<ModelInfo>, QueryError> {
    let token = secret(account, ctx, "CLOUDFLARE_API_TOKEN")?;
    let id = param(account, "account_id")?;
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(QueryError::schema());
    }
    let body: Value = json(
        get(
            ctx,
            format!("{base}/client/v4/accounts/{id}/ai/models/search?per_page=1000"),
            &token,
        )
        .timeout(ctx.timeout),
    )
    .await?;
    let items = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(QueryError::schema)?;
    items
        .iter()
        .map(|m| {
            let id = text(m, &["name", "id"]).ok_or_else(QueryError::schema)?;
            Ok(ModelInfo {
                id,
                name: None,
                description: text(m, &["description"]),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Creds;
    impl Credentials for Creds {
        fn get(&self, name: &str) -> Option<String> {
            (name == "T").then(|| "secret-token".to_string())
        }
    }

    fn account(provider: crate::Provider, params: &[(&str, &str)]) -> Account {
        Account {
            id: None,
            name: "a".into(),
            provider,
            credential_env: Some("T".into()),
            session: None,
            oauth: None,
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            refresh_seconds: None,
        }
    }

    fn ctx(http: &reqwest::Client) -> Context<'_> {
        Context {
            http,
            credentials: &Creds,
            timeout: Duration::from_secs(5),
        }
    }

    fn ids(models: &[ModelInfo]) -> Vec<&str> {
        models.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn collect_accepts_the_shapes_providers_use_and_rejects_the_rest() {
        let list = |v: Value| collect(&v);
        assert_eq!(
            ids(
                &list(serde_json::json!({"data":[{"id":"a"},{"id":"b","display_name":"B"}]}))
                    .unwrap()
            ),
            ["a", "b"]
        );
        assert_eq!(
            ids(&list(serde_json::json!({"models":[{"name":"x","model":"x"}]})).unwrap()),
            ["x"]
        );
        assert_eq!(
            ids(&list(serde_json::json!(["p", "q"])).unwrap()),
            ["p", "q"]
        );
        assert_eq!(
            list(serde_json::json!({"data":[{"id":"a"},{"object":"model"}]}))
                .unwrap_err()
                .note
                .id,
            "query.schema"
        );
        for bad in [
            serde_json::json!({}),
            serde_json::json!("x"),
            serde_json::json!({"data":"x"}),
        ] {
            assert!(list(bad).is_err());
        }
    }

    #[tokio::test]
    async fn bearer_models_use_the_account_key_and_classify_refusals() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer secret-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list", "data": [{"id": "gpt-x"}, {"id": "gpt-y"}]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let acc = account(crate::Provider::OpenAiApi, &[]);
        let models = bearer_models(
            &acc,
            &ctx(&http),
            format!("{}/v1/models", server.uri()),
            "X",
        )
        .await
        .unwrap();
        assert_eq!(ids(&models), ["gpt-x", "gpt-y"]);

        let denied = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("token-in-body"))
            .mount(&denied)
            .await;
        let err = bearer_models(
            &acc,
            &ctx(&http),
            format!("{}/v1/models", denied.uri()),
            "X",
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, crate::Status::AuthenticationError);
        assert!(!err.note.fallback.contains("token-in-body"));
    }

    #[tokio::test]
    async fn kimi_coding_uses_key_header_or_oauth_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", "secret-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "kimi-for-coding", "display_name": "K2.8"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer secret-token"))
            .and(header("x-msh-device-id", "dev-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "oauth-model"}]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let key = account(crate::Provider::Kimi, &[("product", "coding")]);
        let models = kimi_coding_models(&key, &ctx(&http), &server.uri())
            .await
            .unwrap();
        assert_eq!(models[0].name.as_deref(), Some("K2.8"));

        let mut oauth = account(
            crate::Provider::Kimi,
            &[("token_env", "T"), ("device_id", "dev-1")],
        );
        oauth.credential_env = Some("NO_SUCH_KEY".into());
        assert_eq!(
            ids(&kimi_coding_models(&oauth, &ctx(&http), &server.uri())
                .await
                .unwrap()),
            ["oauth-model"]
        );

        let mut none = account(crate::Provider::Kimi, &[]);
        none.credential_env = Some("NO_SUCH_KEY".into());
        assert_eq!(
            kimi_coding_models(&none, &ctx(&http), &server.uri())
                .await
                .unwrap_err()
                .status,
            crate::Status::NeedsConfiguration
        );
    }

    #[tokio::test]
    async fn cursor_posts_connect_json_and_reads_model_ids() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/aiserver.v1.AiService/GetUsableModels"))
            .and(header("authorization", "Bearer secret-token"))
            .and(header("connect-protocol-version", "1"))
            .and(body_json(serde_json::json!({})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [
                    {"modelId": "default", "displayName": "Auto"},
                    {"modelId": "gpt-5.3-codex-low", "displayName": "Codex 5.3 Low"}
                ]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let acc = account(crate::Provider::Cursor, &[]);
        let models = cursor_models(&acc, &ctx(&http), &server.uri())
            .await
            .unwrap();
        assert_eq!(ids(&models), ["default", "gpt-5.3-codex-low"]);
        assert_eq!(models[0].name.as_deref(), Some("Auto"));
    }

    #[tokio::test]
    async fn kiro_lists_on_the_arn_region_with_auth_method_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ListAvailableModels"))
            .and(query_param("origin", "AI_EDITOR"))
            .and(query_param(
                "profileArn",
                "arn:aws:codewhisperer:us-east-1:1:profile/X",
            ))
            .and(header("tokentype", "API_KEY"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "defaultModel": {"modelId": "auto"},
                "models": [
                    {"modelId": "auto", "modelName": "Auto", "description": "d"},
                    {"modelId": "claude-sonnet-4", "modelName": "Claude Sonnet 4"}
                ]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let acc = account(
            crate::Provider::Kiro,
            &[
                ("profile_arn", "arn:aws:codewhisperer:us-east-1:1:profile/X"),
                ("auth_method", "api_key"),
            ],
        );
        let models = kiro_models(&acc, &ctx(&http), Some(&server.uri()))
            .await
            .unwrap();
        assert_eq!(ids(&models), ["auto", "claude-sonnet-4"]);
        assert_eq!(models[0].description.as_deref(), Some("d"));
        assert_eq!(models[1].name.as_deref(), Some("Claude Sonnet 4"));
    }

    #[tokio::test]
    async fn zed_resolves_the_organization_then_lists_with_the_llm_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/client/users/me"))
            .and(header("authorization", "user-1 secret-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "organizations": [{"id": "org-team"}, {"id": "org-me", "is_personal": true}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/client/llm_tokens"))
            .and(body_json(serde_json::json!({"organization_id": "org-me"})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "llm-token"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("authorization", "Bearer llm-token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models": [
                    {"id": "claude-x", "display_name": "Claude X"},
                    {"id": ["array-id"]},
                    {"id": "off", "is_disabled": true}
                ]})),
            )
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let acc = account(crate::Provider::Zed, &[("user_id", "user-1")]);
        let models = zed_models(&acc, &ctx(&http), &server.uri()).await.unwrap();
        assert_eq!(ids(&models), ["claude-x", "array-id"]);
    }

    #[tokio::test]
    async fn azure_pages_stay_on_the_arm_host() {
        let server = MockServer::start().await;
        let base = server.uri();
        Mock::given(method("GET"))
            .and(path("/subscriptions/s/resourceGroups/g/providers/Microsoft.CognitiveServices/accounts/r/models"))
            .and(query_param("api-version", "2024-10-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [{"model": {"name": "gpt-4o"}}],
                "nextLink": "https://evil.example/steal"
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let acc = account(
            crate::Provider::AzureOpenAi,
            &[("resource_id", "/subscriptions/s/resourceGroups/g/providers/Microsoft.CognitiveServices/accounts/r")],
        );
        let err = azure_models(&acc, &ctx(&http), &base).await.unwrap_err();
        assert_eq!(&*err.note.id, "query.schema");
    }

    #[tokio::test]
    async fn grok_uses_cli_headers_for_sessions_and_the_api_for_keys() {
        let cli = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("x-xai-token-auth", "xai-grok-cli"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "grok-cli-1"}]})),
            )
            .mount(&cli)
            .await;
        let api = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "grok-api-1"}]})),
            )
            .mount(&api)
            .await;
        let http = reqwest::Client::new();
        let session = account(crate::Provider::Grok, &[("session_env", "T")]);
        assert_eq!(
            ids(&grok_models(&session, &ctx(&http), &cli.uri(), &api.uri())
                .await
                .unwrap()),
            ["grok-cli-1"]
        );
        let key = account(crate::Provider::Grok, &[]);
        assert_eq!(
            ids(&grok_models(&key, &ctx(&http), &cli.uri(), &api.uri())
                .await
                .unwrap()),
            ["grok-api-1"]
        );
    }
}
