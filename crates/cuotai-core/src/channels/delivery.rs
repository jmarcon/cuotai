//! PRD 0044 RF3: delivery to the channels.
//!
//! A front end calls [`Dispatcher::dispatch`] with the notification it has
//! just shown, after PRD 0032's decision (preferences, quiet hours) and the
//! shared-ledger claim, so one crossing reaches the channels once across
//! processes. Each enabled channel renders it with its own filters; the
//! requests run in the background on the captured Tokio runtime, at most
//! [`MAX_IN_FLIGHT`] at a time, each with a 10-second timeout and 3
//! attempts (5 s and 30 s apart) on network errors and `5xx`. A `4xx` or a
//! redirect is not retried: the channel shows as failing with the status.
//! Nothing here ever blocks or delays the system notification or a
//! refresh.
//!
//! RF3.2: at most [`HOURLY_CAP`] deliveries per channel per hour; the rest
//! of the hour becomes one "N more notifications were not sent" message
//! when the hour ends ([`Dispatcher::tick`]). Within the first minute after
//! the dispatcher starts only critical notifications go out.

use super::log::{self, DeliveryLog, Entry, LogOutcome};
use super::payload::{self, Outgoing, Source};
use super::{Channel, Severity, ValidUrl, Vault};
use crate::text::Localized;
use crate::Provider;
use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Requests in flight at once, across every channel.
pub const MAX_IN_FLIGHT: usize = 2;
/// Deliveries per channel per hour.
pub const HOURLY_CAP: usize = 20;
/// Attempts per delivery.
pub const ATTEMPTS: u8 = 3;
/// RF3.4: a channel failing this long gets one system notification.
pub const FAILING_NOTICE_AFTER: TimeDelta = TimeDelta::hours(24);

/// The clock of the delivery policy (tests shorten it).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub timeout: Duration,
    /// The waits before the second and the third attempt.
    pub backoff: [Duration; 2],
    pub startup_grace: TimeDelta,
    pub window: TimeDelta,
    pub hourly_cap: usize,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            backoff: [Duration::from_secs(5), Duration::from_secs(30)],
            startup_grace: TimeDelta::minutes(1),
            window: TimeDelta::hours(1),
            hourly_cap: HOURLY_CAP,
        }
    }
}

/// What happened to one delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Delivered {
        status: u16,
    },
    Failed {
        status: Option<u16>,
        /// `network`, `timeout`, `vault` or `invalid_url`.
        error: Option<&'static str>,
        attempt: u8,
    },
}

/// RF6.2: a notification a channel accepted, for hooks
/// (`notification_sent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub channel: String,
    pub kind: &'static str,
    pub account_id: Option<String>,
    pub provider: Option<Provider>,
    pub at: DateTime<Utc>,
}

pub type OnSent = Arc<dyn Fn(Sent) + Send + Sync>;

/// RF3.4: a channel failing for 24 hours, for one system notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailingNotice {
    pub channel: String,
    pub label: String,
    pub status: Option<u16>,
}

impl FailingNotice {
    /// "Channel Phone is failing (403)".
    pub fn text(&self) -> Localized {
        let status = self
            .status
            .map_or_else(|| "network".to_owned(), |status| status.to_string());
        Localized::with_params(
            "channels.failing.notice",
            crate::text::params([
                ("label", self.label.as_str().into()),
                ("status", status.into()),
            ]),
            "Channel {label} is failing ({status})",
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct Window {
    start: DateTime<Utc>,
    sent: usize,
    suppressed: usize,
}

struct Shared {
    settings: PathBuf,
    log: Option<DeliveryLog>,
    vault: Vault,
    http: reqwest::Client,
    /// The same client without the environment's proxy, for the plain
    /// `http://` ntfy server on this computer or the private network
    /// ([`super::validate_url`] allows no other plain address): a proxy
    /// could not reach it, and would read every notification.
    direct: reqwest::Client,
    handle: tokio::runtime::Handle,
    permits: tokio::sync::Semaphore,
    windows: Mutex<BTreeMap<String, Window>>,
    started: DateTime<Utc>,
    timing: Timing,
    on_sent: Mutex<Option<OnSent>>,
    last_prune: Mutex<Option<DateTime<Utc>>>,
    /// Tests: accept a plain-HTTP loopback address for every kind (the
    /// mock servers), skipping the host allow-lists.
    #[cfg_attr(not(test), allow(dead_code))]
    loose: bool,
}

/// The channels of one profile, ready to deliver. Cheap to clone.
#[derive(Clone)]
pub struct Dispatcher(Arc<Shared>);

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dispatcher")
    }
}

impl Dispatcher {
    /// Channels defined in `settings` (`desktop.json`), URLs in `vault`,
    /// the log in the data directory; requests run on the current Tokio
    /// runtime. `None` outside a runtime or without a TLS backend.
    pub fn new(settings: PathBuf, vault: Vault, started: DateTime<Utc>) -> Option<Self> {
        Self::with(
            settings,
            vault,
            DeliveryLog::in_data_dir(),
            started,
            Timing::default(),
        )
    }

