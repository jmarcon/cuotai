//! Consulta read-only de créditos da ElevenLabs (PRD 0037).
//!
//! Contrato documentado em
//! <https://elevenlabs.io/docs/api-reference/user/subscription/get>:
//!
//! - `GET {base}/v1/user/subscription`, sem corpo nem parâmetros de consulta;
//! - autenticação somente pelo cabeçalho `xi-api-key`;
//! - o campo da API ainda se chama `character_count`/`character_limit`, mas o
//!   produto os chama de *créditos*; o adaptador exibe "créditos".
//!
//! O adaptador lê apenas os campos que usa de um `serde_json::Value`: faturas,
//! cupons, `pending_change` e qualquer outro campo nunca são copiados para
//! métricas, notas ou logs. Nenhum endpoint de geração é usado e a base é
//! escolhida numa lista fixa de regiões, de modo que a chave nunca é enviada a
//! um host arbitrário.

use super::{secret, send, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Status};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

const PATH: &str = "/v1/user/subscription";
/// Largest error body read to choose a fixed note id (RF7.1).
const ERROR_BODY_LIMIT: usize = 16 * 1024;
const CREDITS_UNIT: &str = "créditos";
const CURRENCIES: [&str; 5] = ["usd", "eur", "inr", "pln", "gbp"];

/// Fixed allow-list of hosts that may receive the key (RF1.3). `None` input is
/// the default region; an unknown value is `None` output.
pub(crate) fn base_for_region(region: Option<&str>) -> Option<&'static str> {
    match region {
        None | Some("default") => Some("https://api.elevenlabs.io"),
        Some("us") => Some("https://api.us.elevenlabs.io"),
        Some("eu") => Some("https://api.eu.residency.elevenlabs.io"),
        Some("in") => Some("https://api.in.residency.elevenlabs.io"),
        Some("sg") => Some("https://api.sg.residency.elevenlabs.io"),
        Some(_) => None,
    }
}

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    let base =
        base_for_region(account.params.get("region").map(String::as_str)).ok_or_else(|| {
            QueryError::config(
                "query.eleven_labs.region",
                "Região inválida: use default, us, eu, in ou sg em params.region.",
            )
        })?;
    eleven_labs_at(account, ctx, base).await
}

async fn eleven_labs_at(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Data, QueryError> {
    let key = secret(account, ctx, "ELEVENLABS_API_KEY")?;
    let url = format!("{}{PATH}", base.trim_end_matches('/'));
    let response = send(
        ctx.http
            .get(&url)
            .header("xi-api-key", key.trim())
            .header("Accept", "application/json")
            .timeout(ctx.timeout),
    )
    .await?;
    let code = response.status();
    if !code.is_success() {
        let detail = error_code(response).await;
        return Err(classify_http(code.as_u16(), detail.as_deref()));
    }
    let value: Value = response.json().await.map_err(|_| QueryError::schema())?;
    parse(&value, url)
}

/// `detail.code` (or the legacy `detail.status`) of an error body, bounded.
async fn error_code(mut response: reqwest::Response) -> Option<String> {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() >= ERROR_BODY_LIMIT {
            body.truncate(ERROR_BODY_LIMIT);
            break;
        }
    }
    let value: Value = serde_json::from_slice(&body).ok()?;
    let detail = value.get("detail")?.as_object()?;
    ["code", "status"]
        .iter()
        .find_map(|key| detail.get(*key).and_then(Value::as_str))
        .map(str::to_owned)
}

fn failure(status: Status, id: &'static str, text: &'static str) -> QueryError {
    QueryError {
        status,
        note: Localized::fixed(id, text),
        diagnostic: None,
    }
}

