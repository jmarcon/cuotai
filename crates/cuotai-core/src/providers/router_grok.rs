//! Read-only Grok CLI (Grok Build) internal quota endpoints observed in 9router
//! f01fb909e37189008080632ddaf404f096345cde (`open-sse/services/usage/grok-cli.js`
//! and `grokCliQuotaFrame.js`). See THIRD_PARTY_NOTICES for MIT attribution.
//! These are internal endpoints of the official Grok CLI: they require the CLI
//! OAuth access token (device code login), never the Management API key, and are
//! subject to change by the provider. No inference is generated and no
//! quota-reset or credit-consume operation is performed.
use super::{json, send as secure_send, Context, Data, QueryError};
use crate::text::{params, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Provider};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

const REF: &str = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/";
const BASE: &str = "https://cli-chat-proxy.grok.com";
const GRPC_BASE: &str = "https://grok.com";
const CLIENT_VERSION: &str = "0.2.99";
const CLIENT_IDENTIFIER: &str = "grok-shell";
const USER_AGENT: &str = "grok-shell/0.2.99 (linux; x86_64)";
/// Empty gRPC-web request frame (flag 0 + length 0); without it upstream
/// answers grpc-status 13 "Missing request message." with a 0-byte body.
static GRPC_WEB_EMPTY_REQUEST_FRAME: [u8; 5] = [0, 0, 0, 0, 0];

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::Grok => grok_cli(account, ctx, BASE, GRPC_BASE).await,
        _ => Err(QueryError::unsupported(
            "query.router_grok.provider_not_supported",
            "Provedor sem consulta neste adapter.",
        )),
    }
}

pub(crate) fn grok_headers(
    request: reqwest::RequestBuilder,
    token: &str,
    account: &Account,
) -> reqwest::RequestBuilder {
    let mut request = request
        .bearer_auth(token)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .header("x-xai-token-auth", "xai-grok-cli")
        .header("x-grok-client-identifier", CLIENT_IDENTIFIER)
        .header("x-grok-client-version", CLIENT_VERSION)
        .header("x-grok-client-mode", "headless");
    if let Some(email) = account.params.get("email").filter(|v| !v.trim().is_empty()) {
        request = request.header("x-email", email);
    }
    if let Some(user_id) = account
        .params
        .get("user_id")
        .filter(|v| !v.trim().is_empty())
    {
        request = request.header("x-userid", user_id);
    }
    request
}

async fn grok_cli(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
    grpc_base: &str,
) -> Result<Data, QueryError> {
    let env = account
        .params
        .get("session_env")
        .map(String::as_str)
        .unwrap_or("GROK_CLI_ACCESS_TOKEN");
    let token = ctx
        .credentials
        .get(env)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            QueryError::config_params(
                "query.router_grok.missing_oauth_token",
                params([("env", ParamValue::from(env.to_string()))]),
                format!(
                    "Token de acesso OAuth do Grok CLI ausente em {env}. A chave X_GROK_API_KEY da Management API não autentica estes endpoints internos; aponte params.session_env apenas para uma variável com o token do login device code."
                ),
            )
        })?;

    let billing: Value = json(grok_headers(
        ctx.http.get(format!("{base}/v1/billing?format=credits")),
        &token,
        account,
    ))
    .await?;
    // The user profile enriches the plan label only; any failure is non-fatal.
    let user: Option<Value> = json(grok_headers(
        ctx.http.get(format!("{base}/v1/user?include=subscription")),
        &token,
        account,
    ))
    .await
    .ok();

    let parsed = parse_billing(&billing, user.as_ref());
    let plan = plan_from_access_token(&token)
        .unwrap_or(&parsed.plan)
        .to_owned();

    let mut notes = Notes::new();
    notes.push(
        "note.router_grok.internal_endpoints",
        "Endpoints internos do Grok CLI oficial (cli-chat-proxy.grok.com /v1/billing?format=credits e /v1/user?include=subscription), observados no 9router; sujeitos a mudanças e restrições do provedor.",
    );
    notes.push(
        "note.router_grok.credit_unit",
        "Valores em créditos Grok Build (format=credits), não em USD.",
    );
    notes.push_params(
        "note.router_grok.plan",
        params([("plan", ParamValue::from(plan.clone()))]),
        format!("Plano: {plan}"),
    );
    let mut metrics = parsed.metrics;
    if parsed.on_demand_exhausted {
        notes.push(
            "note.router_grok.on_demand_exhausted",
            "Cota sob demanda esgotada (cap 0): neste estado o provedor bloqueia o chat com 402 spending-limit.",
        );
    }
    if parsed.has_prepaid {
        notes.push(
            "note.router_grok.prepaid_total_unknown",
            "Pré-pago: o provedor expõe apenas o saldo restante; o total recarregado é desconhecido.",
        );
    }

    if metrics.is_empty() {
        // Paid SuperGrok often returns cap=0 over REST but exposes the shared
        // weekly pool on GetGrokCreditsConfig — try it before giving up.
        if let Some(grpc) = fetch_grpc_credits(ctx, grpc_base, &token).await {
            let used = grpc.percent_used.round().clamp(0.0, 100.0);
            metrics.push(Metric {
                billing: Default::default(),
                scope: None,
                label: "Grok CLI: semanal SuperGrok".into(),
                used: Some(used),
                limit: Some(100.0),
                remaining: Some(100.0 - used),
                unit: "%".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Weekly),
                period: Some("semanal".into()),
                resets_at: grpc.reset_at,
            });
            notes.push(
                "note.router_grok.supergrok_weekly_pool",
                "Pool semanal SuperGrok via GetGrokCreditsConfig (gRPC-web interno em grok.com); o REST não expôs quota numérica.",
            );
        } else {
            if parsed.subscription_access {
                notes.push(
                    "note.router_grok.subscription_quota_unavailable",
                    "Assinatura ativa; a Grok não expõe quota numérica incluída e o pool semanal gRPC não respondeu.",
                );
            } else {
                notes.push(
                    "note.router_grok.no_credit_allocation",
                    "Conectado, mas nenhuma alocação de créditos foi retornada; a promoção gratuita pode estar esgotada.",
                );
            }
            notes.push(
                "note.router_grok.no_numeric_quota",
                "Nenhuma quota numérica retornada; ausente não significa ilimitado.",
            );
        }
    }
    Ok(Data::new(metrics, notes, format!("{REF}grok-cli.js")))
}

