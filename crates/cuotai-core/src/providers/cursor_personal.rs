//! Read-only personal Cursor usage query via the same cookie session the Cursor
//! IDE reuses. Cuotai never generates tokens, refreshes them, or reads local
//! secrets; the imported access token or the manual `CURSOR_ACCESS_TOKEN`
//! env var is the only secret involved, and it is forwarded as a session
//! cookie scoped to `cursor.com`.
//!
//! Endpoints (all read-only):
//!   - GET  https://cursor.com/api/usage-summary
//!     Included plan used/limit (cents), total/auto/api percent used,
//!     on-demand used/limit, billing cycle start/end, membership type.
//!     Percent fields are already in percentage units (0..100+); raw
//!     over-limit values are preserved so the display layer can detect
//!     `Exhausted` without rounding away the fact.
//!   - GET  https://cursor.com/api/auth/me (best effort)
//!     Stable user id, email, name. Used only to attach a note to the
//!     report; absence never fabricates a fake zero.
//!   - GET  https://cursor.com/api/usage?user=<id> (best effort)
//!     Legacy request-based plan shape, only relevant when the new
//!     token-based summary is absent.
//!
//! Authentication is always a session cookie:
//!   Cookie: WorkosCursorSessionToken=<userID>%3A%3A<JWT>
//! where `<userID>` is the final non-empty component of a legacy Auth0
//! `sub`, or the complete modern subject (for example `user_AbC123`).
//! Its spelling is case-sensitive and MUST NOT be lowercased. A bare JWT is
//! auto-prefixed with that user id; a complete cookie value is forwarded
//! verbatim. Personal mode is the only path that uses these endpoints;
//! team/admin spend keeps going through the Admin API
//! (`subscriptions::cursor`).
//!
//! Endpoint contract derived from steipete/CodexBar (MIT). No code ported;
//! only documented public URLs and JSON field names are reused. See
//! `THIRD_PARTY_NOTICES` at the workspace root for the project-wide list.
use super::{json, param, secret, Context, Data, QueryError};
use crate::oauth::cursor_import::session_user_id_from_jwt;
use crate::text::{params, Notes, ParamValue};
use crate::{Account, Metric, MetricKind, MetricWindow};
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Fixed, trusted base URL. No endpoint or host override is exposed: any
/// extension would defeat the purpose of a personal session.
const CURSOR_BASE: &str = "https://cursor.com";

/// Header name sent on every request. The Cookie: header is required even
/// when the payload is empty; Cursor's reverse proxy rejects anonymous
/// requests to these routes with 401.
const COOKIE_HEADER: &str = "Cookie";
const SESSION_COOKIE: &str = "WorkosCursorSessionToken";

/// References: steipete/CodexBar CursorStatusProbe and CursorAppAuth under
/// MIT (see THIRD_PARTY_NOTICES).
const REF_PROBE: &str =
    "https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift";
const REF_APP_AUTH: &str =
    "https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorAppAuth.swift";
const REF_DOCS: &str = "https://github.com/steipete/CodexBar/blob/main/docs/cursor.md";

/// Public, non-secret params expected from the parent dispatcher
/// (see `crate::oauth::account_params`):
///   `device_id` and `user_id` are projected from the JWT/Keychain
///   metadata. `user_id` is the exact, case-sensitive ID used in the session
///   cookie. `query_mode=personal` is set by the parent to route the account
///   to this adapter; this module never inspects it.
const PARAM_DEVICE_ID: &str = "device_id";
const PARAM_USER_ID: &str = "user_id";

pub(crate) async fn fetch(account: &Account, ctx: &Context<'_>) -> Result<Data, QueryError> {
    fetch_at_base(account, ctx, CURSOR_BASE).await
}

