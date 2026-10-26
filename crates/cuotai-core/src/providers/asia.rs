//! Account balance queries through official Asian APIs (read-only).
//!
//! Primary sources consulted on 2026-09-28:
//!
//! - **MiniMax account balance** (`MINIMAX_IO_API_KEY`)
//!   - `GET https://www.minimax.io/account/query_balance` (Bearer)
//!     - Selected automatically when the key starts with
//!       `sk-api-` (`src/client/endpoints.ts::selectUsageEndpoint`); Token/Coding
//!       Plan keys (without that prefix) are handled by the
//!       `router_minimax` adapter.
//!     - Fields: `available_amount`, `cash_balance`, `voucher_balance`,
//!       `credit_balance`, `owed_amount`, `balance_alert_switch`,
//!       `balance_alert_threshold`, `base_resp`. The currency is **not
//!       documented** by MiniMax for this endpoint; the unit
//!       label is neutral so as not to infer.
//!   - Every response carries `base_resp.status_code`; a nonzero status is
//!     considered an error even on HTTP 200
//!     (`src/client/http.ts::requestJson`).
//!
//! - **Kimi / Moonshot Open Platform** (KIMI_API_KEY, account balance)
//!   - `GET https://api.moonshot.ai/v1/users/me/balance` (Bearer)
//!     - Documented at <https://platform.kimi.ai/docs/api/balance.md>.
//!     - `available_balance`, `voucher_balance`, `cash_balance` in
//!       **USD**. `cash_balance` may be negative (debt);
//!       `voucher_balance` never.
//!     - Requires `code == 0` and `status == true`.
//!   - Kimi Code (subscription/programming) does **not** expose a public
//!     quota endpoint — only the console and the proprietary `/usage` CLI
//!     (<https://www.kimi.com/code/docs/en/kimi-code/membership.html>).
//!     ⇒ Do not implement a Kimi Coding mode.
//!
//! - **Qwen / DashScope / Model Studio** (QWEN_API_KEY)
//!   - DashScope exposes only inference
//!     (`POST /api/v1/services/aigc/...`, `/v1/chat/completions`,
//!     `/anthropic/v1/messages`) and the Batch API
//!     (<https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope>).
//!   - The Alibaba Cloud account's billing balance requires
//!     `QueryAccountBalance` (Financial Management API), authenticated
//!     with the master account's `AccessKey ID` + `AccessKey Secret`
//!     (<https://www.alibabacloud.com/help/en/finance/account-management/latest/api-bill-overview>).
//!   - ⇒ `Unsupported`.

use super::{json, secret, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, Provider};
use serde::Deserialize;

/// Official international URLs (not configurable by the end user;
/// only tests receive a private base through `*_at(account, ctx, base)`).
const MINIMAX_ACCOUNT_BALANCE_URL: &str = "https://www.minimax.io/account/query_balance";
const KIMI_BALANCE_URL: &str = "https://api.moonshot.ai/v1/users/me/balance";
/// OpenAI-shaped model list for the same platform key (`platform.kimi.ai/docs/api/list-models`).
pub(crate) const KIMI_MODELS_URL: &str = "https://api.moonshot.ai/v1/models";
const QWEN_DASHSCOPE_DOCS_URL: &str =
    "https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope";
const QWEN_BILLING_DOCS_URL: &str =
    "https://www.alibabacloud.com/help/en/finance/account-management/latest/api-bill-overview";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::MiniMax => minimax(account, ctx).await,
        Provider::Kimi => kimi(account, ctx).await,
        Provider::Qwen => Err(QueryError::unsupported_params(
            "query.qwen.billing_balance_unsupported",
            params([
                ("dashscope_docs", ParamValue::from(QWEN_DASHSCOPE_DOCS_URL)),
                ("billing_docs", ParamValue::from(QWEN_BILLING_DOCS_URL)),
            ]),
            "Qwen / Alibaba DashScope Model Studio (QWEN_API_KEY) does not grant access to the account's billing balance. The key only authorises inference via /api/v1/services/aigc/...; the Alibaba Cloud balance requires QueryAccountBalance (Financial Management API), authenticated with the master account's AccessKey ID/Secret. Primary sources: {dashscope_docs} and {billing_docs}",
        )),
        other => Err(QueryError::unsupported_params(
            "query.asia.other_provider",
            params([(
                "provider",
                ParamValue::from(crate::text::provider_l10n(other)),
            )]),
            "{provider} is handled by another adapter.",
        )),
    }
}

