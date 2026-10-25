//! Consulta read-only da OpenRouter (PRD 0038).
//!
//! Contrato documentado em
//! <https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key>
//! e <https://openrouter.ai/docs/api/api-reference/credits/get-credits>:
//!
//! - `params.product = "key"` (padrão): um `GET /api/v1/key` com uma chave de
//!   **inferência**: limite da chave, orçamentos, gasto por período, BYOK e o
//!   contador diário de requisições dos modelos `:free`;
//! - `params.product = "account"`: `GET /api/v1/credits` com uma chave de
//!   **gerenciamento** (saldo de créditos) e, opcionalmente
//!   (`params.breakdown`), `GET /api/v1/activity` (gasto dos últimos 30 dias
//!   concluídos por provedor de origem ou modelo).
//!
//! O adaptador lê apenas os campos que usa de um `serde_json::Value`:
//! `label`, `creator_user_id`, `organization_id` e `workspace_id` nunca são
//! copiados para métricas, notas ou logs. Nenhum endpoint de inferência é
//! usado e a base é uma constante, de modo que a chave nunca é enviada a um
//! host arbitrário; um redirecionamento é uma resposta rejeitada.

use super::{secret, send, Context, Data, QueryError};
use crate::text::{params, Localized, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow, Scope, Status};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex, PoisonError};

/// Constant host that may receive the key (RF1.4).
pub(crate) const BASE: &str = "https://openrouter.ai";
const KEY_PATH: &str = "/api/v1/key";
const CREDITS_PATH: &str = "/api/v1/credits";
const ACTIVITY_PATH: &str = "/api/v1/activity";
/// Largest body of the small endpoints.
const BODY_LIMIT: usize = 1024 * 1024;
/// RF6.3: largest `/activity` body and row count accepted.
const ACTIVITY_BODY_LIMIT: usize = 5 * 1024 * 1024;
const ACTIVITY_ROW_LIMIT: usize = 10_000;
/// Largest error body read to choose a fixed note id.
const ERROR_BODY_LIMIT: usize = 16 * 1024;
const USD: &str = "USD";
/// RF5.3: `/credits` documents "credits", not a currency; the front ends show
/// "créditos" until the owner confirms the 1 credit = 1 USD assumption (RF5.4).
const CREDITS_UNIT: &str = "créditos";
const ACTIVITY_PERIOD: &str = "30 dias concluídos (UTC)";
/// Breakdown rows shown before the rest collapses into "Outros".
const BREAKDOWN_TOP: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Product {
    Key,
    Account,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Breakdown {
    Off,
    Provider,
    Model,
}

fn product_of(params: &BTreeMap<String, String>) -> Option<Product> {
    match params.get("product").map(String::as_str) {
        None | Some("key") => Some(Product::Key),
        Some("account") => Some(Product::Account),
        Some(_) => None,
    }
}

fn breakdown_of(params: &BTreeMap<String, String>) -> Option<Breakdown> {
    match params.get("breakdown").map(String::as_str) {
        None | Some("off") => Some(Breakdown::Off),
        Some("provider") => Some(Breakdown::Provider),
        Some("model") => Some(Breakdown::Model),
        Some(_) => None,
    }
}

/// `Config::validate` (RF1.2): a known product, and a breakdown only with the
/// account product.
pub(crate) fn params_valid(params: &BTreeMap<String, String>) -> bool {
    let product = product_of(params);
    let low_balance_ok = match params.get("low_balance") {
        None => true,
        Some(text) => product == Some(Product::Account) && parse_threshold(text).is_some(),
    };
    low_balance_ok
        && matches!(
            (product, breakdown_of(params)),
            (Some(_), Some(Breakdown::Off)) | (Some(Product::Account), Some(_))
        )
}

/// Default low-balance threshold, in the credits the endpoint reports: five.
/// `params.low_balance = "0"` turns the note off.
pub(crate) const DEFAULT_LOW_BALANCE: f64 = 5.0;

/// A finite, non-negative threshold.
pub(crate) fn parse_threshold(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    open_router_at(account, ctx, BASE, Utc::now()).await
}

async fn open_router_at(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
    now: DateTime<Utc>,
) -> Result<Data, QueryError> {
    if !params_valid(&account.params) {
        return Err(QueryError::config(
            "query.open_router.params",
            "Parâmetros inválidos: params.product deve ser key ou account e params.breakdown (somente com account) off, provider ou model.",
        ));
    }
    let product = product_of(&account.params).unwrap_or(Product::Key);
    let breakdown = breakdown_of(&account.params).unwrap_or(Breakdown::Off);
    let default_env = match product {
        Product::Key => "OPENROUTER_API_KEY",
        Product::Account => "OPENROUTER_MANAGEMENT_KEY",
    };
    let key = secret(account, ctx, default_env)?;
    let key = key.trim();
    let base = base.trim_end_matches('/');
    match product {
        Product::Key => {
            let url = format!("{base}{KEY_PATH}");
            let value = get(ctx, &url, key, product, BODY_LIMIT).await?;
            parse_key(&value, now, url)
        }
        Product::Account => {
            let url = format!("{base}{CREDITS_PATH}");
            let value = get(ctx, &url, key, product, BODY_LIMIT).await?;
            let mut data = parse_credits(&value, url)?;
            let threshold = account
                .params
                .get("low_balance")
                .and_then(|text| parse_threshold(text))
                .unwrap_or(DEFAULT_LOW_BALANCE);
            low_balance_note(&mut data, threshold);
            if breakdown != Breakdown::Off {
                let outcome = activity(&account.name, ctx, base, key, breakdown, now).await;
                let mut notes = Notes::new();
                match outcome {
                    Some(metrics) => {
                        data.metrics.extend(metrics);
                        notes.push(
                            "note.open_router.activity_window",
                            "O gasto por origem cobre apenas dias UTC concluídos e é atualizado depois da meia-noite UTC.",
                        );
                    }
                    None => notes.push(
                        "note.open_router.activity_failed",
                        "O gasto por origem está indisponível agora; o saldo continua correto.",
                    ),
                }
                let (texts, l10n) = notes.finish();
                data.notes.extend(texts);
                data.notes_l10n.extend(l10n);
            }
            Ok(data)
        }
    }
}

// ------------------------------------------------------------------ transport

fn failure(status: Status, id: &'static str, text: &'static str) -> QueryError {
    QueryError {
        status,
        note: Localized::fixed(id, text),
        diagnostic: None,
    }
}

/// Reads at most `limit` bytes; more is a schema error, never a truncation.
async fn read_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, QueryError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| QueryError::transport())?
    {
        body.extend_from_slice(&chunk);
        if body.len() > limit {
            return Err(QueryError::schema());
        }
    }
    Ok(body)
}

/// Whether an error body says the key expired. Only the verdict leaves this
/// function: the provider's message is never copied (RF7.2).
async fn says_expired(mut response: reqwest::Response) -> bool {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() >= ERROR_BODY_LIMIT {
            body.truncate(ERROR_BODY_LIMIT);
            break;
        }
    }
    serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(|message| message.to_ascii_lowercase().contains("expired"))
        })
        .unwrap_or(false)
}

