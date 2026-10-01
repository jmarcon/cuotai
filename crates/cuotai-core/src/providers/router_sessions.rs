//! Read-only internal quota interfaces observed in 9router f01fb909e37189008080632ddaf404f096345cde
//! (MIT; see THIRD_PARTY_NOTICES). Session-token routes: Kiro, CodeBuddy, Zed.
//! Only read endpoints from the reference usage handlers are called — no token
//! generation/refresh, purchase, or credit-consumption endpoints. These are
//! unofficial internal endpoints and may change without notice.
//! Kiro overage/bonus semantics additionally follow CodexBar
//! (steipete/CodexBar KiroUsageLimitsAPI.swift), which mirrors the official
//! Kiro CLI's GetUsageLimits handling.
use super::{json, param, secret, Context, Data, QueryError};
use crate::{Account, Metric, Provider, Status};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use reqwest::RequestBuilder;
use serde_json::Value;
use std::collections::BTreeMap;

const REF: &str = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/";

// Kiro (providers/registry/kiro.js + config/kiroConstants.js): fixed us-east-1 hosts.
const KIRO_CW_HOST: &str = "https://codewhisperer.us-east-1.amazonaws.com";
const KIRO_Q_HOST: &str = "https://q.us-east-1.amazonaws.com";
const KIRO_LIMITS_PATH: &str = "/getUsageLimits";
const KIRO_BUILDER_PROFILE_ARN: &str =
    "arn:aws:codewhisperer:us-east-1:638616132270:profile/AAAACCCCXXXX";
const KIRO_SOCIAL_PROFILE_ARN: &str =
    "arn:aws:codewhisperer:us-east-1:699475941385:profile/EHGA3GRVQMUK";

// CodeBuddy (providers/registry/codebuddy-cn.js + codebuddy-intl.js).
const CODEBUDDY_CN_URL: &str = "https://copilot.tencent.com/v2/billing/meter/get-user-resource";
const CODEBUDDY_INTL_URL: &str = "https://www.codebuddy.ai/v2/billing/meter/get-user-resource";
const CODEBUDDY_CN_AGENT: &str = "CLI/2.108.1 CodeBuddy/2.108.1";
const CODEBUDDY_INTL_AGENT: &str = "IDE/2.108.1 CodeBuddy/2.108.1";

// CodexBar KiroUsageLimitsAPI profileEndpoints: eu-central-1 profiles resolve
// against the regional Q host.
const KIRO_EU_Q_HOST: &str = "https://q.eu-central-1.amazonaws.com";

// Zed (providers/registry/zed.js + shared/zedAuth.js).
const ZED_CLOUD: &str = "https://cloud.zed.dev";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    match account.provider {
        Provider::Kiro => kiro(account, ctx, KIRO_CW_HOST, KIRO_Q_HOST, KIRO_EU_Q_HOST).await,
        Provider::CodeBuddy => match account
            .params
            .get("region")
            .map(String::as_str)
            .unwrap_or("cn")
        {
            "cn" => codebuddy(account, ctx, CODEBUDDY_CN_URL, CODEBUDDY_CN_AGENT, "CLI").await,
            "intl" => {
                codebuddy(
                    account,
                    ctx,
                    CODEBUDDY_INTL_URL,
                    CODEBUDDY_INTL_AGENT,
                    "IDE",
                )
                .await
            }
            other => Err(QueryError::config(format!(
                "params.region desconhecido: {other}. Use cn ou intl."
            ))),
        },
        Provider::Zed => zed(account, ctx, ZED_CLOUD).await,
        _ => Err(QueryError::unsupported(
            "Provedor sem consulta neste adapter.",
        )),
    }
}

/// 9router coerces with Number() and falls back to 0; here a missing or
/// non-numeric value stays unknown (None) instead of becoming a fake zero.
fn number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64().filter(|v| v.is_finite()),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}

/// Prefer the *Precise string fields (exact), fall back to the numeric ones.
/// Mirrors `num(precise ?? plain)`: if the precise field exists but is not
/// numeric the plain field is NOT consulted (it stays unknown).
fn precise(precise_value: Option<&Value>, plain: Option<&Value>) -> Option<f64> {
    if precise_value.is_some() {
        number(precise_value)
    } else {
        number(plain)
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        _ => true,
    }
}

fn epoch_reset(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() {
        return None;
    }
    // Reference threshold: below 1e12 the timestamp is seconds, else milliseconds.
    let millis = if value < 1e12 { value * 1000.0 } else { value };
    DateTime::from_timestamp_millis(millis as i64)
}

/// Mirrors 9router parseResetTime: epoch seconds/ms, numeric strings, RFC3339,
/// plus Tencent billing "YYYY-MM-DD HH:MM:SS" (treated as UTC). Unparseable
/// values mean unknown, not an error.
fn parse_reset(value: Option<&Value>) -> Option<DateTime<Utc>> {
    match value {
        Some(Value::Number(n)) => n.as_f64().and_then(epoch_reset),
        Some(Value::String(s)) => {
            let text = s.trim();
            if text.is_empty() {
                return None;
            }
            if text.bytes().all(|b| b.is_ascii_digit()) {
                return text.parse::<f64>().ok().and_then(epoch_reset);
            }
            DateTime::parse_from_rfc3339(text)
                .map(|t| t.with_timezone(&Utc))
                .ok()
                .or_else(|| {
                    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|d| d.and_utc())
                })
                .or_else(|| {
                    NaiveDate::parse_from_str(text, "%Y-%m-%d")
                        .ok()
                        .and_then(|d| d.and_hms_opt(0, 0, 0))
                        .map(|d| d.and_utc())
                })
        }
        _ => None,
    }
}

fn quota_metric(
    label: String,
    used: Option<f64>,
    limit: Option<f64>,
    unit: &str,
    period: Option<&str>,
    resets_at: Option<DateTime<Utc>>,
) -> Option<Metric> {
    if used.is_none() && limit.is_none() {
        return None;
    }
    let remaining = used.zip(limit).map(|(u, l)| l - u);
    Some(Metric {
        label,
        used,
        limit,
        remaining,
        unit: unit.into(),
        period: period.map(str::to_owned),
        resets_at,
    })
}

// ---------------------------------------------------------------------------
// Kiro (usage/kiro.js): GET/POST CodeWhisperer getUsageLimits, fallback to
// Amazon Q host. Bearer token plus auth-method headers; profileArn per method.
// When the account supplies its own profileArn, the native credit path the
// official Kiro CLI uses (CodexBar KiroUsageLimitsAPI: POST {profileArn} to the
// ARN's regional host, CREDIT breakdowns) is tried before the legacy attempts.
// ---------------------------------------------------------------------------

