//! The refresh loop of a front end without a screen (`cuotai --serve`,
//! PRD 0012 RF4, and `cuotai --watch`, PRD 0013 RF2): one [`RefreshCycle`]
//! driven like the dashboard's, a first full sweep and then each account at
//! its own deadline with the cycle's backoff, plus the provider status read
//! every [`STATUS_TICK`] while status queries are on.
//!
//! [`Driver`] owns the queries in flight; its caller owns the clock and the
//! pace: it calls [`Driver::step`] every [`TICK`] with the current
//! [`Instant`] and, when a step reports that something finished, reads the
//! cycle ([`Driver::cycle`], [`Driver::export`]). Several consumers in one
//! process (the local server and the event stream) share one driver, so a
//! reading is queried once whoever uses it.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tokio::task::JoinHandle;

use crate::config_file::ConfigIdentity;
use crate::export::v1::{Export, Options};
use crate::refresh::{Generation, RefreshCycle};
use crate::{Config, Monitor, Report, Snapshot, Status};

/// How often a caller should [`Driver::step`].
pub const TICK: Duration = Duration::from_millis(250);

/// How often the due provider status sources are read while status queries
/// are on (each source is read at most once every 5 minutes).
pub const STATUS_TICK: Duration = Duration::from_secs(30);

/// The cycle of one configuration and the queries it started.
pub struct Driver {
    monitor: Arc<Monitor>,
    cycle: Arc<Mutex<RefreshCycle>>,
    full: Option<(JoinHandle<Snapshot>, Generation)>,
    accounts: Vec<Option<(JoinHandle<Report>, Generation)>>,
    status: Option<JoinHandle<()>>,
    status_due: Instant,
    ready_seen: bool,
}

impl Driver {
    /// A driver for `config` (the configuration `monitor` was built for)
    /// that has queried nothing yet; the first [`Self::step`] starts the
    /// full sweep. The last recorded readings (PRD 0010 RF2.3) are the
    /// cycle's preserved readings until each account answers.
    pub fn new(config: &Config, monitor: Arc<Monitor>, now: Instant) -> Self {
        let mut cycle = RefreshCycle::new(config, now);
        cycle.restore_last_good(monitor.restored_readings());
        Self {
            monitor,
            cycle: Arc::new(Mutex::new(cycle)),
            full: None,
            accounts: (0..config.accounts.len()).map(|_| None).collect(),
            status: None,
            status_due: now + STATUS_TICK,
            ready_seen: false,
        }
    }

    /// The shared cycle, for a consumer on another task (the local server).
    pub fn cycle(&self) -> &Arc<Mutex<RefreshCycle>> {
        &self.cycle
    }

    /// PRD 0031 RF12: switch to `new_config` (reread after `SIGHUP`) and
    /// `monitor`, built for it. Queries in flight are abandoned; the shared
    /// cycle (the same `Arc` the local server reads) is replaced in place by
    /// [`RefreshCycle::reload`], so unchanged accounts keep their readings
    /// and are not queried again, and new or changed ones are queried at
    /// the next [`Self::step`].
    pub fn reload(
        &mut self,
        prev_config: &Config,
        new_config: &Config,
        monitor: Arc<Monitor>,
        now: Instant,
    ) {
        self.abort_all();
        {
            let mut cycle = lock(&self.cycle);
            let mut next = cycle.reload(prev_config, new_config, now);
            next.restore_last_good(monitor.restored_readings());
            *cycle = next;
        }
        self.monitor = monitor;
        self.accounts = (0..new_config.accounts.len()).map(|_| None).collect();
    }

    fn abort_all(&mut self) {
        if let Some((job, _)) = self.full.take() {
            job.abort();
        }
        for (job, _) in self.accounts.drain(..).flatten() {
            job.abort();
        }
        if let Some(job) = self.status.take() {
            job.abort();
        }
    }

    /// Hand every finished query to the cycle (a failed task as `None`, so
    /// the cycle backs it off), then start what is due at `now`: the first
    /// full sweep, pending accounts whose deadline came, periodic ones and
    /// the status read. `true` when a query or status read finished, so
    /// the cycle (or the provider statuses) may have changed.
    pub async fn step(&mut self, now: Instant) -> bool {
        let finished = self.apply_finished(now).await;
        self.start_due(now);
        finished
    }