    /// [`Self::new`] with an explicit log and timing.
    pub fn with(
        settings: PathBuf,
        vault: Vault,
        log: Option<DeliveryLog>,
        started: DateTime<Utc>,
        timing: Timing,
    ) -> Option<Self> {
        let handle = tokio::runtime::Handle::try_current().ok()?;
        // No redirect is followed (PRD 0052 RF6.1, §6 Safety).
        let http = crate::network::client(crate::network::ClientPurpose::Channels, timing.timeout)?;
        let direct =
            crate::network::builder(crate::network::ClientPurpose::Channels, timing.timeout)
                .no_proxy()
                .build()
                .ok()?;
        Some(Self(Arc::new(Shared {
            settings,
            log,
            vault,
            http,
            direct,
            handle,
            permits: tokio::sync::Semaphore::new(MAX_IN_FLIGHT),
            windows: Mutex::new(BTreeMap::new()),
            started,
            timing,
            on_sent: Mutex::new(None),
            last_prune: Mutex::new(None),
            loose: false,
        })))
    }

    /// Tests: route the HTTPS client through `proxy`.
    #[cfg(test)]
    pub(crate) fn proxied(self, proxy: &str) -> Self {
        let shared = Arc::try_unwrap(self.0).unwrap_or_else(|_| panic!("unshared dispatcher"));
        let http = crate::network::builder(
            crate::network::ClientPurpose::Channels,
            shared.timing.timeout,
        )
        .proxy(reqwest::Proxy::all(proxy).unwrap())
        .build()
        .unwrap();
        Self(Arc::new(Shared { http, ..shared }))
    }

    #[cfg(test)]
    pub(crate) fn loose(self) -> Self {
        let shared = Arc::try_unwrap(self.0).unwrap_or_else(|_| panic!("unshared dispatcher"));
        Self(Arc::new(Shared {
            loose: true,
            ..shared
        }))
    }

    /// RF6.2: call `on_sent` for every accepted delivery.
    pub fn set_on_sent(&self, on_sent: OnSent) {
        *self
            .0
            .on_sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(on_sent);
    }

    /// The channels' definitions file.
    pub fn settings(&self) -> &std::path::Path {
        &self.0.settings
    }

    pub fn log(&self) -> Option<&DeliveryLog> {
        self.0.log.as_ref()
    }

    fn append(&self, entry: Entry) {
        if let Some(log) = &self.0.log {
            log.append(&entry);
        }
    }

    /// RF3.1: deliver the notification `source` (what the front end just
    /// showed) to every enabled channel whose filters keep it, rendered in
    /// `locale` with PRD 0032's `private` setting. Returns how many
    /// requests were started; they finish in the background.
    pub fn dispatch(
        &self,
        source: &Source,
        private: bool,
        locale: &str,
        now: DateTime<Utc>,
    ) -> usize {
        let mut started = 0;
        for channel in super::read(&self.0.settings) {
            if !channel.enabled {
                continue;
            }
            let Some(out) = payload::prepare(
                source,
                &channel.filters,
                private,
                locale,
                now,
                &chrono::Local,
            ) else {
                continue;
            };
            if self.admit(&channel, &out, now) {
                self.spawn(channel, out);
                started += 1;
            }
        }
        started
    }