// ------------------------------------------------------------------ MiniMax

/// Only the account balance (`MINIMAX_IO_API_KEY` pay-as-you-go) remains
/// here: `sk-api-…` keys call `/account/query_balance`; Token/Coding Plan
/// keys (any other value) go to
/// `super::router_minimax::fetch`, which implements the quota reading in
/// in the registry order (`/token_plan/remains` → `/coding_plan/remains`).
#[derive(Deserialize)]
struct MinimaxBaseResp {
    status_code: i64,
}

#[derive(Deserialize)]
struct MinimaxAccountBalanceResponse {
    available_amount: String,
    cash_balance: String,
    voucher_balance: String,
    credit_balance: String,
    owed_amount: String,
    #[serde(default)]
    balance_alert_switch: bool,
    base_resp: MinimaxBaseResp,
}

fn minimax_check_base(resp: &MinimaxBaseResp) -> Result<(), QueryError> {
    if resp.status_code == 0 {
        Ok(())
    } else {
        Err(QueryError {
            diagnostic: None,
            status: crate::Status::Error,
            note: Localized::with_params(
                "query.minimax.balance_status",
                params([("code", ParamValue::from(resp.status_code.to_string()))]),
                "MiniMax responded base_resp.status_code={code} on the balance probe.",
            ),
        })
    }
}

fn parse_balance_amount(raw: &str) -> Result<f64, QueryError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(QueryError::schema());
    }
    let value: f64 = trimmed.parse().map_err(|_| QueryError::schema())?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(QueryError::schema())
    }
}

/// Internal envelope that accepts a private `base` (for wiremock). Production
/// passes `MINIMAX_ACCOUNT_BALANCE_URL`; tests inject the mock's
/// local URL directly.
async fn minimax_balance_at(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "MINIMAX_IO_API_KEY")?;
    let request = ctx
        .http
        .get(base)
        .bearer_auth(&key)
        .header("Content-Type", "application/json")
        .timeout(ctx.timeout);
    let resp: MinimaxAccountBalanceResponse = json(request).await?;
    minimax_check_base(&resp.base_resp)?;
    let available = parse_balance_amount(&resp.available_amount)?;
    let cash = parse_balance_amount(&resp.cash_balance)?;
    let voucher = parse_balance_amount(&resp.voucher_balance)?;
    let credit = parse_balance_amount(&resp.credit_balance)?;
    let owed = parse_balance_amount(&resp.owed_amount)?;
    let mut notes = Notes::new();
    notes.push_params(
        "note.minimax.balance_endpoint",
        params([("endpoint", ParamValue::from(MINIMAX_ACCOUNT_BALANCE_URL))]),
        "Pay-as-you-go key (sk-api-…): account balance query at {endpoint}.",
    );
    notes.push(
        "note.minimax.currency_undocumented",
        "The currency returned by /account/query_balance is not explicitly documented; the label stays neutral (—) so the unit is not inferred.",
    );
    let state = if resp.balance_alert_switch {
        Localized::fixed("note.minimax.balance_alert.on", "on")
    } else {
        Localized::fixed("note.minimax.balance_alert.off", "off")
    };
    notes.push_params(
        "note.minimax.balance_alert",
        params([
            ("enabled", ParamValue::from(resp.balance_alert_switch)),
            ("state", state.into()),
        ]),
        "Balance alert: {state}.",
    );
    Ok(Data::new(
        vec![
            Metric {
                billing: Default::default(),
                scope: None,
                label: "Saldo disponível".into(),
                used: None,
                limit: None,
                remaining: Some(available),
                unit: "—".into(),
                kind: MetricKind::Balance,
                window: None,
                period: Some("saldo atual".into()),
                resets_at: None,
            },
            Metric {
                billing: Default::default(),
                scope: None,
                label: "Saldo em caixa".into(),
                used: None,
                limit: None,
                remaining: Some(cash),
                unit: "—".into(),
                period: Some("saldo atual".into()),
                kind: MetricKind::Balance,
                window: None,
                resets_at: None,
            },
            Metric {
                billing: Default::default(),
                scope: None,
                label: "Voucher".into(),
                used: None,
                limit: None,
                remaining: Some(voucher),
                unit: "—".into(),
                period: Some("saldo atual".into()),
                resets_at: None,
                kind: MetricKind::Credits,
                window: None,
            },
            Metric {
                billing: Default::default(),
                scope: None,
                label: "Crédito".into(),
                used: None,
                limit: None,
                remaining: Some(credit),
                unit: "—".into(),
                period: Some("saldo atual".into()),
                kind: MetricKind::Credits,
                window: None,
                resets_at: None,
            },
            Metric {
                billing: Default::default(),
                scope: None,
                label: "Valor devido (dívida)".into(),
                used: None,
                limit: None,
                remaining: Some(owed),
                unit: "—".into(),
                period: Some("saldo atual".into()),
                kind: MetricKind::Balance,
                window: None,
                resets_at: None,
            },
        ],
        notes,
        MINIMAX_ACCOUNT_BALANCE_URL,
    ))
}