    /// No query or status read is in flight.
    pub fn is_idle(&self) -> bool {
        self.full.is_none() && self.status.is_none() && self.accounts.iter().all(Option::is_none)
    }

    /// Some query brought a `ready` report since the driver was built.
    pub fn ready_seen(&self) -> bool {
        self.ready_seen
    }

    /// Some account's latest reading failed: its failure lasted until now.
    pub fn failing(&self) -> bool {
        let cycle = lock(&self.cycle);
        (0..cycle.len()).any(|index| {
            cycle
                .account_report(index)
                .is_some_and(|report| report.status != Status::Ready)
        })
    }

    /// The `export::v1` of the cycle at `now`, with the provider statuses
    /// already read. Never queries anything.
    pub fn export(&self, config: &Config, identity: &ConfigIdentity, now: DateTime<Utc>) -> Export {
        let incidents = self.monitor.incident_statuses();
        let cycle = lock(&self.cycle);
        // PRD 0031 RF13: the same recent samples the app projects its pace from.
        Export::from_cycle_with_history(
            config,
            identity,
            &cycle,
            &incidents,
            now,
            Options::default(),
            |index, metrics, at| self.monitor.recent_points(index, metrics, at),
        )
    }

    async fn apply_finished(&mut self, now: Instant) -> bool {
        let mut finished = false;
        if self.full.as_ref().is_some_and(|(job, _)| job.is_finished()) {
            if let Some((job, token)) = self.full.take() {
                let snapshot = job.await.ok();
                self.ready_seen |= snapshot
                    .as_ref()
                    .is_some_and(|s| s.reports.iter().any(|r| r.status == Status::Ready));
                lock(&self.cycle).finish_full(snapshot, token, now);
                finished = true;
            }
        }
        for (index, slot) in self.accounts.iter_mut().enumerate() {
            if !slot.as_ref().is_some_and(|(job, _)| job.is_finished()) {
                continue;
            }
            if let Some((job, token)) = slot.take() {
                let report = job.await.ok();
                self.ready_seen |= report.as_ref().is_some_and(|r| r.status == Status::Ready);
                lock(&self.cycle).finish_account(index, report, token, now);
                finished = true;
            }
        }
        if self.status.as_ref().is_some_and(JoinHandle::is_finished) {
            self.status = None;
            self.status_due = now + STATUS_TICK;
            finished = true;
        }
        finished
    }

    fn start_due(&mut self, now: Instant) {
        if self.full.is_none() {
            let mut cycle = lock(&self.cycle);
            if !cycle.attempted_full() && self.accounts.iter().all(Option::is_none) {
                // `Monitor::refresh` also reads the provider statuses when on.
                if let Some(token) = cycle.begin_full() {
                    let monitor = Arc::clone(&self.monitor);
                    self.full = Some((tokio::spawn(async move { monitor.refresh().await }), token));
                }
            } else {
                let mut due = cycle.pending_now_indices(now);
                due.extend(cycle.due_indices(now));
                due.sort_unstable();
                due.dedup();
                for index in due {
                    if self.accounts[index].is_some() {
                        continue;
                    }
                    if let Some(token) = cycle.begin(index) {
                        let monitor = Arc::clone(&self.monitor);
                        let job = tokio::spawn(async move { monitor.refresh_account(index).await });
                        self.accounts[index] = Some((job, token));
                    }
                }
            }
        }
        if self.status.is_none() && self.monitor.incidents().is_enabled() && now >= self.status_due
        {
            let monitor = Arc::clone(&self.monitor);
            self.status = Some(tokio::spawn(async move {
                monitor.refresh_incidents().await;
            }));
        }
    }
}

impl Drop for Driver {
    /// Queries still in flight are abandoned.
    fn drop(&mut self) {
        self.abort_all();
    }
}

fn lock(cycle: &Mutex<RefreshCycle>) -> MutexGuard<'_, RefreshCycle> {
    cycle.lock().unwrap_or_else(PoisonError::into_inner)
}
