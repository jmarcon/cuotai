//! Opt-in public status client, cache and failure correlation (PRD 0011
//! RF2/RF3).
//!
//! [`IncidentMonitor`] keeps the preference (off until a frontend turns it
//! on) and one cached reading per source URL. The [`crate::Monitor`] drives
//! it with its own HTTP client and query semaphore, so status requests share
//! the quota queries' connection pool and concurrency bound. Requests carry
//! no credentials, cookies or account data, and nothing about a status
//! failure (URL, body, error) reaches a report or a frontend.

use super::parse::{self, Baseline, Event, Reading};
use super::{Format, StatusSource};
use crate::text::Notes;
use crate::{Provider, Status};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// One reading per source is reused for this long (AWS publishes `ttl=5`).
pub const CACHE_TTL: Duration = Duration::from_secs(300);
/// Deadline for one status request, independent of the quota timeout.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Larger bodies are refused unread (Cloudflare's summary is ~240 KiB).
pub const MAX_BODY_BYTES: usize = 1 << 20;
/// Identified client, as Cloudflare's API policy asks.
const USER_AGENT: &str = concat!(
    "cuotai/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/jmarcon/cuotai)"
);

/// Normalized state of a provider's scoped components (RF2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentState {
    Operational,
    Degraded,
    PartialOutage,
    MajorOutage,
    Maintenance,
    /// No valid, unambiguous reading. Never shown as healthy.
    Unknown,
}

impl IncidentState {
    /// `Degraded` or worse: an ongoing incident (RF3.1).
    pub fn is_incident(self) -> bool {
        matches!(
            self,
            Self::Degraded | Self::PartialOutage | Self::MajorOutage
        )
    }

    /// `PartialOutage` or worse.
    pub fn is_outage(self) -> bool {
        matches!(self, Self::PartialOutage | Self::MajorOutage)
    }

    fn rank(self) -> u8 {
        match self {
            Self::Operational => 0,
            Self::Maintenance => 1,
            Self::Unknown => 2,
            Self::Degraded => 3,
            Self::PartialOutage => 4,
            Self::MajorOutage => 5,
        }
    }

    fn worst(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// The most severe active event in the provider's scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incident {
    /// Provider title without markup, at most 120 characters.
    pub title: String,
    pub started_at: Option<DateTime<Utc>>,
    /// Public `https` page on the status host (the incident page when the
    /// format links one, otherwise the status page itself).
    pub url: String,
}

/// What a frontend shows next to a provider's accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider: Provider,
    pub state: IncidentState,
    pub incident: Option<Incident>,
    /// Public status page of the source.
    pub page_url: String,
    /// When the reading shown was obtained; `None` before the first valid one.
    pub read_at: Option<DateTime<Utc>>,
    /// The latest query failed: the state is the last known reading (or
    /// `Unknown` without one).
    pub stale: bool,
}

/// Opt-in status preference and per-source cache. Cloning shares both, so
/// a monitor rebuilt for a new configuration keeps them.
#[derive(Clone, Default)]
pub struct IncidentMonitor {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    enabled: AtomicBool,
    sources: Mutex<HashMap<&'static str, Arc<SourceCache>>>,
    #[cfg(test)]
    knobs: Mutex<TestKnobs>,
}

#[cfg(test)]
#[derive(Default, Clone)]
pub(crate) struct TestKnobs {
    /// Replaces `https://` with `{base}/` so sources reach a local server.
    pub base: Option<String>,
    pub ttl: Option<Duration>,
    pub timeout: Option<Duration>,
}

#[derive(Default)]
struct SourceCache {
    /// Held across a query: concurrent callers wait and reuse its result.
    gate: tokio::sync::Mutex<()>,
    known: Mutex<Known>,
}

#[derive(Default, Clone)]
struct Known {
    reading: Option<Arc<Reading>>,
    read_at: Option<DateTime<Utc>>,
    attempted: Option<Instant>,
    failed: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl IncidentMonitor {
    /// Off, with an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// RF2.4: nothing is queried while off; turning it on queries at the
    /// next refresh.
    pub fn set_enabled(&self, on: bool) {
        self.shared.enabled.store(on, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn set_test_knobs(&self, knobs: TestKnobs) {
        *lock(&self.shared.knobs) = knobs;
    }

    fn ttl(&self) -> Duration {
        #[cfg(test)]
        if let Some(ttl) = lock(&self.shared.knobs).ttl {
            return ttl;
        }
        CACHE_TTL
    }

    fn timeout(&self) -> Duration {
        #[cfg(test)]
        if let Some(timeout) = lock(&self.shared.knobs).timeout {
            return timeout;
        }
        REQUEST_TIMEOUT
    }

    fn endpoint(&self, url: &'static str) -> std::borrow::Cow<'static, str> {
        #[cfg(test)]
        if let Some(base) = &lock(&self.shared.knobs).base {
            return format!("{base}/{}", url.trim_start_matches("https://")).into();
        }
        url.into()
    }

    fn cache(&self, url: &'static str) -> Arc<SourceCache> {
        Arc::clone(lock(&self.shared.sources).entry(url).or_default())
    }

    /// RF2.2: query `source` unless its last attempt is younger than the
    /// TTL; callers racing on one URL share a single request.
    pub(crate) async fn ensure(
        &self,
        source: StatusSource,
        http: &reqwest::Client,
        slots: &Semaphore,
    ) {
        let cache = self.cache(source.url);
        let _gate = cache.gate.lock().await;
        let due = lock(&cache.known)
            .attempted
            .is_none_or(|at| at.elapsed() >= self.ttl());
        if !due {
            return;
        }
        let result = fetch(
            http,
            slots,
            &self.endpoint(source.url),
            source.format,
            host(source.url),
            self.timeout(),
        )
        .await;
        let mut known = lock(&cache.known);
        known.attempted = Some(Instant::now());
        match result {
            Ok(reading) => {
                known.reading = Some(Arc::new(reading));
                known.read_at = Some(Utc::now());
                known.failed = false;
            }
            // RF2.2: keep the last known reading, marked stale.
            Err(parse::Invalid) => known.failed = true,
        }
    }

    /// The cached status of `provider` (no network).
    pub(crate) fn status(&self, provider: Provider, source: StatusSource) -> ProviderStatus {
        let known = lock(&self.cache(source.url).known).clone();
        let (state, incident) = known
            .reading
            .as_deref()
            .map_or((IncidentState::Unknown, None), |reading| {
                evaluate(reading, source)
            });
        ProviderStatus {
            provider,
            state,
            incident,
            page_url: page_url(source),
            read_at: known.read_at,
            stale: known.failed,
        }
    }
}

async fn fetch(
    http: &reqwest::Client,
    slots: &Semaphore,
    url: &str,
    format: Format,
    host: &str,
    timeout: Duration,
) -> Result<Reading, parse::Invalid> {
    let _permit = slots
        .acquire()
        .await
        .expect("monitor query semaphore stays open");
    let accept = match format {
        Format::AtomRss(_) => "application/rss+xml, application/xml;q=0.9",
        Format::StatuspageV2 | Format::JsonCustom(_) => "application/json",
    };
    let request = async {
        let mut response = http
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, accept)
            .timeout(timeout)
            .send()
            .await
            .map_err(|_| parse::Invalid)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_BODY_BYTES as u64)
        {
            return Err(parse::Invalid);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| parse::Invalid)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(parse::Invalid);
            }
            body.extend_from_slice(&chunk);
        }
        parse::parse(format, &body, host)
    };
    tokio::time::timeout(timeout, request)
        .await
        .unwrap_or(Err(parse::Invalid))
}

