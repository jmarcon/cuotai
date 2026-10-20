//! Normalization of each verified wire format (PRD 0011 RF2.3).
//!
//! A parser turns one response body into a [`Reading`]: the component states
//! and active events it publishes, with every provider-supplied text already
//! sanitized. Scope filtering happens later, per provider, because providers
//! sharing a URL share one cached reading. Anything malformed is an error,
//! never an "operational" reading.

use super::{FeedFormat, Format, IncidentState, JsonFormat};
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Longest provider title, in characters, that reaches a frontend.
pub(crate) const TITLE_MAX_CHARS: usize = 120;

/// What a scope without matching components or events means for a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Baseline {
    /// The page lists every component's state; a missing scoped component is
    /// an unknown reading.
    Components,
    /// The response reflects the whole current page: no scoped event means
    /// the scope is operational.
    Operational,
    /// Only events are published (feeds whose history cannot prove health).
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reading {
    pub baseline: Baseline,
    /// `(component id, state)`; only formats with [`Baseline::Components`].
    pub components: Vec<(String, IncidentState)>,
    /// Active (unresolved) incidents and in-progress maintenance.
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Event {
    /// Affected component selectors with their state inside the event when
    /// the format reports one.
    pub components: Vec<(String, Option<IncidentState>)>,
    /// The event's own impact, used when no affected scoped component
    /// reports a non-operational state.
    pub impact: IncidentState,
    /// Sanitized title.
    pub title: String,
    pub started_at: Option<DateTime<Utc>>,
    /// Unvalidated candidate link; checked against the source host later.
    pub link: Option<String>,
}

/// The format could not be read; the reason is deliberately not kept so no
/// response content can travel further.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Invalid;

/// Parse `body` as `format`. `host` is the source host, used to build the
/// public incident links the format implies.
pub(crate) fn parse(format: Format, body: &[u8], host: &str) -> Result<Reading, Invalid> {
    match format {
        Format::StatuspageV2 => statuspage(body, host),
        Format::JsonCustom(JsonFormat::IncidentIoWidget) => widget(body),
        Format::JsonCustom(JsonFormat::GoogleCloud) => google(body, host),
        Format::AtomRss(feed) => {
            let text = std::str::from_utf8(body).map_err(|_| Invalid)?;
            let items = rss_items(text).ok_or(Invalid)?;
            Ok(match feed {
                FeedFormat::AwsHealth => aws(items),
                FeedFormat::Xai => xai(items),
            })
        }
    }
}

fn time(raw: Option<&str>) -> Option<DateTime<Utc>> {
    raw.and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|time| time.with_timezone(&Utc))
}

/// Statuspage/incident.io component state vocabulary.
fn component_state(raw: &str) -> IncidentState {
    match raw {
        "operational" => IncidentState::Operational,
        "degraded_performance" => IncidentState::Degraded,
        "partial_outage" => IncidentState::PartialOutage,
        "major_outage" | "full_outage" => IncidentState::MajorOutage,
        "under_maintenance" => IncidentState::Maintenance,
        _ => IncidentState::Unknown,
    }
}