pub(crate) async fn minimax(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "MINIMAX_IO_API_KEY")?;
    if key.starts_with("sk-api-") {
        minimax_balance_at(account, ctx, MINIMAX_ACCOUNT_BALANCE_URL).await
    } else {
        super::router_minimax::fetch(account, ctx).await
    }
}

// -------------------------------------------------------------------- Kimi

#[derive(Deserialize)]
struct KimiBalanceEnvelope {
    code: i64,
    #[serde(default)]
    status: Option<bool>,
    #[serde(default)]
    data: Option<KimiBalanceData>,
}

#[derive(Deserialize)]
struct KimiBalanceData {
    available_balance: f64,
    voucher_balance: f64,
    cash_balance: f64,
}

/// Internal envelope that accepts a private `base` for wiremock.
async fn kimi_at(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "KIMI_API_KEY")?;
    let url = if base == KIMI_BALANCE_URL {
        KIMI_BALANCE_URL
    } else {
        // Test injection: combines base + path.
        &format!("{}/v1/users/me/balance", base.trim_end_matches('/'))
    };
    let request = ctx.http.get(url).bearer_auth(&key).timeout(ctx.timeout);
    let resp: KimiBalanceEnvelope = json(request).await?;
    if resp.code != 0 || resp.status != Some(true) {
        return Err(QueryError::schema());
    }
    let data = resp.data.ok_or_else(QueryError::schema)?;
    if !data.available_balance.is_finite()
        || !data.voucher_balance.is_finite()
        || !data.cash_balance.is_finite()
    {
        return Err(QueryError::schema());
    }
    let mut metrics = Vec::with_capacity(3);
    let mut add = |label: &str, value: f64, kind: MetricKind| {
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label: label.into(),
            used: None,
            limit: None,
            remaining: Some(value),
            unit: "USD".into(),
            kind,
            window: None,
            period: Some("saldo atual".into()),
            resets_at: None,
        });
    };
    add(
        "Saldo disponível (KIMI_API_KEY)",
        data.available_balance,
        MetricKind::Balance,
    );
    add("Voucher", data.voucher_balance, MetricKind::Credits);
    add(
        "Saldo em caixa (pode ser negativo)",
        data.cash_balance,
        MetricKind::Balance,
    );
    let mut notes = Notes::new();
    notes.push(
        "note.kimi.platform_key_scope",
        "kimi.com / kimi.ai platform key is independent of the kimi.com membership quota; use the key on the platform endpoint.",
    );
    notes.push(
        "note.kimi.balance_semantics",
        "Cash balance may be negative (debt); voucher balance is always non-negative.",
    );
    notes.push(
        "note.kimi.no_membership_quota_endpoint",
        "Without this key there is no public endpoint for the Kimi Code/Membership quota with this key; subscription usage remains restricted to the console and the proprietary CLI.",
    );
    Ok(Data::new(metrics, notes, KIMI_BALANCE_URL))
}

