//! PRD 0006 RF2 — recording refresh results off the refresh's critical path.
//!
//! A [`HistoryRecorder`] owns one worker thread fed by a bounded queue: the
//! refresh only enqueues (never waits for the disk) and a full queue drops
//! the reading, counted in [`HistoryStatus::dropped`]. The worker also
//! prunes by [`Retention`] when it starts and then every 24 hours (RF4.1).
//! Every storage failure lands in [`HistoryStatus`], which front ends show.

use super::{CeilingPrune, HistoryError, HistoryStore, Retention};
use crate::text::{params, Localized};
use crate::{Account, Config, Metric, Report, Status};
use chrono::{DateTime, Utc};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Readings waiting for the disk before new ones are dropped.
const QUEUE_CAPACITY: usize = 64;
const PRUNE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

enum Job {
    Record {
        account: Box<Account>,
        taken_at: DateTime<Utc>,
        metrics: Vec<Metric>,
    },
    MarkRemoved(Box<Config>),
    Prune,
    Flush(mpsc::Sender<()>),
}

/// What front ends show about the history of this session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoryStatus {
    /// The last storage failure; the next successful write or prune clears it.
    pub error: Option<HistoryError>,
    /// Readings dropped because the queue was full (a slow disk).
    pub dropped: u64,
    /// The last time the size ceiling forced pruning (RF4.3).
    pub ceiling: Option<CeilingPrune>,
    /// When the last prune finished.
    pub pruned_at: Option<DateTime<Utc>>,
}

impl HistoryStatus {
    /// The warning to show, most serious first: a storage failure, the size
    /// ceiling pruning old samples, readings dropped by a slow disk.
    pub fn warning(&self) -> Option<Localized> {
        if let Some(error) = self.error {
            return Some(error.l10n());
        }
        if let Some(ceiling) = self.ceiling {
            let max = ceiling.max_bytes >> 20;
            return Some(Localized::with_params(
                "history.warning.ceiling",
                params([("max", max.into()), ("removed", ceiling.removed.into())]),
                format!(
                    "O histórico passou de {max} MiB: as {} medições mais antigas foram apagadas.",
                    ceiling.removed
                ),
            ));
        }
        (self.dropped > 0).then(|| {
            Localized::with_param(
                "history.warning.dropped",
                "count",
                self.dropped,
                format!(
                    "{} leituras não entraram no histórico: o disco demorou a responder.",
                    self.dropped
                ),
            )
        })
    }
}

struct Shared {
    store: Mutex<HistoryStore>,
    status: Mutex<HistoryStatus>,
}