/// Validate a user-supplied profileArn and derive its regional host, mirroring
/// CodexBar readIdentity/fetch: exactly arn:aws:codewhisperer:<region>:<account>
/// :profile/<id>, no whitespace/control chars, and only the two regions with
/// known fixed hosts. Anything else is a config error, never a guess.
fn kiro_native_host<'a>(
    arn: &str,
    cw_host: &'a str,
    eu_q_host: &'a str,
) -> Result<&'a str, QueryError> {
    let invalid = || {
        QueryError::config(
            "params.profile_arn não é um ARN de perfil CodeWhisperer válido (arn:aws:codewhisperer:<região>:<conta>:profile/<id>).",
        )
    };
    if arn.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid());
    }
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() != 6
        || parts[0] != "arn"
        || parts[1] != "aws"
        || parts[2] != "codewhisperer"
        || !parts[5].starts_with("profile/")
        || parts[5].len() == "profile/".len()
    {
        return Err(invalid());
    }
    match parts[3] {
        "us-east-1" => Ok(cw_host),
        "eu-central-1" => Ok(eu_q_host),
        region => Err(QueryError::config(format!(
            "params.profile_arn usa a região {region}, sem host conhecido. Regiões suportadas: us-east-1, eu-central-1."
        ))),
    }
}

async fn kiro(
    account: &Account,
    ctx: &Context<'_>,
    cw_host: &str,
    q_host: &str,
    eu_q_host: &str,
) -> Result<Data, QueryError> {
    let token = secret(account, ctx, "KIRO_ACCESS_TOKEN")?;
    let auth_method = account
        .params
        .get("auth_method")
        .map(String::as_str)
        .unwrap_or("builder-id");
    match auth_method {
        "builder-id" | "idc" | "google" | "github" | "import" | "api_key" | "external_idp" => {}
        other => {
            return Err(QueryError::config(format!(
                "params.auth_method desconhecido: {other}. Use builder-id, idc, google, github, import, api_key ou external_idp."
            )));
        }
    }
    // api_key never gets the shared placeholder profileArn (upstream 403s a
    // foreign ARN); other methods fall back to the shared defaults.
    let explicit_arn = account
        .params
        .get("profile_arn")
        .filter(|s| !s.trim().is_empty())
        .cloned();
    let profile_arn = explicit_arn.clone().or_else(|| match auth_method {
        "api_key" => None,
        "google" | "github" => Some(KIRO_SOCIAL_PROFILE_ARN.to_string()),
        _ => Some(KIRO_BUILDER_PROFILE_ARN.to_string()),
    });
    let auth_header = format!("Bearer {token}");
    let typed_headers = |request: RequestBuilder| match auth_method {
        "api_key" => request.header("tokentype", "API_KEY"),
        "external_idp" => request.header("TokenType", "EXTERNAL_IDP"),
        _ => request,
    };
    let get_query = [
        ("isEmailRequired", "true"),
        ("origin", "AI_EDITOR"),
        ("resourceType", "AGENTIC_REQUEST"),
    ];
    let mut post_body =
        serde_json::json!({"origin": "AI_EDITOR", "resourceType": "AGENTIC_REQUEST"});
    if let Some(arn) = &profile_arn {
        post_body["profileArn"] = Value::String(arn.clone());
    }
    let mut q_query = vec![("origin", "AI_EDITOR"), ("resourceType", "AGENTIC_REQUEST")];
    if let Some(arn) = &profile_arn {
        q_query.push(("profileArn", arn.as_str()));
    }
    // Per-attempt timeout (CodexBar KiroUsageLimitsAPI uses 10s per request): a
    // hung endpoint must not stall the whole query or the fallback chain.
    let mut attempts: Vec<RequestBuilder> = Vec::with_capacity(4);
    // Native credit path first, only when the account supplied its own real
    // profileArn — the shared placeholder defaults stay on the 9router legacy
    // attempts (a placeholder ARN is not owned by the account and would 403).
    let native_attempts = if let Some(arn) = &explicit_arn {
        let host = kiro_native_host(arn, cw_host, eu_q_host)?;
        attempts.push(typed_headers(
            ctx.http
                .post(host)
                .header("Authorization", &auth_header)
                .header("Content-Type", "application/x-amz-json-1.0")
                .header("x-amz-target", "AmazonCodeWhispererService.GetUsageLimits")
                .header("Accept", "application/json")
                .body(serde_json::json!({"profileArn": arn}).to_string())
                .timeout(ctx.timeout),
        ));
        1
    } else {
        0
    };
    attempts.extend([
        typed_headers(
            ctx.http
                .get(format!("{cw_host}{KIRO_LIMITS_PATH}"))
                .query(&get_query)
                .header("Authorization", &auth_header)
                .header("Accept", "application/json")
                .header("x-amz-user-agent", "aws-sdk-js/1.0.0 KiroIDE")
                .header("user-agent", "aws-sdk-js/1.0.0 KiroIDE")
                .timeout(ctx.timeout),
        ),
        typed_headers(
            ctx.http
                .post(cw_host)
                .header("Authorization", &auth_header)
                .header("Content-Type", "application/x-amz-json-1.0")
                .header("x-amz-target", "AmazonCodeWhispererService.GetUsageLimits")
                .header("Accept", "application/json")
                .body(post_body.to_string())
                .timeout(ctx.timeout),
        ),
        typed_headers(
            ctx.http
                .get(format!("{q_host}{KIRO_LIMITS_PATH}"))
                .query(&q_query)
                .header("Authorization", &auth_header)
                .header("Accept", "application/json")
                .timeout(ctx.timeout),
        ),
    ]);
    let mut saw_auth = false;
    let mut saw_transport = false;
    let mut last_status = None;
    for (index, attempt) in attempts.into_iter().enumerate() {
        let response = match attempt.send().await {
            Ok(response) => response,
            Err(_) => {
                saw_transport = true;
                continue;
            }
        };
        let status = response.status();
        if !status.is_success() {
            if matches!(status.as_u16(), 401 | 403) {
                saw_auth = true;
            } else {
                last_status = Some(status.as_u16());
            }
            continue;
        }
        match response.json::<Value>().await {
            Ok(body) => {
                let data = kiro_data(body)?;
                // Native path: a 200 without any numeric quota is not an
                // answer — fall through to the legacy attempts instead of
                // reporting an empty, fake-ready result.
                if index < native_attempts && data.metrics.is_empty() {
                    continue;
                }
                return Ok(data);
            }
            Err(_) => continue,
        }
    }
    // Never include response bodies or URLs: they can carry the session token.
    if saw_auth {
        Err(QueryError {
            status: Status::AuthenticationError,
            message: "API de quota do Kiro rejeitou o token (HTTP 401/403). Renove a sessão no Kiro e atualize a credencial."
                .into(),
        })
    } else if let Some(code) = last_status {
        Err(QueryError {
            status: Status::Error,
            message: format!("Consulta de quota do Kiro retornou HTTP {code}."),
        })
    } else if saw_transport {
        Err(QueryError::transport())
    } else {
        Err(QueryError::schema())
    }
}

