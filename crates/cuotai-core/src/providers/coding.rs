//! Consultas de uso e cota de assistentes de codificação com chave de API
//! fornecida pelo usuário (somente leitura).
//!
//! Cline "usage-billing" (Bearer em `CLINE_API_KEY`):
//!   Implementa apenas saldo pessoal ou organizacional. Não exibimos
//!   agregados de uso: o endpoint público de uso agregado por organização
//!   não está descrito nas fontes primárias abertas; o cliente
//!   open-source usa
//!   `/api/v1/organizations/{id}/members/{memberId}/usages`, que exige o
//!   `memberId`. Para a conta pessoal, `/users/{uid}/usages` não traz
//!   contrato de janela (o cliente oficial faz uma única chamada sem
//!   paginação), e qualquer item incompleto invalidaria uma soma
//!   nomeada "histórico completo" — então preferimos não chamá-lo:
//!   saldo é a única informação de quota que o usuário precisa e a
//!   chamada extra só adicionaria custo, latência e pontos de falha.
//!   Sequência:
//!     1. `GET /api/v1/users/me` → `{ id, ... }` (precisa do uid).
//!     2. `GET /api/v1/users/{uid}/balance` → `BalanceResponse`
//!        (`{ balance: number, userId: string }`) em dólares. Saldo pode
//!        ser negativo (dívida); preservamos o valor bruto.
//!   Fontes primárias: `https://docs.cline.bot/enterprise-solutions/api-reference.md`
//!     e o código aberto
//!     `apps/vscode/src/services/account/ClineAccountService.ts` +
//!     `apps/vscode/src/shared/ClineAccount.ts` do repositório
//!     `cline/cline`.
//!
//! Parâmetros em `Account.params`:
//!   - Cline: `organization_id` (opcional). Quando presente, consulta
//!     apenas `/api/v1/organizations/{id}/balance` (saldo).

use super::{json, secret, Context, Data, QueryError};
use crate::text::{params, Notes};
use crate::{Account, Metric, MetricKind};
use serde::Deserialize;

const CLINE_BASE: &str = "https://api.cline.bot";

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

/// Saldo puro: usado é None, remaining é o valor bruto do provedor
/// (pode ser negativo para dívida). Preserva o sinal exatamente como
/// veio do /balance.
fn push_balance(metrics: &mut Vec<Metric>, label: &str, balance: f64) -> Result<(), QueryError> {
    require_finite(balance)?;
    metrics.push(Metric {
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
                "params.organization_id deve conter apenas letras, dígitos, _ e -.",
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
            "Cline org não tem endpoint público de uso \
             agregado por organização sem memberId. Apenas \
             saldo exposto; para histórico, use o painel \
             autenticado em app.cline.bot.",
        );
        Ok(Data::new(
            metrics,
            notes,
            format!("{base}{org_balance_path}"),
        ))
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
            params([]),
            format!(
                "Bearer CLINE_API_KEY (criada em app.cline.bot → Settings → \
                 API Keys) serve para /api/v1/users/me e para {balance_path}. \
                 O endpoint /api/v1/users/{{uid}}/usages não é consultado: \
                 o cliente open-source do Cline (repo cline/cline) não \
                 documenta contrato de janela, e qualquer agregação \
                 nomearia somas parciais como histórico completo. Saldo é \
                 a única métrica de quota. Para histórico, use o painel \
                 autenticado em app.cline.bot."
            ),
        );
        Ok(Data::new(metrics, notes, format!("{base}{balance_path}")))
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
            name: "conta".to_string(),
            provider,
            credential_env: Some("TEST_TOKEN".to_string()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
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
        // /api/v1/users/user_abc123/usages deliberadamente NÃO é
        // registrado: se o código o chamasse, o wiremock retornaria 404
        // e o teste falharia. Isso garante que não fazemos mais essa
        // requisição.

        let client = reqwest::Client::new();
        let data = cline(&account(Provider::Cline), &context(&client), &server.uri())
            .await
            .unwrap();

        assert_eq!(
            data.metrics.len(),
            1,
            "esperava só o saldo, recebi: {:?}",
            data.metrics
        );
        let saldo = metric(&data, "Saldo — conta pessoal");
        assert_eq!(saldo.used, None);
        assert_eq!(saldo.limit, None);
        assert_eq!(saldo.remaining, Some(-2.5));
        assert_eq!(saldo.kind, MetricKind::Balance);
        assert_eq!(saldo.window, None);
        assert_eq!(saldo.unit, "USD");
        assert_eq!(
            data.source,
            format!("{}/api/v1/users/user_abc123/balance", server.uri())
        );
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
        assert!(data
            .source
            .ends_with("/api/v1/organizations/org_xyz/balance"));
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