/// Path segment safe to place in a link (provider IDs are alphanumeric).
fn safe_segment(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct StateComponent {
    id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    current_status: Option<String>,
}

impl StateComponent {
    fn into_event_component(self) -> (String, Option<IncidentState>) {
        let state = self
            .status
            .or(self.current_status)
            .map(|raw| component_state(&raw));
        (self.id, state)
    }
}

#[derive(Deserialize)]
struct SpSummary {
    components: Vec<StateComponent>,
    incidents: Vec<SpEvent>,
    #[serde(default)]
    scheduled_maintenances: Vec<SpEvent>,
}

#[derive(Deserialize)]
struct SpEvent {
    id: String,
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    impact: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    scheduled_for: Option<String>,
    #[serde(default)]
    components: Vec<StateComponent>,
}

fn statuspage(body: &[u8], host: &str) -> Result<Reading, Invalid> {
    let summary: SpSummary = serde_json::from_slice(body).map_err(|_| Invalid)?;
    let link = |id: &str| safe_segment(id).then(|| format!("https://{host}/incidents/{id}"));
    let mut events = Vec::new();
    for incident in summary.incidents {
        if matches!(incident.status.as_str(), "resolved" | "postmortem") {
            continue;
        }
        let impact = match incident.impact.as_deref() {
            Some("minor") => IncidentState::Degraded,
            Some("major") => IncidentState::PartialOutage,
            Some("critical") => IncidentState::MajorOutage,
            Some("maintenance") => IncidentState::Maintenance,
            _ => IncidentState::Unknown,
        };
        events.push(Event {
            link: link(&incident.id),
            title: plain_text(&incident.name, TITLE_MAX_CHARS),
            started_at: time(incident.started_at.as_deref())
                .or_else(|| time(incident.created_at.as_deref())),
            components: incident
                .components
                .into_iter()
                .map(StateComponent::into_event_component)
                .collect(),
            impact,
        });
    }
    for maintenance in summary.scheduled_maintenances {
        if !matches!(maintenance.status.as_str(), "in_progress" | "verifying") {
            continue;
        }
        events.push(Event {
            link: link(&maintenance.id),
            title: plain_text(&maintenance.name, TITLE_MAX_CHARS),
            started_at: time(maintenance.started_at.as_deref())
                .or_else(|| time(maintenance.scheduled_for.as_deref())),
            components: maintenance
                .components
                .into_iter()
                .map(|component| (component.id, Some(IncidentState::Maintenance)))
                .collect(),
            impact: IncidentState::Maintenance,
        });
    }
    Ok(Reading {
        baseline: Baseline::Components,
        components: summary
            .components
            .into_iter()
            .map(|component| {
                let state = component
                    .status
                    .as_deref()
                    .map_or(IncidentState::Unknown, component_state);
                (component.id, state)
            })
            .collect(),
        events,
    })
}

#[derive(Deserialize)]
struct Widget {
    ongoing_incidents: Vec<WidgetEvent>,
    in_progress_maintenances: Vec<WidgetEvent>,
}

#[derive(Deserialize)]
struct WidgetEvent {
    name: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    current_worst_impact: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    starts_at: Option<String>,
    #[serde(default)]
    affected_components: Vec<StateComponent>,
}

fn widget(body: &[u8]) -> Result<Reading, Invalid> {
    let widget: Widget = serde_json::from_slice(body).map_err(|_| Invalid)?;
    let incidents = widget.ongoing_incidents.into_iter().map(|incident| Event {
        impact: incident
            .current_worst_impact
            .as_deref()
            .map_or(IncidentState::Unknown, component_state),
        title: plain_text(&incident.name, TITLE_MAX_CHARS),
        started_at: time(incident.started_at.as_deref()),
        link: incident.url,
        components: incident
            .affected_components
            .into_iter()
            .map(StateComponent::into_event_component)
            .collect(),
    });
    let maintenances = widget
        .in_progress_maintenances
        .into_iter()
        .map(|maintenance| Event {
            impact: IncidentState::Maintenance,
            title: plain_text(&maintenance.name, TITLE_MAX_CHARS),
            started_at: time(maintenance.starts_at.as_deref())
                .or_else(|| time(maintenance.started_at.as_deref())),
            link: maintenance.url,
            components: maintenance
                .affected_components
                .into_iter()
                .map(|component| (component.id, Some(IncidentState::Maintenance)))
                .collect(),
        });
    Ok(Reading {
        baseline: Baseline::Operational,
        components: Vec::new(),
        events: incidents.chain(maintenances).collect(),
    })
}

#[derive(Deserialize)]
struct GoogleIncident {
    #[serde(default)]
    begin: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    external_desc: String,
    #[serde(default)]
    status_impact: Option<String>,
    #[serde(default)]
    affected_products: Vec<Id>,
    #[serde(default)]
    uri: Option<String>,
}

fn google(body: &[u8], host: &str) -> Result<Reading, Invalid> {
    let incidents: Vec<GoogleIncident> = serde_json::from_slice(body).map_err(|_| Invalid)?;
    let events = incidents
        .into_iter()
        .filter(|incident| incident.end.is_none())
        .map(|incident| Event {
            impact: match incident.status_impact.as_deref() {
                Some("SERVICE_INFORMATION") => IncidentState::Degraded,
                Some("SERVICE_DISRUPTION") => IncidentState::PartialOutage,
                Some("SERVICE_OUTAGE") => IncidentState::MajorOutage,
                _ => IncidentState::Unknown,
            },
            title: plain_text(&incident.external_desc, TITLE_MAX_CHARS),
            started_at: time(incident.begin.as_deref()),
            link: incident
                .uri
                .as_deref()
                .and_then(|uri| uri.strip_prefix("incidents/"))
                .filter(|id| safe_segment(id))
                .map(|id| format!("https://{host}/incidents/{id}")),
            components: incident
                .affected_products
                .into_iter()
                .map(|product| (product.id, None))
                .collect(),
        })
        .collect();
    Ok(Reading {
        baseline: Baseline::Operational,
        components: Vec::new(),
        events,
    })
}

/// One RSS 2.0 `<item>`, with CDATA unwrapped and entities decoded.
#[derive(Debug, Default)]
struct Item {
    title: String,
    link: Option<String>,
    guid: String,
    published: Option<DateTime<Utc>>,
    categories: Vec<String>,
}

/// Minimal RSS 2.0 reader for the two verified feeds: `None` unless the body
/// is an `<rss>` document with a `<channel>`.
fn rss_items(text: &str) -> Option<Vec<Item>> {
    let channel = element(text, "rss").and_then(|rss| element(rss, "channel"))?;
    let mut items = Vec::new();
    let mut rest = channel;
    while let Some((item, after)) = next_element(rest, "item") {
        items.push(Item {
            title: element(item, "title").map(xml_text).unwrap_or_default(),
            link: element(item, "link").map(xml_text),
            guid: element(item, "guid").map(xml_text).unwrap_or_default(),
            published: element(item, "pubDate")
                .and_then(|raw| DateTime::parse_from_rfc2822(xml_text(raw).trim()).ok())
                .map(|time| time.with_timezone(&Utc)),
            categories: elements(item, "category")
                .map(|raw| xml_text(raw).trim().to_ascii_lowercase())
                .collect(),
        });
        rest = after;
    }
    Some(items)
}

/// Content of the first `<name …>…</name>` in `text`, and the text after it.
fn next_element<'a>(text: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut from = 0;
    loop {
        let start = from + text[from..].find(&open)?;
        let after_name = start + open.len();
        // `<item>` must not match `<items>`: the name ends at `>` or a space.
        match text[after_name..].chars().next() {
            Some('>') | Some(' ') | Some('\t') | Some('\n') | Some('\r') => {}
            Some('/') => {
                // Self-closing element: empty content.
                let end = after_name + text[after_name..].find('>')? + 1;
                return Some(("", &text[end..]));
            }
            _ => {
                from = after_name;
                continue;
            }
        }
        let body_start = after_name + text[after_name..].find('>')? + 1;
        let body_end = body_start + text[body_start..].find(&close)?;
        return Some((&text[body_start..body_end], &text[body_end + close.len()..]));
    }
}