struct ParsedBilling {
    plan: String,
    subscription_access: bool,
    metrics: Vec<Metric>,
    on_demand_exhausted: bool,
    has_prepaid: bool,
}

/// Unwrap protobuf-json `{ val: n }` or plain numbers/numeric strings.
fn unwrap_val(value: Option<&Value>) -> Option<f64> {
    let inner = match value? {
        Value::Object(map) => map.get("val")?,
        other => other,
    };
    let number = match inner {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

fn field<'a>(config: &'a Value, root: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names
        .iter()
        .find_map(|name| config.get(name).or_else(|| root.get(name)))
}

fn epoch_reset(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() {
        return None;
    }
    // Provider timestamps may be seconds or milliseconds.
    let millis = if value < 1e12 { value * 1000.0 } else { value };
    DateTime::from_timestamp_millis(millis as i64)
}

fn parse_reset(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            if text.bytes().all(|b| b.is_ascii_digit()) {
                epoch_reset(text.parse().ok()?)
            } else {
                DateTime::parse_from_rfc3339(text)
                    .map(|t| t.with_timezone(&Utc))
                    .ok()
            }
        }
        Value::Number(n) => epoch_reset(n.as_f64()?),
        _ => None,
    }
}

fn period_end(config: &Value, root: &Value) -> Option<DateTime<Utc>> {
    [
        config.get("billingPeriodEnd"),
        config.get("billing_period_end"),
        config.get("currentPeriod").and_then(|p| p.get("end")),
        config
            .get("resetAt")
            .or_else(|| config.get("resetsAt"))
            .or_else(|| config.get("periodEnd")),
        root.get("billingPeriodEnd"),
        root.get("billing_period_end"),
        root.get("resetAt")
            .or_else(|| root.get("resetsAt"))
            .or_else(|| root.get("periodEnd")),
    ]
    .into_iter()
    .flatten()
    .find_map(parse_reset)
}

fn subscription_tier<'a>(user: Option<&'a Value>, config: &'a Value) -> Option<&'a str> {
    user.and_then(|u| {
        u.get("subscriptionTier")
            .or_else(|| u.get("subscription_tier"))
            .or_else(|| u.get("subscription").and_then(|s| s.get("tier")))
            .and_then(Value::as_str)
    })
    .or_else(|| {
        config
            .get("subscriptionTier")
            .or_else(|| config.get("subscription_tier"))
            .and_then(Value::as_str)
    })
    .map(str::trim)
    .filter(|t| !t.is_empty())
}