fn classify_http(code: u16, product: Product, expired: bool) -> QueryError {
    match code {
        401 if expired => failure(
            Status::AuthenticationError,
            "query.open_router.key_expired",
            "A chave da OpenRouter expirou; crie uma nova chave em openrouter.ai/settings/keys.",
        ),
        401 => failure(
            Status::AuthenticationError,
            "query.open_router.auth",
            "A OpenRouter recusou a chave: ausente, inválida ou expirada.",
        ),
        403 if product == Product::Account => failure(
            Status::AuthenticationError,
            "query.open_router.management_key_required",
            "Esta operação exige uma chave de gerenciamento; crie uma em openrouter.ai/settings/management-keys ou use a visão padrão da chave.",
        ),
        403 => failure(
            Status::AuthenticationError,
            "query.open_router.auth",
            "A OpenRouter recusou a chave: ausente, inválida ou expirada.",
        ),
        429 => failure(
            Status::RateLimited,
            "query.open_router.rate_limited",
            "A OpenRouter limitou as consultas; a próxima atualização tenta de novo.",
        ),
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

async fn get(
    ctx: &Context<'_>,
    url: &str,
    key: &str,
    product: Product,
    limit: usize,
) -> Result<Value, QueryError> {
    let response = send(
        ctx.http
            .get(url)
            .bearer_auth(key)
            .header("Accept", "application/json")
            .timeout(ctx.timeout),
    )
    .await?;
    let status = response.status();
    if status.is_redirection() {
        // The key is never sent elsewhere: a redirect is a rejected response.
        return Err(QueryError::insecure());
    }
    if !status.is_success() {
        let code = status.as_u16();
        let retry_after = (code == 429)
            .then(|| super::retry_after_seconds(response.headers()))
            .flatten();
        let expired = code == 401 && says_expired(response).await;
        return Err(classify_http(code, product, expired).with_retry_after(retry_after));
    }
    let body = read_limited(response, limit).await?;
    serde_json::from_slice(&body).map_err(|_| QueryError::schema())
}

// -------------------------------------------------------------------- numbers

/// Absent or `null` is `Ok(None)`; a non-number or a non-finite or negative
/// figure is a schema error (RF7.1).
fn nonneg(object: &Map<String, Value>, key: &str) -> Result<Option<f64>, QueryError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(v) if v.is_finite() && v >= 0.0 => Ok(Some(v)),
            _ => Err(QueryError::schema()),
        },
        Some(_) => Err(QueryError::schema()),
    }
}

/// Like [`nonneg`] but a negative value is kept verbatim (remaining amounts).
fn finite(object: &Map<String, Value>, key: &str) -> Result<Option<f64>, QueryError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(v) if v.is_finite() => Ok(Some(v)),
            _ => Err(QueryError::schema()),
        },
        Some(_) => Err(QueryError::schema()),
    }
}

// ----------------------------------------------------------------- UTC clocks

fn midnight(date: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
}

fn next_day(now: DateTime<Utc>) -> DateTime<Utc> {
    midnight(now.date_naive() + Duration::days(1))
}

/// Next Monday 00:00 UTC (weeks run Monday to Sunday).
fn next_week(now: DateTime<Utc>) -> DateTime<Utc> {
    let date = now.date_naive();
    let ahead = 7 - i64::from(date.weekday().num_days_from_monday());
    midnight(date + Duration::days(ahead))
}

fn next_month(now: DateTime<Utc>) -> DateTime<Utc> {
    let date = now.date_naive();
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    midnight(NaiveDate::from_ymd_opt(year, month, 1).expect("first of month"))
}

/// Window and computed reset of a `limit_reset`/`reset_interval` value; an
/// unknown string is `Other` without a guessed reset (RF2.2).
fn window_of(
    interval: Option<&Value>,
    now: DateTime<Utc>,
) -> (Option<MetricWindow>, Option<DateTime<Utc>>) {
    match interval {
        None | Some(Value::Null) => (None, None),
        Some(Value::String(s)) => match s.as_str() {
            "daily" => (Some(MetricWindow::Daily), Some(next_day(now))),
            "weekly" => (Some(MetricWindow::Weekly), Some(next_week(now))),
            "monthly" => (Some(MetricWindow::Monthly), Some(next_month(now))),
            _ => (Some(MetricWindow::Other), None),
        },
        Some(_) => (Some(MetricWindow::Other), None),
    }
}

fn instant(value: Option<&Value>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?.as_str()?)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

fn metric(label: impl Into<String>, kind: MetricKind, unit: &str) -> Metric {
    Metric {
        label: label.into(),
        used: None,
        limit: None,
        remaining: None,
        unit: unit.into(),
        kind,
        window: None,
        period: None,
        resets_at: None,
        billing: Default::default(),
        scope: None,
    }
}

fn spend(
    label: &str,
    used: f64,
    window: Option<MetricWindow>,
    resets_at: Option<DateTime<Utc>>,
) -> Metric {
    Metric {
        used: Some(used),
        window,
        resets_at,
        ..metric(label, MetricKind::Spend, USD)
    }
}

/// A scope name that is safe to show: short and made of identifier characters.
fn safe_scope(scope: &str) -> String {
    if (1..=40).contains(&scope.len())
        && scope
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
    {
        scope.to_string()
    } else {
        "outro".to_string()
    }
}

// --------------------------------------------------------------------- /key

const KEY_GROUPS: [&str; 9] = [
    "limit",
    "limit_remaining",
    "usage",
    "usage_daily",
    "usage_weekly",
    "usage_monthly",
    "free_model_daily_requests",
    "budgets",
    "effective_budget",
];

