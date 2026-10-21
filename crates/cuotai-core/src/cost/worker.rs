//! RF2.2: the scan on a thread of its own, never on the refresh's path.
//!
//! A [`CostWorker`] owns one low-priority thread (see
//! `system::lower_thread_priority`) that scans every
//! [`Schedule::interval`], sooner ([`Schedule::catch_up`]) while a limited
//! scan left work behind, and at once when [`CostWorker::wake`] is called
//! after a preference change. While no source is on it reads only the
//! preferences; in energy-saving mode it pauses. Front ends read the last
//! [`CostStatus`]; the quota refresh never waits for it.

use super::system::{energy_saving, lower_thread_priority};
use super::{ScanReport, Scanner};
use crate::text::Localized;
use chrono::Utc;
use serde::Serialize;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How often the worker scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    /// Between two scans that read everything.
    pub interval: Duration,
    /// After a scan stopped by a limit.
    pub catch_up: Duration,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5 * 60),
            catch_up: Duration::from_secs(20),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    /// Not started yet.
    #[default]
    Starting,
    /// No source is on: no log is read.
    Off,
    /// Between scans.
    Waiting,
    Scanning,
    /// The computer is saving energy.
    Paused,
    /// The preferences or the index failed ([`CostStatus::error`]).
    Failed,
}

/// What front ends show about the local cost scan.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CostStatus {
    pub state: WorkerState,
    /// The last scan, while some source is on.
    pub last: Option<ScanReport>,
    pub error: Option<Localized>,
}

/// Scans from a worker thread. Clones share it; it ends once every clone is
/// dropped (after the scan in progress, bounded by the limits).
#[derive(Clone)]
pub struct CostWorker {
    wake: Sender<()>,
    status: Arc<Mutex<CostStatus>>,
}

impl CostWorker {
    /// The worker with the default schedule, pausing in energy-saving mode.
    pub fn spawn(scanner: Scanner) -> Self {
        Self::spawn_with(scanner, Schedule::default(), Box::new(energy_saving))
    }

    /// The worker with `schedule`, pausing while `saving` says so.
    pub fn spawn_with(
        scanner: Scanner,
        schedule: Schedule,
        saving: Box<dyn Fn() -> bool + Send>,
    ) -> Self {
        let (wake, woken) = mpsc::channel::<()>();
        let status = Arc::new(Mutex::new(CostStatus::default()));
        let shared = Arc::clone(&status);
        let started = std::thread::Builder::new()
            .name("cuotai-cost".into())
            .spawn(move || {
                lower_thread_priority();
                loop {
                    let pause = cycle(&scanner, &*saving, &shared, schedule);
                    match woken.recv_timeout(pause) {
                        Ok(()) => while woken.try_recv().is_ok() {},
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        if started.is_err() {
            set(&status, |s| {
                s.state = WorkerState::Failed;
                s.error = Some(super::CostError::Storage.l10n());
            });
        }
        Self { wake, status }
    }

    /// Scan now (after a source was turned on or off, or a preference
    /// changed).
    pub fn wake(&self) {
        let _ = self.wake.send(());
    }

    pub fn status(&self) -> CostStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn set(status: &Mutex<CostStatus>, change: impl FnOnce(&mut CostStatus)) {
    change(&mut status.lock().unwrap_or_else(PoisonError::into_inner));
}

/// One pass of the worker; returns how long to wait before the next.
fn cycle(
    scanner: &Scanner,
    saving: &dyn Fn() -> bool,
    status: &Mutex<CostStatus>,
    schedule: Schedule,
) -> Duration {
    let failed = |error: super::CostError| {
        set(status, |s| {
            s.state = WorkerState::Failed;
            s.error = Some(error.l10n());
        });
        schedule.interval
    };
    let settings = match scanner.settings().load() {
        Ok(settings) => settings,
        Err(error) => return failed(error),
    };
    if settings.enabled_sources(scanner.env()).is_empty() {
        set(status, |s| {
            *s = CostStatus {
                state: WorkerState::Off,
                ..CostStatus::default()
            }
        });
        return schedule.interval;
    }
    if saving() {
        set(status, |s| {
            s.state = WorkerState::Paused;
            s.error = None;
        });
        return schedule.interval;
    }
    set(status, |s| s.state = WorkerState::Scanning);
    match scanner.scan(Utc::now()) {
        Ok(report) => {
            let pause = if report.complete {
                schedule.interval
            } else {
                schedule.catch_up
            };
            set(status, |s| {
                *s = CostStatus {
                    state: WorkerState::Waiting,
                    last: Some(report),
                    error: None,
                }
            });
            pause
        }
        Err(error) => failed(error),
    }
}