fn kiro_data(body: Value) -> Result<Data, QueryError> {
    // Per-breakdown nextDateReset wins over the top-level one (CodexBar
    // KiroUsageLimitsAPI.parse: credit.nextDateReset ?? response.nextDateReset).
    let top_reset =
        parse_reset(body.get("nextDateReset")).or_else(|| parse_reset(body.get("resetDate")));
    // Overage (official Kiro CLI GetUsageLimits shape, per CodexBar):
    // currentUsageWithPrecision is the TOTAL including overage spend; the plan
    // alone is total minus currentOveragesWithPrecision. overageConfiguration
    // .overageStatus gates whether overageCapWithPrecision is a real ceiling.
    let overage_status = body
        .pointer("/overageConfiguration/overageStatus")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_uppercase())
        .and_then(|s| match s.as_str() {
            "ENABLED" => Some(true),
            "DISABLED" => Some(false),
            _ => None,
        });
    let plan = body
        .pointer("/subscriptionInfo/subscriptionTitle")
        .and_then(Value::as_str)
        .unwrap_or("Kiro")
        .to_string();
    let mut metrics = Vec::new();
    let mut notes = Vec::new();
    let empty = Vec::new();
    let breakdowns = body
        .get("usageBreakdownList")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for breakdown in breakdowns {
        let resource = breakdown
            .get("resourceType")
            .and_then(Value::as_str)
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".into());
        let reset_at = parse_reset(breakdown.get("nextDateReset")).or(top_reset);
        let total_used = number(breakdown.get("currentUsageWithPrecision"));
        let overage_used = number(breakdown.get("currentOveragesWithPrecision"));
        // Split plan usage from overage only when the relation is consistent
        // (overage larger than the total is contradictory — keep the total
        // rather than inventing a split). Overuse above the limit is preserved:
        // remaining goes negative, never clamped.
        let plan_used = match (total_used, overage_used) {
            (Some(total), Some(overage)) if overage <= total => Some(total - overage),
            _ => total_used,
        };
        if let Some(metric) = quota_metric(
            format!("Kiro: {resource}"),
            plan_used,
            number(breakdown.get("usageLimitWithPrecision")),
            "créditos",
            None,
            reset_at,
        ) {
            metrics.push(metric);
        }
        // Overage allowance only exists when the account opted in (ENABLED);
        // a DISABLED or omitted status means the cap field is not a ceiling.
        let overage_cap = match overage_status {
            Some(true) => number(breakdown.get("overageCapWithPrecision")),
            _ => None,
        };
        if let Some(metric) = quota_metric(
            format!("Kiro: {resource} (excedente)"),
            overage_used.filter(|v| *v > 0.0),
            overage_cap,
            "créditos",
            None,
            reset_at,
        ) {
            metrics.push(metric);
        }
        if overage_status == Some(true) {
            if overage_cap.is_none() {
                notes.push(format!(
                    "Excedente habilitado para {resource}, mas a API não informou o teto."
                ));
            }
            let currency = breakdown
                .get("currency")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("USD");
            if let Some(charges) = number(breakdown.get("overageCharges")) {
                match number(breakdown.get("overageRate")) {
                    Some(rate) => notes.push(format!(
                        "Excedente {resource}: {charges} {currency} cobrados ({rate} {currency} por crédito)."
                    )),
                    None => notes.push(format!(
                        "Excedente {resource}: {charges} {currency} cobrados."
                    )),
                }
            }
        }
        // Bonus spend is folded into currentUsage (CodexBar hasUnseparatedBonus):
        // with bonuses the plan figure cannot be separated from bonus spend, so
        // plan usage legitimately exceeds the plan limit. Say so; do not clamp.
        if breakdown
            .get("bonuses")
            .and_then(Value::as_array)
            .is_some_and(|b| !b.is_empty())
        {
            notes.push(format!(
                "{resource}: bônus ativos somados ao uso; o consumo do plano não é separável e pode exceder o limite do plano."
            ));
        }
        if let Some(trial) = breakdown.get("freeTrialInfo") {
            let trial_reset = parse_reset(trial.get("freeTrialExpiry")).or(reset_at);
            if let Some(metric) = quota_metric(
                format!("Kiro: {resource} (teste gratuito)"),
                number(trial.get("currentUsageWithPrecision")),
                number(trial.get("usageLimitWithPrecision")),
                "créditos",
                None,
                trial_reset,
            ) {
                metrics.push(metric);
            }
        }
    }
    notes.splice(
        0..0,
        [
            format!("Plano: {plan}"),
            "Endpoint interno getUsageLimits (CodeWhisperer/Amazon Q, us-east-1) observado no 9router; não documentado e sujeito a mudanças.".into(),
        ],
    );
    if metrics.is_empty() {
        notes.push("Nenhuma quota numérica retornada; ausente não significa ilimitado.".into());
    }
    Ok(Data {
        metrics,
        notes,
        source: format!("{REF}kiro.js"),
    })
}

// ---------------------------------------------------------------------------
// CodeBuddy (usage/codebuddy-cn.js): POST Tencent billing get-user-resource.
// Refill packs (cycle ends long before resource expiry) use the *Cycle*
// balances and renew; bonus packs are one-shot and use the plain Capacity
// balances. CN and Intl differ only in host and CLI/IDE headers.
// ---------------------------------------------------------------------------