async fn fetch_at_base(
    account: &Account,
    ctx: &Context<'_>,
    base: &str,
) -> Result<Data, QueryError> {
    let secret_value = secret(account, ctx, "CURSOR_ACCESS_TOKEN")?;
    let cookie = build_session_cookie(&secret_value, param(account, PARAM_USER_ID).ok())?;
    let mut notes = Notes::new();
    if let Ok(device) = param(account, PARAM_DEVICE_ID) {
        notes.push_params(
            "note.cursor_personal.imported_device",
            params([("device_id", ParamValue::from(device.to_string()))]),
            "Session imported from the Cursor IDE (device_id {device_id}); no password is stored.",
        );
    } else {
        notes.push(
            "note.cursor_personal.imported_manual_token",
            "Session imported from the Cursor IDE (manual token); no password is stored.",
        );
    }
    notes.push(
        "note.cursor_personal.personal_endpoint",
        "Personal cursor.com/api/usage-summary endpoint; not the team Admin API.",
    );

    let summary: UsageSummary = json(
        ctx.http
            .get(format!("{base}/api/usage-summary"))
            .header(COOKIE_HEADER, &cookie)
            .header("Accept", "application/json")
            .timeout(ctx.timeout),
    )
    .await?;

    let user_info = fetch_user_info(ctx, &cookie, base).await;

    let mut metrics = Vec::new();
    metrics.extend(plan_metrics(&summary)?);
    metrics.extend(on_demand_metrics(&summary)?);

    if let Some(info) = user_info.as_ref() {
        if let Some(email) = info.email.as_deref().filter(|s| !s.trim().is_empty()) {
            notes.push_params(
                "note.cursor_personal.account_email",
                params([("email", ParamValue::from(email.to_string()))]),
                "Account: {email}.",
            );
        }
        if let Some(name) = info.name.as_deref().filter(|s| !s.trim().is_empty()) {
            notes.push_params(
                "note.cursor_personal.user_name",
                params([("name", ParamValue::from(name.to_string()))]),
                "User: {name}.",
            );
        }
        if let Some(membership) = summary
            .membership_type
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            notes.push_params(
                "note.cursor_personal.membership",
                params([("membership", ParamValue::from(membership.to_string()))]),
                "Cursor plan: {membership}.",
            );
        }
    } else if let Some(membership) = summary
        .membership_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        notes.push_params(
            "note.cursor_personal.membership",
            params([("membership", ParamValue::from(membership.to_string()))]),
            "Cursor plan: {membership}.",
        );
    }

    let resets_at = summary.billing_cycle_end.as_deref().and_then(parse_iso8601);

    let has_plan_cents = summary
        .individual_usage
        .as_ref()
        .and_then(|i| i.plan.as_ref())
        .and_then(|p| p.limit)
        .is_some();

    if !has_plan_cents {
        if let Some(sub) = user_info
            .as_ref()
            .and_then(|i| i.sub.as_deref())
            .filter(|s| !s.trim().is_empty())
        {
            if let Ok(snapshot) = fetch_legacy_request_usage(ctx, &cookie, base, sub).await {
                if let Some(metric) = legacy_request_metric(&snapshot, resets_at)? {
                    metrics.push(metric);
                }
            }
        }
    }

    if metrics.is_empty() {
        notes.push(
            "note.cursor_personal.no_numeric_usage",
            "No numeric block returned by the personal API; absent does not mean unlimited.",
        );
    }

    Ok(Data::new(
        metrics,
        notes,
        format!("{REF_PROBE} ; {REF_APP_AUTH} ; {REF_DOCS}"),
    ))
}

