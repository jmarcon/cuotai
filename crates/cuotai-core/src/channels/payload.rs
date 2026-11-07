//! PRD 0044 RF2: one core-rendered notification ([`Outgoing`]) and its
//! four wire formats ([`render`]).
//!
//! The texts come from the same renderers as the system notification
//! (PRD 0032: `alerts::notice_with`, `ResetEvent::notice_with`,
//! `quiet::digest`, the PRD 0043 digest) in the person's language, with
//! the channel's `include_names` deciding whether account names and metric
//! labels appear. Never a credential, variable name, URL, e-mail or raw
//! provider text: an account name passes the log redactor first.

use super::{ChannelKind, Filters, NoticeKind, Severity};
use crate::alerts::{self, AlertEvent, AlertKind};
use crate::resets::ResetEvent;
use crate::text::Localized;
use crate::{Account, MetricWindow, Provider};
use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use std::fmt::Display;

/// RF2.3: body caps.
pub const WEBHOOK_BODY_BYTES: usize = 4096;
pub const NTFY_BODY_BYTES: usize = 4096;
pub const SLACK_BODY_BYTES: usize = 3072;
pub const DISCORD_CHARS: usize = 1900;
/// Slack's header block holds at most 150 characters.
const SLACK_HEADER_CHARS: usize = 150;

/// What produced a notification: the same values the system notice was
/// rendered from, so each channel renders it with its own filters.
#[derive(Debug, Clone)]
pub enum Source {
    /// The usage alerts of one account delivered together.
    Alerts {
        account: Account,
        events: Vec<AlertEvent>,
    },
    /// One reset notice.
    Reset { account: Account, event: ResetEvent },
    /// PRD 0032 RF3.3: the digest of what still holds when quiet hours end.
    QuietDigest {
        groups: Vec<(Account, Vec<AlertEvent>)>,
    },
    /// PRD 0043 RF3: the weekly digest.
    WeeklyDigest(Box<crate::digest::Digest>),
    /// RF4.2: **Send test**.
    Test,
    /// RF3.2: the notifications the hourly cap held back.
    Summary { suppressed: usize },
}

/// The metric a notification is about (labels only with names shown).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<MetricWindow>,
}

/// RF2.1: one notification rendered for one channel.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outgoing {
    /// A [`NoticeKind`] slug, `test` or `rate_limit`.
    pub kind: &'static str,
    pub severity: Severity,
    pub title: String,
    /// Plain text.
    pub body: String,
    /// The Markdown body of the weekly digest (webhook, Slack, Discord).
    #[serde(skip)]
    pub markdown: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub provider: Option<Provider>,
    pub account_id: Option<String>,
    /// Only when the channel shows names; passed through the redactor.
    pub account_name: Option<String>,
    pub metric: Option<MetricOut>,
    /// Consumed percent at the reading.
    pub current: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    /// `cuotai://account/<id>` once the desktop registers the scheme
    /// (PRD 0025); omitted until then (§12).
    pub link: Option<String>,
    pub sample: bool,
}

fn most_severe(events: &[AlertEvent]) -> Option<&AlertEvent> {
    events
        .iter()
        .fold(None, |best: Option<&AlertEvent>, event| {
            let severity = NoticeKind::of_alert(event.kind).severity();
            match best {
                Some(best) if NoticeKind::of_alert(best.kind).severity() >= severity => Some(best),
                _ => Some(event),
            }
        })
}

fn scrubbed_name(account: &Account) -> String {
    crate::diagnostics::redact::text(&account.display_name())
}

/// RF2.2: no e-mail address leaves in a text (an account named after one
/// shows `[email]`), nor a web address.
pub fn scrub(text: &str) -> String {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<(regex::Regex, regex::Regex)> = OnceLock::new();
    let (email, url) = PATTERNS.get_or_init(|| {
        (
            regex::Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").expect("static"),
            regex::Regex::new(r"(?i)\b[a-z][a-z0-9+.-]*://\S+").expect("static"),
        )
    });
    let text = email.replace_all(text, crate::diagnostics::redact::EMAIL);
    url.replace_all(&text, crate::diagnostics::redact::SECRET)
        .into_owned()
}

fn base(
    kind: &'static str,
    severity: Severity,
    title: String,
    body: String,
    now: DateTime<Utc>,
) -> Outgoing {
    Outgoing {
        kind,
        severity,
        title: scrub(&title),
        body: scrub(&body),
        markdown: None,
        occurred_at: now,
        provider: None,
        account_id: None,
        account_name: None,
        metric: None,
        current: None,
        resets_at: None,
        link: None,
        sample: false,
    }
}

