//! Usage and quota queries for coding assistants with a user-supplied
//! API key (read-only).
//!
//! Cline "usage-billing" (Bearer in `CLINE_API_KEY`):
//!   Implements only the personal or organizational balance. We do not show
//!   usage aggregates: the public organization-level aggregated usage endpoint
//!   is not described in the open primary sources; the open-source
//!   client uses
//!   `/api/v1/organizations/{id}/members/{memberId}/usages`, which requires the
//!   `memberId`. For the personal account, `/users/{uid}/usages` has no
//!   window contract (the official client makes a single call without
//!   pagination), and any incomplete item would invalidate a sum
//!   named "full history" — so we prefer not to call it:
//!   balance is the only quota information the user needs and the
//!   extra call would only add cost, latency and failure points.
//!   Sequence:
//!     1. `GET /api/v1/users/me` → `{ id, ... }` (needs the uid).
//!     2. `GET /api/v1/users/{uid}/balance` → `BalanceResponse`
//!        (`{ balance: number, userId: string }`) in dollars. The balance may
//!        be negative (debt); we preserve the raw value.
//!   Primary sources: `https://docs.cline.bot/enterprise-solutions/api-reference.md`
//!     and the open source
//!     `apps/vscode/src/services/account/ClineAccountService.ts` +
//!     `apps/vscode/src/shared/ClineAccount.ts` of the repository
//!     `cline/cline`.
//!
//! Parameters in `Account.params`:
//!   - Cline: `organization_id` (optional). When present, queries
//!     only `/api/v1/organizations/{id}/balance` (balance).

use super::{json, secret, Context, Data, QueryError};
use crate::text::{params, Notes};
use crate::{Account, Metric, MetricKind};
use serde::Deserialize;

const CLINE_BASE: &str = "https://api.cline.bot";
/// The report sources (PRD 0052 RF3.6): the endpoint templates, with the
/// organization and user ids as named placeholders. The ids are only in
/// the request; neither reaches `--json`, `export::v1` or the details.
const CLINE_ORG_BALANCE_SOURCE: &str =
    "https://api.cline.bot/api/v1/organizations/{organization_id}/balance";
const CLINE_USER_BALANCE_SOURCE: &str = "https://api.cline.bot/api/v1/users/{user_id}/balance";
/// The path the personal note names, with the same placeholder.
const CLINE_USER_BALANCE_PATH: &str = "/api/v1/users/{user_id}/balance";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    cline(account, ctx, CLINE_BASE).await
}

// ----------------------------------------------------------- Cline (usage-billing)

#[derive(Deserialize)]
struct ClineMe {
    id: String,
}

#[derive(Deserialize)]
struct ClineBalance {
    balance: f64,
}

#[derive(Deserialize)]
struct ClineOrganizationBalance {
    balance: f64,
}

fn require_finite(value: f64) -> Result<(), QueryError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(QueryError::schema())
    }
}

/// Pure balance: used is None, remaining is the provider's raw value
/// (may be negative for debt). Preserves the sign exactly as
/// it came from /balance.
fn push_balance(metrics: &mut Vec<Metric>, label: &str, balance: f64) -> Result<(), QueryError> {
    require_finite(balance)?;
    metrics.push(Metric {
        billing: Default::default(),
        scope: None,
        label: label.into(),
        used: None,
        limit: None,
        remaining: Some(balance),
        unit: "USD".into(),
        kind: MetricKind::Balance,
        window: None,
        period: None,
        resets_at: None,
    });
    Ok(())
}