fn parse_key(value: &Value, now: DateTime<Utc>, source: String) -> Result<Data, QueryError> {
    let data = value
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(QueryError::schema)?;
    if !KEY_GROUPS.iter().any(|group| data.contains_key(*group)) {
        return Err(QueryError::schema());
    }
    let mut metrics = Vec::new();
    let mut notes = Notes::new();
    notes.push(
        "note.open_router.key_scope",
        "Estes números descrevem esta chave, não o saldo da conta; use o produto account para o saldo.",
    );
    if data.get("is_management_key") == Some(&Value::Bool(true)) {
        notes.push(
            "note.open_router.management_key_view",
            "Esta é uma chave de gerenciamento: ela não faz chamadas de inferência.",
        );
    }

    // RF2.2: the exact instant of a `key_limit` budget wins over the computed one.
    let budgets: &[Value] = data
        .get("budgets")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let key_limit_reset = budgets
        .iter()
        .filter_map(Value::as_object)
        .find(|b| b.get("scope").and_then(Value::as_str) == Some("key_limit"))
        .and_then(|b| instant(b.get("resets_at")));

    // RF2.1: the key cap.
    if let (Some(limit), Some(remaining)) =
        (nonneg(data, "limit")?, finite(data, "limit_remaining")?)
    {
        let (window, computed) = window_of(data.get("limit_reset"), now);
        metrics.push(Metric {
            used: Some(limit - remaining),
            limit: Some(limit),
            remaining: Some(remaining),
            window,
            resets_at: window.and(key_limit_reset.or(computed)),
            ..metric("Limite da chave", MetricKind::Quota, USD)
        });
    }

    // RF2.3: every other budget layer.
    for budget in budgets.iter().filter_map(Value::as_object) {
        let scope = budget
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("outro");
        if scope == "key_limit" {
            continue;
        }
        let (Some(limit), Some(remaining)) = (
            nonneg(budget, "limit_usd")?,
            finite(budget, "remaining_usd")?,
        ) else {
            continue;
        };
        let used = nonneg(budget, "spend_usd")?.unwrap_or(limit - remaining);
        let (window, computed) = window_of(budget.get("reset_interval"), now);
        metrics.push(Metric {
            used: Some(used),
            limit: Some(limit),
            remaining: Some(remaining),
            window,
            resets_at: instant(budget.get("resets_at")).or(window.and(computed)),
            ..metric(
                format!("Orçamento ({})", safe_scope(scope)),
                MetricKind::Quota,
                USD,
            )
        });
    }
    if let Some(scope) = data
        .get("effective_budget")
        .and_then(|budget| budget.get("scope"))
        .and_then(Value::as_str)
        .filter(|scope| *scope != "key_limit")
    {
        let scope = safe_scope(scope);
        notes.push_params(
            "note.open_router.effective_budget",
            params([("scope", ParamValue::from(scope.clone()))]),
            format!("O orçamento que bloqueia as requisições primeiro é o de escopo {scope}."),
        );
    }

    // RF3.1: spend by period.
    for (field, label, window, resets_at) in [
        (
            "usage_daily",
            "Gasto hoje",
            Some(MetricWindow::Daily),
            Some(next_day(now)),
        ),
        (
            "usage_weekly",
            "Gasto na semana",
            Some(MetricWindow::Weekly),
            Some(next_week(now)),
        ),
        (
            "usage_monthly",
            "Gasto no mês",
            Some(MetricWindow::Monthly),
            Some(next_month(now)),
        ),
        ("usage", "Gasto total da chave", None, None),
    ] {
        if let Some(used) = nonneg(data, field)? {
            metrics.push(spend(label, used, window, resets_at));
        }
    }

    // RF3.2: BYOK spend is the user's own upstream bill, not OpenRouter credits.
    let mut byok = false;
    for (field, label, window, resets_at) in [
        (
            "byok_usage_monthly",
            "BYOK no mês (cobrado pelo provedor)",
            Some(MetricWindow::Monthly),
            Some(next_month(now)),
        ),
        ("byok_usage", "BYOK total", None, None),
    ] {
        if let Some(used) = nonneg(data, field)? {
            byok = true;
            metrics.push(spend(label, used, window, resets_at));
        }
    }
    if byok {
        notes.push(
            "note.open_router.byok",
            "BYOK é gasto nas suas próprias chaves de origem, não em créditos da OpenRouter; uma taxa de 5% além da franquia do plano sai dos créditos e a API não a informa.",
        );
    }
    if data.get("include_byok_in_limit") == Some(&Value::Bool(true)) {
        notes.push(
            "note.open_router.byok_in_limit",
            "O gasto BYOK conta para o limite desta chave.",
        );
    }

    // RF4: the free-model daily counter.
    if let Some(free) = data
        .get("free_model_daily_requests")
        .and_then(Value::as_object)
    {
        if let (Some(used), Some(limit), Some(remaining)) = (
            count(free, "used"),
            count(free, "limit"),
            count(free, "remaining"),
        ) {
            if limit > 0 {
                // RF4.2: only a free-tier key is really blocked by this counter.
                let free_tier = data.get("is_free_tier") == Some(&Value::Bool(true));
                let kind = if free_tier {
                    MetricKind::Quota
                } else {
                    MetricKind::Usage
                };
                metrics.push(Metric {
                    used: Some(used as f64),
                    limit: Some(limit as f64),
                    remaining: Some(remaining as f64),
                    window: Some(MetricWindow::Daily),
                    resets_at: Some(next_day(now)),
                    ..metric(
                        "Modelos gratuitos (:free): requisições hoje",
                        kind,
                        "requisições",
                    )
                });
                notes.push(
                    "note.open_router.free_tier",
                    "Modelos :free permitem 50 ou 1000 requisições por dia conforme os créditos já comprados (a faixa segue o total comprado, não is_free_tier) e 20 por minuto, que a API não informa.",
                );
            }
        }
    }

    Ok(Data::new(metrics, notes, source))
}

/// A non-negative integer; anything else is "not reported".
fn count(object: &Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key)?.as_u64()
}

// ------------------------------------------------------------------ /credits

/// A note, not an alert: balances stay out of the threshold-alert engine
/// (PRD 0023 section 4). It appears when the balance is at or below the
/// threshold.
fn low_balance_note(data: &mut Data, threshold: f64) {
    let Some(remaining) = data
        .metrics
        .iter()
        .find(|m| m.kind == MetricKind::Balance)
        .and_then(|m| m.remaining)
    else {
        return;
    };
    if threshold > 0.0 && remaining <= threshold {
        let mut notes = Notes::new();
        notes.push_params(
            "note.open_router.low_balance",
            params([("threshold", ParamValue::from(format!("{threshold}")))]),
            format!(
                "O saldo de créditos está igual ou abaixo do limite de saldo baixo de {threshold}."
            ),
        );
        let (texts, l10n) = notes.finish();
        data.notes.extend(texts);
        data.notes_l10n.extend(l10n);
    }
}

fn parse_credits(value: &Value, source: String) -> Result<Data, QueryError> {
    let data = value
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(QueryError::schema)?;
    let (Some(total), Some(used)) = (nonneg(data, "total_credits")?, nonneg(data, "total_usage")?)
    else {
        return Err(QueryError::schema());
    };
    let mut notes = Notes::new();
    notes.push(
        "note.open_router.credits_unit",
        "Saldo = total_credits − total_usage como a OpenRouter informa; o endpoint documenta apenas \"créditos\", então nenhuma moeda é assumida.",
    );
    // RF5.1: a negative balance is kept; no gauge, because `total_credits` is
    // cumulative over the account's lifetime.
    let balance = Metric {
        remaining: Some(total - used),
        ..metric("Saldo de créditos", MetricKind::Balance, CREDITS_UNIT)
    };
    let used = Metric {
        used: Some(used),
        ..metric("Créditos usados (total)", MetricKind::Spend, CREDITS_UNIT)
    };
    Ok(Data::new(vec![balance, used], notes, source))
}

// ----------------------------------------------------------------- /activity

/// Process-wide cache of the breakdown (RF6.3): one query per account per UTC
/// day, and at least one hour between queries after a failure.
type CachedActivity = (DateTime<Utc>, Option<Vec<Metric>>);
static ACTIVITY_CACHE: LazyLock<Mutex<HashMap<String, CachedActivity>>> =
    LazyLock::new(Default::default);

fn cache_key(name: &str, base: &str, key: &str, breakdown: Breakdown) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    let mode = match breakdown {
        Breakdown::Provider => "provider",
        _ => "model",
    };
    format!("{name}|{base}|{:016x}|{mode}", hasher.finish())
}

async fn activity(
    name: &str,
    ctx: &Context<'_>,
    base: &str,
    key: &str,
    breakdown: Breakdown,
    now: DateTime<Utc>,
) -> Option<Vec<Metric>> {
    let cache_id = cache_key(name, base, key, breakdown);
    let fresh = |at: DateTime<Utc>, ok: bool| {
        (ok && at.date_naive() == now.date_naive()) || (now - at) < Duration::hours(1)
    };
    {
        let cache = ACTIVITY_CACHE
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((at, outcome)) = cache.get(&cache_id) {
            if fresh(*at, outcome.is_some()) && now >= *at {
                return outcome.clone();
            }
        }
    }
    let url = format!("{base}{ACTIVITY_PATH}");
    let outcome = match get(ctx, &url, key, Product::Account, ACTIVITY_BODY_LIMIT).await {
        Ok(value) => parse_activity(&value, breakdown).ok(),
        Err(_) => None,
    };
    ACTIVITY_CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(cache_id, (now, outcome.clone()));
    outcome
}

/// A name that is safe to show: no control characters, bounded length.
fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(60).collect();
    name.trim().to_string()
}