fn with_account(mut out: Outgoing, account: &Account, names: bool) -> Outgoing {
    out.provider = Some(account.provider);
    out.account_id = Some(account.state_key());
    out.account_name = names.then(|| scrubbed_name(account));
    out
}

fn with_event(mut out: Outgoing, event: &AlertEvent, names: bool) -> Outgoing {
    if !event.label.is_empty() || event.window.is_some() {
        out.metric = Some(MetricOut {
            label: (names && !event.label.is_empty()).then(|| event.label.clone()),
            window: event.window,
        });
    }
    if matches!(
        event.kind,
        AlertKind::Threshold | AlertKind::Exhausted | AlertKind::Recovered
    ) {
        out.current = Some((event.consumed * 100.0).clamp(0.0, 100.0).round());
    }
    out.resets_at = event.resets_at;
    out
}

/// RF2.1/RF2.2: the notification `source` becomes for a channel with
/// `filters`, PRD 0032's `private` setting and the person's `locale`;
/// `None` when the filters keep nothing.
pub fn prepare<Tz>(
    source: &Source,
    filters: &Filters,
    private: bool,
    locale: &str,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Option<Outgoing>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let names = filters.names(private);
    match source {
        Source::Alerts { account, events } => {
            if !filters.admits_account(&account.state_key()) {
                return None;
            }
            let kept: Vec<AlertEvent> = events
                .iter()
                .filter(|event| filters.admits(NoticeKind::of_alert(event.kind)))
                .cloned()
                .collect();
            let top = most_severe(&kept)?.clone();
            let notice = alerts::notice_with(&[(account, kept)], now, tz, !names)?;
            let kind = NoticeKind::of_alert(top.kind);
            let out = base(
                kind.slug(),
                kind.severity(),
                notice.title.resolve(locale),
                notice.body.resolve(locale),
                now,
            );
            Some(with_event(with_account(out, account, names), &top, names))
        }
        Source::Reset { account, event } => {
            if !filters.admits_account(&account.state_key()) || !filters.admits(NoticeKind::Reset) {
                return None;
            }
            let notice = event.notice_with(account, now, !names);
            let mut out = with_account(
                base(
                    NoticeKind::Reset.slug(),
                    NoticeKind::Reset.severity(),
                    notice.title.resolve(locale),
                    notice.body.resolve(locale),
                    now,
                ),
                account,
                names,
            );
            if let Some(first) = event.metrics.first() {
                out.metric = Some(MetricOut {
                    label: names.then(|| first.label.clone()),
                    window: Some(first.window),
                });
            }
            out.resets_at = Some(event.resets_at);
            Some(out)
        }
        Source::QuietDigest { groups } => {
            let kept: Vec<(&Account, Vec<AlertEvent>)> = groups
                .iter()
                .filter(|(account, _)| filters.admits_account(&account.state_key()))
                .map(|(account, events)| {
                    let events = events
                        .iter()
                        .filter(|event| filters.admits(NoticeKind::of_alert(event.kind)))
                        .cloned()
                        .collect::<Vec<_>>();
                    (account, events)
                })
                .filter(|(_, events)| !events.is_empty())
                .collect();
            let all: Vec<AlertEvent> = kept.iter().flat_map(|(_, e)| e.clone()).collect();
            let top = most_severe(&all)?;
            let kind = NoticeKind::of_alert(top.kind);
            let notice = crate::quiet::digest(&kept, now, tz, !names)?;
            let out = base(
                kind.slug(),
                kind.severity(),
                notice.title.resolve(locale),
                notice.body.resolve(locale),
                now,
            );
            Some(match kept.as_slice() {
                [(account, _)] => with_account(out, account, names),
                _ => out,
            })
        }
        Source::WeeklyDigest(digest) => {
            if !filters.admits(NoticeKind::Digest) {
                return None;
            }
            let lines: Vec<String> = digest
                .short_lines(locale)
                .iter()
                .map(|line| line.resolve(locale))
                .collect();
            let mut out = base(
                NoticeKind::Digest.slug(),
                NoticeKind::Digest.severity(),
                digest.title().resolve(locale),
                lines.join("\n"),
                now,
            );
            out.markdown = Some(if names {
                scrub(&digest.markdown(locale))
            } else {
                lines
                    .iter()
                    .map(|line| format!("- {line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            Some(out)
        }
        Source::Test => {
            let mut out = base(
                "test",
                Severity::Info,
                Localized::fixed("channels.test.title", "Cuotai test").resolve(locale),
                Localized::fixed(
                    "channels.test.body",
                    "This is a test from Cuotai. If you can read it, the channel works.",
                )
                .resolve(locale),
                now,
            );
            out.sample = true;
            Some(out)
        }
        Source::Summary { suppressed } => Some(base(
            "rate_limit",
            Severity::Info,
            "Cuotai".to_owned(),
            Localized::plural(
                "channels.rate_limit.summary",
                crate::text::params([("count", (*suppressed).into())]),
                "{count} more notification was not sent (rate limit).",
                "{count} more notifications were not sent (rate limit).",
            )
            .resolve(locale),
            now,
        )),
    }
}

/// One HTTP request to a channel.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Request([body redacted])")
    }
}

/// Cut `text` to at most `max` bytes at a character boundary, ending with
/// an ellipsis when it was cut.
pub fn cap_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let ellipsis = "…";
    let mut end = max.saturating_sub(ellipsis.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ellipsis}", &text[..end])
}

/// Cut `text` to at most `max` characters, ending with an ellipsis when
/// it was cut.
pub fn cap_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// No control characters but newlines and tabs.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// A header value: one line; non-ASCII text as an RFC 2047 encoded word,
/// which ntfy decodes.
fn header_value(text: &str) -> String {
    let line: String = text.chars().filter(|c| !c.is_control()).collect();
    if line.is_ascii() {
        line
    } else {
        use base64::Engine as _;
        format!(
            "=?UTF-8?B?{}?=",
            base64::engine::general_purpose::STANDARD.encode(line.as_bytes())
        )
    }
}

/// RF2.1: the ntfy emoji short-code of a notification kind.
fn ntfy_tag(kind: &str) -> &'static str {
    match kind {
        "reset" => "arrows_counterclockwise",
        "threshold" => "warning",
        "exhausted" => "rotating_light",
        "recovered" => "white_check_mark",
        "projected" => "hourglass_flowing_sand",
        "overage" => "moneybag",
        "floor" => "money_with_wings",
        "session_expiring" => "key",
        "limit_changed" => "bar_chart",
        "digest" => "calendar",
        "test" => "test_tube",
        _ => "information_source",
    }
}