async fn codebuddy(
    account: &Account,
    ctx: &Context<'_>,
    url: &str,
    user_agent: &str,
    ide_type: &str,
) -> Result<Data, QueryError> {
    let token = secret(account, ctx, "CODEBUDDY_ACCESS_TOKEN")?;
    let envelope: Value = json(
        ctx.http
            .post(url)
            .header("User-Agent", user_agent)
            .header("X-Product", "SaaS")
            .header("X-IDE-Type", ide_type)
            .header("X-IDE-Name", ide_type)
            .header("x-requested-with", "XMLHttpRequest")
            .header("x-codebuddy-request", "1")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body("{}"),
    )
    .await?;
    let code = envelope
        .get("code")
        .and_then(Value::as_i64)
        .ok_or_else(QueryError::schema)?;
    if code != 0 {
        // The server msg field is deliberately not propagated (response body).
        return Err(QueryError {
            status: Status::Error,
            message: format!("Quota CodeBuddy retornou código {code}."),
        });
    }
    let empty = Vec::new();
    let accounts = envelope
        .pointer("/data/Response/Data/Accounts")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let cycle_end_ms =
        |acc: &Value| parse_reset(acc.get("CycleEndTime")).map(|t| t.timestamp_millis() as f64);
    // Refill packs roll into a new cycle before the resource expires; bonus
    // packs end exactly at expiry. >2d gap between cycle end and validity end.
    const REFILL_GAP_MS: f64 = 2.0 * 24.0 * 60.0 * 60.0 * 1000.0;
    let is_refill = |acc: &Value| match (cycle_end_ms(acc), number(acc.get("DeductionEndTime"))) {
        (Some(cycle_end), Some(deduction_end)) => deduction_end - cycle_end > REFILL_GAP_MS,
        _ => false,
    };
    let expiry_key = |acc: &&Value| cycle_end_ms(acc).unwrap_or(f64::INFINITY);
    let mut refills: Vec<&Value> = accounts.iter().filter(|acc| is_refill(acc)).collect();
    let mut bonuses: Vec<&Value> = accounts.iter().filter(|acc| !is_refill(acc)).collect();
    refills.sort_by(|a, b| {
        expiry_key(a)
            .partial_cmp(&expiry_key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    bonuses.sort_by(|a, b| {
        expiry_key(a)
            .partial_cmp(&expiry_key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut metrics = Vec::new();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for acc in &refills {
        let cadence = refill_cadence(acc);
        let count = seen.entry(cadence).or_insert(0);
        *count += 1;
        let name = if *count > 1 {
            format!("{cadence} {count}")
        } else {
            cadence.to_string()
        };
        if let Some(metric) = quota_metric(
            format!("CodeBuddy: {name}"),
            precise(
                acc.get("CycleCapacityUsedPrecise"),
                acc.get("CycleCapacityUsed"),
            ),
            precise(
                acc.get("CycleCapacitySizePrecise"),
                acc.get("CycleCapacitySize"),
            ),
            "créditos",
            Some(cadence_period(cadence)),
            parse_reset(acc.get("CycleEndTime")),
        ) {
            metrics.push(metric);
        }
    }
    for (index, acc) in bonuses.iter().enumerate() {
        if let Some(metric) = quota_metric(
            format!("CodeBuddy: Bonus Pack {}", index + 1),
            precise(acc.get("CapacityUsedPrecise"), acc.get("CapacityUsed")),
            precise(acc.get("CapacitySizePrecise"), acc.get("CapacitySize")),
            "créditos",
            None,
            parse_reset(acc.get("CycleEndTime")),
        ) {
            metrics.push(metric);
        }
    }
    let base = refills.first().copied().or_else(|| accounts.first());
    let plan = base
        .and_then(|acc| {
            acc.get("PackageName")
                .or_else(|| acc.get("SubProductName"))
                .and_then(Value::as_str)
        })
        .unwrap_or("CodeBuddy")
        .to_string();
    let mut notes = vec![
        format!("Plano: {plan}"),
        "Endpoint interno de billing get-user-resource observado no 9router; não documentado e sujeito a mudanças.".into(),
    ];
    if !refills.is_empty() {
        notes.push(
            "Pacotes Monthly/Weekly/Daily renovam em resets_at (próxima recarga do ciclo).".into(),
        );
    }
    if !bonuses.is_empty() {
        notes.push("Bonus Pack são créditos únicos: expiram em resets_at sem renovação.".into());
    }
    if metrics.is_empty() {
        notes.push("Nenhum pacote de crédito encontrado na resposta.".into());
    }
    Ok(Data {
        metrics,
        notes,
        source: format!("{REF}codebuddy-cn.js"),
    })
}

/// Label a refill pack by its cycle length (Monthly is the common case).
fn refill_cadence(acc: &Value) -> &'static str {
    let start = parse_reset(acc.get("CycleStartTime"));
    let end = parse_reset(acc.get("CycleEndTime"));
    if let (Some(start), Some(end)) = (start, end) {
        let days = (end - start).num_milliseconds() as f64 / 86_400_000.0;
        if days <= 1.5 {
            return "Daily";
        }
        if days <= 10.0 {
            return "Weekly";
        }
    }
    "Monthly"
}

fn cadence_period(cadence: &str) -> &'static str {
    match cadence {
        "Daily" => "diário",
        "Weekly" => "semanal",
        _ => "mensal",
    }
}

// ---------------------------------------------------------------------------
// Zed (usage/zed.js + shared/zedAuth.js): GET cloud.zed.dev/client/users/me
// with "Authorization: <user_id> <access_token>". Quotas come from plan.usage
// (edit_predictions, optional model_requests) and subscription_period.ended_at.
// ---------------------------------------------------------------------------

async fn zed(account: &Account, ctx: &Context<'_>, cloud_base: &str) -> Result<Data, QueryError> {
    let token = secret(account, ctx, "ZED_ACCESS_TOKEN")?;
    let user_id = param(account, "user_id")?;
    let mut request = ctx
        .http
        .get(format!("{cloud_base}/client/users/me"))
        .header("Accept", "application/json")
        .header("Authorization", format!("{user_id} {token}"));
    if let Some(system_id) = account
        .params
        .get("system_id")
        .filter(|s| !s.trim().is_empty())
    {
        request = request.header("x-zed-system-id", system_id);
    }
    let user: Value = json(request).await?;
    Ok(zed_data(&user))
}

/// Zed UsageLimit JSON: "unlimited", {unlimited:true}, a number, or {limited:N}.
/// Reference coerces unknown shapes to 0; here they stay unknown (None limit).
enum ZedLimit {
    Unlimited,
    Limited(f64),
    Unknown,
}

fn zed_limit(value: Option<&Value>) -> ZedLimit {
    match value {
        Some(Value::String(s)) => {
            let text = s.trim();
            if text.eq_ignore_ascii_case("unlimited") {
                ZedLimit::Unlimited
            } else {
                number(value)
                    .map(|v| ZedLimit::Limited(v.max(0.0)))
                    .unwrap_or(ZedLimit::Unknown)
            }
        }
        Some(Value::Number(_)) => number(value)
            .map(|v| ZedLimit::Limited(v.max(0.0)))
            .unwrap_or(ZedLimit::Unknown),
        Some(Value::Object(map)) => {
            if truthy(map.get("unlimited")) {
                return ZedLimit::Unlimited;
            }
            number(map.get("limited").or_else(|| map.get("Limited")))
                .map(|v| ZedLimit::Limited(v.max(0.0)))
                .unwrap_or(ZedLimit::Unknown)
        }
        _ => ZedLimit::Unknown,
    }
}

fn zed_data(user: &Value) -> Data {
    let plan = user.get("plan").cloned().unwrap_or(Value::Null);
    let plan_id = plan
        .get("plan_v3")
        .or_else(|| plan.get("plan_v2"))
        .or_else(|| plan.get("plan"))
        .or_else(|| user.get("plan_v3"))
        .and_then(Value::as_str);
    let reset_at = parse_reset(plan.pointer("/subscription_period/ended_at"))
        .or_else(|| parse_reset(plan.pointer("/subscriptionPeriod/endedAt")));
    let mut label = format_zed_plan_label(plan_id);
    if truthy(
        plan.get("trial_started_at")
            .or_else(|| plan.get("trialStartedAt")),
    ) && !label.to_lowercase().contains("trial")
    {
        label = format!("{label} (Trial active)");
    }
    let mut metrics = Vec::new();
    let mut notes = vec![
        format!("Plano: {label}"),
        "Endpoint interno /client/users/me (cloud.zed.dev) com Authorization \"<user_id> <token>\", observado no 9router; não documentado e sujeito a mudanças.".into(),
    ];
    let usage = plan.get("usage").cloned().unwrap_or(Value::Null);
    if let Some(bucket) = usage
        .get("edit_predictions")
        .or_else(|| usage.get("editPredictions"))
    {
        let used = number(bucket.get("used")).map(|v| v.max(0.0));
        match zed_limit(bucket.get("limit")) {
            ZedLimit::Unlimited => {
                metrics.push(Metric {
                    label: "Zed: Edit Predictions".into(),
                    used,
                    limit: None,
                    remaining: None,
                    unit: "solicitações".into(),
                    period: Some("ciclo de assinatura".into()),
                    resets_at: reset_at,
                });
                notes.push("Edit Predictions: ilimitado conforme resposta do provedor.".into());
            }
            ZedLimit::Limited(total) => {
                metrics.push(Metric {
                    label: "Zed: Edit Predictions".into(),
                    used,
                    limit: Some(total),
                    remaining: used.map(|u| (total - u).max(0.0)),
                    unit: "solicitações".into(),
                    period: Some("ciclo de assinatura".into()),
                    resets_at: reset_at,
                });
            }
            ZedLimit::Unknown => {
                if let Some(metric) = quota_metric(
                    "Zed: Edit Predictions".into(),
                    used,
                    None,
                    "solicitações",
                    Some("ciclo de assinatura"),
                    reset_at,
                ) {
                    metrics.push(metric);
                }
            }
        }
    }
    if let Some(bucket) = usage
        .get("model_requests")
        .or_else(|| usage.get("modelRequests"))
    {
        // A bare value is the limit itself; an object carries {used, limit}.
        let (used, limit_value) = if bucket.is_object() {
            (
                number(bucket.get("used")).map(|v| v.max(0.0)),
                bucket.get("limit").cloned(),
            )
        } else {
            (None, Some(bucket.clone()))
        };
        match zed_limit(limit_value.as_ref()) {
            ZedLimit::Unlimited => {
                metrics.push(Metric {
                    label: "Zed: Hosted Model Requests".into(),
                    used,
                    limit: None,
                    remaining: None,
                    unit: "solicitações".into(),
                    period: Some("ciclo de assinatura".into()),
                    resets_at: reset_at,
                });
                notes.push("Hosted Model Requests: ilimitado conforme resposta do provedor.".into());
            }
            ZedLimit::Limited(total) if total > 0.0 => {
                metrics.push(Metric {
                    label: "Zed: Hosted Model Requests".into(),
                    used,
                    limit: Some(total),
                    remaining: used.map(|u| (total - u).max(0.0)),
                    unit: "solicitações".into(),
                    period: Some("ciclo de assinatura".into()),
                    resets_at: reset_at,
                });
            }
            // limit {limited:0}/0/null on Pro/Student means token billing,
            // not a request quota — surface a note instead of a fake zero cap.
            _ => notes.push(
                "Modelos hospedados são cobrados por token (não por requisição); o gasto por token está em dashboard.zed.dev."
                    .into(),
            ),
        }
    }
    if truthy(
        plan.get("has_overdue_invoices")
            .or_else(|| plan.get("hasOverdueInvoices")),
    ) {
        notes.push(
            "Conta Zed com faturas vencidas; o uso pode estar bloqueado até a regularização."
                .into(),
        );
    }
    if metrics.is_empty() {
        notes.push("Nenhuma quota numérica retornada; ausente não significa ilimitado.".into());
    }
    Data {
        metrics,
        notes,
        source: format!("{REF}zed.js"),
    }
}

/// Map plan_v3 ids to dashboard labels (CodexBar-compatible, per reference).
fn format_zed_plan_label(raw: Option<&str>) -> String {
    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return "Zed".into();
    }
    match raw.to_lowercase().as_str() {
        "zed_free" => "Zed Free".into(),
        "zed_pro" => "Zed Pro".into(),
        "zed_pro_trial" => "Zed Pro Trial".into(),
        "zed_student" => "Zed Student".into(),
        "zed_business" => "Zed Business".into(),
        other => other
            .replace('_', " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => {
                        first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use std::time::Duration;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Creds;
    impl Credentials for Creds {
        fn get(&self, _: &str) -> Option<String> {
            Some("test-token".into())
        }
    }

    fn account(provider: Provider, params: &[(&str, &str)]) -> Account {
        Account {
            name: "test".into(),
            provider,
            credential_env: None,
            oauth: None,
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            refresh_seconds: None,
        }
    }

    fn ctx<'a>(http: &'a reqwest::Client, creds: &'a Creds) -> Context<'a> {
        Context {
            http,
            credentials: creds,
            timeout: Duration::from_secs(2),
        }
    }

    fn header_value(request: &wiremock::Request, name: &str) -> Option<String> {
        request
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    }

    // -- Kiro ---------------------------------------------------------------

    fn kiro_fixture() -> Value {
        serde_json::json!({
            "subscriptionInfo": {"subscriptionTitle": "Kiro Pro"},
            "nextDateReset": "2026-10-01T00:00:00Z",
            "usageBreakdownList": [{
                "resourceType": "AGENTIC_REQUEST",
                "currentUsageWithPrecision": 120.5,
                "usageLimitWithPrecision": 500,
                "freeTrialInfo": {
                    "currentUsageWithPrecision": 10,
                    "usageLimitWithPrecision": 50,
                    "freeTrialExpiry": "2026-07-15T00:00:00Z"
                }
            }]
        })
    }

    #[tokio::test]
    async fn kiro_parses_breakdown_free_trial_and_reset() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(kiro_fixture()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 2);
        let main = &data.metrics[0];
        assert_eq!(main.label, "Kiro: agentic_request");
        assert_eq!(main.used, Some(120.5));
        assert_eq!(main.limit, Some(500.0));
        assert_eq!(main.remaining, Some(379.5));
        assert_eq!(
            main.resets_at.unwrap().to_rfc3339(),
            "2026-10-01T00:00:00+00:00"
        );
        let trial = &data.metrics[1];
        assert_eq!(trial.label, "Kiro: agentic_request (teste gratuito)");
        assert_eq!(trial.used, Some(10.0));
        assert_eq!(trial.limit, Some(50.0));
        assert_eq!(
            trial.resets_at.unwrap().to_rfc3339(),
            "2026-07-15T00:00:00+00:00"
        );
        assert!(data.notes.iter().any(|n| n.contains("Kiro Pro")));
        assert!(data.source.contains("kiro.js"));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            header_value(&requests[0], "authorization").as_deref(),
            Some("Bearer test-token")
        );
        assert_eq!(
            header_value(&requests[0], "x-amz-user-agent").as_deref(),
            Some("aws-sdk-js/1.0.0 KiroIDE")
        );
    }

    #[tokio::test]
    async fn kiro_falls_back_to_post_with_default_profile_arn_after_403() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "subscriptionInfo": {"subscriptionTitle": "Kiro"},
                "usageBreakdownList": [{
                    "resourceType": "AGENTIC_REQUEST",
                    "currentUsageWithPrecision": 3,
                    "usageLimitWithPrecision": 100
                }]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].used, Some(3.0));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(body["profileArn"], KIRO_BUILDER_PROFILE_ARN);
        assert_eq!(body["resourceType"], "AGENTIC_REQUEST");
    }

    #[tokio::test]
    async fn kiro_api_key_sends_tokentype_header_and_no_default_profile_arn() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(kiro_fixture()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[("auth_method", "api_key")]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 2);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            header_value(&requests[1], "tokentype").as_deref(),
            Some("API_KEY")
        );
        let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert!(
            body.get("profileArn").is_none(),
            "api_key must not send the shared profileArn"
        );
    }

    #[tokio::test]
    async fn kiro_external_idp_sends_token_type_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("TokenType", "EXTERNAL_IDP"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "subscriptionInfo": {"subscriptionTitle": "Kiro Enterprise"},
                "usageBreakdownList": [{"resourceType":"CREDIT", "currentUsageWithPrecision":12.5,"usageLimitWithPrecision":100}]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(
                Provider::Kiro,
                &[
                    ("auth_method", "external_idp"),
                    (
                        "profile_arn",
                        "arn:aws:codewhisperer:us-east-1:123456789012:profile/ABC",
                    ),
                ],
            ),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics[0].used, Some(12.5));
        assert_eq!(data.metrics[0].remaining, Some(87.5));
    }

    #[tokio::test]
    async fn kiro_auth_error_when_all_attempts_rejected_without_leaking_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let error = kiro(
            &account(Provider::Kiro, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::AuthenticationError);
        assert!(!error.message.contains("test-token"));
        assert!(!error.message.contains(&server.uri()));
    }

    #[tokio::test]
    async fn kiro_missing_limit_stays_unknown_not_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "usageBreakdownList": [{
                    "resourceType": "AGENTIC_REQUEST",
                    "currentUsageWithPrecision": 7
                }]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].used, Some(7.0));
        assert_eq!(data.metrics[0].limit, None);
        assert_eq!(data.metrics[0].remaining, None);
    }

    #[tokio::test]
    async fn kiro_splits_overage_from_plan_usage() {
        // Official GetUsageLimits shape (CodexBar): currentUsageWithPrecision is
        // the total including overage; the plan spends against its own limit.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "subscriptionInfo": {"subscriptionTitle": "Kiro Pro"},
                "nextDateReset": "2026-10-01T00:00:00Z",
                "overageConfiguration": {"overageStatus": "ENABLED"},
                "usageBreakdownList": [{
                    "resourceType": "CREDIT",
                    "currentUsageWithPrecision": 620.25,
                    "currentOveragesWithPrecision": 150.25,
                    "usageLimitWithPrecision": 500,
                    "overageCapWithPrecision": 200,
                    "overageCharges": 6.01,
                    "overageRate": 0.04,
                    "currency": "USD",
                    "nextDateReset": "2026-11-01T00:00:00Z"
                }]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 2);
        let plan = &data.metrics[0];
        assert_eq!(plan.label, "Kiro: credit");
        assert_eq!(plan.used, Some(470.0));
        assert_eq!(plan.limit, Some(500.0));
        assert_eq!(plan.remaining, Some(30.0));
        // Per-breakdown reset wins over the top-level one.
        assert_eq!(
            plan.resets_at.unwrap().to_rfc3339(),
            "2026-11-01T00:00:00+00:00"
        );
        let overage = &data.metrics[1];
        assert_eq!(overage.label, "Kiro: credit (excedente)");
        assert_eq!(overage.used, Some(150.25));
        assert_eq!(overage.limit, Some(200.0));
        assert_eq!(overage.remaining, Some(49.75));
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("6.01 USD") && n.contains("0.04 USD")));
        assert!(!data.notes.iter().any(|n| n.contains("não informou o teto")));
    }

    #[tokio::test]
    async fn kiro_disabled_overage_drops_cap_and_metric() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "overageConfiguration": {"overageStatus": "DISABLED"},
                "usageBreakdownList": [{
                    "resourceType": "CREDIT",
                    "currentUsageWithPrecision": 10,
                    "usageLimitWithPrecision": 500,
                    "overageCapWithPrecision": 200
                }]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1, "DISABLED overage is not a quota");
        assert_eq!(data.metrics[0].label, "Kiro: credit");
        assert!(!data.notes.iter().any(|n| n.contains("Excedente")));
    }

    #[tokio::test]
    async fn kiro_bonus_overuse_is_preserved_not_clamped() {
        // Bonus spend folds into currentUsage (CodexBar hasUnseparatedBonus):
        // plan usage may legitimately exceed the plan limit — remaining goes
        // negative and a note explains the figures are not separable.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "usageBreakdownList": [{
                    "resourceType": "CREDIT",
                    "currentUsageWithPrecision": 550.5,
                    "usageLimitWithPrecision": 500,
                    "bonuses": [{"bonusId": "launch"}]
                }]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].used, Some(550.5));
        assert_eq!(data.metrics[0].remaining, Some(-50.5));
        assert!(data.notes.iter().any(|n| n.contains("bônus")));
    }

    #[tokio::test]
    async fn kiro_attempt_timeout_surfaces_as_transport_error() {
        // Each attempt carries ctx.timeout; a stalled endpoint must not hang
        // the whole query.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &Creds,
            timeout: Duration::from_millis(100),
        };
        let error = kiro(
            &account(Provider::Kiro, &[]),
            &ctx,
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::Error);
        assert!(error.message.contains("tempo limite"));
        assert!(!error.message.contains("test-token"));
    }

    const NATIVE_ARN: &str = "arn:aws:codewhisperer:us-east-1:123456789012:profile/ABC123";
    const NATIVE_ARN_EU: &str = "arn:aws:codewhisperer:eu-central-1:123456789012:profile/EU9";

    #[tokio::test]
    async fn kiro_native_credit_path_runs_first_when_profile_arn_supplied() {
        // CodexBar KiroUsageLimitsAPI.fetch: POST {profileArn} only, CREDIT
        // breakdowns, no origin/resourceType params.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "subscriptionInfo": {"subscriptionTitle": "Kiro Pro"},
                "nextDateReset": 1767225600,
                "usageBreakdownList": [{
                    "resourceType": "CREDIT",
                    "currentUsageWithPrecision": 42.5,
                    "usageLimitWithPrecision": 1000
                }]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[("profile_arn", NATIVE_ARN)]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].label, "Kiro: credit");
        assert_eq!(data.metrics[0].used, Some(42.5));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests.len(),
            1,
            "native path answered; no legacy attempts"
        );
        assert_eq!(
            header_value(&requests[0], "x-amz-target").as_deref(),
            Some("AmazonCodeWhispererService.GetUsageLimits")
        );
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body, serde_json::json!({"profileArn": NATIVE_ARN}));
    }

    #[tokio::test]
    async fn kiro_native_eu_central_arn_targets_q_host() {
        // CodexBar profileEndpoints: eu-central-1 profiles resolve against the
        // regional Q host, never the us-east-1 CodeWhisperer host.
        let cw = MockServer::start().await;
        let eu = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "usageBreakdownList": [{
                    "resourceType": "CREDIT",
                    "currentUsageWithPrecision": 1,
                    "usageLimitWithPrecision": 10
                }]
            })))
            .mount(&eu)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[("profile_arn", NATIVE_ARN_EU)]),
            &ctx(&http, &Creds),
            &cw.uri(),
            &cw.uri(),
            &eu.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(eu.received_requests().await.unwrap().len(), 1);
        assert!(
            cw.received_requests().await.unwrap().is_empty(),
            "eu-central-1 ARN must not touch the us-east-1 hosts"
        );
    }

    #[tokio::test]
    async fn kiro_native_empty_numerics_fall_through_to_legacy_attempts() {
        // A 200 without any numeric quota is not an answer on the native path.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "subscriptionInfo": {"subscriptionTitle": "Kiro"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(kiro_fixture()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[("profile_arn", NATIVE_ARN)]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(
            data.metrics.len(),
            2,
            "legacy fixture parsed after fall-through"
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2, "native POST then legacy GET");
    }

    #[tokio::test]
    async fn kiro_malformed_or_foreign_region_profile_arn_is_config_error() {
        for arn in [
            "not-an-arn",
            "arn:aws:codewhisperer:us-east-1:123456789012",
            "arn:aws:codewhisperer:us-east-1:123456789012:profile/",
            "arn:aws:execute-api:us-east-1:123456789012:profile/ABC",
            "arn:aws:codewhisperer:us-west-2:123456789012:profile/ABC",
        ] {
            let server = MockServer::start().await;
            let http = reqwest::Client::new();
            let error = kiro(
                &account(Provider::Kiro, &[("profile_arn", arn)]),
                &ctx(&http, &Creds),
                &server.uri(),
                &server.uri(),
                &server.uri(),
            )
            .await
            .unwrap_err();
            assert_eq!(error.status, Status::NeedsConfiguration, "arn: {arn}");
            assert!(
                server.received_requests().await.unwrap().is_empty(),
                "no request may leave with an invalid ARN: {arn}"
            );
            assert!(!error.message.contains(arn));
        }
    }

    #[tokio::test]
    async fn kiro_default_placeholder_arn_stays_on_legacy_attempts() {
        // No explicit profile_arn: the shared builder-id placeholder must NOT
        // trigger the native path — first request stays the legacy GET.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(kiro_fixture()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = kiro(
            &account(Provider::Kiro, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            &server.uri(),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 2);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "GET");
    }

    // -- CodeBuddy ----------------------------------------------------------

    fn codebuddy_fixture() -> Value {
        serde_json::json!({
            "code": 0,
            "msg": "ok",
            "data": {"Response": {"Data": {"Accounts": [
                {
                    "PackageName": "基础体验包",
                    "CycleStartTime": "2026-07-01 00:00:00",
                    "CycleEndTime": "2026-08-01 00:00:00",
                    "DeductionEndTime": 1800000000000_u64,
                    "CycleCapacityUsedPrecise": "6.54",
                    "CycleCapacitySizePrecise": "500"
                },
                {
                    "PackageName": "活动赠送包",
                    "CycleStartTime": "2026-07-01 00:00:00",
                    "CycleEndTime": "2026-07-15 00:00:00",
                    "DeductionEndTime": 1784073600000_u64,
                    "CapacityUsed": 12,
                    "CapacitySize": 100
                }
            ]}}}
        })
    }

    #[tokio::test]
    async fn codebuddy_splits_refill_and_bonus_packs() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("user-agent", CODEBUDDY_CN_AGENT))
            .and(header("x-ide-type", "CLI"))
            .respond_with(ResponseTemplate::new(200).set_body_json(codebuddy_fixture()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = codebuddy(
            &account(Provider::CodeBuddy, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            CODEBUDDY_CN_AGENT,
            "CLI",
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 2);
        let monthly = &data.metrics[0];
        assert_eq!(monthly.label, "CodeBuddy: Monthly");
        assert_eq!(monthly.used, Some(6.54));
        assert_eq!(monthly.limit, Some(500.0));
        assert_eq!(monthly.period.as_deref(), Some("mensal"));
        assert_eq!(
            monthly.resets_at.unwrap().to_rfc3339(),
            "2026-08-01T00:00:00+00:00"
        );
        let bonus = &data.metrics[1];
        assert_eq!(bonus.label, "CodeBuddy: Bonus Pack 1");
        assert_eq!(bonus.used, Some(12.0));
        assert_eq!(bonus.limit, Some(100.0));
        assert_eq!(
            bonus.resets_at.unwrap().to_rfc3339(),
            "2026-07-15T00:00:00+00:00"
        );
        assert!(data.notes.iter().any(|n| n.contains("基础体验包")));
        assert!(data.notes.iter().any(|n| n.contains("sem renovação")));
    }

    #[tokio::test]
    async fn codebuddy_rejects_unknown_region_before_any_request() {
        let server = MockServer::start().await;
        let http = reqwest::Client::new();
        let error = fetch(
            &account(Provider::CodeBuddy, &[("region", "eu")]),
            &ctx(&http, &Creds),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
        assert!(error.message.contains("cn") && error.message.contains("intl"));
        let _keep = server;
    }

    #[tokio::test]
    async fn codebuddy_intl_uses_ide_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("user-agent", CODEBUDDY_INTL_AGENT))
            .and(header("x-ide-type", "IDE"))
            .and(header("x-ide-name", "IDE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(codebuddy_fixture()))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = codebuddy(
            &account(Provider::CodeBuddy, &[("region", "intl")]),
            &ctx(&http, &Creds),
            &server.uri(),
            CODEBUDDY_INTL_AGENT,
            "IDE",
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 2);
    }

    #[tokio::test]
    async fn codebuddy_nonzero_code_is_error_without_leaking_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 11102,
                "msg": "failure mentioning test-token inside body"
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let error = codebuddy(
            &account(Provider::CodeBuddy, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            CODEBUDDY_CN_AGENT,
            "CLI",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::Error);
        assert!(error.message.contains("11102"));
        assert!(!error.message.contains("test-token"));
    }

    #[tokio::test]
    async fn codebuddy_401_is_authentication_error_without_leaking_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let error = codebuddy(
            &account(Provider::CodeBuddy, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
            CODEBUDDY_CN_AGENT,
            "CLI",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::AuthenticationError);
        assert!(!error.message.contains("test-token"));
    }

    // -- Zed ----------------------------------------------------------------

    #[tokio::test]
    async fn zed_maps_edit_predictions_with_billing_cycle_reset() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("authorization", "user-42 test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "plan": {
                    "plan_v3": "zed_pro",
                    "subscription_period": {
                        "started_at": "2026-07-01T00:00:00Z",
                        "ended_at": "2026-08-01T00:00:00Z"
                    },
                    "usage": {"edit_predictions": {"used": 12, "limit": 50}}
                }
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = zed(
            &account(Provider::Zed, &[("user_id", "user-42")]),
            &ctx(&http, &Creds),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        let metric = &data.metrics[0];
        assert_eq!(metric.label, "Zed: Edit Predictions");
        assert_eq!(metric.used, Some(12.0));
        assert_eq!(metric.limit, Some(50.0));
        assert_eq!(metric.remaining, Some(38.0));
        assert_eq!(
            metric.resets_at.unwrap().to_rfc3339(),
            "2026-08-01T00:00:00+00:00"
        );
        assert!(data.notes.iter().any(|n| n.contains("Zed Pro")));
    }

    #[tokio::test]
    async fn zed_unlimited_edit_predictions_camel_case_shape() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "plan": {
                    "plan_v3": "zed_pro",
                    "subscriptionPeriod": {"endedAt": "2026-08-01T00:00:00Z"},
                    "usage": {"editPredictions": {"used": 999, "limit": "unlimited"}}
                }
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = zed(
            &account(Provider::Zed, &[("user_id", "user-42")]),
            &ctx(&http, &Creds),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        let metric = &data.metrics[0];
        assert_eq!(metric.used, Some(999.0));
        assert_eq!(metric.limit, None);
        assert_eq!(metric.remaining, None);
        assert_eq!(
            metric.resets_at.unwrap().to_rfc3339(),
            "2026-08-01T00:00:00+00:00"
        );
        assert!(data.notes.iter().any(|n| n.contains("ilimitado")));
    }

    #[tokio::test]
    async fn zed_token_billed_model_requests_zero_limit_is_skipped_with_note() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "plan": {
                    "plan_v3": "zed_student",
                    "usage": {
                        "model_requests": {"used": 0, "limit": {"limited": 0}},
                        "edit_predictions": {"used": 5, "limit": 100}
                    }
                }
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = zed(
            &account(Provider::Zed, &[("user_id", "user-42")]),
            &ctx(&http, &Creds),
            &server.uri(),
        )
        .await
        .unwrap();
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].label, "Zed: Edit Predictions");
        assert!(data
            .notes
            .iter()
            .any(|n| n.contains("token") && n.contains("dashboard.zed.dev")));
    }

    #[tokio::test]
    async fn zed_missing_user_id_is_config_error_before_any_request() {
        let server = MockServer::start().await;
        let http = reqwest::Client::new();
        let error = zed(
            &account(Provider::Zed, &[]),
            &ctx(&http, &Creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
        assert!(error.message.contains("user_id"));
    }

    #[tokio::test]
    async fn zed_overdue_invoices_and_trial_notes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "plan": {
                    "plan_v3": "zed_pro",
                    "trial_started_at": "2026-07-01T00:00:00Z",
                    "has_overdue_invoices": true,
                    "usage": {"edit_predictions": {"used": 0, "limit": "unlimited"}}
                }
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let data = zed(
            &account(Provider::Zed, &[("user_id", "user-42")]),
            &ctx(&http, &Creds),
            &server.uri(),
        )
        .await
        .unwrap();
        assert!(data.notes.iter().any(|n| n.contains("Trial active")));
        assert!(data.notes.iter().any(|n| n.contains("faturas vencidas")));
    }

    #[tokio::test]
    async fn zed_401_is_authentication_error_without_leaking_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let error = zed(
            &account(Provider::Zed, &[("user_id", "user-42")]),
            &ctx(&http, &Creds),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, Status::AuthenticationError);
        assert!(!error.message.contains("test-token"));
    }
}