fn parse_activity(value: &Value, breakdown: Breakdown) -> Result<Vec<Metric>, QueryError> {
    let rows = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(QueryError::schema)?;
    if rows.len() > ACTIVITY_ROW_LIMIT {
        return Err(QueryError::schema());
    }
    let field = match breakdown {
        Breakdown::Provider => "provider_name",
        _ => "model",
    };
    let mut sums: BTreeMap<String, f64> = BTreeMap::new();
    let mut other = 0.0;
    let mut byok = 0.0;
    for row in rows {
        let row = row.as_object().ok_or_else(QueryError::schema)?;
        byok += nonneg(row, "byok_usage_inference")?.unwrap_or(0.0);
        let usage = nonneg(row, "usage")?.unwrap_or(0.0);
        if usage == 0.0 {
            continue;
        }
        match row.get(field).and_then(Value::as_str).map(clean_name) {
            Some(name) if !name.is_empty() => *sums.entry(name).or_insert(0.0) += usage,
            _ => other += usage,
        }
    }
    let mut ranked: Vec<(String, f64)> = sums.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (_, sum) in ranked.iter().skip(BREAKDOWN_TOP) {
        other += sum;
    }
    ranked.truncate(BREAKDOWN_TOP);

    let item = |label: String, used: f64, scope: Option<Scope>| Metric {
        used: Some(used),
        window: Some(MetricWindow::Other),
        period: Some(ACTIVITY_PERIOD.into()),
        scope,
        ..metric(label, MetricKind::Spend, USD)
    };
    let mut metrics = Vec::new();
    for (name, sum) in ranked {
        let scope =
            (breakdown == Breakdown::Model).then(|| Scope::model(name.clone(), name.clone()));
        metrics.push(item(format!("Gasto 30 dias — {name}"), sum, scope));
    }
    if other > 0.0 {
        metrics.push(item("Gasto 30 dias — Outros".into(), other, None));
    }
    if byok > 0.0 {
        metrics.push(item("BYOK (externo)".into(), byok, None));
    }
    Ok(metrics)
}

// -------------------------------------------------------------------- /models

/// Most pages of the model listing (500 ids each).
const MODEL_PAGES: usize = 50;
const MODEL_PAGE_SIZE: usize = 500;