fn titleize(tier: &str) -> String {
    tier.split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn resolve_plan(user: Option<&Value>, config: &Value) -> String {
    if let Some(tier) = subscription_tier(user, config) {
        return titleize(tier);
    }
    if user
        .and_then(|u| u.get("hasGrokCodeAccess"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return "Grok Code".into();
    }
    "Grok Build".into()
}

/// Display only; upstream remains authoritative for access and quota enforcement.
fn plan_from_access_token(token: &str) -> Option<&'static str> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let tier = serde_json::from_slice::<Value>(&decoded)
        .ok()?
        .get("tier")?
        .as_u64()?;
    Some(match tier {
        0 => "Free",
        1 => "SuperGrok",
        2 => "X Basic",
        3 => "X Premium",
        4 => "X Premium Plus",
        5 => "SuperGrok Heavy",
        6 => "SuperGrok Lite",
        _ => return None,
    })
}

fn quota_metric(
    label: &str,
    used: Option<f64>,
    limit: Option<f64>,
    unit: &str,
    period: Option<&str>,
    window: Option<MetricWindow>,
    resets_at: Option<DateTime<Utc>>,
) -> Metric {
    let remaining = match (used, limit) {
        (Some(used), Some(limit)) => Some((limit - used).max(0.0)),
        _ => None,
    };
    Metric {
        billing: Default::default(),
        scope: None,
        label: label.into(),
        used,
        limit,
        remaining,
        unit: unit.into(),
        kind: MetricKind::Quota,
        window,
        period: period.map(Into::into),
        resets_at,
    }
}

fn parse_billing(billing: &Value, user: Option<&Value>) -> ParsedBilling {
    let root = billing;
    let config = billing
        .get("config")
        .filter(|v| v.is_object())
        .unwrap_or(billing);
    let resets_at = period_end(config, root);
    let tier = subscription_tier(user, config);
    let subscription_access =
        tier.is_some_and(|t| !matches!(t.to_ascii_lowercase().as_str(), "free" | "none" | "null"));

    let mut metrics = Vec::new();
    let mut on_demand_exhausted = false;
    let mut has_prepaid = false;

    // Current Grok Build responses expose included monthly usage at top level.
    if let Some(limit) =
        unwrap_val(field(config, root, &["monthlyLimit", "monthly_limit"])).filter(|l| *l > 0.0)
    {
        let used = unwrap_val(field(config, root, &["includedUsed", "included_used"]))
            .or_else(|| unwrap_val(field(config, root, &["totalUsed", "total_used"])));
        metrics.push(quota_metric(
            "Grok CLI: mensal incluído",
            used,
            Some(limit),
            "créditos",
            Some("mensal"),
            Some(MetricWindow::Monthly),
            resets_at,
        ));
    }

    // Primary: on-demand spending window (subscription / promo credits).
    let cap = unwrap_val(field(config, root, &["onDemandCap"]));
    let used = unwrap_val(field(config, root, &["onDemandUsed"]));
    match cap {
        Some(cap) if cap > 0.0 => metrics.push(
            quota_metric(
                "Grok CLI: sob demanda",
                used,
                Some(cap),
                "créditos",
                Some("ciclo de cobrança"),
                Some(MetricWindow::Monthly),
                resets_at,
            )
            .overage(),
        ),
        // Cap 0 is the exhausted free/promo state (chat returns 402
        // spending-limit). The zero limit is real data from the provider.
        Some(0.0) if !subscription_access && used.is_some() => {
            on_demand_exhausted = true;
            metrics.push(Metric {
                billing: crate::Billing::Overage,
                scope: None,
                label: "Grok CLI: sob demanda".into(),
                used,
                limit: Some(0.0),
                remaining: Some(0.0),
                unit: "créditos".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Monthly),
                period: Some("ciclo de cobrança".into()),
                resets_at,
            });
        }
        _ => {}
    }

    // Prepaid top-up balance: only the remaining pot is known, no allotment.
    if let Some(balance) = unwrap_val(field(config, root, &["prepaidBalance"])).filter(|b| *b > 0.0)
    {
        has_prepaid = true;
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label: "Grok CLI: pré-pago".into(),
            used: None,
            limit: None,
            remaining: Some(balance),
            unit: "créditos".into(),
            kind: MetricKind::Credits,
            window: None,
            period: None,
            resets_at: None,
        });
    }

    // SuperGrok weekly shared-pool usage. creditUsagePercent is the single
    // total used %; productUsage is a breakdown legend, never separate bars.
    if let Some(percent) = unwrap_val(field(
        config,
        root,
        &["creditUsagePercent", "credit_usage_percent"],
    ))
    .filter(|p| *p >= 0.0)
    {
        let used = percent.clamp(0.0, 100.0);
        metrics.push(Metric {
            billing: Default::default(),
            scope: None,
            label: "Grok CLI: semanal SuperGrok".into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: Some(100.0 - used),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Weekly),
            period: Some("semanal".into()),
            resets_at,
        });
    }

    ParsedBilling {
        plan: resolve_plan(user, config),
        subscription_access,
        metrics,
        on_demand_exhausted,
        has_prepaid,
    }
}