/// Normalize the incoming secret value into a `WorkosCursorSessionToken=...`
/// cookie value. Accepts:
///   - `WorkosCursorSessionToken=<encoded>` (passthrough, validated)
///   - `<userID>%3A%3A<JWT>` or `<userID>::<JWT>` (already in Cursor's
///     cookie format)
///   - bare `<JWT>` (extracts the case-sensitive Cursor session user ID)
///
/// The `param_user_id` fallback comes from `account.params.user_id`, set
/// when the parent projects JWT metadata. When neither the JWT nor the
/// param carries a valid user id, the value is rejected — Cursor's reverse
/// proxy would also reject the cookie without it.
fn build_session_cookie(
    secret_value: &str,
    param_user_id: Option<&str>,
) -> Result<String, QueryError> {
    let trimmed = secret_value.trim();
    if trimmed.is_empty() {
        return Err(QueryError::config(
            "query.cursor_personal.missing_token",
            "Missing Cursor token; import via the assistant or set CURSOR_ACCESS_TOKEN.",
        ));
    }
    if let Some(rest) = trimmed
        .strip_prefix("WorkosCursorSessionToken=")
        .or_else(|| trimmed.strip_prefix("workoscursorsessiontoken="))
    {
        let value = rest.trim();
        if value.is_empty() {
            return Err(QueryError::config(
                "query.cursor_personal.empty_cookie",
                "WorkosCursorSessionToken cookie is empty; re-import the Cursor session.",
            ));
        }
        return Ok(format!("{SESSION_COOKIE}={value}"));
    }
    if let Some((user_id, jwt)) = split_user_id_and_jwt(trimmed) {
        if user_id.is_empty() || jwt.is_empty() {
            return Err(QueryError::config(
                "query.cursor_personal.empty_user_token_prefix",
                "userID::token prefix is empty; check the imported value.",
            ));
        }
        let user_id = url_encode_user_id(user_id);
        return Ok(format!("{SESSION_COOKIE}={user_id}%3A%3A{jwt}"));
    }
    if looks_like_jwt(trimmed) {
        let user_id = session_user_id_from_jwt(trimmed)
            .or_else(|| {
                param_user_id
                    .map(str::trim)
                    .filter(|value| is_user_id_like(value))
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                QueryError::config(
                    "query.cursor_personal.jwt_missing_subject",
                    "Cursor JWT without `sub` claim; import the session from the IDE to fill the user id.",
                )
            })?;
        let user_id = url_encode_user_id(&user_id);
        return Ok(format!("{SESSION_COOKIE}={user_id}%3A%3A{trimmed}"));
    }
    Err(QueryError::config(
        "query.cursor_personal.invalid_token_format",
        "Invalid Cursor token format; supply a JWT, `userID::JWT` or `WorkosCursorSessionToken=...`.",
    ))
}

/// Split `<userID>::<JWT>` or `<userID>%3A%3A<JWT>` into its two parts when
/// the head looks like a Cursor user id and the tail looks like a JWT.
fn split_user_id_and_jwt(value: &str) -> Option<(&str, &str)> {
    let pos = value.find("::").or_else(|| value.find("%3A%3A"))?;
    let (head, rest) = value.split_at(pos);
    let tail = rest.trim_start_matches("::").trim_start_matches("%3A%3A");
    if is_user_id_like(head.trim()) && looks_like_jwt(tail) {
        return Some((head.trim(), tail));
    }
    None
}

fn url_encode_user_id(value: &str) -> String {
    // Cursor uses percent-encoded form. The user-id charset is alphanumeric
    // with . _ -, so encoding preserves its case-sensitive ASCII bytes.
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{:02X}", other)),
        }
    }
    out
}

fn looks_like_jwt(value: &str) -> bool {
    let mut parts = value.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(_), Some(p), Some(_), None) if !p.is_empty()
    )
}