    /// RF3.2: the startup grace and the hourly cap.
    fn admit(&self, channel: &Channel, out: &Outgoing, now: DateTime<Utc>) -> bool {
        let timing = self.0.timing;
        let entry = |outcome| Entry {
            at: now,
            channel: channel.id.clone(),
            kind: out.kind.to_owned(),
            outcome,
            status: None,
            attempt: 0,
            error: None,
        };
        if now < self.0.started + timing.startup_grace && out.severity != Severity::Critical {
            self.append(entry(LogOutcome::Held));
            return false;
        }
        let mut windows = self
            .0
            .windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let window = windows.entry(channel.id.clone()).or_insert(Window {
            start: now,
            sent: 0,
            suppressed: 0,
        });
        if now >= window.start + timing.window && window.suppressed == 0 {
            *window = Window {
                start: now,
                sent: 0,
                suppressed: 0,
            };
        }
        if window.sent >= timing.hourly_cap || window.suppressed > 0 {
            window.suppressed += 1;
            drop(windows);
            self.append(entry(LogOutcome::RateLimited));
            return false;
        }
        window.sent += 1;
        true
    }

    fn spawn(&self, channel: Channel, out: Outgoing) {
        let this = self.clone();
        self.0.handle.spawn(async move {
            let _ = this.send(&channel, &out).await;
        });
    }

    /// RF3.2/RF3.4 and the log's retention, every pass of the front end's
    /// timer: the summaries of the hours that ended with notifications
    /// held back, and the channels failing for 24 hours whose system
    /// notification is due now (shown once; the front end shows it).
    pub fn tick(&self, locale: &str, now: DateTime<Utc>) -> Vec<FailingNotice> {
        let channels = super::read(&self.0.settings);
        let due: Vec<(String, usize)> = {
            let mut windows = self
                .0
                .windows
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let window_len = self.0.timing.window;
            let due = windows
                .iter()
                .filter(|(_, w)| w.suppressed > 0 && now >= w.start + window_len)
                .map(|(id, w)| (id.clone(), w.suppressed))
                .collect::<Vec<_>>();
            for (id, _) in &due {
                windows.insert(
                    id.clone(),
                    Window {
                        start: now,
                        sent: 1,
                        suppressed: 0,
                    },
                );
            }
            due
        };
        for (id, suppressed) in due {
            let Some(channel) = channels.iter().find(|c| c.id == id && c.enabled) else {
                continue;
            };
            if let Some(out) = payload::prepare(
                &Source::Summary { suppressed },
                &channel.filters,
                true,
                locale,
                now,
                &chrono::Local,
            ) {
                self.spawn(channel.clone(), out);
            }
        }
        let Some(log) = &self.0.log else {
            return Vec::new();
        };
        {
            let mut last = self
                .0
                .last_prune
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if last.is_none_or(|at| now - at >= TimeDelta::hours(1)) {
                *last = Some(now);
                log.prune(now);
            }
        }
        let entries = log.read();
        let mut notices = Vec::new();
        for channel in &channels {
            let Some((since, status)) = log::failing_since(&entries, &channel.id) else {
                continue;
            };
            if now - since < FAILING_NOTICE_AFTER
                || log::failing_noticed(&entries, &channel.id, since)
            {
                continue;
            }
            log.append(&Entry {
                at: now,
                channel: channel.id.clone(),
                kind: "failing".to_owned(),
                outcome: LogOutcome::FailingNotice,
                status,
                attempt: 0,
                error: None,
            });
            notices.push(FailingNotice {
                channel: channel.id.clone(),
                label: channel.label.clone(),
                status,
            });
        }
        notices
    }

    /// RF4.2: **Send test** to the channel `id`, awaited: a sample marked
    /// as such, past the grace and the cap.
    pub async fn test(
        &self,
        id: &str,
        locale: &str,
        now: DateTime<Utc>,
    ) -> Result<Outcome, Localized> {
        let channel = super::read(&self.0.settings)
            .into_iter()
            .find(|channel| channel.id == id)
            .ok_or_else(|| {
                Localized::fixed("channels.error.unknown", "There is no such channel.")
            })?;
        let out = payload::prepare(
            &Source::Test,
            &channel.filters,
            true,
            locale,
            now,
            &chrono::Local,
        )
        .expect("the test always renders");
        Ok(self.send(&channel, &out).await)
    }

    fn url(&self, channel: &Channel) -> Result<ValidUrl, &'static str> {
        #[cfg(test)]
        if self.0.loose {
            return self.0.vault.loose_url(channel).ok_or("vault");
        }
        match self.0.vault.url(channel) {
            Ok(url) => Ok(url),
            Err(error) if error.id.starts_with("channels.error.vault") => Err("vault"),
            Err(_) => Err("invalid_url"),
        }
    }