fn element<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    next_element(text, name).map(|(content, _)| content)
}

fn elements<'a>(mut text: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    std::iter::from_fn(move || {
        let (content, rest) = next_element(text, name)?;
        text = rest;
        Some(content)
    })
}

/// Unwrap CDATA sections and decode entities; markup inside stays for
/// [`plain_text`] to remove.
fn xml_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<![CDATA[") {
        out.push_str(&decode_entities(&rest[..start]));
        let data = &rest[start + "<![CDATA[".len()..];
        match data.find("]]>") {
            Some(end) => {
                out.push_str(&data[..end]);
                rest = &data[end + "]]>".len()..];
            }
            None => {
                out.push_str(data);
                rest = "";
            }
        }
    }
    out.push_str(&decode_entities(rest));
    out
}

/// AWS Health RSS: the GUID fragment is `<service>-<region>_<timestamp>`;
/// every update is its own item, newest first. The newest item of each
/// service/region event decides whether it is still active.
fn aws(items: Vec<Item>) -> Reading {
    struct Group {
        key: String,
        service: String,
        latest: Item,
        first_seen: Option<DateTime<Utc>>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for item in items {
        let Some(key) = item
            .guid
            .rsplit_once('#')
            .map(|(_, fragment)| fragment)
            .map(|fragment| fragment.rsplit_once('_').map_or(fragment, |(key, _)| key))
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
        else {
            continue;
        };
        match groups.iter_mut().find(|group| group.key == key) {
            Some(group) => {
                if item.published.is_some()
                    && (group.first_seen.is_none() || item.published < group.first_seen)
                {
                    group.first_seen = item.published;
                }
                if item.published > group.latest.published {
                    group.latest = item;
                }
            }
            None => groups.push(Group {
                service: key.split('-').next().unwrap_or_default().to_owned(),
                key,
                first_seen: item.published,
                latest: item,
            }),
        }
    }
    let events = groups
        .into_iter()
        .filter_map(|group| {
            let title = group.latest.title.trim();
            let (kind, text) = title.split_once(':').unwrap_or((title, ""));
            if kind.trim() == "Service is operating normally" || title.contains("[RESOLVED]") {
                return None;
            }
            let impact = match kind.trim() {
                "Informational message" | "Performance issues" => IncidentState::Degraded,
                "Service degradation" => IncidentState::PartialOutage,
                "Service disruption" => IncidentState::MajorOutage,
                _ => IncidentState::Unknown,
            };
            let text = if text.trim().is_empty() { title } else { text };
            Some(Event {
                components: vec![(group.service, None)],
                impact,
                title: plain_text(text, TITLE_MAX_CHARS),
                started_at: group.first_seen,
                link: group.latest.link,
            })
        })
        .collect();
    Reading {
        baseline: Baseline::Unknown,
        components: Vec::new(),
        events,
    }
}

/// xAI RSS: `[component] title`; categories carry severity and resolution.
fn xai(items: Vec<Item>) -> Reading {
    let events = items
        .into_iter()
        .filter(|item| {
            !item
                .categories
                .iter()
                .any(|category| matches!(category.as_str(), "resolved" | "completed"))
        })
        .filter_map(|item| {
            let (component, title) = item.title.trim().strip_prefix('[')?.split_once(']')?;
            let impact = item
                .categories
                .iter()
                .find_map(|category| match category.as_str() {
                    "outage" | "major_outage" => Some(IncidentState::MajorOutage),
                    "disruption" | "partial_outage" => Some(IncidentState::PartialOutage),
                    "degraded" | "degradation" | "degraded_performance" => {
                        Some(IncidentState::Degraded)
                    }
                    "maintenance" => Some(IncidentState::Maintenance),
                    _ => None,
                })
                .unwrap_or(IncidentState::Unknown);
            Some(Event {
                components: vec![(component.to_owned(), None)],
                impact,
                title: plain_text(title, TITLE_MAX_CHARS),
                started_at: item.published,
                link: item.link,
            })
        })
        .collect();
    Reading {
        baseline: Baseline::Unknown,
        components: Vec::new(),
        events,
    }
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let decoded = tail.find(';').filter(|&end| end <= 10).and_then(|end| {
            let name = &tail[1..end];
            let ch = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                    .and_then(char::from_u32),
            };
            ch.map(|ch| (ch, end + 1))
        });
        match decoded {
            Some((ch, len)) => {
                out.push(ch);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Provider free text made safe for any frontend (RF2.3): entities decoded,
/// HTML tags and Markdown emphasis/links removed, control characters and
/// runs of whitespace collapsed, then truncated to `max` characters with an
/// ellipsis.
pub(crate) fn plain_text(raw: &str, max: usize) -> String {
    let decoded = decode_entities(raw);
    let mut text = String::with_capacity(decoded.len());
    let mut chars = decoded.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            // A tag starts with a letter, `/`, `!` or `?`; "a < b" stays.
            '<' if chars.peek().is_some_and(|next| {
                next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?')
            }) =>
            {
                for inner in chars.by_ref() {
                    if inner == '>' {
                        break;
                    }
                }
                text.push(' ');
            }
            '*' | '`' => {}
            // `[label](target)` keeps only the label.
            ']' if chars.peek() == Some(&'(') => {
                for inner in chars.by_ref() {
                    if inner == ')' {
                        break;
                    }
                }
            }
            '[' => {}
            ch if ch.is_control() || ch.is_whitespace() => text.push(' '),
            ch => text.push(ch),
        }
    }
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.chars().count() > max {
        let mut truncated: String = out.chars().take(max.saturating_sub(1)).collect();
        truncated.truncate(truncated.trim_end().len());
        truncated.push('…');
        return truncated;
    }
    out
}