async fn cline(account: &Account, ctx: &Context<'_>, base: &str) -> Result<Data, QueryError> {
    let token = secret(account, ctx, "CLINE_API_KEY")?;
    let org_id = optional_param(account, "organization_id");

    if let Some(org) = org_id.as_deref() {
        if !is_safe_org_id(org) {
            return Err(QueryError::config(
                "query.coding.invalid_organization_id",
                "params.organization_id must contain only letters, digits, _ and -.",
            ));
        }
        let org_balance_path = format!("/api/v1/organizations/{org}/balance");
        let body: ClineOrganizationBalance = json(
            ctx.http
                .get(format!("{base}{org_balance_path}"))
                .bearer_auth(&token)
                .timeout(ctx.timeout),
        )
        .await?;
        let mut metrics = Vec::new();
        push_balance(&mut metrics, "Saldo — organização", body.balance)?;
        let mut notes = Notes::new();
        notes.push(
            "note.coding.organization_balance_only",
            "Cline org has no public usage endpoint aggregated by organization without memberId. Only balance is shown; for history, use the authenticated panel at app.cline.bot.",
        );
        Ok(Data::new(metrics, notes, CLINE_ORG_BALANCE_SOURCE))
    } else {
        let me: ClineMe = json(
            ctx.http
                .get(format!("{base}/api/v1/users/me"))
                .bearer_auth(&token)
                .timeout(ctx.timeout),
        )
        .await?;
        if !is_safe_org_id(&me.id) {
            return Err(QueryError::schema());
        }
        let me_path = format!("/api/v1/users/{}", me.id);

        let balance_path = format!("{me_path}/balance");
        let balance: ClineBalance = json(
            ctx.http
                .get(format!("{base}{balance_path}"))
                .bearer_auth(&token)
                .timeout(ctx.timeout),
        )
        .await?;

        let mut metrics = Vec::new();
        push_balance(&mut metrics, "Saldo — conta pessoal", balance.balance)?;

        let mut notes = Notes::new();
        notes.push_params(
            "note.coding.personal_balance_only",
            params([(
                "balance_path",
                crate::text::ParamValue::from(CLINE_USER_BALANCE_PATH),
            )]),
            "Bearer CLINE_API_KEY (created at app.cline.bot → Settings → API Keys) is used for /api/v1/users/me and for {balance_path}. The endpoint /api/v1/users/{uid}/usages is not queried: the open-source Cline client (repo cline/cline) documents no window contract, and any aggregation would present partial sums as full history. Balance is the only quota metric. For history, use the authenticated dashboard at app.cline.bot.",
        );
        Ok(Data::new(metrics, notes, CLINE_USER_BALANCE_SOURCE))
    }
}