impl Shared {
    fn store(&self) -> HistoryStore {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn update(&self, change: impl FnOnce(&mut HistoryStatus)) {
        change(&mut self.status.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn outcome<T>(&self, result: Result<T, HistoryError>) -> Option<T> {
        match result {
            Ok(value) => {
                self.update(|status| status.error = None);
                Some(value)
            }
            Err(error) => {
                self.update(|status| status.error = Some(error));
                None
            }
        }
    }

    fn prune(&self) {
        let now = Utc::now();
        if let Some(report) = self.outcome(self.store().prune(now)) {
            self.update(|status| {
                status.pruned_at = Some(now);
                status.ceiling = report.ceiling.or(status.ceiling);
            });
        }
    }

    fn work(&self, jobs: Receiver<Job>) {
        // Prune on open, then every 24 hours.
        let mut next_prune = Instant::now();
        loop {
            match jobs.recv_timeout(next_prune.saturating_duration_since(Instant::now())) {
                Ok(Job::Record {
                    account,
                    taken_at,
                    metrics,
                }) => {
                    self.outcome(self.store().record(&account, taken_at, &metrics));
                }
                Ok(Job::MarkRemoved(saved)) => {
                    self.outcome(self.store().mark_removed(&saved, Utc::now()));
                }
                Ok(Job::Prune) | Err(RecvTimeoutError::Timeout) => {
                    self.prune();
                    next_prune = Instant::now() + PRUNE_EVERY;
                }
                Ok(Job::Flush(done)) => {
                    let _ = done.send(());
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// Records `Ready` reports into a [`HistoryStore`] from a worker thread.
/// Clones share the worker, which ends once every clone is dropped and the
/// queue is drained.
#[derive(Clone)]
pub struct HistoryRecorder {
    shared: Arc<Shared>,
    jobs: SyncSender<Job>,
}

impl HistoryRecorder {
    /// Start recording into `store`, pruned by its [`Retention`] right away
    /// and then every 24 hours.
    pub fn spawn(store: HistoryStore) -> Self {
        Self::with_capacity(store, QUEUE_CAPACITY)
    }

    pub(crate) fn with_capacity(store: HistoryStore, capacity: usize) -> Self {
        let (jobs, queue) = mpsc::sync_channel(capacity);
        let shared = Arc::new(Shared {
            store: Mutex::new(store),
            status: Mutex::new(HistoryStatus::default()),
        });
        let worker = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("cuotai-history".into())
            .spawn(move || worker.work(queue));
        if spawned.is_err() {
            // Without a worker every job is refused and counted as dropped.
            shared.update(|status| status.error = Some(HistoryError::Storage));
        }
        Self { shared, jobs }
    }

    /// The store with the current retention, for queries.
    pub fn store(&self) -> HistoryStore {
        self.shared.store()
    }

    pub fn status(&self) -> HistoryStatus {
        self.shared
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A failure the caller met reading the store (shown like a write one).
    pub fn note_error(&self, error: HistoryError) {
        self.shared.update(|status| status.error = Some(error));
    }

    fn enqueue(&self, job: Job) {
        match self.jobs.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.shared.update(|status| status.dropped += 1);
            }
        }
    }

    /// RF2.2: queue `report` of `account` when it is `Ready`, with its
    /// metrics that carry a finite `used` or `remaining` (taken at its
    /// `checked_at`). Any other status, or no such metric, records nothing.
    /// Never waits: a full queue drops the reading (RF2.4).
    pub fn submit(&self, account: &Account, report: &Report) {
        if report.status != Status::Ready {
            return;
        }
        let measured = |value: Option<f64>| value.is_some_and(f64::is_finite);
        let metrics: Vec<Metric> = report
            .metrics
            .iter()
            .filter(|metric| measured(metric.used) || measured(metric.remaining))
            .cloned()
            .collect();
        if metrics.is_empty() {
            return;
        }
        self.enqueue(Job::Record {
            account: Box::new(account.clone()),
            taken_at: report.checked_at,
            metrics,
        });
    }

    /// Queue [`HistoryStore::mark_removed`] for `saved`, the configuration
    /// just loaded from or saved to the TOML.
    pub fn mark_removed(&self, saved: &Config) {
        self.enqueue(Job::MarkRemoved(Box::new(saved.clone())));
    }

    /// Use `retention` from now on and prune by it at once.
    pub fn set_retention(&self, retention: Retention) {
        {
            let mut store = self
                .shared
                .store
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *store = store.clone().with_retention(retention);
        }
        self.enqueue(Job::Prune);
    }

    /// Wait up to `timeout` for every job queued so far to finish (a
    /// process about to exit calls it). `false` when the time ran out.
    pub fn flush(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (done, finished) = mpsc::channel();
        let mut job = Job::Flush(done);
        loop {
            match self.jobs.try_send(job) {
                Ok(()) => break,
                Err(TrySendError::Full(back)) if Instant::now() < deadline => {
                    job = back;
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return false,
            }
        }
        finished
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{config, quota, store, t0, CONFIG};
    use super::*;
    use crate::{MetricKind, MetricWindow};

    fn report(status: Status, metrics: Vec<Metric>) -> Report {
        Report {
            account: "alpha".into(),
            provider: crate::Provider::Groq,
            checked_at: t0(),
            status,
            metrics,
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
        }
    }

    #[test]
    fn records_ready_reports_with_measured_metrics_only() {
        let dir = tempfile::tempdir().unwrap();
        let history = store(&dir.path().join("history.sqlite3"), "one");
        let alpha = config(CONFIG).accounts.remove(0);
        let recorder = HistoryRecorder::spawn(history.clone());
        let limit_only = Metric {
            window: Some(MetricWindow::Other),
            kind: MetricKind::Spend,
            ..quota("cap", None, Some(100.0))
        };
        let remaining_only = Metric {
            remaining: Some(3.0),
            ..quota("left", None, None)
        };
        let metrics = vec![
            quota("5h", Some(10.0), Some(100.0)),
            limit_only,
            remaining_only,
        ];
        for status in [
            Status::Error,
            Status::AuthenticationError,
            Status::RateLimited,
            Status::NeedsConfiguration,
            Status::Unsupported,
        ] {
            recorder.submit(&alpha, &report(status, metrics.clone()));
        }
        assert!(recorder.flush(Duration::from_secs(10)));
        assert!(
            history.latest(&alpha).unwrap().is_none(),
            "no failure recorded"
        );

        recorder.submit(&alpha, &report(Status::Ready, metrics));
        assert!(recorder.flush(Duration::from_secs(10)));
        let labels: Vec<String> = history
            .latest(&alpha)
            .unwrap()
            .unwrap()
            .metrics
            .into_iter()
            .map(|m| m.metric.label)
            .collect();
        assert_eq!(labels, ["5h", "left"]);
        assert_eq!(recorder.status().warning(), None);
    }

    #[test]
    fn storage_failures_and_drops_become_visible_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = config(CONFIG).accounts.remove(0);
        // The parent of the file is a file: every write fails.
        let blocked = dir.path().join("file");
        std::fs::write(&blocked, b"").unwrap();
        let recorder = HistoryRecorder::spawn(store(&blocked.join("history.sqlite3"), "one"));
        recorder.submit(
            &alpha,
            &report(Status::Ready, vec![quota("5h", Some(1.0), None)]),
        );
        assert!(recorder.flush(Duration::from_secs(10)));
        let status = recorder.status();
        assert_eq!(status.error, Some(HistoryError::Storage));
        assert_eq!(
            status.warning().map(|w| w.id),
            Some("history.error.storage".into())
        );

        let ceiling = HistoryStatus {
            ceiling: Some(CeilingPrune {
                removed: 1200,
                max_bytes: 64 << 20,
            }),
            dropped: 3,
            ..HistoryStatus::default()
        };
        assert_eq!(
            ceiling.warning().unwrap().resolve("en-US"),
            "The history went over 64 MiB: its 1200 oldest measurements were deleted."
        );
        let dropped = HistoryStatus {
            dropped: 3,
            ..HistoryStatus::default()
        };
        assert_eq!(
            dropped.warning().unwrap().resolve("pt-BR"),
            "3 leituras não entraram no histórico: o disco demorou a responder."
        );
        for warning in [ceiling.warning(), dropped.warning(), status.warning()] {
            let warning = warning.unwrap();
            for locale in ["en-US", "es"] {
                let catalog = crate::text::core_catalog(locale).unwrap();
                assert!(
                    catalog.contains_key(&*warning.id),
                    "{locale} {}",
                    warning.id
                );
            }
        }
    }
}