fn ntfy_priority(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "3",
        Severity::Warning => "4",
        Severity::Critical => "5",
    }
}

/// Slack's `mrkdwn` and `text` escaping.
fn slack_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The fixed webhook JSON (RF2.1).
fn webhook_json(out: &Outgoing) -> serde_json::Value {
    let body = out.markdown.as_deref().unwrap_or(&out.body);
    let mut value = serde_json::json!({
        "schema": 1,
        "kind": out.kind,
        "severity": out.severity,
        "title": clean(&out.title),
        "body": cap_bytes(&clean(body), WEBHOOK_BODY_BYTES),
        "occurred_at": out.occurred_at,
    });
    let object = value.as_object_mut().expect("a JSON object");
    if let Some(provider) = out.provider {
        object.insert("provider".into(), serde_json::json!(provider));
    }
    if let Some(id) = &out.account_id {
        let mut account = serde_json::json!({ "id": id });
        if let Some(name) = &out.account_name {
            account["name"] = serde_json::json!(name);
        }
        object.insert("account".into(), account);
    }
    if let Some(metric) = &out.metric {
        object.insert("metric".into(), serde_json::json!(metric));
    }
    if let Some(percent) = out.current {
        object.insert("current".into(), serde_json::json!({ "percent": percent }));
    }
    if let Some(at) = out.resets_at {
        object.insert("resets_at".into(), serde_json::json!(at));
    }
    if let Some(link) = &out.link {
        object.insert("link".into(), serde_json::json!(link));
    }
    if out.sample {
        object.insert("sample".into(), serde_json::json!(true));
    }
    value
}

/// HMAC-SHA256 of `body` with `secret`, as `sha256=<hex>`.
pub fn signature(secret: &str, body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC takes a key of any length");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256={hex}")
}