// --- gRPC-web GetGrokCreditsConfig frame decoding (grokCliQuotaFrame.js) ---
//
// Real response shape (live capture 2026-07-20):
//   top-level field 1 (length-delimited) — nested credits info
//     subfield 1  (fixed32 float)            — usage ratio 0..1
//     subfield 5  (Timestamp{seconds,nanos}) — credit-pool reset time
// Fail-open: any malformed buffer returns None, never panics.

struct GrpcCredits {
    percent_used: f64,
    reset_at: Option<DateTime<Utc>>,
}

enum ProtoField<'a> {
    Varint(u64),
    Fixed32(&'a [u8]),
    Fixed64(&'a [u8]),
    Bytes(&'a [u8]),
}

/// Valid gRPC-web frame header → (flag, payload_start, payload_length).
fn probe_frame_header(buf: &[u8], offset: usize) -> Option<(u8, usize, usize)> {
    let flag = *buf.get(offset)?;
    if !matches!(flag, 0x00 | 0x01 | 0x80 | 0x81) {
        return None;
    }
    let length = u32::from_be_bytes(buf.get(offset + 1..offset + 5)?.try_into().ok()?) as usize;
    let payload_start = offset + 5;
    if length > buf.len().checked_sub(payload_start)? {
        return None;
    }
    Some((flag, payload_start, length))
}

fn read_varint(buf: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(pos)?;
        result |= u64::from(byte & 0x7f) << shift;
        pos += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    Some((result, pos))
}

fn read_field(buf: &[u8], offset: usize) -> Option<(u32, ProtoField<'_>, usize)> {
    let (tag, next) = read_varint(buf, offset)?;
    let number = (tag >> 3) as u32;
    if number == 0 {
        return None;
    }
    match tag & 0x7 {
        0 => {
            let (value, next) = read_varint(buf, next)?;
            Some((number, ProtoField::Varint(value), next))
        }
        1 => {
            let bytes = buf.get(next..next + 8)?;
            Some((number, ProtoField::Fixed64(bytes), next + 8))
        }
        2 => {
            let (length, start) = read_varint(buf, next)?;
            let end = start.checked_add(length as usize)?;
            let bytes = buf.get(start..end)?;
            Some((number, ProtoField::Bytes(bytes), end))
        }
        5 => {
            let bytes = buf.get(next..next + 4)?;
            Some((number, ProtoField::Fixed32(bytes), next + 4))
        }
        _ => None,
    }
}

fn decode_fields(buf: &[u8]) -> Option<HashMap<u32, ProtoField<'_>>> {
    let mut fields = HashMap::new();
    let mut offset = 0;
    while offset < buf.len() {
        let (number, field, next) = read_field(buf, offset)?;
        fields.insert(number, field);
        offset = next;
    }
    Some(fields)
}

fn find_data_frame_payload(buf: &[u8]) -> Option<&[u8]> {
    let mut offset = 0;
    while offset < buf.len() {
        let (flag, start, length) = probe_frame_header(buf, offset)?;
        let end = start + length;
        if flag & 0x80 == 0 {
            return buf.get(start..end);
        }
        offset = end;
    }
    None
}

fn timestamp_reset(field: Option<&ProtoField<'_>>) -> Option<DateTime<Utc>> {
    let bytes = match field {
        Some(ProtoField::Bytes(bytes)) => bytes,
        _ => return None,
    };
    let fields = decode_fields(bytes)?;
    let seconds = match fields.get(&1) {
        Some(ProtoField::Varint(v)) => *v,
        _ => 0,
    };
    let nanos = match fields.get(&2) {
        Some(ProtoField::Varint(v)) => *v,
        _ => 0,
    };
    let millis = (seconds as i64)
        .checked_mul(1000)?
        .checked_add((nanos as f64 / 1_000_000.0).round() as i64)?;
    DateTime::from_timestamp_millis(millis)
}

fn decode_grok_credits_frame(buf: &[u8]) -> Option<GrpcCredits> {
    if buf.is_empty() {
        return None;
    }
    let payload: &[u8] = if probe_frame_header(buf, 0).is_some() {
        find_data_frame_payload(buf)?
    } else {
        buf
    };
    let top = decode_fields(payload)?;
    let credits = match top.get(&1) {
        Some(ProtoField::Bytes(bytes)) => decode_fields(bytes)?,
        _ => return None,
    };
    // Proto3 omission of the usage ratio means 0% used.
    let ratio = match credits.get(&1) {
        None => 0.0,
        Some(ProtoField::Fixed32(bytes)) => {
            f64::from(f32::from_le_bytes(<[u8; 4]>::try_from(*bytes).ok()?))
        }
        Some(ProtoField::Fixed64(bytes)) => f64::from_le_bytes(<[u8; 8]>::try_from(*bytes).ok()?),
        Some(_) => return None,
    };
    if !ratio.is_finite() || ratio < 0.0 {
        return None;
    }
    Some(GrpcCredits {
        percent_used: (ratio * 100.0).min(100.0),
        reset_at: timestamp_reset(credits.get(&5)),
    })
}

/// Live SuperGrok weekly pool; fail-open on any network/auth/parse failure.
async fn fetch_grpc_credits(
    ctx: &Context<'_>,
    grpc_base: &str,
    token: &str,
) -> Option<GrpcCredits> {
    let response = secure_send(
        ctx.http
            .post(format!(
                "{grpc_base}/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig"
            ))
            .bearer_auth(token)
            .header("Content-Type", "application/grpc-web+proto")
            .header("X-Grpc-Web", "1")
            .header("Accept", "application/grpc-web+proto")
            .body(&GRPC_WEB_EMPTY_REQUEST_FRAME[..]),
    )
    .await
    .ok()?;
    if !response.status().is_success() {
        return None;
    }
    decode_grok_credits_frame(&response.bytes().await.ok()?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn grok_on_demand_cap_zero_is_disabled_not_cap_reached() {
        use crate::overage::{assess_metrics, OverageState};
        let billing = serde_json::json!({"config":{
            "onDemandCap":{"val":0},"onDemandUsed":{"val":0},
            "billingPeriodEnd":"2026-07-24T12:42:26+00:00"}});
        let user = serde_json::json!({"subscriptionTier":"Free"});
        let parsed = parse_billing(&billing, Some(&user));
        assert!(parsed.on_demand_exhausted);
        let state = assess_metrics(&parsed.metrics, None).unwrap();
        assert_eq!(state.state, OverageState::Disabled);
        // A positive cap in use is active; a spent cap is reached.
        for (used, want) in [
            (2.0, OverageState::Active),
            (10.0, OverageState::CapReached),
        ] {
            let billing = serde_json::json!({"config":{
                "onDemandCap":{"val":10},"onDemandUsed":{"val":used}}});
            let parsed = parse_billing(&billing, Some(&user));
            assert_eq!(assess_metrics(&parsed.metrics, None).unwrap().state, want);
        }
    }

    use super::*;
    use crate::Credentials;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    struct Creds;
    impl Credentials for Creds {
        fn get(&self, _: &str) -> Option<String> {
            Some("test-token".into())
        }
    }
    struct NoCreds;
    impl Credentials for NoCreds {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    fn account(params: &[(&str, &str)]) -> Account {
        Account {
            id: None,
            name: "test".into(),
            provider: Provider::Grok,
            credential_env: None,
            session: None,
            oauth: None,
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            refresh_seconds: None,
            group: None,
        }
    }

    fn ctx<'a>(http: &'a reqwest::Client, creds: &'a dyn Credentials) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: Duration::from_secs(2),
        }
    }

    // Minimal protobuf/gRPC-web encoders mirroring the reference fixtures.
    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                break;
            }
        }
        bytes
    }
    fn encode_tag(field: u64, wire: u64) -> Vec<u8> {
        encode_varint((field << 3) | wire)
    }
    fn fixed32_field(field: u64, value: f32) -> Vec<u8> {
        [encode_tag(field, 5), value.to_le_bytes().to_vec()].concat()
    }
    fn length_delimited(field: u64, body: &[u8]) -> Vec<u8> {
        [
            encode_tag(field, 2),
            encode_varint(body.len() as u64),
            body.to_vec(),
        ]
        .concat()
    }
    fn varint_field(field: u64, value: u64) -> Vec<u8> {
        [encode_tag(field, 0), encode_varint(value)].concat()
    }
    fn timestamp_field(field: u64, seconds: u64, nanos: u64) -> Vec<u8> {
        let mut body = Vec::new();
        if seconds != 0 {
            body.extend(varint_field(1, seconds));
        }
        if nanos != 0 {
            body.extend(varint_field(2, nanos));
        }
        length_delimited(field, &body)
    }
    fn credits_info(ratio: Option<f32>, reset: Option<(u64, u64)>) -> Vec<u8> {
        let mut body = Vec::new();
        if let Some(ratio) = ratio {
            body.extend(fixed32_field(1, ratio));
        }
        if let Some((seconds, nanos)) = reset {
            body.extend(timestamp_field(5, seconds, nanos));
        }
        body
    }
    fn frame_data(payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0x00];
        frame.extend((payload.len() as u32).to_be_bytes());
        frame.extend(payload);
        frame
    }
    fn frame_trailer() -> Vec<u8> {
        let body = b"grpc-status:0\r\n";
        let mut frame = vec![0x80];
        frame.extend((body.len() as u32).to_be_bytes());
        frame.extend(body);
        frame
    }
    fn credits_response(ratio: f32, reset: (u64, u64)) -> Vec<u8> {
        let message = length_delimited(1, &credits_info(Some(ratio), Some(reset)));
        [frame_data(&message), frame_trailer()].concat()
    }

    const RESET_SECONDS: u64 = 1784825940;
    const RESET_NANOS: u64 = 867850000;

    fn expected_reset() -> DateTime<Utc> {
        DateTime::from_timestamp_millis((RESET_SECONDS * 1000 + RESET_NANOS / 1_000_000) as i64)
            .unwrap()
            // JS rounds nanos/1e6 (867.85 → 868); assert the same instant.
            + chrono::Duration::milliseconds(1)
    }

    fn exhausted_billing() -> Value {
        serde_json::json!({"config":{
            "currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY",
                "start":"2026-07-08T00:00:00+00:00","end":"2026-07-15T00:00:00+00:00"},
            "onDemandCap":{"val":0},"onDemandUsed":{"val":0},
            "isUnifiedBillingUser":true,"prepaidBalance":{"val":0},
            "billingPeriodStart":"2026-07-08T00:00:00+00:00",
            "billingPeriodEnd":"2026-07-15T00:00:00+00:00"}})
    }
    fn user_profile() -> Value {
        serde_json::json!({"userId":"d84768dd-224d-4052-ba49-0d336fa9160c",
            "email":"user@example.com","hasGrokCodeAccess":true,"subscriptionTier":null})
    }

    #[test]
    fn decodes_real_framed_shape_with_trailer() {
        let buffer = credits_response(1.0, (RESET_SECONDS, RESET_NANOS));
        let decoded = decode_grok_credits_frame(&buffer).unwrap();
        assert_eq!(decoded.percent_used, 100.0);
        assert_eq!(decoded.reset_at, Some(expected_reset()));
    }

    #[test]
    fn decodes_unframed_payload_and_proto3_defaults() {
        let payload = length_delimited(
            1,
            &credits_info(Some(0.75), Some((RESET_SECONDS, RESET_NANOS))),
        );
        assert!(probe_frame_header(&payload, 0).is_none());
        let decoded = decode_grok_credits_frame(&payload).unwrap();
        assert!((decoded.percent_used - 75.0).abs() < 1e-4);
        // Omitted ratio is the proto3 default 0%.
        let payload = length_delimited(1, &credits_info(None, Some((RESET_SECONDS, RESET_NANOS))));
        let decoded = decode_grok_credits_frame(&frame_data(&payload)).unwrap();
        assert_eq!(decoded.percent_used, 0.0);
        assert_eq!(decoded.reset_at, Some(expected_reset()));
    }

    #[test]
    fn clamps_ratio_above_one_and_rejects_negative() {
        let payload = length_delimited(1, &credits_info(Some(1.5), None));
        assert_eq!(
            decode_grok_credits_frame(&frame_data(&payload))
                .unwrap()
                .percent_used,
            100.0
        );
        let payload = length_delimited(1, &credits_info(Some(-0.1), None));
        assert!(decode_grok_credits_frame(&frame_data(&payload)).is_none());
    }

    #[test]
    fn rejects_malformed_frames() {
        // Top-level field 1 not length-delimited.
        assert!(decode_grok_credits_frame(&frame_data(&varint_field(1, 42))).is_none());
        // No top-level field 1.
        assert!(decode_grok_credits_frame(&frame_data(&varint_field(9, 1))).is_none());
        // Nested usage ratio with unexpected wire type.
        let bad = length_delimited(1, &length_delimited(1, b"not-a-float"));
        assert!(decode_grok_credits_frame(&frame_data(&bad)).is_none());
        // Truncated, trailer-only and empty bodies. The reference fixture
        // truncates a data-only frame (no trailer) by 3 bytes; the decoder
        // must refuse to return a value when the payload itself is cut.
        let data_only = frame_data(&length_delimited(
            1,
            &credits_info(Some(0.5), Some((RESET_SECONDS, RESET_NANOS))),
        ));
        assert!(decode_grok_credits_frame(&data_only[..data_only.len() - 3]).is_none());
        assert!(decode_grok_credits_frame(&frame_trailer()).is_none());
        assert!(decode_grok_credits_frame(&[]).is_none());
        // Header rejecting oversize declared length and invalid flags.
        let mut header = vec![0x00];
        header.extend(9999u32.to_be_bytes());
        header.extend([0x01, 0x02]);
        assert!(probe_frame_header(&header, 0).is_none());
        let mut bad_flag = vec![0x07];
        bad_flag.extend(0u32.to_be_bytes());
        assert!(probe_frame_header(&bad_flag, 0).is_none());
    }

    #[tokio::test]
    async fn active_billing_maps_credits_and_preserves_reset_without_grpc() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/billing"))
            .and(query_param("format", "credits"))
            .and(header("x-grok-client-identifier", "grok-shell"))
            .and(header("x-grok-client-version", "0.2.99"))
            .and(header("x-xai-token-auth", "xai-grok-cli"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"config":{
                "currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY",
                    "start":"2026-07-08T00:00:00+00:00","end":"2026-07-15T00:00:00+00:00"},
                "onDemandCap":{"val":100},"onDemandUsed":{"val":35},
                "isUnifiedBillingUser":true,"prepaidBalance":{"val":12.5},
                "billingPeriodStart":"2026-07-08T00:00:00+00:00",
                "billingPeriodEnd":"2026-07-15T00:00:00+00:00"}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/user"))
            .and(query_param("include", "subscription"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_profile()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = grok_cli(
            &account(&[("email", "user@example.com"), ("user_id", "d84768dd")]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        let on_demand = &data.metrics[0];
        assert_eq!(on_demand.label, "Grok CLI: sob demanda");
        assert_eq!(on_demand.used, Some(35.0));
        assert_eq!(on_demand.limit, Some(100.0));
        assert_eq!(on_demand.kind, MetricKind::Quota);
        assert_eq!(on_demand.window, Some(MetricWindow::Monthly));
        assert_eq!(on_demand.remaining, Some(65.0));
        assert_eq!(on_demand.unit, "créditos");
        assert_eq!(
            on_demand.resets_at.unwrap().to_rfc3339(),
            "2026-07-15T00:00:00+00:00"
        );
        let prepaid = &data.metrics[1];
        assert_eq!(prepaid.remaining, Some(12.5));
        assert_eq!(prepaid.used, None);
        assert_eq!(prepaid.limit, None);
        assert_eq!(prepaid.kind, MetricKind::Credits);
        assert_eq!(prepaid.window, None);
        assert!(data.notes.iter().any(|n| n.contains("Plano: Grok Code")));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2, "REST quotas present: no gRPC fallback");
        let billing = &requests[0];
        assert_eq!(
            billing.headers.get("authorization").unwrap(),
            "Bearer test-token"
        );
        assert_eq!(billing.headers.get("x-email").unwrap(), "user@example.com");
        assert_eq!(billing.headers.get("x-userid").unwrap(), "d84768dd");
        assert_eq!(
            billing.headers.get("x-grok-client-mode").unwrap(),
            "headless"
        );
    }

    #[tokio::test]
    async fn exhausted_free_account_shows_real_zero_cap_without_grpc() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/billing"))
            .respond_with(ResponseTemplate::new(200).set_body_json(exhausted_billing()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_profile()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = grok_cli(
            &account(&[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        let on_demand = &data.metrics[0];
        assert_eq!(on_demand.used, Some(0.0));
        assert_eq!(on_demand.limit, Some(0.0));
        assert_eq!(on_demand.remaining, Some(0.0));
        assert!(data.notes.iter().any(|n| n.contains("esgotada")));
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            2,
            "depleted bar exists: no gRPC fallback"
        );
    }

    #[tokio::test]
    async fn paid_subscription_without_rest_quota_uses_grpc_weekly_pool() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/billing"))
            .respond_with(ResponseTemplate::new(200).set_body_json(exhausted_billing()))
            .mount(&server)
            .await;
        let mut paid_user = user_profile();
        paid_user["subscriptionTier"] = Value::from("XPremiumPlus");
        Mock::given(method("GET"))
            .and(path("/v1/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(paid_user))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig"))
            .and(header("Content-Type", "application/grpc-web+proto"))
            .and(header("X-Grpc-Web", "1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(credits_response(0.35, (RESET_SECONDS, RESET_NANOS))),
            )
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = grok_cli(
            &account(&[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        let weekly = &data.metrics[0];
        assert_eq!(weekly.label, "Grok CLI: semanal SuperGrok");
        assert_eq!(weekly.used, Some(35.0));
        assert_eq!(weekly.limit, Some(100.0));
        assert_eq!(weekly.kind, MetricKind::Quota);
        assert_eq!(weekly.window, Some(MetricWindow::Weekly));
        assert_eq!(weekly.remaining, Some(65.0));
        assert_eq!(weekly.unit, "%");
        assert_eq!(weekly.resets_at, Some(expected_reset()));
        assert!(data.notes.iter().any(|n| n.contains("Plano: XPremiumPlus")));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 3);
        let grpc = &requests[2];
        assert_eq!(grpc.body, GRPC_WEB_EMPTY_REQUEST_FRAME);
        assert_eq!(
            grpc.headers.get("authorization").unwrap(),
            "Bearer test-token"
        );
    }

    #[tokio::test]
    async fn grpc_failure_keeps_subscription_note_without_fabricated_metrics() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/billing"))
            .respond_with(ResponseTemplate::new(200).set_body_json(exhausted_billing()))
            .mount(&server)
            .await;
        let mut paid_user = user_profile();
        paid_user["subscriptionTier"] = Value::from("XPremiumPlus");
        Mock::given(method("GET"))
            .and(path("/v1/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(paid_user))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = grok_cli(
            &account(&[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert!(data.metrics.is_empty());
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("não expõe quota numérica incluída")));
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("ausente não significa ilimitado")));
    }

    #[tokio::test]
    async fn missing_token_is_config_error_naming_session_env() {
        let http = reqwest::Client::new();
        let error = grok_cli(
            &account(&[]),
            &ctx(&http, &NoCreds),
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, crate::Status::NeedsConfiguration);
        assert!(error.note.fallback.contains("GROK_CLI_ACCESS_TOKEN"));
        assert!(error.note.fallback.contains("X_GROK_API_KEY"));
        // An explicit session_env override is honored in the message.
        let error = grok_cli(
            &account(&[("session_env", "MY_GROK_TOKEN")]),
            &ctx(&http, &NoCreds),
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap_err();
        assert!(error.note.fallback.contains("MY_GROK_TOKEN"));
    }

    #[test]
    fn monthly_fields_and_weekly_percent_are_single_bars() {
        let billing = serde_json::json!({
            "monthlyLimit":{"val":1000},"includedUsed":{"val":275},"totalUsed":{"val":300},
            "resetAt":"2026-08-01T00:00:00Z"});
        let user = serde_json::json!({"subscription_tier":"premium_plus"});
        let parsed = parse_billing(&billing, Some(&user));
        assert_eq!(parsed.plan, "Premium Plus");
        let monthly = &parsed.metrics[0];
        assert_eq!(monthly.label, "Grok CLI: mensal incluído");
        assert_eq!(monthly.used, Some(275.0));
        assert_eq!(monthly.limit, Some(1000.0));
        assert_eq!(monthly.remaining, Some(725.0));
        assert_eq!(monthly.kind, MetricKind::Quota);
        assert_eq!(monthly.window, Some(MetricWindow::Monthly));
        assert_eq!(
            monthly.resets_at.unwrap().to_rfc3339(),
            "2026-08-01T00:00:00+00:00"
        );

        // productUsage is a breakdown legend: only one shared-pool bar exists.
        let billing = serde_json::json!({"config":{
            "currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY",
                "start":"2026-07-17T12:42:26.494595+00:00","end":"2026-07-24T12:42:26.494595+00:00"},
            "creditUsagePercent":99.0,
            "onDemandCap":{"val":0},"onDemandUsed":{"val":0},
            "productUsage":[{"product":"GrokBuild","usagePercent":97.0},
                            {"product":"GrokImagine","usagePercent":2.0}],
            "isUnifiedBillingUser":true,"prepaidBalance":{"val":0},
            "billingPeriodEnd":"2026-07-24T12:42:26.494595+00:00"}});
        let user = serde_json::json!({"subscriptionTier":"XPremiumPlus","hasGrokCodeAccess":true});
        let parsed = parse_billing(&billing, Some(&user));
        assert_eq!(parsed.metrics.len(), 1);
        let weekly = &parsed.metrics[0];
        assert_eq!(weekly.label, "Grok CLI: semanal SuperGrok");
        assert_eq!(weekly.used, Some(99.0));
        assert_eq!(weekly.remaining, Some(1.0));
        assert_eq!(weekly.kind, MetricKind::Quota);
        assert_eq!(weekly.window, Some(MetricWindow::Weekly));
        assert_eq!(
            weekly.resets_at.unwrap().to_rfc3339(),
            "2026-07-24T12:42:26.494595+00:00"
        );
        // Paid tier with cap 0 is not reported as depleted on-demand credit.
        assert!(parsed.subscription_access);
        assert!(!parsed.on_demand_exhausted);
    }
}