    /// One delivery with its retries, logged once with its last attempt.
    async fn send(&self, channel: &Channel, out: &Outgoing) -> Outcome {
        let _permit = self.0.permits.acquire().await;
        let at =
            |outcome: LogOutcome, status: Option<u16>, attempt: u8, error: Option<&str>| Entry {
                at: Utc::now(),
                channel: channel.id.clone(),
                kind: out.kind.to_owned(),
                outcome,
                status,
                attempt,
                error: error.map(str::to_owned),
            };
        let this = self.clone();
        let owned = channel.clone();
        let target = tokio::task::spawn_blocking(move || {
            let url = this.url(&owned)?;
            let secret = this.0.vault.secret(&owned).map_err(|_| "vault")?;
            Ok::<_, &'static str>((url, secret))
        })
        .await
        .unwrap_or(Err("vault"));
        let (url, secret) = match target {
            Ok(target) => target,
            Err(error) => {
                self.append(at(LogOutcome::Failed, None, 0, Some(error)));
                return Outcome::Failed {
                    status: None,
                    error: Some(error),
                    attempt: 0,
                };
            }
        };
        let request = payload::render(channel.kind, out, secret.as_deref());
        let http = if url.expose().scheme() == "http" {
            &self.0.direct
        } else {
            &self.0.http
        };
        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut builder = http
                .post(url.expose().clone())
                .timeout(self.0.timing.timeout)
                .body(request.body.clone());
            for (name, value) in &request.headers {
                builder = builder.header(*name, value);
            }
            let (status, error) = match builder.send().await {
                Ok(response) => {
                    let status = response.status();
                    drop(response);
                    if status.is_success() {
                        self.append(at(
                            LogOutcome::Delivered,
                            Some(status.as_u16()),
                            attempt,
                            None,
                        ));
                        self.sent(channel, out);
                        return Outcome::Delivered {
                            status: status.as_u16(),
                        };
                    }
                    if !status.is_server_error() {
                        // A 4xx or a redirect: the address or the
                        // destination refuses; retrying cannot help.
                        self.append(at(LogOutcome::Failed, Some(status.as_u16()), attempt, None));
                        return Outcome::Failed {
                            status: Some(status.as_u16()),
                            error: None,
                            attempt,
                        };
                    }
                    (Some(status.as_u16()), None)
                }
                Err(error) => (
                    None,
                    Some(if error.is_timeout() {
                        "timeout"
                    } else {
                        "network"
                    }),
                ),
            };
            if attempt >= ATTEMPTS {
                self.append(at(LogOutcome::Failed, status, attempt, error));
                return Outcome::Failed {
                    status,
                    error,
                    attempt,
                };
            }
            tokio::time::sleep(self.0.timing.backoff[usize::from(attempt) - 1]).await;
        }
    }

    fn sent(&self, channel: &Channel, out: &Outgoing) {
        let on_sent = self
            .0
            .on_sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(on_sent) = on_sent {
            on_sent(Sent {
                channel: channel.id.clone(),
                kind: out.kind,
                account_id: out.account_id.clone(),
                provider: out.provider,
                at: out.occurred_at,
            });
        }
    }
}

/// One channel as Settings and `--channel-list` show it (RF5.1).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelView {
    #[serde(flatten)]
    pub channel: Channel,
    /// The filters in words, resolved by the front end.
    pub filters_text: super::FilterWords,
    pub last: Option<log::LastDelivery>,
    /// The failure that has lasted since this time, with its last status.
    pub failing_since: Option<DateTime<Utc>>,
    pub failing_status: Option<u16>,
    /// RF3.4: failing for 24 hours or more (the Settings banner).
    pub failing_long: bool,
}

/// RF5.1: every channel in `settings` with its last delivery from `log`.
pub fn views(
    settings: &std::path::Path,
    log: Option<&DeliveryLog>,
    private: bool,
    now: DateTime<Utc>,
) -> Vec<ChannelView> {
    let entries = log.map(DeliveryLog::read).unwrap_or_default();
    super::read(settings)
        .into_iter()
        .map(|channel| {
            let failing = log::failing_since(&entries, &channel.id);
            ChannelView {
                filters_text: super::filter_words(&channel.filters, private),
                last: log::last_delivery(&entries, &channel.id),
                failing_since: failing.map(|(since, _)| since),
                failing_status: failing.and_then(|(_, status)| status),
                failing_long: failing.is_some_and(|(since, _)| now - since >= FAILING_NOTICE_AFTER),
                channel,
            }
        })
        .collect()
}