/// Maps status and an allow-listed code to fixed notes; the provider's message
/// never reaches a note (RF7).
fn classify_http(code: u16, detail: Option<&str>) -> QueryError {
    match code {
        401 => failure(
            Status::AuthenticationError,
            "note.eleven_labs.key_rejected",
            "A ElevenLabs recusou a chave de API; confira ou recrie a chave e o nome da variável.",
        ),
        403 => match detail {
            Some("workspace_access_denied") => failure(
                Status::AuthenticationError,
                "note.eleven_labs.workspace_denied",
                "A chave não tem acesso ao workspace; use a chave do workspace que quer acompanhar.",
            ),
            _ => failure(
                Status::AuthenticationError,
                "note.eleven_labs.scope_missing",
                "A chave é válida, mas talvez não leia a assinatura; confira a restrição da chave (e, no Enterprise, a lista de IPs ou o workspace).",
            ),
        },
        429 => failure(
            Status::RateLimited,
            "note.eleven_labs.rate_limited",
            "A ElevenLabs limitou as consultas; o intervalo será ampliado automaticamente.",
        ),
        422 => QueryError::schema(),
        _ => QueryError {
            status: Status::Error,
            note: Localized::with_params(
                "query.http",
                params([("code", ParamValue::from(code))]),
                format!("Consulta retornou HTTP {code}. Verifique acesso e permissões."),
            ),
            diagnostic: None,
        },
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64().filter(|v| v.is_finite()),
        _ => None,
    }
}

fn non_negative(value: Option<&Value>) -> Option<f64> {
    number(value).filter(|v| *v >= 0.0)
}

fn window_for(period: Option<&str>) -> MetricWindow {
    match period {
        Some("monthly_period") => MetricWindow::Monthly,
        _ => MetricWindow::Other,
    }
}

/// Extra usage enablement and its credit cap (`None` cap is unlimited).
#[derive(Debug, PartialEq)]
enum Extension {
    Disabled,
    Enabled { cap: Option<f64> },
}

fn extension_state(body: &Map<String, Value>) -> Option<Extension> {
    let positive = |cap: f64| {
        if cap > 0.0 {
            Extension::Enabled { cap: Some(cap) }
        } else {
            Extension::Disabled
        }
    };
    match body.get("max_credit_limit_extension") {
        Some(Value::String(s)) if s == "unlimited" => Some(Extension::Enabled { cap: None }),
        Some(Value::Number(_)) => {
            non_negative(body.get("max_credit_limit_extension")).map(positive)
        }
        Some(_) => None,
        None => match (
            body.get("allowed_to_extend_character_limit"),
            body.get("max_character_limit_extension"),
        ) {
            (_, Some(cap @ Value::Number(_))) => non_negative(Some(cap)).map(positive),
            (Some(Value::Bool(true)), _) => Some(Extension::Enabled { cap: None }),
            (Some(Value::Bool(false)), _) => Some(Extension::Disabled),
            _ => None,
        },
    }
}