/// RF11: `GET /api/v1/models` with the inference key, paginated by `offset`.
/// `output_modalities=all` keeps embedding, image and audio models in the
/// list. A repeated page, a count mismatch or the page limit fails the whole
/// listing; ids are deduplicated and never rewritten.
pub(crate) async fn models(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Vec<crate::models::ModelInfo>, QueryError> {
    let key = secret(account, ctx, "OPENROUTER_API_KEY")?;
    let key = key.trim();
    let base = base.trim_end_matches('/');
    let mut out: Vec<crate::models::ModelInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut offset = 0usize;
    let mut raw = 0usize;
    for _ in 0..MODEL_PAGES {
        let url = format!(
            "{base}/api/v1/models?offset={offset}&limit={MODEL_PAGE_SIZE}&output_modalities=all"
        );
        let value = get(ctx, &url, key, Product::Key, BODY_LIMIT * 8).await?;
        let page = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(QueryError::schema)?;
        for item in page {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(QueryError::schema)?;
            raw += 1;
            if seen.insert(id.to_string()) {
                out.push(crate::models::ModelInfo {
                    id: id.to_string(),
                    name: item
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .filter(|t| !t.is_empty()),
                    description: None,
                });
            }
        }
        let total = value.get("total_count").and_then(Value::as_u64);
        let has_next = value
            .get("links")
            .and_then(|links| links.get("next"))
            .is_some_and(|next| !next.is_null());
        if !has_next || total.is_some_and(|total| raw as u64 >= total) {
            if total.is_some_and(|total| raw as u64 != total) {
                return Err(QueryError::schema());
            }
            return Ok(out);
        }
        if page.is_empty() {
            return Err(QueryError::schema());
        }
        offset += page.len();
    }
    Err(QueryError::schema())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Context;
    use crate::Provider;
    use serde_json::json;
    use std::time::Duration as StdDuration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "sk-or-test-secret-key";

    struct TestCredentials;
    impl crate::Credentials for TestCredentials {
        fn get(&self, name: &str) -> Option<String> {
            (name == "TEST_OPENROUTER_KEY").then(|| KEY.to_string())
        }
    }

    fn account(product: Option<&str>, breakdown: Option<&str>) -> Account {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut account = Account {
            name: format!("OpenRouter {n}"),
            provider: Provider::OpenRouter,
            credential_env: Some("TEST_OPENROUTER_KEY".into()),
            ..Default::default()
        };
        if let Some(product) = product {
            account.params.insert("product".into(), product.into());
        }
        if let Some(breakdown) = breakdown {
            account.params.insert("breakdown".into(), breakdown.into());
        }
        account
    }

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    /// Thursday 2026-10-22 12:00 UTC.
    fn clock() -> DateTime<Utc> {
        at(2026, 10, 22, 12)
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    async fn run_at(
        account: &Account,
        server: &MockServer,
        now: DateTime<Utc>,
    ) -> Result<Data, QueryError> {
        let http = client();
        let ctx = Context {
            http: &http,
            credentials: &TestCredentials,
            timeout: StdDuration::from_secs(5),
        };
        open_router_at(account, &ctx, &server.uri(), now).await
    }

    async fn serve(route: &str, response: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    async fn key_view(body: Value) -> Data {
        let server = serve(KEY_PATH, ResponseTemplate::new(200).set_body_json(body)).await;
        run_at(&account(None, None), &server, clock())
            .await
            .expect("ready")
    }

    async fn key_err(response: ResponseTemplate) -> QueryError {
        let server = serve(KEY_PATH, response).await;
        run_at(&account(None, None), &server, clock())
            .await
            .unwrap_err()
    }

    async fn credits_err(response: ResponseTemplate) -> QueryError {
        let server = serve(CREDITS_PATH, response).await;
        run_at(&account(Some("account"), None), &server, clock())
            .await
            .unwrap_err()
    }

    async fn credits(body: Value) -> Result<Data, QueryError> {
        let server = serve(CREDITS_PATH, ResponseTemplate::new(200).set_body_json(body)).await;
        run_at(&account(Some("account"), None), &server, clock()).await
    }

    fn ids(data: &Data) -> Vec<&str> {
        data.notes_l10n.iter().map(|n| &*n.id).collect()
    }

    fn find<'a>(data: &'a Data, label: &str) -> &'a Metric {
        data.metrics
            .iter()
            .find(|m| m.label == label)
            .unwrap_or_else(|| panic!("no metric {label}"))
    }

    fn report(account: &Account, data: Data) -> crate::Report {
        crate::Report {
            as_of: Default::default(),
            overage_hint: Default::default(),
            sub_reports: Vec::new(),
            account_id: None,
            diagnostic: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: clock(),
            status: Status::Ready,
            metrics: data.metrics,
            notes: data.notes,
            notes_l10n: data.notes_l10n,
            source: data.source,
            session_expires_at: None,
        }
    }

    fn key_body(limit: Value, remaining: Value, reset: Value) -> Value {
        json!({"data": {
            "label": "sk-or-v1-abc...xyz", "limit": limit, "limit_remaining": remaining,
            "limit_reset": reset, "usage": 30.5, "usage_daily": 1.25,
            "usage_weekly": 4.5, "usage_monthly": 25.5,
            "is_free_tier": false, "is_management_key": false,
            "creator_user_id": "user_secret", "organization_id": "org_secret",
        }})
    }

    // F1
    #[tokio::test]
    async fn f1_key_cap_is_a_monthly_quota_with_four_spend_metrics() {
        let data = key_view(key_body(json!(100), json!(74.5), json!("monthly"))).await;
        let cap = find(&data, "Limite da chave");
        assert_eq!(cap.kind, MetricKind::Quota);
        assert_eq!(cap.unit, "USD");
        assert_eq!(cap.window, Some(MetricWindow::Monthly));
        assert_eq!(cap.resets_at, Some(at(2026, 11, 1, 0)));
        assert_eq!(
            (cap.used, cap.limit, cap.remaining),
            (Some(25.5), Some(100.0), Some(74.5))
        );
        let spends: Vec<_> = data
            .metrics
            .iter()
            .filter(|m| m.kind == MetricKind::Spend)
            .map(|m| (m.label.as_str(), m.window, m.resets_at))
            .collect();
        assert_eq!(
            spends,
            [
                (
                    "Gasto hoje",
                    Some(MetricWindow::Daily),
                    Some(at(2026, 10, 23, 0))
                ),
                (
                    "Gasto na semana",
                    Some(MetricWindow::Weekly),
                    Some(at(2026, 10, 26, 0))
                ),
                (
                    "Gasto no mês",
                    Some(MetricWindow::Monthly),
                    Some(at(2026, 11, 1, 0))
                ),
                ("Gasto total da chave", None, None),
            ]
        );
        assert!(ids(&data).contains(&"note.open_router.key_scope"));
    }

    // F2
    #[tokio::test]
    async fn f2_low_key_credits_bind_the_account() {
        let account = account(None, None);
        let data = key_view(key_body(json!(100), json!(2.1), json!("monthly"))).await;
        let report = report(&account, data);
        let binding = crate::metrics::binding(&report).expect("binding");
        assert_eq!(report.metrics[binding.index].label, "Limite da chave");
        assert!(
            (binding.used_ratio - 0.979).abs() < 1e-9,
            "{}",
            binding.used_ratio
        );
    }

    // F3
    #[tokio::test]
    async fn f3_exhausted_cap_keeps_remaining_verbatim_and_blocks_until_reset() {
        let mut body = key_body(json!(100), json!(0), json!("daily"));
        body["data"]["budgets"] = json!([
            {"scope": "key_limit", "limit_usd": 100, "spend_usd": 100, "remaining_usd": 0,
             "reset_interval": "daily", "resets_at": "2026-10-23T00:00:00Z"},
            {"scope": "guardrail", "limit_usd": 10, "spend_usd": 12, "remaining_usd": -2,
             "reset_interval": "weekly", "resets_at": "2026-10-26T00:00:00Z"},
        ]);
        let account = account(None, None);
        let data = key_view(body).await;
        let cap = find(&data, "Limite da chave");
        assert_eq!(cap.remaining, Some(0.0));
        assert_eq!(cap.resets_at, Some(at(2026, 10, 23, 0)));
        let layer = find(&data, "Orçamento (guardrail)");
        assert_eq!(layer.remaining, Some(-2.0));
        assert_eq!(layer.window, Some(MetricWindow::Weekly));
        assert_eq!(
            data.metrics
                .iter()
                .filter(|m| m.label.starts_with("Orçamento"))
                .count(),
            1,
            "the key_limit entry is not duplicated"
        );
        let report = report(&account, data);
        assert_eq!(
            crate::metrics::classify_report(Some(&report)),
            crate::metrics::AccountClass::Exhausted
        );
    }

    // F4
    #[tokio::test]
    async fn f4_no_limit_set_yields_no_quota_and_a_ready_account() {
        let mut body = key_body(Value::Null, Value::Null, Value::Null);
        body["data"]["usage"] = json!(3.0);
        let account = account(None, None);
        let data = key_view(body).await;
        assert!(data.metrics.iter().all(|m| m.kind != MetricKind::Quota));
        assert!(data.metrics.iter().any(|m| m.kind == MetricKind::Spend));
        assert!(ids(&data).contains(&"note.open_router.key_scope"));
        let report = report(&account, data);
        assert_ne!(
            crate::metrics::classify_report(Some(&report)),
            crate::metrics::AccountClass::Exhausted
        );
    }

    // F5
    #[tokio::test]
    async fn f5_lifetime_cap_has_no_window_and_no_reset() {
        let data = key_view(key_body(json!(50), json!(20), Value::Null)).await;
        let cap = find(&data, "Limite da chave");
        assert_eq!((cap.window, cap.resets_at), (None, None));
        assert_eq!(cap.used, Some(30.0));
        let report = report(&account(None, None), data);
        assert!(crate::metrics::binding(&report).is_some());
    }

    fn free_body(is_free: bool, used: u64, limit: u64) -> Value {
        let mut body = key_body(Value::Null, Value::Null, Value::Null);
        body["data"]["is_free_tier"] = json!(is_free);
        body["data"]["free_model_daily_requests"] =
            json!({"used": used, "limit": limit, "remaining": limit.saturating_sub(used)});
        body
    }

    // F6
    #[tokio::test]
    async fn f6_free_tier_counter_is_a_daily_quota() {
        let data = key_view(free_body(true, 12, 50)).await;
        let counter = find(&data, "Modelos gratuitos (:free): requisições hoje");
        assert_eq!(counter.kind, MetricKind::Quota);
        assert_eq!(counter.window, Some(MetricWindow::Daily));
        assert_eq!(counter.resets_at, Some(at(2026, 10, 23, 0)));
        assert_eq!(counter.unit, "requisições");
        let report = report(&account(None, None), data);
        let binding = crate::metrics::binding(&report).expect("binding");
        assert!((binding.used_ratio - 0.24).abs() < 1e-9);
        assert!(ids_of(&report).contains(&"note.open_router.free_tier"));
    }

    fn ids_of(report: &crate::Report) -> Vec<&str> {
        report.notes_l10n.iter().map(|n| &*n.id).collect()
    }

    // F7
    #[tokio::test]
    async fn f7_exhausted_free_tier_blocks_until_utc_midnight() {
        let account = account(None, None);
        let data = key_view(free_body(true, 50, 50)).await;
        let report = report(&account, data);
        assert_eq!(
            crate::metrics::classify_report(Some(&report)),
            crate::metrics::AccountClass::Exhausted
        );
        assert_eq!(report.metrics[0].resets_at, Some(at(2026, 10, 23, 0)));
    }

    // F8
    #[tokio::test]
    async fn f8_paid_key_counter_is_usage_and_never_blocks() {
        let account = account(None, None);
        let data = key_view(free_body(false, 1000, 1000)).await;
        let counter = find(&data, "Modelos gratuitos (:free): requisições hoje");
        assert_eq!(counter.kind, MetricKind::Usage);
        let report = report(&account, data);
        assert_ne!(
            crate::metrics::classify_report(Some(&report)),
            crate::metrics::AccountClass::Exhausted
        );
        assert!(crate::metrics::binding(&report).is_none());
        // A zero limit omits the metric.
        let data = key_view(free_body(true, 0, 0)).await;
        assert!(data
            .metrics
            .iter()
            .all(|m| m.label != "Modelos gratuitos (:free): requisições hoje"));
    }

    #[tokio::test]
    async fn week_and_month_boundaries_roll_over_at_a_fixed_clock() {
        let sunday_night = at(2026, 10, 25, 23);
        assert_eq!(next_week(sunday_night), at(2026, 10, 26, 0));
        assert_eq!(next_week(at(2026, 10, 26, 0)), at(2026, 11, 2, 0));
        assert_eq!(next_month(at(2026, 12, 31, 23)), at(2027, 1, 1, 0));
        assert_eq!(next_day(at(2026, 12, 31, 23)), at(2027, 1, 1, 0));
    }

    #[tokio::test]
    async fn byok_is_spend_with_its_own_notes_and_missing_fields_add_nothing() {
        let mut body = key_body(Value::Null, Value::Null, Value::Null);
        body["data"]["byok_usage"] = json!(8.0);
        body["data"]["byok_usage_monthly"] = json!(2.0);
        body["data"]["include_byok_in_limit"] = json!(true);
        let data = key_view(body).await;
        assert_eq!(
            find(&data, "BYOK no mês (cobrado pelo provedor)").used,
            Some(2.0)
        );
        assert_eq!(find(&data, "BYOK total").used, Some(8.0));
        let notes = ids(&data);
        assert!(notes.contains(&"note.open_router.byok"));
        assert!(notes.contains(&"note.open_router.byok_in_limit"));

        let sparse = json!({"data": {"limit": null, "usage": 1.0}});
        let data = key_view(sparse).await;
        assert_eq!(data.metrics.len(), 1);
        assert_eq!(data.metrics[0].label, "Gasto total da chave");
    }

    // F11 (budgets part)
    #[tokio::test]
    async fn unknown_budget_scopes_and_extra_fields_are_accepted() {
        let mut body = key_body(Value::Null, Value::Null, Value::Null);
        body["data"]["future_field"] = json!({"x": 1});
        body["data"]["budgets"] = json!([
            {"scope": "workspace", "limit_usd": 50, "spend_usd": 10, "remaining_usd": 40,
             "reset_interval": "monthly", "resets_at": "2026-11-01T00:00:00Z"},
            {"scope": "quarterly_thing", "limit_usd": 5, "spend_usd": 1, "remaining_usd": 4,
             "reset_interval": null, "resets_at": null},
            {"scope": "bad scope!", "limit_usd": 5, "spend_usd": 1, "remaining_usd": 4},
        ]);
        body["data"]["effective_budget"] = json!({"scope": "workspace"});
        let data = key_view(body).await;
        assert_eq!(
            find(&data, "Orçamento (workspace)").window,
            Some(MetricWindow::Monthly)
        );
        let unknown = find(&data, "Orçamento (quarterly_thing)");
        assert_eq!((unknown.window, unknown.resets_at), (None, None));
        find(&data, "Orçamento (outro)");
        assert!(ids(&data).contains(&"note.open_router.effective_budget"));
    }

    // F9
    #[tokio::test]
    async fn f9_credits_balance_is_purchased_minus_used_without_a_gauge() {
        let data = credits(json!({"data": {"total_credits": 100.5, "total_usage": 25.75}}))
            .await
            .expect("ready");
        let balance = find(&data, "Saldo de créditos");
        assert_eq!(balance.kind, MetricKind::Balance);
        assert_eq!(balance.remaining, Some(74.75));
        assert_eq!((balance.used, balance.limit), (None, None));
        assert_eq!((balance.window, balance.resets_at), (None, None));
        assert_eq!(balance.unit, "créditos");
        let spent = find(&data, "Créditos usados (total)");
        assert_eq!((spent.kind, spent.used), (MetricKind::Spend, Some(25.75)));
        assert!(ids(&data).contains(&"note.open_router.credits_unit"));
    }

    // F10 + F11 (negative)
    #[tokio::test]
    async fn f10_low_and_negative_balances_are_kept_verbatim() {
        let low = credits(json!({"data": {"total_credits": 100.0, "total_usage": 99.2}}))
            .await
            .unwrap();
        assert!((find(&low, "Saldo de créditos").remaining.unwrap() - 0.8).abs() < 1e-9);
        let negative = credits(json!({"data": {"total_credits": 10.0, "total_usage": 11.5}}))
            .await
            .unwrap();
        assert_eq!(find(&negative, "Saldo de créditos").remaining, Some(-1.5));
        let report = report(&account(Some("account"), None), negative);
        assert_eq!(
            crate::metrics::classify_report(Some(&report)),
            crate::metrics::AccountClass::Exhausted
        );
    }

    // F12
    #[tokio::test]
    async fn f12_a_half_reading_is_a_schema_error() {
        for body in [
            json!({"data": {"total_credits": 10.0}}),
            json!({"data": {"total_usage": 1.0}}),
            json!({"data": {"total_credits": -1.0, "total_usage": 1.0}}),
            json!({"data": {"total_credits": "10", "total_usage": 1.0}}),
        ] {
            let error = credits(body).await.unwrap_err();
            assert_eq!(
                (error.status, &*error.note.id),
                (Status::Error, "query.schema")
            );
        }
    }

    fn activity_rows() -> Value {
        json!({"data": [
            {"date": "2026-10-20", "model": "a/m1", "provider_name": "OpenAI", "usage": 3.0, "byok_usage_inference": 0.5},
            {"date": "2026-10-20", "model": "a/m2", "provider_name": "Anthropic", "usage": 2.0, "byok_usage_inference": 0},
            {"date": "2026-10-21", "model": "a/m1", "provider_name": "OpenAI", "usage": 1.5, "byok_usage_inference": 0.25},
            {"date": "2026-10-21", "model": "g/free", "provider_name": "Google", "usage": 0, "byok_usage_inference": 0},
        ]})
    }

    async fn with_breakdown(
        mode: &str,
        activity: ResponseTemplate,
        _unused: Option<&MockServer>,
    ) -> (Result<Data, QueryError>, MockServer) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(CREDITS_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"data": {"total_credits": 10.0, "total_usage": 4.0}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(ACTIVITY_PATH))
            .respond_with(activity)
            .mount(&server)
            .await;
        let result = run_at(&account(Some("account"), Some(mode)), &server, clock()).await;
        (result, server)
    }

    // F13
    #[tokio::test]
    async fn f13_activity_sums_per_provider_and_keeps_byok_apart() {
        let (data, _server) = with_breakdown(
            "provider",
            ResponseTemplate::new(200).set_body_json(activity_rows()),
            None,
        )
        .await;
        let data = data.expect("ready");
        let openai = find(&data, "Gasto 30 dias — OpenAI");
        assert_eq!((openai.kind, openai.used), (MetricKind::Spend, Some(4.5)));
        assert_eq!(openai.window, Some(MetricWindow::Other));
        assert_eq!(openai.period.as_deref(), Some("30 dias concluídos (UTC)"));
        assert_eq!(find(&data, "Gasto 30 dias — Anthropic").used, Some(2.0));
        assert!(
            data.metrics.iter().all(|m| !m.label.contains("Google")),
            "free rows are skipped"
        );
        assert_eq!(find(&data, "BYOK (externo)").used, Some(0.75));
        assert_eq!(find(&data, "Saldo de créditos").remaining, Some(6.0));
        assert!(ids(&data).contains(&"note.open_router.activity_window"));
        assert!(crate::metrics::binding(&report(&account(Some("account"), None), data)).is_none());
    }

    // F14
    #[tokio::test]
    async fn f14_empty_activity_shows_no_breakdown_and_no_zero() {
        let (data, _server) = with_breakdown(
            "model",
            ResponseTemplate::new(200).set_body_json(json!({"data": []})),
            None,
        )
        .await;
        let data = data.expect("ready");
        assert!(data
            .metrics
            .iter()
            .all(|m| !m.label.starts_with("Gasto 30 dias")));
        assert!(data.metrics.iter().all(|m| m.label != "BYOK (externo)"));
    }

    // F15
    #[tokio::test]
    async fn f15_six_providers_collapse_to_five_plus_others() {
        let mut rows: Vec<Value> = (1..=6)
            .map(|n| json!({"provider_name": format!("P{n}"), "usage": f64::from(n), "byok_usage_inference": 0}))
            .collect();
        rows.push(json!({"provider_name": null, "usage": 0.5, "byok_usage_inference": 0}));
        rows.push(json!({"provider_name": "", "usage": 0.25, "byok_usage_inference": 0}));
        rows.push(json!({"provider_name": "Free", "usage": 0, "byok_usage_inference": 0}));
        let (data, _server) = with_breakdown(
            "provider",
            ResponseTemplate::new(200).set_body_json(json!({"data": rows})),
            None,
        )
        .await;
        let data = data.expect("ready");
        let named = data
            .metrics
            .iter()
            .filter(|m| m.label.starts_with("Gasto 30 dias — P"))
            .count();
        assert_eq!(named, 5);
        // P1 (1.0) falls out of the top five; null and empty names join it.
        assert_eq!(find(&data, "Gasto 30 dias — Outros").used, Some(1.75));
        assert!(data.metrics.iter().all(|m| !m.label.ends_with("Free")));
    }

    #[tokio::test]
    async fn model_breakdown_scopes_each_metric_to_its_model() {
        let (data, _server) = with_breakdown(
            "model",
            ResponseTemplate::new(200).set_body_json(activity_rows()),
            None,
        )
        .await;
        let data = data.expect("ready");
        let metric = find(&data, "Gasto 30 dias — a/m1");
        assert_eq!(metric.used, Some(4.5));
        assert!(metric.scope.is_some());
    }

    // F16
    #[tokio::test]
    async fn f16_activity_failure_keeps_the_balance_and_is_cached() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(CREDITS_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"data": {"total_credits": 10.0, "total_usage": 4.0}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(ACTIVITY_PATH))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        let account = account(Some("account"), Some("provider"));
        for _ in 0..2 {
            let data = run_at(&account, &server, clock()).await.expect("ready");
            assert_eq!(find(&data, "Saldo de créditos").remaining, Some(6.0));
            assert!(ids(&data).contains(&"note.open_router.activity_failed"));
        }
        // `expect(1)` is verified when the server drops.
    }

    #[tokio::test]
    async fn activity_success_is_served_from_the_cache_for_the_utc_day() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(CREDITS_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"data": {"total_credits": 10.0, "total_usage": 4.0}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(ACTIVITY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(activity_rows()))
            .expect(1)
            .mount(&server)
            .await;
        let account = account(Some("account"), Some("provider"));
        let first = run_at(&account, &server, at(2026, 10, 22, 1))
            .await
            .unwrap();
        let later = run_at(&account, &server, at(2026, 10, 22, 20))
            .await
            .unwrap();
        assert_eq!(first.metrics.len(), later.metrics.len());
    }

    #[test]
    fn an_oversized_activity_is_a_schema_error() {
        let rows: Vec<Value> = (0..=ACTIVITY_ROW_LIMIT)
            .map(|_| json!({"usage": 1.0}))
            .collect();
        let error = parse_activity(&json!({"data": rows}), Breakdown::Provider).unwrap_err();
        assert_eq!(&*error.note.id, "query.schema");
    }

    // F17
    #[tokio::test]
    async fn f17_401_is_an_authentication_error_and_expiry_has_its_own_note() {
        let error = key_err(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error": {"message": "No auth credentials found"}})),
        )
        .await;
        assert_eq!(
            (error.status, &*error.note.id),
            (Status::AuthenticationError, "query.open_router.auth")
        );
        let error = credits_err(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error": {"message": "API key expired"}})),
        )
        .await;
        assert_eq!(&*error.note.id, "query.open_router.key_expired");
        assert!(!error.note.fallback.contains("API key expired"));
    }

    // F18
    #[tokio::test]
    async fn f18_403_on_credits_names_the_missing_management_key() {
        let error = credits_err(ResponseTemplate::new(403).set_body_json(
            json!({"error": {"message": "Only management keys can perform this operation"}}),
        ))
        .await;
        assert_eq!(
            (error.status, &*error.note.id),
            (
                Status::AuthenticationError,
                "query.open_router.management_key_required"
            )
        );
        // The key product has no such diagnosis.
        let error = key_err(ResponseTemplate::new(403)).await;
        assert_eq!(&*error.note.id, "query.open_router.auth");
    }

    // F19
    #[tokio::test]
    async fn f19_402_is_a_generic_http_error_without_the_body() {
        let error = key_err(
            ResponseTemplate::new(402)
                .set_body_json(json!({"error": {"message": "Insufficient credits SECRET"}})),
        )
        .await;
        assert_eq!(
            (error.status, &*error.note.id),
            (Status::Error, "query.http")
        );
        assert!(!error.note.fallback.contains("SECRET"));
    }

    // F20
    #[tokio::test]
    async fn f20_429_is_rate_limited_without_an_inner_retry() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(KEY_PATH))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "3600"))
            .expect(1)
            .mount(&server)
            .await;
        let error = run_at(&account(None, None), &server, clock())
            .await
            .unwrap_err();
        assert_eq!(
            (error.status, &*error.note.id),
            (Status::RateLimited, "query.open_router.rate_limited")
        );
        assert_eq!(
            error.note.params.get(crate::Report::RETRY_AFTER_PARAM),
            Some(&ParamValue::from(3600u64))
        );
    }

    // F21
    #[tokio::test]
    async fn f21_server_errors_and_timeouts_are_plain_errors() {
        let error = key_err(ResponseTemplate::new(500)).await;
        assert_eq!(
            (error.status, &*error.note.id),
            (Status::Error, "query.http")
        );
        let server = serve(
            KEY_PATH,
            ResponseTemplate::new(200)
                .set_body_json(key_body(Value::Null, Value::Null, Value::Null))
                .set_delay(StdDuration::from_millis(400)),
        )
        .await;
        let http = client();
        let ctx = Context {
            http: &http,
            credentials: &TestCredentials,
            timeout: StdDuration::from_millis(50),
        };
        let error = open_router_at(&account(None, None), &ctx, &server.uri(), clock())
            .await
            .unwrap_err();
        assert_eq!(&*error.note.id, "query.timeout");
    }

    // F22
    #[tokio::test]
    async fn f22_bad_shapes_are_schema_errors_and_unknown_windows_are_not_guessed() {
        for response in [
            ResponseTemplate::new(200).set_body_string("{\"data\": {\"limit\": 1"),
            ResponseTemplate::new(200).set_body_json(json!({"nope": 1})),
            ResponseTemplate::new(200).set_body_json(json!({"data": {"unrelated": true}})),
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"limit": -1, "limit_remaining": 0}})),
        ] {
            assert_eq!(&*key_err(response).await.note.id, "query.schema");
        }
        let data = key_view(key_body(json!(10), json!(5), json!("yearly"))).await;
        let cap = find(&data, "Limite da chave");
        assert_eq!(
            (cap.window, cap.resets_at),
            (Some(MetricWindow::Other), None)
        );
    }

    #[tokio::test]
    async fn a_redirect_is_rejected_and_never_followed() {
        let error = key_err(
            ResponseTemplate::new(302).insert_header("Location", "https://evil.test/steal"),
        )
        .await;
        assert_eq!(&*error.note.id, "query.insecure_transport");
    }

    #[tokio::test]
    async fn request_is_a_bare_bearer_get() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(KEY_PATH))
            .and(header("authorization", format!("Bearer {KEY}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(key_body(
                Value::Null,
                Value::Null,
                Value::Null,
            )))
            .expect(1)
            .mount(&server)
            .await;
        run_at(&account(None, None), &server, clock())
            .await
            .expect("ready");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests[0].url.query(), None);
        assert!(requests[0].body.is_empty());
    }

    #[tokio::test]
    async fn management_key_on_the_key_view_adds_a_note() {
        let mut body = key_body(Value::Null, Value::Null, Value::Null);
        body["data"]["is_management_key"] = json!(true);
        let data = key_view(body).await;
        assert!(ids(&data).contains(&"note.open_router.management_key_view"));
    }

    #[tokio::test]
    async fn nothing_leaks_the_key_the_label_the_ids_or_the_base() {
        let mut body = key_body(json!(100), json!(10), json!("monthly"));
        body["data"]["budgets"] =
            json!([{"scope": "guardrail", "limit_usd": 1, "remaining_usd": 0.5}]);
        let data = key_view(body).await;
        let mut haystack = format!("{:?} {:?}", data.notes, data.notes_l10n);
        for metric in &data.metrics {
            haystack.push_str(&format!("{metric:?}"));
        }
        for secret in [
            KEY,
            "Bearer",
            "sk-or-v1-abc",
            "user_secret",
            "org_secret",
            "127.0.0.1",
        ] {
            assert!(!haystack.contains(secret), "leaked {secret}");
        }
        for error in [
            key_err(ResponseTemplate::new(401)).await,
            key_err(ResponseTemplate::new(500)).await,
        ] {
            let text = format!("{error:?}");
            for secret in [KEY, "Bearer", "127.0.0.1"] {
                assert!(!text.contains(secret), "leaked {secret}");
            }
        }
    }

    #[test]
    fn params_are_validated_at_the_boundary() {
        let p = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert!(params_valid(&p(&[])));
        assert!(params_valid(&p(&[("product", "key")])));
        assert!(params_valid(&p(&[("product", "account")])));
        assert!(params_valid(&p(&[
            ("product", "account"),
            ("breakdown", "provider")
        ])));
        assert!(params_valid(&p(&[
            ("product", "account"),
            ("breakdown", "off")
        ])));
        assert!(!params_valid(&p(&[("product", "management")])));
        assert!(!params_valid(&p(&[("breakdown", "provider")])));
        assert!(!params_valid(&p(&[
            ("product", "account"),
            ("breakdown", "daily")
        ])));

        let config = |account: Account| crate::Config {
            accounts: vec![account],
            ..crate::Config::default()
        };
        config(account(None, None)).validate().expect("default");
        config(account(Some("account"), Some("model")))
            .validate()
            .expect("account");
        assert!(matches!(
            config(account(Some("nope"), None)).validate(),
            Err(crate::ConfigError::OpenRouter)
        ));
    }

    #[tokio::test]
    async fn low_balance_note_follows_the_default_and_the_configured_threshold() {
        let server = serve(
            CREDITS_PATH,
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"total_credits": 10.0, "total_usage": 6.0}})),
        )
        .await;
        let mut acct = account(Some("account"), None);
        let data = run_at(&acct, &server, clock()).await.unwrap();
        assert!(
            ids(&data).contains(&"note.open_router.low_balance"),
            "4 <= 5"
        );
        acct.params.insert("low_balance".into(), "3.5".into());
        let data = run_at(&acct, &server, clock()).await.unwrap();
        assert!(!ids(&data).contains(&"note.open_router.low_balance"));
        acct.params.insert("low_balance".into(), "0".into());
        let data = run_at(&acct, &server, clock()).await.unwrap();
        assert!(!ids(&data).contains(&"note.open_router.low_balance"));
        let mut bad = account(None, None);
        bad.params.insert("low_balance".into(), "5".into());
        assert!(!params_valid(&bad.params), "only with the account product");
        acct.params.insert("low_balance".into(), "-1".into());
        assert!(!params_valid(&acct.params));
    }

    #[test]
    fn open_router_is_not_metered() {
        assert!(!Provider::OpenRouter.is_metered());
        assert_eq!(
            Provider::from_slug("open_router"),
            Some(Provider::OpenRouter)
        );
        assert_eq!(Provider::OpenRouter.slug(), "open_router");
    }

    #[tokio::test]
    async fn a_missing_variable_needs_configuration() {
        let server = MockServer::start().await;
        let mut missing = account(None, None);
        missing.credential_env = Some("UNSET".into());
        let error = run_at(&missing, &server, clock()).await.unwrap_err();
        assert_eq!(error.status, Status::NeedsConfiguration);
        assert!(server.received_requests().await.unwrap().is_empty());
    }
    async fn list(server: &MockServer) -> Result<Vec<crate::models::ModelInfo>, QueryError> {
        let http = client();
        let ctx = Context {
            http: &http,
            credentials: &TestCredentials,
            timeout: StdDuration::from_secs(5),
        };
        models(&account(None, None), &ctx, &server.uri()).await
    }

    #[tokio::test]
    async fn f23_models_follow_offset_pages_and_keep_free_ids() {
        use wiremock::matchers::query_param;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(query_param("offset", "0"))
            .and(query_param("output_modalities", "all"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "openai/gpt-4", "name": "GPT-4"}, {"id": "meta/llama:free"}],
                "links": {"next": "https://openrouter.ai/next"}, "total_count": 3})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(query_param("offset", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "openai/gpt-4"}], "links": {"next": null}, "total_count": 3})))
            .expect(1)
            .mount(&server)
            .await;
        let ids: Vec<_> = list(&server)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(
            ids,
            ["openai/gpt-4", "meta/llama:free"],
            "deduplicated, real ids"
        );
    }

    #[tokio::test]
    async fn models_reject_a_count_mismatch_an_empty_page_and_auth_failures() {
        let mismatch = serve(
            "/api/v1/models",
            ResponseTemplate::new(200).set_body_json(
                json!({"data": [{"id": "a/b"}], "links": {"next": null}, "total_count": 5}),
            ),
        )
        .await;
        assert_eq!(&*list(&mismatch).await.unwrap_err().note.id, "query.schema");
        let looping = serve(
            "/api/v1/models",
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": [], "links": {"next": "x"}, "total_count": 9})),
        )
        .await;
        assert_eq!(&*list(&looping).await.unwrap_err().note.id, "query.schema");
        let unauthorized = serve("/api/v1/models", ResponseTemplate::new(401)).await;
        assert_eq!(
            list(&unauthorized).await.unwrap_err().status,
            Status::AuthenticationError
        );
    }

    #[test]
    fn account_product_has_no_model_listing() {
        let source = crate::models::support(&account(Some("account"), None));
        assert!(source.is_err());
        assert!(crate::models::support(&account(None, None)).is_ok());
    }
    #[test]
    fn native_sign_in_issues_an_inference_key_so_it_cannot_use_the_account_product() {
        let mut signed_in = account(None, None);
        signed_in.credential_env = None;
        signed_in.oauth = Some(crate::oauth::OAuthConfig::new(String::new()));
        let ok = crate::Config {
            accounts: vec![signed_in.clone()],
            ..crate::Config::default()
        };
        ok.validate().expect("key view with a native sign-in");
        signed_in.params.insert("product".into(), "account".into());
        let bad = crate::Config {
            accounts: vec![signed_in],
            ..crate::Config::default()
        };
        assert!(matches!(
            bad.validate(),
            Err(crate::ConfigError::OpenRouter)
        ));
    }
}