fn optional_param(account: &Account, key: &str) -> Option<String> {
    account
        .params
        .get(key)
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

fn is_safe_org_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Provider, Status};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct TestCredentials;
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_TOKEN").then(|| "test-secret".to_string())
        }
    }

    fn account(provider: Provider) -> Account {
        Account {
            id: None,
            name: "conta".to_string(),
            provider,
            credential_env: Some("TEST_TOKEN".to_string()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn context(http: &reqwest::Client) -> Context<'_> {
        Context {
            http,
            credentials: &TestCredentials,
            timeout: Duration::from_secs(5),
        }
    }

    fn metric<'a>(data: &'a Data, label: &str) -> &'a Metric {
        data.metrics
            .iter()
            .find(|m| m.label == label)
            .expect("métrica esperada ausente")
    }

    #[tokio::test]
    async fn cline_pessoal_descobre_uid_e_reporta_saldo_negativo() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "user_abc123",
                "email": "x@example.com"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/user_abc123/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "balance": -2.5,
                "userId": "user_abc123"
            })))
            .mount(&server)
            .await;
        // /api/v1/users/user_abc123/usages is deliberately NOT
        // registered: if the code called it, wiremock would return 404
        // and the test would fail. This guarantees we no longer make that
        // request.

        let client = reqwest::Client::new();
        let data = cline(&account(Provider::Cline), &context(&client), &server.uri())
            .await
            .unwrap();

        assert_eq!(data.metrics.len(), 1, "esperava só o saldo");
        let saldo = metric(&data, "Saldo — conta pessoal");
        assert_eq!(saldo.used, None);
        assert_eq!(saldo.limit, None);
        assert_eq!(saldo.remaining, Some(-2.5));
        assert_eq!(saldo.kind, MetricKind::Balance);
        assert_eq!(saldo.window, None);
        assert_eq!(saldo.unit, "USD");
        // PRD 0052 RF3.6: the request used the returned user id (the mock
        // answers only at its path), but the report names the template and
        // carries neither the id nor the e-mail.
        assert_eq!(data.source, CLINE_USER_BALANCE_SOURCE);
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("for /api/v1/users/{user_id}/balance.")));
        data.assert_id_free(&["user_abc123", "x@example.com", &server.uri()]);
    }

    #[tokio::test]
    async fn cline_pessoal_balance_nao_finite_vira_schema() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "user_x"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/user_x/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "balance": "NaN",
                "userId": "user_x"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = cline(&account(Provider::Cline), &context(&client), &server.uri())
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::Error);
    }

    #[tokio::test]
    async fn cline_org_apenas_balance_sem_historico() {
        let mut acct = account(Provider::Cline);
        acct.params
            .insert("organization_id".into(), "org_xyz".into());

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/organizations/org_xyz/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "balance": 7.5,
                "organizationId": "org_xyz"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = cline(&acct, &context(&client), &server.uri())
            .await
            .unwrap();

        assert_eq!(data.metrics.len(), 1);
        let saldo = metric(&data, "Saldo — organização");
        assert_eq!(saldo.used, None);
        assert_eq!(saldo.remaining, Some(7.5));
        assert!(!data.metrics.iter().any(|m| m.label.starts_with("Janela")));
        // PRD 0052 RF3.6: the configured organization id is in the request
        // path only.
        assert_eq!(data.source, CLINE_ORG_BALANCE_SOURCE);
        data.assert_id_free(&["org_xyz", &server.uri()]);
    }

    #[tokio::test]
    async fn cline_org_balance_negativo_e_preservado() {
        let mut acct = account(Provider::Cline);
        acct.params
            .insert("organization_id".into(), "org_xyz".into());

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/organizations/org_xyz/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "balance": -0.01,
                "organizationId": "org_xyz"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let data = cline(&acct, &context(&client), &server.uri())
            .await
            .unwrap();

        let saldo = metric(&data, "Saldo — organização");
        assert_eq!(saldo.remaining, Some(-0.01));
    }

    #[tokio::test]
    async fn cline_org_id_inseguro_rejeita() {
        let mut acct = account(Provider::Cline);
        acct.params
            .insert("organization_id".into(), "../etc/passwd".into());
        let client = reqwest::Client::new();
        let err = cline(&acct, &context(&client), "https://unused.invalid")
            .await
            .unwrap_err();
        assert_eq!(err.status, Status::NeedsConfiguration);
    }

    #[test]
    fn is_safe_org_id_aceita_apenas_caracteres_seguros() {
        assert!(is_safe_org_id("org_abc123"));
        assert!(is_safe_org_id("ORG-123_xyz"));
        assert!(!is_safe_org_id(""));
        assert!(!is_safe_org_id("org 123"));
        assert!(!is_safe_org_id("org/123"));
        assert!(!is_safe_org_id("org.123"));
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    /// PRD 0052 RF3.6: the Cline sources are the base plus the endpoint
    /// templates, with named placeholders instead of ids.
    #[test]
    fn the_sources_are_templates_on_the_base() {
        assert_eq!(
            CLINE_USER_BALANCE_SOURCE,
            format!("{CLINE_BASE}{CLINE_USER_BALANCE_PATH}")
        );
        assert!(CLINE_ORG_BALANCE_SOURCE.starts_with(CLINE_BASE));
        assert!(CLINE_ORG_BALANCE_SOURCE.contains("/{organization_id}/"));
        assert!(CLINE_USER_BALANCE_SOURCE.contains("/{user_id}/"));
    }
}