fn is_user_id_like(value: &str) -> bool {
    let len = value.len();
    (4..=128).contains(&len)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

async fn fetch_user_info(ctx: &Context<'_>, cookie: &str, base: &str) -> Option<UserInfo> {
    let request = ctx
        .http
        .get(format!("{base}/api/auth/me"))
        .header(COOKIE_HEADER, cookie)
        .header("Accept", "application/json")
        .timeout(ctx.timeout);
    let result: Result<UserInfo, _> = json(request).await;
    match result {
        Ok(info) if info.email.is_some() || info.sub.is_some() => Some(info),
        _ => None,
    }
}

async fn fetch_legacy_request_usage(
    ctx: &Context<'_>,
    cookie: &str,
    base: &str,
    sub: &str,
) -> Result<UsageResponse, QueryError> {
    let user = sub
        .split('|')
        .next_back()
        .unwrap_or(sub)
        .trim()
        .to_lowercase();
    let request = ctx
        .http
        .get(format!("{base}/api/usage"))
        .query(&[("user", user.as_str())])
        .header(COOKIE_HEADER, cookie)
        .header("Accept", "application/json")
        .timeout(ctx.timeout);
    json(request).await
}

fn plan_metrics(summary: &UsageSummary) -> Result<Vec<Metric>, QueryError> {
    let plan = match summary
        .individual_usage
        .as_ref()
        .and_then(|i| i.plan.as_ref())
    {
        Some(plan) => plan,
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    let used_usd = plan.used.map(cents_to_dollars).transpose()?;
    let limit_usd = plan.limit.map(cents_to_dollars).transpose()?;
    let resets_at = summary.billing_cycle_end.as_deref().and_then(parse_iso8601);
    // Primary bar: when the API publishes the headline percent, use it as
    // the total cap of 100. `used` and `remaining` are the raw arithmetic
    // pair (100 - used): over-limit values like 142.7 stay verbatim on both
    // sides so the dashboard can classify the account as Exhausted without
    // the provider silently reinterpreting the math (negative remaining
    // is meaningful here). The display layer clamps the gauge; the data
    // stays exact.
    if let Some(total) = plan.total_percent_used {
        out.push(Metric {
            billing: Default::default(),
            scope: None,
            label: "Cursor plano (incluído)".into(),
            used: Some(total),
            limit: Some(100.0),
            remaining: Some(100.0 - total),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Monthly),
            period: Some("ciclo de cobrança".into()),
            resets_at,
        });
    } else if used_usd.is_some() || limit_usd.is_some() {
        // No totalPercentUsed: fall back to raw cents, no fake synthetic
        // overall from auto/api (they understate the total).
        out.push(Metric {
            billing: Default::default(),
            scope: None,
            label: "Cursor plano (incluído)".into(),
            used: used_usd,
            limit: limit_usd,
            remaining: None,
            unit: "USD".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Monthly),
            period: Some("ciclo de cobrança".into()),
            resets_at,
        });
    }
    // Independent contributions: auto and api are each their own percent
    // with no per-window cap documented; do not claim a synthetic limit.
    if let (Some(auto), Some(api)) = (plan.auto_percent_used, plan.api_percent_used) {
        out.push(Metric {
            billing: Default::default(),
            scope: None,
            label: "Cursor Auto (Composer)".into(),
            used: Some(auto),
            limit: None,
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Usage,
            window: Some(MetricWindow::Monthly),
            period: Some("ciclo de cobrança".into()),
            resets_at,
        });
        out.push(Metric {
            billing: Default::default(),
            scope: None,
            label: "Cursor modelos nomeados".into(),
            used: Some(api),
            limit: None,
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Usage,
            window: Some(MetricWindow::Monthly),
            period: Some("ciclo de cobrança".into()),
            resets_at,
        });
    } else if let Some(auto) = plan.auto_percent_used {
        out.push(Metric {
            billing: Default::default(),
            scope: None,
            label: "Cursor Auto (Composer)".into(),
            used: Some(auto),
            limit: None,
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Usage,
            window: Some(MetricWindow::Monthly),
            period: Some("ciclo de cobrança".into()),
            resets_at,
        });
    } else if let Some(api) = plan.api_percent_used {
        out.push(Metric {
            billing: Default::default(),
            scope: None,
            label: "Cursor modelos nomeados".into(),
            used: Some(api),
            limit: None,
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Usage,
            window: Some(MetricWindow::Monthly),
            period: Some("ciclo de cobrança".into()),
            resets_at,
        });
    }
    Ok(out)
}

fn on_demand_metrics(summary: &UsageSummary) -> Result<Vec<Metric>, QueryError> {
    let on_demand = match summary
        .individual_usage
        .as_ref()
        .and_then(|i| i.on_demand.as_ref())
    {
        Some(value) => value,
        None => return Ok(Vec::new()),
    };
    let used = on_demand.used.map(cents_to_dollars).transpose()?;
    let limit = on_demand.limit.map(cents_to_dollars).transpose()?;
    if used.is_none() && limit.is_none() {
        return Ok(Vec::new());
    }
    let resets_at = summary.billing_cycle_end.as_deref().and_then(parse_iso8601);
    Ok(vec![Metric {
        billing: crate::Billing::Overage,
        scope: None,
        label: "Sob demanda (on-demand)".into(),
        used,
        limit,
        remaining: None,
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: Some(MetricWindow::Monthly),
        period: Some("ciclo de cobrança".into()),
        resets_at,
    }])
}

fn legacy_request_metric(
    snapshot: &UsageResponse,
    resets_at: Option<DateTime<Utc>>,
) -> Result<Option<Metric>, QueryError> {
    let gpt4 = match snapshot.gpt4.as_ref() {
        Some(value) => value,
        None => return Ok(None),
    };
    let used = gpt4.num_requests_total.or(gpt4.num_requests);
    let limit = gpt4.max_request_usage;
    if used.is_none() && limit.is_none() {
        return Ok(None);
    }
    let percent = match (used, limit) {
        (Some(used), Some(limit)) if limit > 0 => Some((used as f64 / limit as f64) * 100.0),
        _ => None,
    };
    Ok(Some(Metric {
        billing: Default::default(),
        scope: None,
        label: "Cursor plano baseado em requisições".into(),
        used: percent,
        limit: limit.map(|v| v as f64),
        remaining: None,
        unit: percent.map(|_| "%").unwrap_or("solicitações").into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Monthly),
        period: Some("ciclo de cobrança".into()),
        resets_at,
    }))
}