/// Host of a registry URL (all are `https://host/path` constants).
fn host(url: &'static str) -> &'static str {
    let rest = url.strip_prefix("https://").unwrap_or(url);
    rest.split('/').next().unwrap_or(rest)
}

fn page_url(source: StatusSource) -> String {
    format!("https://{}/", host(source.url))
}

/// Keep a provider link only when it is a plain `https` URL on the source
/// host; query and fragment are dropped.
fn public_link(candidate: &str, source: StatusSource) -> Option<String> {
    let mut url = url::Url::parse(candidate).ok()?;
    let valid = url.scheme() == "https"
        && url.host_str() == Some(host(source.url))
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    if !valid {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url.into())
}

/// The state an event has inside `source`'s scope, or `None` outside it.
fn scoped_state(event: &Event, source: StatusSource) -> Option<IncidentState> {
    let mut touched = false;
    let mut all_reported = true;
    let mut worst: Option<IncidentState> = None;
    for (id, state) in &event.components {
        if !source.includes_component(id) {
            continue;
        }
        touched = true;
        match state {
            Some(IncidentState::Operational) => {}
            Some(state) => worst = Some(worst.map_or(*state, |w| w.worst(*state))),
            None => all_reported = false,
        }
    }
    if !touched {
        return None;
    }
    Some(match worst {
        Some(state) => state,
        // The event lists the scoped components as operational: its impact
        // may lie elsewhere on the page, so this is a doubt, not health.
        None if all_reported => IncidentState::Unknown,
        None => event.impact,
    })
}

/// Normalize one cached reading for `source`'s scope (RF2.3). Only scoped
/// components and events count; page-wide rollups never do.
pub(crate) fn evaluate(
    reading: &Reading,
    source: StatusSource,
) -> (IncidentState, Option<Incident>) {
    let mut state = match reading.baseline {
        Baseline::Operational => IncidentState::Operational,
        Baseline::Unknown => IncidentState::Unknown,
        Baseline::Components => reading
            .components
            .iter()
            .filter(|(id, _)| source.includes_component(id))
            .map(|(_, state)| *state)
            .reduce(IncidentState::worst)
            .unwrap_or(IncidentState::Unknown),
    };
    let mut active: Option<(&Event, IncidentState)> = None;
    for event in &reading.events {
        let Some(event_state) = scoped_state(event, source) else {
            continue;
        };
        state = state.worst(event_state);
        let replace = active.is_none_or(|(current, current_state)| {
            (event_state.rank(), event.started_at) > (current_state.rank(), current.started_at)
        });
        if replace {
            active = Some((event, event_state));
        }
    }
    let incident = active.map(|(event, _)| Incident {
        title: event.title.clone(),
        started_at: event.started_at,
        url: event
            .link
            .as_deref()
            .and_then(|link| public_link(link, source))
            .unwrap_or_else(|| page_url(source)),
    });
    (state, incident)
}

/// RF3: notes for a failed query while the provider reports an incident.
/// They never change the `Status` nor name the incident as the cause; an
/// authentication failure also suggests waiting, without blocking login.
pub(crate) fn failure_notes(status: Status, state: IncidentState) -> Notes {
    let mut notes = Notes::new();
    let failed = matches!(
        status,
        Status::Error | Status::RateLimited | Status::AuthenticationError
    );
    if !failed || !state.is_incident() {
        return notes;
    }
    notes.push(
        "note.incident.ongoing",
        "O provedor informa um incidente em andamento; a falha pode não ser da sua conta.",
    );
    if status == Status::AuthenticationError {
        notes.push(
            "note.incident.auth_wait",
            "Enquanto o incidente durar, considere aguardar antes de autenticar de novo; reautenticar continua disponível.",
        );
    }
    notes
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