pub(crate) async fn kimi(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    kimi_at(account, ctx, KIMI_BALANCE_URL).await
}

// -------------------------------------------------------------------- testes

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Status};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct TestCredentials(&'static str, &'static str);
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            if name == self.0 {
                Some(self.1.to_string())
            } else {
                None
            }
        }
    }

    fn account(provider: Provider, env: &'static str) -> Account {
        Account {
            id: None,
            name: "conta".into(),
            provider,
            credential_env: Some(env.into()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn ctx_with<'a>(http: &'a reqwest::Client, creds: &'a dyn Credentials) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn minimax_check_base_trata_ausente_e_zero() {
        assert!(minimax_check_base(&MinimaxBaseResp { status_code: 0 }).is_ok());
        let err = minimax_check_base(&MinimaxBaseResp { status_code: 7 }).unwrap_err();
        assert!(matches!(err.status, Status::Error));
        assert!(err.note.fallback.contains("status_code=7"));
    }

    #[test]
    fn parse_balance_amount_rejeita_strings_invalidas() {
        assert!(parse_balance_amount("0").is_ok());
        assert!(parse_balance_amount("3.5").is_ok());
        assert!(parse_balance_amount("").is_err());
        assert!(parse_balance_amount("   ").is_err());
        assert!(parse_balance_amount("nan").is_err());
        assert!(parse_balance_amount("inf").is_err());
    }

    #[tokio::test]
    async fn minimax_account_balance_http_sucesso() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "available_amount": "10.5",
                "cash_balance": "8.0",
                "voucher_balance": "2.5",
                "credit_balance": "0",
                "owed_amount": "0",
                "balance_alert_switch": true,
                "balance_alert_threshold": "5",
                "base_resp": { "status_code": 0 }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let acc = account(Provider::MiniMax, "MINIMAX_IO_API_KEY");
        let creds = TestCredentials("MINIMAX_IO_API_KEY", "sk-api-test");
        let ctx = ctx_with(&http, &creds);
        let data = minimax_balance_at(&acc, &ctx, &server.uri()).await.unwrap();
        let find = |label: &str| -> f64 {
            data.metrics
                .iter()
                .find(|m| m.label == label)
                .unwrap_or_else(|| panic!("faltando {label}"))
                .remaining
                .unwrap()
        };
        assert_eq!(find("Saldo disponível"), 10.5);
        assert_eq!(find("Saldo em caixa"), 8.0);
        assert_eq!(find("Voucher"), 2.5);
        assert_eq!(find("Crédito"), 0.0);
        assert_eq!(find("Valor devido (dívida)"), 0.0);
        assert!(data.metrics.iter().all(|m| m.window.is_none()));
        for index in [0, 1, 4] {
            assert_eq!(data.metrics[index].kind, MetricKind::Balance);
        }
        for index in [2, 3] {
            assert_eq!(data.metrics[index].kind, MetricKind::Credits);
        }
        assert!(data.notes.iter().any(|n| n == "Balance alert: on."));
        assert_eq!(data.source, MINIMAX_ACCOUNT_BALANCE_URL);
    }

    #[tokio::test]
    async fn minimax_account_balance_owed_negativo_e_dividido_de_saldo() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "available_amount": "-3.0",
                "cash_balance": "-3.0",
                "voucher_balance": "0",
                "credit_balance": "0",
                "owed_amount": "3.0",
                "balance_alert_switch": false,
                "balance_alert_threshold": "",
                "base_resp": { "status_code": 0 }
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let acc = account(Provider::MiniMax, "MINIMAX_IO_API_KEY");
        let creds = TestCredentials("MINIMAX_IO_API_KEY", "sk-api-test");
        let ctx = ctx_with(&http, &creds);
        let data = minimax_balance_at(&acc, &ctx, &server.uri()).await.unwrap();
        let avail = data
            .metrics
            .iter()
            .find(|m| m.label == "Saldo disponível")
            .unwrap()
            .remaining
            .unwrap();
        let owed = data
            .metrics
            .iter()
            .find(|m| m.label == "Valor devido (dívida)")
            .unwrap()
            .remaining
            .unwrap();
        assert_eq!(avail, -3.0);
        assert_eq!(owed, 3.0);
    }

    // ------------------------------------------------------------ Kimi

    #[tokio::test]
    async fn kimi_balance_http_sucesso() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("Authorization", "Bearer mk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {
                    "available_balance": 49.58894,
                    "voucher_balance": 46.58893,
                    "cash_balance": -3.0
                },
                "scode": "0x0",
                "status": true
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let acc = account(Provider::Kimi, "KIMI_API_KEY");
        let creds = TestCredentials("KIMI_API_KEY", "mk-test");
        let ctx = ctx_with(&http, &creds);
        let data = kimi_at(&acc, &ctx, &server.uri()).await.unwrap();

        let find = |label: &str| -> f64 {
            data.metrics
                .iter()
                .find(|m| m.label == label)
                .unwrap_or_else(|| panic!("faltando {label}"))
                .remaining
                .unwrap()
        };
        assert!((find("Saldo disponível (KIMI_API_KEY)") - 49.58894).abs() < 1e-9);
        assert!((find("Voucher") - 46.58893).abs() < 1e-9);
        assert!((find("Saldo em caixa (pode ser negativo)") + 3.0).abs() < 1e-9);
        assert!(data.metrics.iter().all(|m| m.window.is_none()));
        assert_eq!(data.metrics[0].kind, MetricKind::Balance);
        assert_eq!(data.metrics[1].kind, MetricKind::Credits);
        assert_eq!(data.metrics[2].kind, MetricKind::Balance);
        assert_eq!(data.source, KIMI_BALANCE_URL);
    }

    #[tokio::test]
    async fn kimi_balance_code_nao_zero_vira_schema() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 401,
                "status": false
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let acc = account(Provider::Kimi, "KIMI_API_KEY");
        let creds = TestCredentials("KIMI_API_KEY", "mk-test");
        let ctx = ctx_with(&http, &creds);
        let err = kimi_at(&acc, &ctx, &server.uri()).await.unwrap_err();
        assert!(matches!(err.status, Status::Error));
    }

    // ------------------------------------------ dispatch + credenciais

    #[test]
    fn fetch_dispatch_qwen_continua_unsupported() {
        // dispatch does not hit the network: unsupported providers have a message.
        let http = reqwest::Client::new();
        let acc_qwen = account(Provider::Qwen, "QWEN_API_KEY");
        let creds_qwen = TestCredentials("QWEN_API_KEY", "sk-test");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let err = fetch(&acc_qwen, &ctx_with(&http, &creds_qwen))
                .await
                .unwrap_err();
            assert!(matches!(err.status, Status::Unsupported));
            assert!(err.note.fallback.contains(QWEN_DASHSCOPE_DOCS_URL));
        });
    }

    #[tokio::test]
    async fn credenciais_ausentes_viram_needs_configuration() {
        let http = reqwest::Client::new();
        let acc_k = account(Provider::Kimi, "OTHER_ENV");
        let acc_m = account(Provider::MiniMax, "OTHER_ENV");
        let creds = TestCredentials("KIMI_API_KEY", "mk-test");
        let creds_m = TestCredentials("MINIMAX_IO_API_KEY", "sk-api-test");
        let err_k = kimi_at(&acc_k, &ctx_with(&http, &creds), "http://unused")
            .await
            .unwrap_err();
        let err_m = minimax_balance_at(
            &acc_m,
            &ctx_with(&http, &creds_m),
            MINIMAX_ACCOUNT_BALANCE_URL,
        )
        .await
        .unwrap_err();
        assert!(matches!(err_k.status, Status::NeedsConfiguration));
        assert!(matches!(err_m.status, Status::NeedsConfiguration));
    }
}