fn valid_tier(tier: &str) -> bool {
    (1..=40).contains(&tier.len())
        && tier
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn parse(value: &Value, source: String) -> Result<Data, QueryError> {
    let body = value.as_object().ok_or_else(QueryError::schema)?;
    let used = non_negative(body.get("character_count")).ok_or_else(QueryError::schema)?;
    let limit = non_negative(body.get("character_limit"));
    let resets_at = number(body.get("next_character_count_reset_unix"))
        .filter(|secs| *secs > 0.0 && *secs <= i64::MAX as f64)
        .and_then(|secs| DateTime::<Utc>::from_timestamp(secs as i64, 0));
    let window = window_for(body.get("character_refresh_period").and_then(Value::as_str));

    let mut notes = Notes::new();
    notes.push(
        "note.eleven_labs.source",
        "Lido por GET /v1/user/subscription com a chave de API; a consulta não gera áudio.",
    );
    if let Some(tier) = body
        .get("tier")
        .and_then(Value::as_str)
        .filter(|tier| valid_tier(tier))
    {
        notes.push_params(
            "note.eleven_labs.tier",
            params([("tier", ParamValue::from(tier.to_string()))]),
            format!("Plano reportado pela ElevenLabs: {tier}."),
        );
    }
    if limit.is_none() {
        notes.push(
            "note.eleven_labs.limit_missing",
            "A ElevenLabs não informou o limite de créditos; apenas o consumo é exibido.",
        );
    }

    let mut metrics = vec![Metric {
        label: "Créditos do plano".into(),
        used: Some(used),
        limit,
        remaining: limit.map(|limit| (limit - used).max(0.0)),
        unit: CREDITS_UNIT.into(),
        kind: MetricKind::Quota,
        window: Some(window),
        period: None,
        resets_at,
        billing: Default::default(),
        scope: None,
    }];

    // Extra usage (RF5).
    let mut state = extension_state(body);
    if let Some(Extension::Enabled { cap: Some(_) }) = state {
        if body.get("can_extend_character_limit") == Some(&Value::Bool(false)) {
            state = Some(Extension::Disabled);
            notes.push(
                "note.eleven_labs.overage_inconsistent",
                "A ElevenLabs informou um teto de uso extra, mas não permite estender o limite; o uso extra foi tratado como desativado.",
            );
        }
    }
    let enabled = matches!(state, Some(Extension::Enabled { .. }));
    if let (Some(Extension::Enabled { cap }), Some(limit)) = (&state, limit) {
        let extra = (used - limit).max(0.0);
        if cap.is_none() {
            notes.push(
                "note.eleven_labs.overage_unlimited",
                "O uso extra não tem teto de créditos.",
            );
        }
        metrics.push(
            Metric {
                label: "Créditos extras usados".into(),
                used: Some(extra),
                limit: *cap,
                remaining: cap.map(|cap| (cap - extra).max(0.0)),
                unit: CREDITS_UNIT.into(),
                kind: MetricKind::Usage,
                window: Some(window),
                period: None,
                resets_at,
                billing: Default::default(),
                scope: None,
            }
            .overage(),
        );
    }
    if let Some(overage) = body.get("current_overage").and_then(Value::as_object) {
        let amount = match overage.get("amount") {
            Some(Value::String(s)) => s
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0),
            _ => None,
        };
        let currency = overage
            .get("currency")
            .or_else(|| body.get("currency"))
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .filter(|c| CURRENCIES.contains(&c.as_str()));
        match (amount, currency) {
            (Some(amount), Some(currency)) if enabled || amount > 0.0 => metrics.push(
                Metric {
                    label: "Custo do uso extra".into(),
                    used: Some(amount),
                    limit: None,
                    remaining: None,
                    unit: currency.to_ascii_uppercase(),
                    kind: MetricKind::Spend,
                    window: Some(window),
                    period: None,
                    resets_at,
                    billing: Default::default(),
                    scope: None,
                }
                .overage(),
            ),
            (Some(_), Some(_)) => {}
            _ if overage.get("amount").is_some_and(|v| !v.is_null()) => notes.push(
                "note.eleven_labs.overage_amount_unreadable",
                "A ElevenLabs informou um valor de uso extra ilegível; o custo foi omitido.",
            ),
            _ => {}
        }
    }

    // Plan state (RF6).
    match body.get("status").and_then(Value::as_str) {
        Some("past_due") => notes.push(
            "note.eleven_labs.status.past_due",
            "A assinatura da ElevenLabs está com pagamento em atraso; os créditos exibidos podem não estar mais disponíveis.",
        ),
        Some("incomplete") => notes.push(
            "note.eleven_labs.status.incomplete",
            "A assinatura da ElevenLabs está incompleta; os créditos exibidos podem não estar mais disponíveis.",
        ),
        Some("free_disabled") => notes.push(
            "note.eleven_labs.status.free_disabled",
            "O plano gratuito da ElevenLabs está desativado; os créditos exibidos podem não estar mais disponíveis.",
        ),
        _ => {}
    }
    if body.get("has_open_invoices") == Some(&Value::Bool(true)) {
        notes.push(
            "note.eleven_labs.open_invoices",
            "A ElevenLabs informa faturas em aberto.",
        );
    }
    for (label, used_key, limit_key) in [
        ("Vozes personalizadas", "voice_slots_used", "voice_limit"),
        (
            "Vozes profissionais",
            "professional_voice_slots_used",
            "professional_voice_limit",
        ),
    ] {
        if let (Some(used), Some(limit)) = (
            non_negative(body.get(used_key)),
            non_negative(body.get(limit_key)),
        ) {
            metrics.push(Metric {
                label: label.into(),
                used: Some(used),
                limit: Some(limit),
                remaining: Some((limit - used).max(0.0)),
                unit: "vozes".into(),
                kind: MetricKind::Usage,
                window: None,
                period: None,
                resets_at: None,
                billing: Default::default(),
                scope: None,
            });
        }
    }
    notes.push(
        "note.eleven_labs.payg_not_reported",
        "Saldo Pay As You Go: não informado por este endpoint.",
    );

    Ok(Data::new(metrics, notes, source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, Provider};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "sk_test_eleven_secret";

    struct TestCredentials;
    impl Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_ELEVEN_KEY").then(|| KEY.into())
        }
    }

    fn account() -> Account {
        Account {
            id: None,
            name: "ElevenLabs".into(),
            provider: Provider::ElevenLabs,
            credential_env: Some("TEST_ELEVEN_KEY".into()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    async fn run(response: ResponseTemplate) -> (Result<Data, QueryError>, MockServer) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(PATH))
            .respond_with(response)
            .mount(&server)
            .await;
        let http = client();
        let ctx = Context {
            http: &http,
            credentials: &TestCredentials,
            timeout: Duration::from_secs(5),
        };
        let result = eleven_labs_at(&account(), &ctx, &server.uri()).await;
        (result, server)
    }

    async fn ok(body: Value) -> Data {
        run(ResponseTemplate::new(200).set_body_json(body))
            .await
            .0
            .expect("ready")
    }

    fn reset() -> i64 {
        (Utc::now() + chrono::Duration::days(12)).timestamp()
    }

    fn base(count: i64, limit: i64) -> Value {
        json!({
            "tier": "starter", "status": "active",
            "character_count": count, "character_limit": limit,
            "next_character_count_reset_unix": reset(),
            "character_refresh_period": "monthly_period",
            "max_credit_limit_extension": 0,
            "can_extend_character_limit": false,
            "current_overage": {"amount": "0.00", "currency": "usd"},
        })
    }

    fn ids(data: &Data) -> Vec<&str> {
        data.notes_l10n.iter().map(|n| &*n.id).collect()
    }

    fn metric<'a>(data: &'a Data, label: &str) -> Option<&'a Metric> {
        data.metrics.iter().find(|m| m.label == label)
    }

    #[test]
    fn region_allow_list_accepts_five_and_rejects_urls() {
        for (region, host) in [
            (None, "https://api.elevenlabs.io"),
            (Some("default"), "https://api.elevenlabs.io"),
            (Some("us"), "https://api.us.elevenlabs.io"),
            (Some("eu"), "https://api.eu.residency.elevenlabs.io"),
            (Some("in"), "https://api.in.residency.elevenlabs.io"),
            (Some("sg"), "https://api.sg.residency.elevenlabs.io"),
        ] {
            assert_eq!(base_for_region(region), Some(host));
        }
        assert_eq!(base_for_region(Some("https://evil.test")), None);
        assert_eq!(base_for_region(Some("")), None);
    }

    #[test]
    fn config_validate_rejects_a_free_form_region() {
        let mut bad = account();
        bad.params
            .insert("region".into(), "https://evil.test".into());
        let config = crate::Config {
            accounts: vec![bad],
            ..crate::Config::default()
        };
        assert!(matches!(config.validate(), Err(crate::ConfigError::Region)));
        for region in ["default", "us", "eu", "in", "sg"] {
            let mut good = account();
            good.params.insert("region".into(), region.into());
            let config = crate::Config {
                accounts: vec![good],
                ..crate::Config::default()
            };
            config.validate().expect(region);
        }
    }

    #[tokio::test]
    async fn request_shape_is_a_bare_get_with_one_key_header() {
        let (result, server) = run(ResponseTemplate::new(200).set_body_json(base(1, 10))).await;
        result.expect("ready");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method.as_str(), "GET");
        assert_eq!(request.url.path(), PATH);
        assert_eq!(request.url.query(), None);
        assert!(request.body.is_empty());
        assert_eq!(request.headers.get_all("xi-api-key").iter().count(), 1);
        assert_eq!(request.headers.get("xi-api-key").unwrap(), KEY);
        assert!(request.headers.get("authorization").is_none());
    }

    #[tokio::test]
    async fn normal_reports_a_monthly_quota_without_overage() {
        let data = ok(base(4000, 10000)).await;
        let credits = metric(&data, "Créditos do plano").unwrap();
        assert_eq!(credits.kind, MetricKind::Quota);
        assert_eq!(credits.window, Some(MetricWindow::Monthly));
        assert_eq!(
            (credits.used, credits.limit, credits.remaining),
            (Some(4000.0), Some(10000.0), Some(6000.0))
        );
        assert!(credits.resets_at.is_some());
        assert_eq!(data.metrics.len(), 1);
        let notes = ids(&data);
        assert!(notes.contains(&"note.eleven_labs.tier"));
        assert!(notes.contains(&"note.eleven_labs.payg_not_reported"));
    }

    #[tokio::test]
    async fn exhausted_credits_have_nothing_remaining() {
        let data = ok(base(10000, 10000)).await;
        let credits = metric(&data, "Créditos do plano").unwrap();
        assert_eq!(credits.remaining, Some(0.0));
        assert_eq!(credits.used, credits.limit);
    }

    #[tokio::test]
    async fn armed_reports_zero_extra_credits_against_the_cap() {
        let mut body = base(4000, 10000);
        body["max_credit_limit_extension"] = json!(5000);
        body["can_extend_character_limit"] = json!(true);
        let data = ok(body).await;
        let extra = metric(&data, "Créditos extras usados").unwrap();
        assert_eq!(extra.kind, MetricKind::Usage);
        assert_eq!(
            (extra.used, extra.limit, extra.remaining),
            (Some(0.0), Some(5000.0), Some(5000.0))
        );
        assert_eq!(
            metric(&data, "Custo do uso extra").map(|m| m.used),
            Some(Some(0.0))
        );
    }

    #[tokio::test]
    async fn extended_active_reports_credits_and_cost() {
        let mut body = base(10600, 10000);
        body["max_credit_limit_extension"] = json!(5000);
        body["can_extend_character_limit"] = json!(true);
        body["current_overage"] = json!({"amount": "0.18", "currency": "usd"});
        let data = ok(body).await;
        let credits = metric(&data, "Créditos do plano").unwrap();
        assert_eq!(
            (credits.used, credits.remaining),
            (Some(10600.0), Some(0.0))
        );
        let extra = metric(&data, "Créditos extras usados").unwrap();
        assert_eq!((extra.used, extra.limit), (Some(600.0), Some(5000.0)));
        let cost = metric(&data, "Custo do uso extra").unwrap();
        assert_eq!(cost.kind, MetricKind::Spend);
        assert_eq!((cost.used, cost.unit.as_str()), (Some(0.18), "USD"));
        // PRD 0023 RF1: extra credits and their cost are overage, the plan
        // credits stay included.
        assert_eq!(credits.billing, crate::model::Billing::Included);
        assert_eq!(extra.billing, crate::model::Billing::Overage);
        assert_eq!(cost.billing, crate::model::Billing::Overage);
    }

    #[tokio::test]
    async fn cap_reached_and_unlimited() {
        let mut body = base(15000, 10000);
        body["max_credit_limit_extension"] = json!(5000);
        body["can_extend_character_limit"] = json!(true);
        let data = ok(body).await;
        let extra = metric(&data, "Créditos extras usados").unwrap();
        assert_eq!(
            (extra.used, extra.limit, extra.remaining),
            (Some(5000.0), Some(5000.0), Some(0.0))
        );

        let mut body = base(12000, 10000);
        body["max_credit_limit_extension"] = json!("unlimited");
        body["can_extend_character_limit"] = json!(true);
        let data = ok(body).await;
        let extra = metric(&data, "Créditos extras usados").unwrap();
        assert_eq!(
            (extra.used, extra.limit, extra.remaining),
            (Some(2000.0), None, None)
        );
        assert!(ids(&data).contains(&"note.eleven_labs.overage_unlimited"));
    }

    #[tokio::test]
    async fn disabled_with_an_amount_still_reports_the_cost_only() {
        let mut body = base(10000, 10000);
        body["current_overage"] = json!({"amount": "2.50", "currency": "eur"});
        let data = ok(body).await;
        assert!(metric(&data, "Créditos extras usados").is_none());
        let cost = metric(&data, "Custo do uso extra").unwrap();
        assert_eq!((cost.used, cost.unit.as_str()), (Some(2.5), "EUR"));
    }

    #[tokio::test]
    async fn inconsistent_cap_is_treated_as_disabled() {
        let mut body = base(10000, 10000);
        body["max_credit_limit_extension"] = json!(5000);
        body["can_extend_character_limit"] = json!(false);
        let data = ok(body).await;
        assert!(metric(&data, "Créditos extras usados").is_none());
        assert!(ids(&data).contains(&"note.eleven_labs.overage_inconsistent"));
    }

    #[tokio::test]
    async fn deprecated_fields_are_a_fallback_only() {
        let mut body = base(10500, 10000);
        let object = body.as_object_mut().unwrap();
        object.remove("max_credit_limit_extension");
        object.insert("max_character_limit_extension".into(), json!(1000));
        object.insert("can_extend_character_limit".into(), json!(true));
        let data = ok(body).await;
        let extra = metric(&data, "Créditos extras usados").unwrap();
        assert_eq!((extra.used, extra.limit), (Some(500.0), Some(1000.0)));
    }

    #[tokio::test]
    async fn unreadable_amount_is_omitted_never_zero() {
        let mut body = base(4000, 10000);
        body["max_credit_limit_extension"] = json!(5000);
        body["can_extend_character_limit"] = json!(true);
        body["current_overage"] = json!({"amount": "lots", "currency": "usd"});
        let data = ok(body).await;
        assert!(metric(&data, "Custo do uso extra").is_none());
        assert!(ids(&data).contains(&"note.eleven_labs.overage_amount_unreadable"));
    }

    #[tokio::test]
    async fn missing_fields_mean_unknown_not_zero() {
        let data = ok(json!({
            "character_count": 1234,
            "next_character_count_reset_unix": null
        }))
        .await;
        assert_eq!(data.metrics.len(), 1);
        let credits = &data.metrics[0];
        assert_eq!(credits.used, Some(1234.0));
        assert_eq!(
            (credits.limit, credits.remaining, credits.resets_at),
            (None, None, None)
        );
        assert_eq!(credits.window, Some(MetricWindow::Other));
        assert!(ids(&data).contains(&"note.eleven_labs.limit_missing"));
    }

    #[tokio::test]
    async fn zero_limit_is_kept() {
        let data = ok(base(5, 0)).await;
        let credits = &data.metrics[0];
        assert_eq!((credits.limit, credits.remaining), (Some(0.0), Some(0.0)));
    }

    #[tokio::test]
    async fn bad_count_is_a_schema_error_and_bad_limit_is_dropped() {
        for count in [json!("1000"), json!(-1), json!(null)] {
            let mut body = base(1, 10);
            body["character_count"] = count;
            let (result, _) = run(ResponseTemplate::new(200).set_body_json(body)).await;
            assert_eq!(result.unwrap_err().note.id, "query.schema");
        }
        let mut body = base(1, 10);
        body.as_object_mut().unwrap().remove("character_count");
        let (result, _) = run(ResponseTemplate::new(200).set_body_json(body)).await;
        assert_eq!(result.unwrap_err().note.id, "query.schema");

        let mut body = base(7, 10);
        body["character_limit"] = json!(-4);
        let data = ok(body).await;
        assert_eq!(data.metrics[0].limit, None);
    }

    #[tokio::test]
    async fn exponent_numbers_are_accepted() {
        let mut body = base(1, 10);
        body["character_count"] = serde_json::from_str("1.0e4").unwrap();
        body["character_limit"] = serde_json::from_str("2.0e4").unwrap();
        let data = ok(body).await;
        assert_eq!(data.metrics[0].used, Some(10000.0));
    }

    #[tokio::test]
    async fn refresh_period_maps_to_window() {
        for (period, expected) in [
            (Some("monthly_period"), MetricWindow::Monthly),
            (Some("3_month_period"), MetricWindow::Other),
            (Some("6_month_period"), MetricWindow::Other),
            (Some("annual_period"), MetricWindow::Other),
            (None, MetricWindow::Other),
        ] {
            let mut body = base(1, 10);
            match period {
                Some(p) => body["character_refresh_period"] = json!(p),
                None => {
                    body.as_object_mut()
                        .unwrap()
                        .remove("character_refresh_period");
                }
            }
            assert_eq!(
                ok(body).await.metrics[0].window,
                Some(expected),
                "{period:?}"
            );
        }
    }

    #[tokio::test]
    async fn reset_is_in_seconds_and_invalid_values_are_none() {
        let mut body = base(1, 10);
        body["next_character_count_reset_unix"] = json!(1_800_000_000);
        let at = ok(body).await.metrics[0].resets_at.unwrap();
        assert_eq!(at.timestamp(), 1_800_000_000);
        for bad in [json!(0), json!(-5), json!(null), json!("x")] {
            let mut body = base(1, 10);
            body["next_character_count_reset_unix"] = bad;
            assert_eq!(ok(body).await.metrics[0].resets_at, None);
        }
    }

    #[tokio::test]
    async fn past_due_and_unknown_status_notes() {
        let mut body = base(1, 10);
        body["status"] = json!("past_due");
        body["has_open_invoices"] = json!(true);
        let data = ok(body).await;
        let notes = ids(&data);
        assert!(notes.contains(&"note.eleven_labs.status.past_due"));
        assert!(notes.contains(&"note.eleven_labs.open_invoices"));

        let mut body = base(1, 10);
        body["status"] = json!("something_new");
        let data = ok(body).await;
        assert!(!ids(&data).iter().any(|id| id.contains("status")));
    }

    #[tokio::test]
    async fn voice_slots_need_complete_pairs() {
        let mut body = base(1, 10);
        body["voice_slots_used"] = json!(3);
        body["voice_limit"] = json!(10);
        body["professional_voice_slots_used"] = json!(1);
        let data = ok(body).await;
        let voices = metric(&data, "Vozes personalizadas").unwrap();
        assert_eq!(voices.kind, MetricKind::Usage);
        assert_eq!((voices.used, voices.limit), (Some(3.0), Some(10.0)));
        assert!(metric(&data, "Vozes profissionais").is_none());
    }

    #[tokio::test]
    async fn unusual_tier_is_not_echoed() {
        let mut body = base(1, 10);
        body["tier"] = json!("Starter plan <b>");
        let data = ok(body).await;
        assert!(!ids(&data).contains(&"note.eleven_labs.tier"));
    }

    async fn failure_of(response: ResponseTemplate) -> QueryError {
        run(response).await.0.unwrap_err()
    }

    #[tokio::test]
    async fn http_errors_map_to_fixed_notes() {
        let body =
            json!({"detail": {"code": "invalid_api_key", "message": "x", "request_id": "req_1"}});
        let err = failure_of(ResponseTemplate::new(401).set_body_json(body)).await;
        assert_eq!(err.status, Status::AuthenticationError);
        assert_eq!(err.note.id, "note.eleven_labs.key_rejected");

        let body = json!({"detail": {"code": "insufficient_permissions"}});
        let err = failure_of(ResponseTemplate::new(403).set_body_json(body)).await;
        assert_eq!(err.note.id, "note.eleven_labs.scope_missing");
        let body = json!({"detail": {"code": "workspace_access_denied"}});
        let err = failure_of(ResponseTemplate::new(403).set_body_json(body)).await;
        assert_eq!(err.note.id, "note.eleven_labs.workspace_denied");

        for code in ["concurrent_limit_exceeded", "system_busy"] {
            let body = json!({"detail": {"code": code}});
            let err = failure_of(ResponseTemplate::new(429).set_body_json(body)).await;
            assert_eq!(err.status, Status::RateLimited);
            assert_eq!(err.note.id, "note.eleven_labs.rate_limited");
        }
        let err = failure_of(ResponseTemplate::new(500)).await;
        assert_eq!((err.status, &*err.note.id), (Status::Error, "query.http"));
        let err = failure_of(
            ResponseTemplate::new(503).set_body_json(json!({"detail": {"code": "maintenance"}})),
        )
        .await;
        assert_eq!((err.status, &*err.note.id), (Status::Error, "query.http"));
        let err = failure_of(ResponseTemplate::new(402)).await;
        assert_eq!(&*err.note.id, "query.http");
        let err = failure_of(ResponseTemplate::new(422)).await;
        assert_eq!(&*err.note.id, "query.schema");
    }

    #[tokio::test]
    async fn errors_never_leak_key_url_or_message() {
        let leak = format!("echo {KEY}");
        let body = json!({"detail": {"code": "invalid_api_key", "message": leak}});
        let err = failure_of(ResponseTemplate::new(401).set_body_json(body)).await;
        let rendered = format!("{err:?} {}", serde_json::to_string(&err.note).unwrap());
        assert!(!rendered.contains(KEY));
        assert!(!rendered.contains("127.0.0.1"));
        assert!(!rendered.contains("echo"));
    }

    #[tokio::test]
    async fn malformed_json_and_redirects_are_errors() {
        let err = failure_of(ResponseTemplate::new(200).set_body_string("not json")).await;
        assert_eq!(&*err.note.id, "query.schema");
        let err = failure_of(ResponseTemplate::new(200).set_body_json(json!([1, 2]))).await;
        assert_eq!(&*err.note.id, "query.schema");
        let err =
            failure_of(ResponseTemplate::new(302).insert_header("location", "https://evil.test/"))
                .await;
        assert_eq!((err.status, &*err.note.id), (Status::Error, "query.http"));
    }

    #[tokio::test]
    async fn successful_notes_carry_no_secret_or_url() {
        let data = ok(base(1, 10)).await;
        let all = format!("{:?}", data.notes);
        assert!(!all.contains(KEY) && !all.contains("http"));
    }
}