fn cents_to_dollars(cents: i64) -> Result<f64, QueryError> {
    let value = cents as f64 / 100.0;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(QueryError::schema())
    }
}

fn parse_iso8601(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .ok()
}

// ------------------------------------------------------------------ response models

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct UsageSummary {
    #[serde(default)]
    billing_cycle_end: Option<String>,
    #[serde(default)]
    membership_type: Option<String>,
    #[serde(default)]
    individual_usage: Option<IndividualUsage>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct IndividualUsage {
    #[serde(default)]
    plan: Option<PlanUsage>,
    #[serde(default)]
    on_demand: Option<OnDemandUsage>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PlanUsage {
    #[serde(default)]
    used: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    auto_percent_used: Option<f64>,
    #[serde(default)]
    api_percent_used: Option<f64>,
    #[serde(default)]
    total_percent_used: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct OnDemandUsage {
    #[serde(default)]
    used: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct UserInfo {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    sub: Option<String>,
}

#[derive(Deserialize, Default)]
struct UsageResponse {
    #[serde(default, rename = "gpt-4")]
    gpt4: Option<ModelUsage>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ModelUsage {
    #[serde(default)]
    num_requests: Option<i64>,
    #[serde(default)]
    num_requests_total: Option<i64>,
    #[serde(default)]
    max_request_usage: Option<i64>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn on_demand_limit_zero_means_disabled_and_a_missing_amount_stays_unknown() {
        use crate::overage::{assess_metrics, OverageState};
        let state = |json: serde_json::Value| {
            let summary: UsageSummary = serde_json::from_value(json).unwrap();
            let metrics = on_demand_metrics(&summary).unwrap();
            assess_metrics(&metrics, None).map(|o| (o.state, o.spent))
        };
        let od = |v: serde_json::Value| serde_json::json!({ "individualUsage": { "onDemand": v } });
        assert_eq!(
            state(od(serde_json::json!({ "used": 0, "limit": 0 }))),
            Some((OverageState::Disabled, Some(0.0)))
        );
        assert_eq!(
            state(od(serde_json::json!({ "limit": 0 }))),
            Some((OverageState::Disabled, None))
        );
        assert_eq!(
            state(od(serde_json::json!({ "limit": 5000 }))),
            Some((OverageState::Unknown, None))
        );
        assert_eq!(
            state(od(serde_json::json!({ "used": 625, "limit": 5000 }))),
            Some((OverageState::Active, Some(6.25)))
        );
        assert_eq!(
            state(od(serde_json::json!({ "used": 5000, "limit": 5000 }))),
            Some((OverageState::CapReached, Some(50.0)))
        );
        assert_eq!(state(serde_json::json!({})), None);
    }

    use super::*;
    use crate::{Credentials, Provider};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    struct StaticCred(String);
    impl Credentials for StaticCred {
        fn get(&self, name: &str) -> Option<String> {
            if name == "CURSOR_ACCESS_TOKEN" {
                Some(self.0.clone())
            } else {
                None
            }
        }
    }

    fn account() -> Account {
        Account {
            id: None,
            name: "cursor-personal".into(),
            provider: Provider::Cursor,
            credential_env: Some("CURSOR_ACCESS_TOKEN".into()),
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
            group: None,
        }
    }

    #[test]
    fn bare_jwt_uses_legacy_subject_user_id_without_changing_case() {
        // CursorAppAuth.userID() takes the final Auth0 subject component but
        // does not lowercase it because the cookie prefix is case-sensitive.
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"auth0|UserABC","exp":9999999999}"#);
        let token = format!("{header}.{payload}.sig");
        let cookie = build_session_cookie(&token, None).unwrap();
        assert_eq!(
            cookie,
            "WorkosCursorSessionToken=UserABC%3A%3A".to_string() + &token
        );
    }

    #[test]
    fn already_prefixed_cookie_value_passes_through() {
        let cookie = build_session_cookie(
            "userabc%3A%3AeyJhbGciOiJub25lIn0.eyJzdWIiOiJ8dXNlcmFiYyJ9.sig",
            None,
        )
        .unwrap();
        assert_eq!(
            cookie,
            "WorkosCursorSessionToken=userabc%3A%3AeyJhbGciOiJub25lIn0.eyJzdWIiOiJ8dXNlcmFiYyJ9.sig"
        );
    }

    #[test]
    fn full_cookie_header_passes_through() {
        let cookie = build_session_cookie(
            "WorkosCursorSessionToken=userabc%3A%3AeyJhbGciOiJub25lIn0.eyJzdWIiOiJ8dXNlcmFiYyJ9.sig",
            None,
        )
        .unwrap();
        assert!(cookie.starts_with("WorkosCursorSessionToken=userabc%3A%3A"));
    }

    #[test]
    fn empty_value_is_rejected() {
        assert!(build_session_cookie("", None).is_err());
        assert!(build_session_cookie("   ", None).is_err());
    }

    #[test]
    fn malformed_value_is_rejected() {
        assert!(build_session_cookie("not-a-jwt", None).is_err());
    }

    #[test]
    fn bare_jwt_without_sub_uses_case_sensitive_param_user_id() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(br#"{"exp":9999999999}"#);
        let token = format!("{header}.{payload}.sig");
        let cookie = build_session_cookie(&token, Some("user_FallbackABC")).unwrap();
        assert_eq!(
            cookie,
            "WorkosCursorSessionToken=user_FallbackABC%3A%3A".to_string() + &token
        );
    }

    #[tokio::test]
    async fn fetches_summary_with_case_sensitive_modern_session_cookie() {
        let fake_jwt = fake_jwt_with_sub("user_AbC123_XyZ");
        let expected_cookie = format!("WorkosCursorSessionToken=user_AbC123_XyZ%3A%3A{fake_jwt}");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage-summary"))
            .and(header("Cookie", expected_cookie.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "billingCycleStart": "2026-09-01T00:00:00Z",
                "billingCycleEnd": "2026-10-01T00:00:00Z",
                "membershipType": "pro",
                "individualUsage": {
                    "plan": {
                        "used": 1234,
                        "limit": 2000,
                        "autoPercentUsed": 36.0,
                        "apiPercentUsed": 12.5,
                        "totalPercentUsed": 24.25
                    },
                    "onDemand": {
                        "used": 50,
                        "limit": 5000
                    }
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/auth/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "email": "[email protected]",
                "name": "Ada",
                "sub": "user_AbC123_XyZ"
            })))
            .mount(&server)
            .await;
        let creds = StaticCred(fake_jwt);
        let account = account();
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &creds,
            timeout: Duration::from_secs(2),
        };
        let data = fetch_at_base(&account, &ctx, &server.uri()).await.unwrap();
        assert_eq!(data.metrics.len(), 4);
        let plan = data
            .metrics
            .iter()
            .find(|m| m.label == "Cursor plano (incluído)")
            .expect("plan metric present");
        assert_eq!(plan.unit, "%");
        assert_eq!(plan.used, Some(24.25));
        assert_eq!(plan.limit, Some(100.0));
        assert_eq!(plan.remaining, Some(75.75));
        assert_eq!(plan.kind, MetricKind::Quota);
        assert_eq!(plan.window, Some(MetricWindow::Monthly));
        for label in ["Cursor Auto (Composer)", "Cursor modelos nomeados"] {
            let usage = data
                .metrics
                .iter()
                .find(|m| m.label == label)
                .expect("usage metric");
            assert_eq!(usage.kind, MetricKind::Usage);
            assert_eq!(usage.window, Some(MetricWindow::Monthly));
        }
        let on_demand = data
            .metrics
            .iter()
            .find(|m| m.label == "Sob demanda (on-demand)")
            .expect("on-demand metric present");
        assert_eq!(on_demand.unit, "USD");
        assert_eq!(on_demand.used, Some(0.5));
        assert_eq!(on_demand.limit, Some(50.0));
        assert_eq!(on_demand.kind, MetricKind::Spend);
        let state = crate::overage::assess_metrics(std::slice::from_ref(on_demand), None).unwrap();
        assert_eq!(state.state, crate::overage::OverageState::Active);
        assert_eq!(on_demand.window, Some(MetricWindow::Monthly));
        assert!(data.notes.iter().any(|n| n.contains("[email protected]")));
    }

    #[tokio::test]
    async fn over_limit_percent_is_preserved_verbatim() {
        // Cursor reports >100 when an overage is already past the included
        // cap. Cuotai must keep that raw value so the UI can classify the
        // account as Exhausted without rounding to a synthetic 100.
        let fake_jwt = fake_jwt_with_sub("auth0|userabc");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage-summary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "billingCycleEnd": "2026-10-01T00:00:00Z",
                "membershipType": "pro",
                "individualUsage": {
                    "plan": { "totalPercentUsed": 142.7 }
                }
            })))
            .mount(&server)
            .await;
        let creds = StaticCred(format!("userabc%3A%3A{fake_jwt}"));
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &creds,
            timeout: Duration::from_secs(2),
        };
        let data = fetch_at_base(&account(), &ctx, &server.uri())
            .await
            .unwrap();
        let plan = data
            .metrics
            .iter()
            .find(|m| m.label == "Cursor plano (incluído)")
            .expect("plan metric present");
        // Raw over-limit percent stays verbatim on both sides: `used` is
        // 142.7 (well above the cap) and `remaining` is the matching
        // negative (-42.7). The provider does not silently clamp; the
        // display layer is responsible for the gauge clamp.
        assert_eq!(plan.used, Some(142.7));
        assert_eq!(plan.unit, "%");
        assert_eq!(plan.limit, Some(100.0));
        assert!((plan.remaining.unwrap() + 42.7).abs() < 1e-9);
    }

    #[tokio::test]
    async fn missing_user_info_does_not_break_summary() {
        let fake_jwt = fake_jwt_with_sub("auth0|userabc");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage-summary"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let creds = StaticCred(format!("userabc%3A%3A{fake_jwt}"));
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &creds,
            timeout: Duration::from_secs(2),
        };
        let err = fetch_at_base(&account(), &ctx, &server.uri())
            .await
            .unwrap_err();
        assert!(matches!(err.status, crate::Status::AuthenticationError));
    }

    #[tokio::test]
    async fn legacy_request_usage_only_when_no_plan_cents() {
        let fake_jwt = fake_jwt_with_sub("google-oauth2|userabc");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage-summary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "billingCycleEnd": "2026-10-01T00:00:00Z",
                "membershipType": "hobby"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/auth/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "google-oauth2|userabc"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/usage"))
            .and(query_param("user", "userabc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "gpt-4": {
                    "numRequests": 120,
                    "numRequestsTotal": 500,
                    "maxRequestUsage": 500
                }
            })))
            .mount(&server)
            .await;
        let creds = StaticCred(format!("userabc%3A%3A{fake_jwt}"));
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &creds,
            timeout: Duration::from_secs(2),
        };
        let data = fetch_at_base(&account(), &ctx, &server.uri())
            .await
            .unwrap();
        let legacy = data
            .metrics
            .iter()
            .find(|m| m.label == "Cursor plano baseado em requisições")
            .expect("legacy metric present");
        assert_eq!(legacy.used, Some(100.0));
        assert_eq!(legacy.limit, Some(500.0));
    }

    #[tokio::test]
    async fn legacy_request_usage_is_skipped_when_plan_cents_present() {
        let fake_jwt = fake_jwt_with_sub("auth0|userabc");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage-summary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "billingCycleEnd": "2026-10-01T00:00:00Z",
                "membershipType": "pro",
                "individualUsage": {
                    "plan": { "used": 100, "limit": 1000, "totalPercentUsed": 10.0 }
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/auth/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "auth0|userabc"
            })))
            .mount(&server)
            .await;
        let creds = StaticCred(format!("userabc%3A%3A{fake_jwt}"));
        let http = reqwest::Client::new();
        let ctx = Context {
            http: &http,
            credentials: &creds,
            timeout: Duration::from_secs(2),
        };
        let data = fetch_at_base(&account(), &ctx, &server.uri())
            .await
            .unwrap();
        assert!(data
            .metrics
            .iter()
            .all(|m| m.label != "Cursor plano baseado em requisições"));
    }

    fn fake_jwt_with_sub(sub: &str) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload =
            URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{sub}","exp":9999999999}}"#).as_bytes());
        format!("{header}.{payload}.signature")
    }
}