/// RF2.1: the request for a `kind` channel; `secret` signs a webhook body.
pub fn render(kind: ChannelKind, out: &Outgoing, secret: Option<&str>) -> Request {
    match kind {
        ChannelKind::Webhook => {
            let body = serde_json::to_vec(&webhook_json(out)).unwrap_or_default();
            let mut headers = vec![("content-type", "application/json".to_owned())];
            if let Some(secret) = secret {
                headers.push(("x-cuotai-signature", signature(secret, &body)));
            }
            Request { headers, body }
        }
        ChannelKind::Ntfy => {
            let mut headers = vec![
                ("content-type", "text/plain; charset=utf-8".to_owned()),
                ("title", header_value(&out.title)),
                ("priority", ntfy_priority(out.severity).to_owned()),
                ("tags", ntfy_tag(out.kind).to_owned()),
            ];
            if let Some(link) = &out.link {
                headers.push(("click", header_value(link)));
            }
            Request {
                headers,
                body: cap_bytes(&clean(&out.body), NTFY_BODY_BYTES).into_bytes(),
            }
        }
        ChannelKind::Slack => {
            let body = out.markdown.as_deref().unwrap_or(&out.body);
            let title = slack_escape(&clean(&out.title));
            let body = cap_bytes(&slack_escape(&clean(body)), SLACK_BODY_BYTES);
            let json = serde_json::json!({
                "text": format!("{title}\n{body}"),
                "blocks": [
                    {"type": "header", "text": {"type": "plain_text", "text": cap_chars(&clean(&out.title), SLACK_HEADER_CHARS)}},
                    {"type": "section", "text": {"type": "mrkdwn", "text": body}},
                ],
            });
            Request {
                headers: vec![("content-type", "application/json".to_owned())],
                body: serde_json::to_vec(&json).unwrap_or_default(),
            }
        }
        ChannelKind::Discord => {
            let body = out.markdown.as_deref().unwrap_or(&out.body);
            let content = cap_chars(
                &format!("**{}**\n{}", clean(&out.title), clean(body)),
                DISCORD_CHARS,
            );
            // No mention in a notification ever pings anyone.
            let json = serde_json::json!({
                "content": content,
                "allowed_mentions": {"parse": []},
            });
            Request {
                headers: vec![("content-type", "application/json".to_owned())],
                body: serde_json::to_vec(&json).unwrap_or_default(),
            }
        }
    }
}

/// The sample notification of the consent dialog: a 90% threshold of a
/// Claude account named "Work".
pub fn sample_source(now: DateTime<Utc>) -> Source {
    let account = Account {
        name: "Work".into(),
        provider: Provider::Claude,
        ..Account::default()
    };
    let event = AlertEvent {
        account_key: account.state_key(),
        label: "Session (5h)".into(),
        window: Some(MetricWindow::Window5h),
        kind: AlertKind::Threshold,
        threshold: 90,
        consumed: 0.9,
        resets_at: Some(now + chrono::TimeDelta::hours(2)),
        projection: None,
        overage: None,
        limit_change: None,
    };
    Source::Alerts {
        account,
        events: vec![event],
    }
}

/// RF4.1: the example payload of a draft channel, as it would be sent
/// (marked as a sample): the JSON body pretty-printed, or ntfy's headers
/// and text.
pub fn example(
    kind: ChannelKind,
    filters: &Filters,
    private: bool,
    locale: &str,
    now: DateTime<Utc>,
) -> String {
    let names = Filters {
        kinds: super::NoticeKind::ALL.into_iter().collect(),
        accounts: Default::default(),
        min_severity: Severity::Info,
        include_names: Some(filters.names(private)),
    };
    let Some(mut out) = prepare(
        &sample_source(now),
        &names,
        private,
        locale,
        now,
        &chrono::Local,
    ) else {
        return String::new();
    };
    out.sample = true;
    let request = render(kind, &out, None);
    match kind {
        ChannelKind::Ntfy => {
            let mut text = String::new();
            for (name, value) in &request.headers {
                if *name != "content-type" {
                    let mut chars = name.chars();
                    let head: String = chars
                        .next()
                        .map(|c| c.to_ascii_uppercase())
                        .into_iter()
                        .chain(chars)
                        .collect();
                    text.push_str(&format!("{head}: {value}\n"));
                }
            }
            text.push('\n');
            text.push_str(&String::from_utf8_lossy(&request.body));
            text
        }
        _ => serde_json::from_slice::<serde_json::Value>(&request.body)
            .and_then(|value| serde_json::to_string_pretty(&value))
            .unwrap_or_default(),
    }
}
