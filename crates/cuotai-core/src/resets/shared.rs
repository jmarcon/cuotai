//! The ledger as one process shares it with the others on the same profile
//! (PRD 0032 RF4): `notified.json` plus the knowledge of when this process
//! last read it. Reading is cheap (a modification-time check re-reads the
//! file when the desktop or the TUI wrote it); every change goes through
//! the advisory lock of [`Ledger::claim`], so what one process records is
//! never overwritten by another.

use super::{Ledger, ResetEvent};
use crate::alerts::{AlertEvent, AlertPrefs};
use crate::model::Report;
use chrono::{DateTime, Utc};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A [`Ledger`] backed by `notified.json`, or by memory alone (tests, no
/// data directory).
#[derive(Debug, Default)]
pub struct SharedLedger {
    ledger: Ledger,
    path: Option<PathBuf>,
    stamp: Option<SystemTime>,
}

impl Deref for SharedLedger {
    type Target = Ledger;

    fn deref(&self) -> &Ledger {
        &self.ledger
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

impl SharedLedger {
    /// An in-memory ledger: nothing touches the disk.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The ledger stored at `path` (`None`: in memory).
    pub fn open(path: Option<PathBuf>) -> Self {
        let ledger = path.as_deref().map(Ledger::load).unwrap_or_default();
        let stamp = path.as_deref().and_then(modified);
        Self {
            ledger,
            path,
            stamp,
        }
    }

    /// The ledger at the default `<OS data dir>/cuotai/notified.json`.
    pub fn open_default() -> Self {
        Self::open(Ledger::default_path())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn remember_stamp(&mut self) {
        self.stamp = self.path.as_deref().and_then(modified);
    }

    /// Take the version on disk when another process wrote it since this
    /// one last looked.
    pub fn sync(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let stamp = modified(&path);
        if stamp != self.stamp {
            self.ledger.reload(&path);
            self.stamp = stamp;
        }
    }

    /// Apply `change` (which says whether it changed anything). With a
    /// path it runs on the copy on disk under the lock and is saved, only
    /// when a dry run on the in-memory copy changed something; otherwise
    /// on the in-memory ledger.
    pub fn mutate(&mut self, change: impl Fn(&mut Ledger) -> bool) {
        let mut probe = self.ledger.clone();
        if !change(&mut probe) {
            return;
        }
        match self.path.clone() {
            Some(path) => {
                self.ledger.claim(&path, |ledger| change(ledger));
                self.remember_stamp();
            }
            None => self.ledger = probe,
        }
    }

    /// [`Ledger::claim_resets`] on this ledger's file.
    pub fn claim_resets(
        &mut self,
        due: Vec<ResetEvent>,
        skipped: Vec<ResetEvent>,
        now: DateTime<Utc>,
    ) -> Vec<ResetEvent> {
        if due.is_empty() && skipped.is_empty() {
            return Vec::new();
        }
        let path = self.path.clone();
        let sent = self.ledger.claim_resets(path.as_deref(), due, skipped, now);
        self.remember_stamp();
        sent
    }

    /// [`Ledger::claim_alerts`] on this ledger's file.
    pub fn claim_alerts(
        &mut self,
        next: &Report,
        prefs: &AlertPrefs,
        events: Vec<AlertEvent>,
        deferred: Vec<(AlertEvent, DateTime<Utc>)>,
        now: DateTime<Utc>,
    ) -> Vec<AlertEvent> {
        let path = self.path.clone();
        let sent = self
            .ledger
            .claim_alerts(path.as_deref(), next, prefs, events, deferred, now);
        self.remember_stamp();
        sent
    }

    /// [`Ledger::claim_digest`] on this ledger's file.
    pub fn claim_digest<'r>(
        &mut self,
        reports: impl IntoIterator<Item = &'r Report>,
        prefs: &AlertPrefs,
        digest: bool,
        now: DateTime<Utc>,
    ) -> Vec<AlertEvent> {
        let path = self.path.clone();
        let events = self
            .ledger
            .claim_digest(path.as_deref(), reports, prefs, digest, now);
        self.remember_stamp();
        events
    }
}
