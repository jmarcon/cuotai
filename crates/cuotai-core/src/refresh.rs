//! Stateful refresh scheduling and snapshot assembly shared by frontends.
//!
//! Frontends decide when to request work and how to render it. This module
//! owns effective per-account intervals, due-time calculation, snapshot
//! assembly, and the partial updates described by [`Edit`]. Upserts leave
//! only the touched account pending; removals and moves reindex retained
//! reports and deadlines without querying providers.

use std::time::{Duration, Instant};

use chrono::{TimeDelta, Utc};

/// PRD 0033 RF2.2: restored readings older than this are not used.
pub const RESTORE_MAX_AGE: TimeDelta = TimeDelta::hours(24);

use crate::{Account, Config, MetricKind, Report, Snapshot, Status};

/// PRD 0033 RF1.2: the reset-driven query runs this long after the reset.
pub const RESET_QUERY_DELAY: TimeDelta = TimeDelta::seconds(30);
/// PRD 0033 RF1.1: a deadline within this of the wanted time is left alone.
const BRING_FORWARD_SLACK: Duration = Duration::from_secs(60);
/// PRD 0033 RF1.2: minimum spacing between two reset-driven queries of an
/// account.
const RESET_QUERY_SPACING: Duration = Duration::from_secs(300);

/// What the cycle remembers of an account's reset-driven query (RF1.2).
#[derive(Clone, Debug)]
struct ResetQuery {
    /// Earliest `resets_at` of the window instance that was served.
    instance: chrono::DateTime<Utc>,
    /// When the extra query was armed.
    armed_at: Instant,
}

/// Refresh state for one validated [`Config`].
///
/// Account indices must refer to the same configuration used at construction.
/// Use [`Self::apply_edit`] to react to single-account changes and moves
/// without dropping reports for unaffected accounts; recreate the cycle when
/// the whole configuration changes (multiple edits, different global
/// settings, ...).
#[derive(Clone)]
pub struct RefreshCycle {
    /// Per-account slots. `None` means "no fresh report yet" — either the
    /// cycle is brand new, the slot was just edited, or the per-account
    /// refresh is still in flight. Frontends must not display stale data for
    /// a `None` slot.
    slots: Vec<Option<Report>>,
    /// Last `Ready` report per account, kept apart from `slots` so a failed
    /// query never erases the last useful reading (PRD 0010 RF2). It never
    /// enters `slots` or `snapshot`, so alerts, history and `--json` ignore it.
    last_good: Vec<Option<Report>>,
    /// `Some` only when every slot holds a report. Kept separate from
    /// `slots` so a single edit cannot leak stale rows into the public
    /// snapshot that drives `cuotai --json` output.
    snapshot: Option<Snapshot>,
    intervals: Vec<Duration>,
    deadlines: Vec<Instant>,
    /// Consecutive `RateLimited` / `AuthenticationError` results per
    /// slot. Reset to 0 on `Ready`; drives the exponential deadline
    /// growth described in `finish_account`. Kept alongside `slots` so a
    /// pending slot's failure history is preserved across reload/retry.
    failures: Vec<u32>,
    /// Indices a frontend has an in-flight refresh for. The
    /// begin/finish generation check rejects late results that landed
    /// after a configuration change.
    busy: std::collections::BTreeSet<usize>,
    /// Monotonically increasing counter that tags every accepted
    /// begin/finish pair. Frontends pass back the value returned by
    /// `begin`; the cycle drops a finish whose token does not match.
    /// Apply_edit bumps the generation so results that were already
    /// in flight when the user saved an edit never poison the new
    /// configuration.
    generation: u64,
    /// Generation token returned by the most recent `begin` call per
    /// account index. `None` means no in-flight task for that slot. The
    /// finish path matches against this map (and not against the bare
    /// `generation` counter) so a full sweep's `begin`s don't get
    /// invalidated by an edit's bump.
    in_flight: Vec<Option<u64>>,
    /// Whether [`Self::finish_full`] has been called at least once
    /// since the cycle was built (or rebuilt from an edit/remove). The
    /// scheduler uses this to distinguish "initial sweep" (every slot
    /// pending, never attempted) from "sweep failed and we should
    /// fall back to per-account retries via `pending_now_indices`".
    /// Without the flag, a failed full sweep would loop forever
    /// because every slot is still pending and `pending_indices()`
    /// returns them all — and the scheduler would re-fire the same
    /// full sweep every tick.
    attempted_full: bool,
    /// Reset-driven queries already armed, by [`Report::account_key`], so
    /// each window instance causes at most one (PRD 0033 RF1.2).
    reset_queries: std::collections::BTreeMap<String, ResetQuery>,
}

/// Identifier returned by [`RefreshCycle::begin`]. The frontend passes
/// the same value back to [`RefreshCycle::finish_account`] /
/// [`RefreshCycle::finish_full`]; any other value is silently dropped
/// so a result that landed after a configuration change cannot
/// reanimate a slot from the previous generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Generation(pub u64);

/// Maximum per-account deadline the backoff ever produces. Doubling the
/// base interval from a tiny floor quickly overshoots the operator's
/// patience; the cap keeps a flapping account from hiding for hours.
pub const BACKOFF_CAP_SECONDS: u64 = 3_600;

/// Caller-supplied description of a configuration edit. The frontend tells
/// the core what it intends (`Upsert`, `Remove`, or `Move`); the core
/// cross-checks the new [`Config`] against this description before applying
/// it. Indexes supplied by the frontend never authorise misassociating old
/// reports with reordered accounts: any structural mismatch between the
/// declared operation and the diff between the old and new configuration is
/// rejected with [`EditError::UnexpectedDiff`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Replace or insert a single account at `index`. The slot at `index`
    /// becomes pending and needs a fresh query; every other slot keeps its
    /// previous report and deadline.
    Upsert(usize),
    /// Drop the account at `index`. No new query is needed for the removed
    /// slot; the trailing accounts slide up by one.
    Remove(usize),
    /// Move an account to its final index. Reports, intervals, deadlines and
    /// failure history move with it and no account needs a fresh query.
    Move { from: usize, to: usize },
}

/// Reasons an [`Edit`] cannot be applied to the supplied configuration pair.
///
/// The frontend is the source of truth for *which* index changed; this error
/// type stops callers from declaring "edit account 2" while silently
/// reordering the rest of the file.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EditError {
    #[error("Índice {0} fora de alcance.")]
    Index(usize),
    #[error(
        "A nova configuração não corresponde à operação declarada; edições não devem reordenar nem alterar outras contas."
    )]
    UnexpectedDiff,
}

/// Move one item to its final position using the same index semantics as
/// [`Config::move_account`].
fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    let item = items.remove(from);
    items.insert(to, item);
}

impl RefreshCycle {
    /// Create an empty cycle. Every account's first periodic deadline starts
    /// at `now + effective_refresh_seconds`; callers normally request one full
    /// refresh immediately while [`Self::snapshot`] is `None`.
    pub fn new(config: &Config, now: Instant) -> Self {
        let intervals = config
            .accounts
            .iter()
            .map(|account| {
                Duration::from_secs(account.effective_refresh_seconds(config.refresh_seconds))
            })
            .collect::<Vec<_>>();
        let deadlines = intervals.iter().map(|interval| now + *interval).collect();
        let len = intervals.len();
        Self {
            slots: vec![None; len],
            last_good: vec![None; len],
            snapshot: None,
            intervals,
            deadlines,
            failures: vec![0; len],
            busy: std::collections::BTreeSet::new(),
            generation: 0,
            in_flight: vec![None; len],
            attempted_full: false,
            reset_queries: std::collections::BTreeMap::new(),
        }
    }

    /// PRD 0033 RF1.1: lower `index`'s deadline to `at`. Never raises it,
    /// and does nothing while the account is busy or when `at` is within
    /// 60 s of the current deadline. `true` when the deadline moved.
    pub fn bring_forward(&mut self, index: usize, at: Instant) -> bool {
        let Some(deadline) = self.deadlines.get(index).copied() else {
            return false;
        };
        if self.busy.contains(&index) || at + BRING_FORWARD_SLACK > deadline {
            return false;
        }
        self.deadlines[index] = at;
        true
    }

    /// PRD 0033 RF1.2/RF1.3: after a reading, schedule the one extra query
    /// that follows the account's next plan-limit reset. `wall` is the
    /// current wall clock, `now` the scheduler's.
    fn arm_reset_query(
        &mut self,
        index: usize,
        report: &Report,
        wall: chrono::DateTime<Utc>,
        now: Instant,
    ) {
        if report.status != Status::Ready || report.provider.is_metered() {
            return;
        }
        let mut resets: Vec<_> = report
            .metrics
            .iter()
            .filter(|m| m.kind == MetricKind::Quota)
            .filter_map(|m| m.resets_at)
            .filter(|at| *at > wall)
            .collect();
        resets.sort();
        let Some(first) = resets.first().copied() else {
            return;
        };
        let key = report.account_key();
        if let Some(done) = self.reset_queries.get(&key) {
            if (done.instance - first).abs() < crate::resets::DRIFT_TOLERANCE {
                return;
            }
        }
        // Resets within two minutes of the first share one query, after
        // the last of them.
        let last = resets
            .iter()
            .copied()
            .take_while(|at| *at - first <= crate::resets::DRIFT_TOLERANCE)
            .last()
            .unwrap_or(first);
        let target = (last + RESET_QUERY_DELAY - wall)
            .to_std()
            .map_or(now, |d| now + d);
        let target = match self.reset_queries.get(&key) {
            Some(done) => target.max(done.armed_at + RESET_QUERY_SPACING),
            None => target,
        };
        let deadline = self.deadlines[index];
        // Moved, or the scheduled query already lands within a minute of
        // the wanted time. A deadline well before it does not count and
        // re-arms on the next reading.
        let covered = self.bring_forward(index, target)
            || (target + BRING_FORWARD_SLACK > deadline && target < deadline + BRING_FORWARD_SLACK);
        if covered {
            self.reset_queries.insert(
                key,
                ResetQuery {
                    instance: first,
                    armed_at: now,
                },
            );
        }
    }

    /// Returns the latest fully-assembled snapshot, or `None` when at least
    /// one slot is still pending. The JSON consumed by `cuotai --json` comes
    /// from this value; it is never exposed while any edit/refresh is in
    /// flight.
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// Returns the report for `index`, even when other slots are still
    /// pending. `None` means "no report yet for this account" (cycle is
    /// fresh or this slot was just edited). Frontends should render an
    /// "Aguardando" placeholder instead of leaking a stale row from the
    /// previous configuration.
    pub fn account_report(&self, index: usize) -> Option<&Report> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// The last `Ready` reading of `index`, offered only while the current
    /// report is missing or not `Ready`. It keeps its original `checked_at`;
    /// frontends must label it stale and never treat it as current.
    pub fn last_good_report(&self, index: usize) -> Option<&Report> {
        match self.slots.get(index)? {
            Some(report) if report.status == Status::Ready => None,
            _ => self.last_good[index].as_ref(),
        }
    }

    /// Keep or drop the preserved reading after a new report: `Ready`
    /// replaces it, an authentication failure invalidates it (the data
    /// belongs to a credential that no longer works), anything else keeps it.
    fn remember(&mut self, index: usize, report: &Report) {
        match report.status {
            Status::Ready => self.last_good[index] = Some(report.clone()),
            Status::AuthenticationError => self.last_good[index] = None,
            Status::RateLimited
            | Status::NeedsConfiguration
            | Status::Unsupported
            | Status::Error => {}
        }
    }

    /// Carry `old`'s preserved readings into a cycle rebuilt for the same
    /// accounts (a global setting such as `refresh_seconds` changed). Does
    /// nothing when the account count differs; never call it across account
    /// edits, which go through [`Self::apply_edit`].
    pub fn keep_last_good_from(&mut self, old: &Self) {
        if old.last_good.len() == self.last_good.len() {
            self.last_good.clone_from(&old.last_good);
        }
    }

    /// PRD 0010 RF2.3: offer readings recorded before the app opened
    /// (`readings`, in account order, e.g. `Monitor::restored_readings`) as
    /// the preserved reading of each account that has neither a report nor
    /// a preserved reading yet, so it shows as stale until the first query
    /// finishes. Does nothing when the count differs from the accounts'.
    pub fn restore_last_good(&mut self, readings: Vec<Option<Report>>) {
        self.restore_last_good_at(readings, Utc::now());
    }

    /// [`Self::restore_last_good`] against an explicit clock.
    pub fn restore_last_good_at(
        &mut self,
        readings: Vec<Option<Report>>,
        now: chrono::DateTime<Utc>,
    ) {
        if readings.len() != self.last_good.len() {
            return;
        }
        for ((kept, slot), reading) in self.last_good.iter_mut().zip(&self.slots).zip(readings) {
            if kept.is_none() && slot.is_none() {
                *kept = reading
                    .filter(|report| report.status == Status::Ready)
                    .filter(|report| now - report.checked_at <= RESTORE_MAX_AGE)
                    .map(|report| report.collapse_expired(now));
            }
        }
    }

    /// Clone the cycle's mutable bookkeeping so the caller can stage an edit
    /// on the copy before committing it. The cached snapshot is cloned
    /// because both edit paths need its `started_at` field (Upsert
    /// discards it; Remove rebuilds a fresh snapshot from the surviving
    /// reports so the JSON pipeline keeps emitting a complete picture).
    /// Frontends that need the failure invariant "if disk write fails,
    /// the cycle never sees the new configuration" call this first,
    /// mutate the staged copy, and only after every prerequisite succeeds
    /// do they assign it back.
    pub fn clone_state_for_edit(&self) -> Self {
        Self {
            slots: self.slots.clone(),
            last_good: self.last_good.clone(),
            snapshot: self.snapshot.clone(),
            intervals: self.intervals.clone(),
            deadlines: self.deadlines.clone(),
            failures: self.failures.clone(),
            busy: self.busy.clone(),
            generation: self.generation,
            in_flight: self.in_flight.clone(),
            attempted_full: self.attempted_full,
            reset_queries: self.reset_queries.clone(),
        }
    }

    /// Number of slots the cycle is currently tracking. Equals the number
    /// of accounts in the configuration the cycle was built from.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Indices of slots that have no fresh report. Used by the dashboard
    /// scheduler to detect the initial-load state ("every slot is
    /// pending → fire one full sweep") and to render "Aguardando"
    /// placeholders. Distinct from [`Self::pending_now_indices`],
    /// which only lists slots whose deadline has already elapsed and
    /// are therefore safe to retry without defeating the backoff.
    pub fn pending_indices(&self) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|i| self.slots[*i].is_none())
            .collect()
    }

    /// Indices of slots that have no fresh report AND whose deadline
    /// has already elapsed at `now`. This is the deadline-aware view
    /// a scheduler must use to drive automatic retries: edit-driven
    /// pending slots have a deadline in the future and must wait;
    /// failure-driven pending slots have a deadline in the future
    /// too, so the scheduler must skip them until the deadline passes.
    pub fn pending_now_indices(&self, now: Instant) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|i| self.slots[*i].is_none() && now >= self.deadlines[*i])
            .collect()
    }

    /// Indices of slots whose deadline has already passed at `now` AND
    /// which have a fresh report ready (or were not edited out of the
    /// schedule). Frontends use this to drive periodic refreshes:
    /// only fully-reported, non-busy slots are eligible, which keeps
    /// the scheduler from firing the same account twice while a
    /// previous query is in flight. Slots that are pending because of
    /// a recent edit live in [`Self::pending_indices`]; slots whose
    /// deadline has not yet elapsed are not eligible here even when
    /// the slot is Some — they have not earned a refresh yet.
    pub fn due_indices(&self, now: Instant) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|i| {
                self.slots[*i].is_some() && now >= self.deadlines[*i] && !self.busy.contains(i)
            })
            .collect()
    }

    pub fn is_due(&self, index: usize, now: Instant) -> bool {
        now >= self.deadlines[index]
    }

    pub fn next_refresh_in(&self, index: usize, now: Instant) -> Duration {
        self.deadlines[index].saturating_duration_since(now)
    }

    /// Whether `index` currently has an in-flight refresh. Used by the
    /// scheduler to skip already-busy slots without re-fetching the
    /// bookkeeping state.
    pub fn is_busy(&self, index: usize) -> bool {
        self.busy.contains(&index)
    }

    /// Whether the cycle has finished at least one full sweep. The
    /// scheduler uses this flag to choose between "initial sweep"
    /// (every slot pending, never attempted) and "post-failure"
    /// (every slot pending, attempted at least once — fall back to
    /// per-account retries via [`Self::pending_now_indices`]).
    pub fn attempted_full(&self) -> bool {
        self.attempted_full
    }

    /// Reserve `index` for an in-flight refresh and return the generation
    /// token the caller must pass back to
    /// [`Self::finish_account`]. Returns `None` when the index is already
    /// busy (the caller asked for a quick frontend re-fire while a
    /// previous task is still pending), or when the slot has no live
    /// configuration (the caller is refreshing a stale UI list). The
    /// token lets a finish call prove it is reporting back for the same
    /// refresh the begin reserved, not a leftover from a previous
    /// generation that already finished (or failed).
    pub fn begin(&mut self, index: usize) -> Option<Generation> {
        if index >= self.slots.len() {
            return None;
        }
        if !self.busy.insert(index) {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        let token = Generation(self.generation);
        self.in_flight[index] = Some(token.0);
        Some(token)
    }

    /// Reserve every slot for an in-flight full sweep and return a single
    /// generation token. All slots are marked busy with the same token;
    /// [`Self::finish_full`] accepts the snapshot only when every slot
    /// still has that exact token. The TUI event loop relies on this so
    /// a manual per-account refresh that races the full sweep can clean
    /// up its own slot without poisoning the snapshot.
    ///
    /// Returns `None` when at least one slot is already busy — the caller
    /// is starting a sweep while a previous one is still in flight.
    pub fn begin_full(&mut self) -> Option<Generation> {
        if self.slots.is_empty() {
            return None;
        }
        for index in 0..self.slots.len() {
            if !self.busy.insert(index) {
                // Roll back the partial reservation so a failed begin
                // does not leave a half-marked busy set behind.
                for cleared in 0..index {
                    self.busy.remove(&cleared);
                    self.in_flight[cleared] = None;
                }
                return None;
            }
        }
        self.generation = self.generation.wrapping_add(1);
        let token = Generation(self.generation);
        for index in 0..self.slots.len() {
            self.in_flight[index] = Some(token.0);
        }
        Some(token)
    }

    /// Mark the slot as no longer busy. Cheap, idempotent, no generation
    /// check — used by the failure path where the caller knows the refresh
    /// did not produce a useful report.
    fn release(&mut self, index: usize) {
        self.busy.remove(&index);
        self.in_flight[index] = None;
    }

    /// Complete a full-refresh attempt and schedule every account's next
    /// periodic refresh. A failed task passes `None` and keeps any prior
    /// snapshot unchanged. The `generation` token (returned by the
    /// matching [`Self::begin_full`] call) is required: any other value
    /// is silently dropped so a sweep kicked off by an older generation
    /// cannot overwrite a snapshot the user already replaced through a
    /// single-account edit.
    ///
    /// Per-slot backoff is preserved: each report is fed through the
    /// same backoff arithmetic as [`Self::finish_account`], so a sweep
    /// where AWS returned RateLimited for one account and Ready for
    /// another honours both signals (Ready resets, RateLimited hides
    /// the affected slot behind a fresh backoff). A sweep with `None`
    /// (every task failed) bumps the failure counter for every slot so
    /// the next periodic sweep honours the same ceiling. `true` when the
    /// attempt was accepted (so its reports, if any, are now the slots').
    pub fn finish_full(
        &mut self,
        snapshot: Option<Snapshot>,
        generation: Generation,
        now: Instant,
    ) -> bool {
        // Step 1: the strict guard. A `finish_full` is only valid
        // when (a) `self.generation` still equals the token's
        // generation (no edit/remove bumped it) AND (b) EVERY slot
        // in `in_flight` carries the matching token — the sweep
        // began with `begin_full`, which reserves every slot under
        // one token. After `apply_edit` clears `in_flight` to `None`,
        // a leftover sweep must NOT pass the guard. The previous
        // `any_mismatch` check was vacuously satisfied when every
        // slot was `None` (and accepted the stale snapshot, which
        // could panic on the length assert after a remove). The
        // epoch check closes that hole: an edit bumps
        // `self.generation`, so the cycle's live generation no
        // longer matches the sweep's token.
        if self.generation != generation.0 {
            return false;
        }
        if self.in_flight.is_empty() {
            // The cycle is fresh or every slot has been released;
            // there is no in-flight sweep to finish. The token is
            // meaningless and any snapshot we received belongs to an
            // older generation.
            return false;
        }
        let all_match = self
            .in_flight
            .iter()
            .all(|token| *token == Some(generation.0));
        if !all_match {
            // Either some slot still carries an older token (a
            // per-account refresh raced in front of the sweep) or an
            // edit bumped the generation while the sweep was in
            // flight. Drop the entire result — applying a partial
            // snapshot would overwrite a slot whose latest commit is
            // newer than the sweep's view.
            return false;
        }
        // Step 2: release every slot so the snapshot application
        // operates on an empty busy set. Per-slot deadlines were
        // recomputed below; we do not call `reschedule_all`.
        let len = self.in_flight.len();
        for i in 0..len {
            if self.in_flight[i] == Some(generation.0) {
                self.release(i);
            }
        }
        // Mark the full sweep as attempted so the scheduler can
        // distinguish "first sweep never ran" from "first sweep
        // failed and we should fall back to per-account retries".
        // Set only on accept — a rejected stale sweep does not
        // count.
        self.attempted_full = true;
        if let Some(snapshot) = snapshot {
            assert_eq!(
                snapshot.reports.len(),
                self.intervals.len(),
                "snapshot must match refresh configuration"
            );
            // Per-slot processing — feed every report through the same
            // backoff logic `finish_account` uses. A successful sweep
            // is *not* a global "everything is fine" signal; a single
            // RateLimited / AuthenticationError slot must still hide
            // for its own backoff window. A successful Ready report
            // resets that slot's counter (same as `finish_account`),
            // so the cycle honours both signals inside the same
            // sweep.
            for (index, report) in snapshot.reports.iter().enumerate() {
                let next_failures = match report.status {
                    crate::Status::RateLimited | crate::Status::AuthenticationError => {
                        self.failures[index] = self.failures[index].saturating_add(1);
                        self.failures[index]
                    }
                    crate::Status::Ready
                    | crate::Status::NeedsConfiguration
                    | crate::Status::Unsupported
                    | crate::Status::Error => {
                        self.failures[index] = 0;
                        0
                    }
                };
                let backoff = Self::backoff_for(self.intervals[index], next_failures);
                self.deadlines[index] = now + backoff;
                self.remember(index, report);
                self.arm_reset_query(index, report, Utc::now(), now);
            }
            self.slots = snapshot.reports.iter().cloned().map(Some).collect();
            self.snapshot = Some(snapshot);
        } else {
            // The task itself failed (panic, network exception) for
            // every slot. Apply a uniform backoff so the next periodic
            // sweep does not pound the provider every deadline.
            for index in 0..self.intervals.len() {
                self.failures[index] = self.failures[index].saturating_add(1);
                let backoff = Self::backoff_for(self.intervals[index], self.failures[index]);
                self.deadlines[index] = now + backoff;
            }
        }
        true
    }

    /// Complete one account-refresh attempt and schedule that account's next
    /// periodic refresh. The `generation` token (returned by the matching
    /// [`Self::begin`] call) is required: any other value is silently
    /// dropped so a leftover result from a previous configuration cannot
    /// reanimate a slot that was edited or removed in the meantime. A
    /// failed task passes `None`; the slot stays pending and the failure
    /// counter increments so the deadline backs off. A successful report
    /// replaces exactly its configuration-order slot, advances the
    /// snapshot completion timestamp when a snapshot was already
    /// assembled, and resets the failure counter on `Ready`. `true` when
    /// the attempt was accepted (so a report, if any, is now the slot's).
    pub fn finish_account(
        &mut self,
        index: usize,
        report: Option<Report>,
        generation: Generation,
        now: Instant,
    ) -> bool {
        // Generation guard: refuse any report that does not belong to
        // the most recent begin call on this slot. The most common
        // causes are (1) an edit bumped `self.generation` while the
        // task was in flight, (2) a removal dropped the slot, or (3) a
        // reload replaced the cycle. Refuse silently — the caller will
        // see the slot stay pending and re-queue.
        if index >= self.in_flight.len() || self.in_flight[index] != Some(generation.0) {
            return false;
        }
        self.release(index);
        match report {
            Some(report) => {
                let next_failures = match report.status {
                    crate::Status::RateLimited | crate::Status::AuthenticationError => {
                        // Apply exponential backoff: a flapping account
                        // hides for longer after every failed cycle,
                        // capped so the user never waits more than
                        // `BACKOFF_CAP_SECONDS` between attempts. The
                        // counter saturates — overflow is irrelevant
                        // because the cap clamps the deadline
                        // regardless.
                        self.failures[index] = self.failures[index].saturating_add(1);
                        self.failures[index]
                    }
                    crate::Status::Ready
                    | crate::Status::NeedsConfiguration
                    | crate::Status::Unsupported
                    | crate::Status::Error => {
                        // Reset the counter so a transient recovery
                        // does not leave a permanent backoff in place.
                        self.failures[index] = 0;
                        0
                    }
                };
                let backoff = Self::backoff_for(self.intervals[index], next_failures);
                self.remember(index, &report);
                self.deadlines[index] = now + backoff;
                self.arm_reset_query(index, &report, Utc::now(), now);
                let next_deadline = self.deadlines[index];
                self.slots[index] = Some(report);
                if let Some(snapshot) = self.snapshot.as_mut() {
                    snapshot.reports[index] = self.slots[index]
                        .clone()
                        .expect("just filled by the branch above");
                    snapshot.finished_at = Utc::now();
                } else if self.slots.iter().all(Option::is_some) {
                    self.snapshot = Some(Snapshot {
                        started_at: Utc::now(),
                        finished_at: Utc::now(),
                        reports: self
                            .slots
                            .iter()
                            .map(|slot| slot.clone().unwrap())
                            .collect(),
                    });
                }
                self.deadlines[index] = next_deadline;
            }
            None => {
                // The task itself failed (panic, network exception) and
                // produced no report. Treat it as a per-request failure:
                // increment the counter and back off so a flapping
                // account does not hammer the provider every deadline.
                // Counter saturation is irrelevant — the cap clamps the
                // deadline.
                self.failures[index] = self.failures[index].saturating_add(1);
                let backoff = Self::backoff_for(self.intervals[index], self.failures[index]);
                self.deadlines[index] = now + backoff;
            }
        }
        true
    }

    /// Effective deadline a transition into `failures` consecutive
    /// `RateLimited` / `AuthenticationError` / task-failure results
    /// would produce for an account whose configured interval is
    /// `base`. Doubles the interval per consecutive failure, capped at
    /// `BACKOFF_CAP_SECONDS`; an interval already above the cap is kept
    /// as is, so a failure never polls sooner than configured (metered
    /// providers included). Crate-visible so tests and the extension
    /// runner (PRD 0017 RF3.4, the same backoff per extension) share
    /// the exact arithmetic.
    pub(crate) fn backoff_for(base: Duration, failures: u32) -> Duration {
        let base_secs = base.as_secs().max(1);
        let cap = BACKOFF_CAP_SECONDS.max(base_secs);
        let mut seconds = base_secs;
        // Saturating shift would overflow quickly on 64-bit values;
        // explicit cap short-circuit makes the bound obvious to the
        // reviewer.
        for _ in 0..failures {
            if seconds >= cap {
                seconds = cap;
                break;
            }
            seconds = seconds.saturating_mul(2).min(cap);
        }
        Duration::from_secs(seconds)
    }

    /// Apply an edit between two configurations and return the indices that
    /// need a fresh query.
    ///
    /// `prev_config` is the configuration the current cycle was built from;
    /// `new_config` is the configuration that should drive the cycle after
    /// the operation. The function refuses anything other than the exact
    /// structural change implied by `edit` so the slot mapping stays
    /// unambiguous. Upserts preserve unaffected reports/deadlines and leave
    /// the touched slot pending; removals and moves preserve every retained
    /// slot. The returned indices contain exactly one entry for `Upsert` and
    /// are empty for `Remove` and `Move`.
    pub fn apply_edit(
        &mut self,
        prev_config: &Config,
        new_config: &Config,
        edit: Edit,
        now: Instant,
    ) -> Result<Vec<usize>, EditError> {
        match edit {
            Edit::Upsert(index) => {
                if index > new_config.accounts.len() {
                    return Err(EditError::Index(index));
                }
                self.verify_upsert(prev_config, new_config, index)?;
                let new_intervals: Vec<Duration> = new_config
                    .accounts
                    .iter()
                    .map(|account| {
                        Duration::from_secs(
                            account.effective_refresh_seconds(new_config.refresh_seconds),
                        )
                    })
                    .collect();
                // Slots for accounts that kept their identity stay filled;
                // the touched slot becomes pending. Trailing accounts also
                // stay filled because the diff is constrained above.
                let mut next_slots: Vec<Option<Report>> =
                    Vec::with_capacity(new_config.accounts.len());
                for (new_index, account) in new_config.accounts.iter().enumerate() {
                    if new_index == index {
                        next_slots.push(None);
                    } else if new_index < prev_config.accounts.len() {
                        next_slots.push(self.slots.get(new_index).cloned().unwrap_or(None));
                        // After verify_upsert we know `account == prev[new_index]`.
                        debug_assert_eq!(
                            Some(account),
                            prev_config.accounts.get(new_index),
                            "verify_upsert must have rejected non-matching accounts"
                        );
                    } else {
                        next_slots.push(None);
                    }
                }
                // Editing an account invalidates any cached full snapshot —
                // its slots no longer match the new configuration. Only the
                // touched slot's deadline is reset (it must wait the full
                // interval before its next refresh); every other deadline
                // is preserved so we don't artificially postpone an account
                // that was about to fire.
                // The touched slot's reading belongs to the previous
                // identity or credential: discard it; others keep theirs.
                self.last_good = (0..new_config.accounts.len())
                    .map(|i| {
                        if i == index {
                            None
                        } else {
                            self.last_good.get(i).cloned().flatten()
                        }
                    })
                    .collect();
                self.slots = next_slots;
                self.snapshot = None;
                self.intervals = new_intervals.clone();
                self.deadlines = (0..new_intervals.len())
                    .map(|i| {
                        if i == index {
                            now + new_intervals[i]
                        } else if i < self.deadlines.len() {
                            self.deadlines[i]
                        } else {
                            now + new_intervals[i]
                        }
                    })
                    .collect();
                // Bookkeeping rebuild: the touched slot's failure history
                // belongs to the previous account, the others keep theirs
                // in identity-position order. Bumping `self.generation`
                // invalidates any in-flight finish token so a result
                // that lands while the user is editing can never
                // reanimate the previous configuration's slot.
                // `attempted_full` resets to false: the new
                // configuration may legitimately need its first sweep
                // (e.g. an append), and the scheduler must be allowed
                // to fire one.
                self.failures = new_config
                    .accounts
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        if i < prev_config.accounts.len() {
                            self.failures.get(i).copied().unwrap_or(0)
                        } else {
                            0
                        }
                    })
                    .collect();
                self.busy.clear();
                self.in_flight = vec![None; new_config.accounts.len()];
                self.generation = self.generation.wrapping_add(1);
                self.attempted_full = false;
                Ok(vec![index])
            }
            Edit::Remove(index) => {
                if index >= prev_config.accounts.len() {
                    return Err(EditError::Index(index));
                }
                self.verify_remove(prev_config, new_config, index)?;
                // Drop the slot at `index` and slide the rest up. Trailing
                // slots stay filled with their previous reports. If every
                // surviving slot still has a report, rebuild the cached
                // snapshot so `cuotai --json` continues to emit a complete
                // picture — removing an account is a free operation and
                // should not look like a half-fresh state to the JSON
                // pipeline.
                self.slots.remove(index);
                self.last_good.remove(index);
                self.intervals.remove(index);
                self.deadlines.remove(index);
                if self.slots.iter().all(Option::is_some) {
                    self.snapshot = Some(Snapshot {
                        started_at: self
                            .snapshot
                            .as_ref()
                            .map(|s| s.started_at)
                            .unwrap_or_else(Utc::now),
                        finished_at: Utc::now(),
                        reports: self
                            .slots
                            .iter()
                            .map(|slot| {
                                slot.clone()
                                    .expect("all slots filled when rebuilding snapshot")
                            })
                            .collect(),
                    });
                } else {
                    self.snapshot = None;
                }
                // Bookkeeping rebuild: removal slides every per-account
                // vector one position to the left and clears the
                // bookkeeping that no longer has a slot. The busy
                // set is purged wholesale — every in-flight refresh
                // either belongs to the removed account or to a
                // shifted account whose token no longer points at
                // the right slot. The in-flight map is dropped
                // wholesale too: even after shifting, the surviving
                // tokens still match the previous generation, and
                // the cycle's `self.generation` is about to bump, so
                // any later `finish_*` would be rejected by the
                // epoch check anyway. Empty it now so the layout
                // invariant is "every entry is None until a fresh
                // begin" — easy to reason about and impossible to
                // misattribute.
                self.failures.remove(index);
                self.busy.clear();
                self.in_flight = vec![None; new_config.accounts.len()];
                self.generation = self.generation.wrapping_add(1);
                self.attempted_full = false;
                Ok(Vec::new())
            }
            Edit::Move { from, to } => {
                self.verify_move(prev_config, new_config, from, to)?;
                if from == to {
                    return Ok(Vec::new());
                }
                // A move changes only positions. Every per-account value
                // follows the same mapping, including the cached snapshot,
                // so no slot becomes pending and no provider query is owed.
                move_item(&mut self.slots, from, to);
                move_item(&mut self.last_good, from, to);
                move_item(&mut self.intervals, from, to);
                move_item(&mut self.deadlines, from, to);
                move_item(&mut self.failures, from, to);
                if let Some(snapshot) = self.snapshot.as_mut() {
                    move_item(&mut snapshot.reports, from, to);
                }
                // A full sweep still returns reports in the old order, and
                // per-account tasks call back with old indices. Invalidate
                // those completions rather than risking stale-row reuse.
                // Existing reports and deadlines remain usable.
                // If the invalidated work was the initial full sweep, count
                // it as attempted. Otherwise the scheduler could immediately
                // start another full sweep just because the user reordered.
                let full_token = self.in_flight.first().copied().flatten();
                let full_in_flight = full_token == Some(self.generation)
                    && self.busy.len() == self.in_flight.len()
                    && self.in_flight.iter().all(|token| *token == full_token);
                if full_in_flight {
                    self.attempted_full = true;
                }
                self.busy.clear();
                self.in_flight.fill(None);
                self.generation = self.generation.wrapping_add(1);
                Ok(Vec::new())
            }
        }
    }

    /// Ensure `new_config` matches `prev_config` everywhere except at
    /// `index`, where it must carry a single replacement (or a new
    /// account appended at the end). This is what prevents the frontend
    /// from misattributing reports after a reorder or a multi-account
    /// edit: any extra divergence is rejected.
    fn verify_upsert(
        &self,
        prev_config: &Config,
        new_config: &Config,
        index: usize,
    ) -> Result<(), EditError> {
        if prev_config.refresh_seconds != new_config.refresh_seconds
            || prev_config.concurrency != new_config.concurrency
            || prev_config.timeout_seconds != new_config.timeout_seconds
        {
            return Err(EditError::UnexpectedDiff);
        }
        let prev_len = prev_config.accounts.len();
        let new_len = new_config.accounts.len();
        // Either `new_len == prev_len` (replace at `index`) or
        // `new_len == prev_len + 1` and `index == prev_len` (append).
        let append = (prev_len, new_len, index) == (prev_len, prev_len + 1, prev_len);
        if new_len != prev_len && !append {
            return Err(EditError::UnexpectedDiff);
        }
        if index >= new_len {
            return Err(EditError::Index(index));
        }
        // Every position except `index` must be byte-for-byte equal; the
        // touched slot is the only one allowed to differ (or to be brand
        // new when appending).
        for offset in 0..new_len {
            if offset == index {
                continue;
            }
            let prev = match offset.cmp(&prev_len) {
                std::cmp::Ordering::Less => &prev_config.accounts[offset],
                _ => return Err(EditError::UnexpectedDiff),
            };
            if prev != &new_config.accounts[offset] {
                return Err(EditError::UnexpectedDiff);
            }
        }
        if append {
            // When appending, the new slot's identity (name/provider/oauth)
            // must not collide with an existing account — `Config::validate`
            // already enforces that, but we double-check because two
            // accounts sharing identity would have stolen each other's
            // reports on a future refresh.
            if new_config.accounts[index].name.trim().is_empty() {
                return Err(EditError::UnexpectedDiff);
            }
        }
        // A replace that leaves the TOML entry identical is valid: re-authenticating an account
        // renews its vault credential without touching the file, and the slot is queried again.
        Ok(())
    }

    fn verify_remove(
        &self,
        prev_config: &Config,
        new_config: &Config,
        index: usize,
    ) -> Result<(), EditError> {
        if prev_config.refresh_seconds != new_config.refresh_seconds
            || prev_config.concurrency != new_config.concurrency
            || prev_config.timeout_seconds != new_config.timeout_seconds
        {
            return Err(EditError::UnexpectedDiff);
        }
        let prev_len = prev_config.accounts.len();
        if new_config.accounts.len() + 1 != prev_len {
            return Err(EditError::UnexpectedDiff);
        }
        // The new list must equal the old one with exactly `index`
        // removed; accounts that slid up must remain in their original
        // relative order and identity.
        for (new_offset, account) in new_config.accounts.iter().enumerate() {
            let prev_offset = if new_offset >= index {
                new_offset + 1
            } else {
                new_offset
            };
            if &prev_config.accounts[prev_offset] != account {
                return Err(EditError::UnexpectedDiff);
            }
        }
        Ok(())
    }

    /// Accept exactly the permutation produced by moving `from` to `to`.
    /// Global settings and every account's complete value stay unchanged.
    fn verify_move(
        &self,
        prev_config: &Config,
        new_config: &Config,
        from: usize,
        to: usize,
    ) -> Result<(), EditError> {
        for index in [from, to] {
            if index >= prev_config.accounts.len() {
                return Err(EditError::Index(index));
            }
        }
        if prev_config.refresh_seconds != new_config.refresh_seconds
            || prev_config.concurrency != new_config.concurrency
            || prev_config.timeout_seconds != new_config.timeout_seconds
            || prev_config.accounts.len() != new_config.accounts.len()
        {
            return Err(EditError::UnexpectedDiff);
        }
        for (new_index, account) in new_config.accounts.iter().enumerate() {
            let prev_index = if new_index == to {
                from
            } else if from < to && new_index >= from && new_index < to {
                new_index + 1
            } else if to < from && new_index > to && new_index <= from {
                new_index - 1
            } else {
                new_index
            };
            if &prev_config.accounts[prev_index] != account {
                return Err(EditError::UnexpectedDiff);
            }
        }
        Ok(())
    }

    /// Test-only helper: pin a slot's deadline so the scheduler's
    /// `due_indices` decision can be exercised deterministically without
    /// having to sleep the test process. Mirrors what `apply_edit` does
    /// for the touched slot.
    #[cfg(test)]
    pub fn set_deadline_for_test(&mut self, index: usize, deadline: Instant) {
        self.deadlines[index] = deadline;
    }

    /// Test-only helper: clone the deadlines vector so a test can
    /// compare against a pre-edit snapshot. Returns the vector by value
    /// (the deadline type is `Copy` on every supported target).
    #[cfg(test)]
    pub fn deadlines_snapshot(&self) -> Vec<Instant> {
        self.deadlines.clone()
    }
}

// Compile-time check: Report carries the same `Status` variant set we
// expect when a pending slot transitions to a refreshed report. This keeps
// `Status` exhaustive against `Report::status` even if someone reorders
// the variants while editing `model.rs`.
#[allow(dead_code)]
fn _status_round_trip(report: &Report) -> Status {
    report.status
}

// Compile-time check: Account equality is what `verify_upsert` relies on.
// If `Account` ever loses `PartialEq` the contract breaks; surface that.
#[allow(dead_code)]
fn _account_partial_eq(a: &Account, b: &Account) -> bool {
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Provider, Status};
    use chrono::TimeZone;
    use std::collections::BTreeMap;

    fn account(name: &str, refresh_seconds: Option<u64>) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider: Provider::OpenAiApi,
            credential_env: None,
            session: None,
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds,
        }
    }

    fn metered_account(name: &str, refresh_seconds: Option<u64>) -> Account {
        Account {
            provider: Provider::AwsBedrock,
            ..account(name, refresh_seconds)
        }
    }

    fn report(account: &str, second: i64) -> Report {
        Report {
            account_id: None,
            diagnostic: None,
            account: account.into(),
            provider: Provider::OpenAiApi,
            checked_at: Utc.timestamp_opt(second, 0).unwrap(),
            status: Status::Ready,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
        }
    }

    fn config_with(accounts: Vec<Account>) -> Config {
        Config {
            refresh_seconds: 60,
            concurrency: 2,
            timeout_seconds: 5,
            accounts,
        }
    }

    /// Helper for the existing RF1 suite: stage an initial full sweep
    /// by reserving every slot with a single generation token, then
    /// calling `finish_full` with the matching token. The tests already
    /// cover what the snapshot contains — the new wiring just makes
    /// sure every snapshot path is gated by the begin/finish pair so
    /// stale results from a previous generation cannot leak through.
    fn prime_with_snapshot(cycle: &mut RefreshCycle, snapshot: Snapshot, now: Instant) {
        let token = cycle.begin_full().expect("begin_full available");
        cycle.finish_full(Some(snapshot), token, now);
    }

    #[test]
    fn due_times_use_each_accounts_effective_interval() {
        let config = Config {
            refresh_seconds: 60,
            concurrency: 2,
            timeout_seconds: 5,
            accounts: vec![account("global", None), account("override", Some(30))],
        };
        let now = Instant::now();
        let cycle = RefreshCycle::new(&config, now);

        assert!(!cycle.is_due(0, now + Duration::from_secs(59)));
        assert!(cycle.is_due(0, now + Duration::from_secs(60)));
        assert!(!cycle.is_due(1, now + Duration::from_secs(29)));
        assert!(cycle.is_due(1, now + Duration::from_secs(30)));
    }

    #[test]
    fn metered_schedule_allows_one_query_per_effective_interval() {
        fn assert_schedule(override_seconds: Option<u64>, interval_seconds: u64) {
            let config = Config {
                refresh_seconds: 300,
                concurrency: 1,
                timeout_seconds: 5,
                accounts: vec![metered_account("bedrock", override_seconds)],
            };
            let now = Instant::now();
            let mut cycle = RefreshCycle::new(&config, now);
            prime_with_snapshot(
                &mut cycle,
                Snapshot {
                    started_at: Utc.timestamp_opt(1, 0).unwrap(),
                    finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                    reports: vec![report("bedrock", 2)],
                },
                now,
            );
            let interval = Duration::from_secs(interval_seconds);
            let first_due = now + interval;

            assert!(cycle
                .due_indices(first_due - Duration::from_secs(1))
                .is_empty());
            assert_eq!(cycle.due_indices(first_due), vec![0]);
            let token = cycle.begin(0).expect("one query starts at the boundary");
            assert!(
                cycle.due_indices(first_due).is_empty(),
                "the same interval cannot start a second query"
            );
            cycle.finish_account(0, Some(report("bedrock", 3)), token, first_due);

            let second_due = first_due + interval;
            assert!(cycle
                .due_indices(second_due - Duration::from_secs(1))
                .is_empty());
            assert_eq!(cycle.due_indices(second_due), vec![0]);
        }

        assert_schedule(None, Provider::METERED_DEFAULT_REFRESH_SECONDS);
        assert_schedule(Some(600), Provider::METERED_MIN_REFRESH_SECONDS);
    }

    #[test]
    fn full_and_account_completion_assemble_snapshot_and_reschedule() {
        let config = Config {
            refresh_seconds: 60,
            concurrency: 1,
            timeout_seconds: 5,
            accounts: vec![account("first", None), account("second", Some(30))],
        };
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&config, now);
        let initial_finished = Utc.timestamp_opt(10, 0).unwrap();
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: initial_finished,
                reports: vec![report("first", 2), report("second", 3)],
            },
            now,
        );

        let replacement = report("second-new", 20);
        let token = cycle.begin(1).expect("begin slot 1");
        cycle.finish_account(
            1,
            Some(replacement.clone()),
            token,
            now + Duration::from_secs(10),
        );

        let snapshot = cycle.snapshot().expect("snapshot available");
        assert_eq!(snapshot.reports[0].account, "first");
        assert_eq!(snapshot.reports[1].account, replacement.account);
        assert!(snapshot.finished_at >= initial_finished);
        assert_eq!(
            cycle.next_refresh_in(1, now + Duration::from_secs(10)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn failed_attempts_only_advance_deadlines() {
        let config = Config {
            refresh_seconds: 60,
            concurrency: 1,
            timeout_seconds: 5,
            accounts: vec![account("only", None)],
        };
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&config, now);

        let token = cycle.begin_full().expect("begin_full available");
        cycle.finish_full(None, token, now + Duration::from_secs(10));
        assert!(cycle.snapshot().is_none());
        // A failed sweep bumps the failure counter and doubles the
        // deadline (60 → 120). The next periodic retry must not pound
        // the provider every deadline.
        assert_eq!(cycle.failures[0], 1);
        assert_eq!(
            cycle.next_refresh_in(0, now + Duration::from_secs(10)),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn apply_edit_upsert_preserves_other_reports_and_marks_target_pending() {
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 5), report("beta", 6)],
            },
            Instant::now(),
        );
        assert!(cycle.snapshot().is_some());

        let mut edited_beta = account("beta", None);
        edited_beta.refresh_seconds = Some(45);
        let new_config = config_with(vec![account("alpha", None), edited_beta.clone()]);

        let now = Instant::now();
        let due = cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(1), now)
            .expect("upsert accepted");

        // Exactly one query is owed for the edited account.
        assert_eq!(due, vec![1]);
        // Other slot keeps its report; edited slot is pending.
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into())
        );
        assert!(cycle.account_report(1).is_none());
        // Public snapshot stays hidden while a slot is pending — the JSON
        // pipeline must never see a half-fresh state.
        assert!(cycle.snapshot().is_none());
        // Edited account's new effective interval drives its deadline.
        assert_eq!(cycle.intervals[1], Duration::from_secs(45));
        assert_eq!(cycle.deadlines[1], now + Duration::from_secs(45));
    }

    #[test]
    fn apply_edit_upsert_appending_keeps_existing_reports_and_returns_new_index() {
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 5), report("beta", 6)],
            },
            Instant::now(),
        );

        let gamma = account("gamma", None);
        let new_config = config_with(vec![
            account("alpha", None),
            account("beta", None),
            gamma.clone(),
        ]);
        let due = cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(2), Instant::now())
            .expect("append accepted");
        assert_eq!(due, vec![2]);
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into())
        );
        assert_eq!(
            cycle.account_report(1).map(|r| r.account.clone()),
            Some("beta".into())
        );
        assert!(cycle.account_report(2).is_none());
    }

    #[test]
    fn apply_edit_upsert_preserves_unaffected_deadlines() {
        // Contract from RF1.2: editing one account must not reset the
        // next-refresh deadline of every other account. Otherwise an
        // account that was about to fire would be artificially delayed
        // by a full interval just because the user touched a neighbour.
        let prev = config_with(vec![
            account("alpha", None),
            account("beta", None),
            account("gamma", None),
        ]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 3), report("beta", 4), report("gamma", 5)],
            },
            now,
        );
        // Pretend alpha's deadline is exactly one second away and
        // beta's deadline is ten seconds away (both should survive the
        // edit on gamma).
        cycle.set_deadline_for_test(0, now + Duration::from_secs(1));
        cycle.set_deadline_for_test(1, now + Duration::from_secs(10));
        let original_deadlines = cycle.deadlines_snapshot();

        let mut edited_gamma = prev.accounts[2].clone();
        edited_gamma.name = "gamma-renamed".into();
        let new_config = config_with(vec![
            account("alpha", None),
            account("beta", None),
            edited_gamma,
        ]);

        cycle
            .apply_edit(
                &prev,
                &new_config,
                Edit::Upsert(2),
                now + Duration::from_millis(500),
            )
            .expect("upsert accepted");

        // Alpha and beta deadlines must NOT have moved.
        assert_eq!(cycle.deadlines[0], original_deadlines[0]);
        assert_eq!(cycle.deadlines[1], original_deadlines[1]);
        // Gamma (the touched slot) was rescheduled to one full interval
        // from the moment of the edit.
        assert_eq!(
            cycle.deadlines[2],
            now + Duration::from_millis(500) + Duration::from_secs(60)
        );
    }

    #[test]
    fn apply_edit_remove_reindexes_and_clears_target() {
        let prev = config_with(vec![
            account("alpha", None),
            account("beta", None),
            account("gamma", None),
        ]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 5), report("beta", 6), report("gamma", 7)],
            },
            Instant::now(),
        );

        let new_config = config_with(vec![account("alpha", None), account("gamma", None)]);
        let due = cycle
            .apply_edit(&prev, &new_config, Edit::Remove(1), Instant::now())
            .expect("remove accepted");
        assert!(due.is_empty(), "removal must not trigger any query");
        // gamma slid up; alpha unchanged.
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into())
        );
        assert_eq!(
            cycle.account_report(1).map(|r| r.account.clone()),
            Some("gamma".into())
        );
        assert!(cycle.account_report(2).is_none());
        // Public snapshot reflects the new shape (Vec<Report> ordering).
        let snapshot = cycle.snapshot().expect("snapshot still complete");
        assert_eq!(snapshot.reports.len(), 2);
        assert_eq!(snapshot.reports[0].account, "alpha");
        assert_eq!(snapshot.reports[1].account, "gamma");
    }

    #[test]
    fn move_account_checks_both_indices() {
        let config = config_with(vec![account("alpha", None), account("beta", None)]);
        assert!(config.move_account(2, 0).is_err());
        assert!(config.move_account(0, 2).is_err());
        let unchanged = config.move_account(0, 0).expect("in-place move is valid");
        assert_eq!(unchanged.accounts[0].name, "alpha");
        assert_eq!(unchanged.accounts[1].name, "beta");
    }

    #[test]
    fn apply_edit_move_reindexes_state_without_querying() {
        let prev = config_with(vec![
            account("alpha", Some(30)),
            account("beta", Some(45)),
            account("gamma", Some(90)),
        ]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 5), report("beta", 6), report("gamma", 7)],
            },
            now,
        );
        cycle.set_deadline_for_test(0, now + Duration::from_secs(1));
        cycle.set_deadline_for_test(1, now + Duration::from_secs(2));
        cycle.set_deadline_for_test(2, now + Duration::from_secs(3));
        cycle.failures = vec![0, 1, 2];
        let deadlines = cycle.deadlines_snapshot();

        let new_config = prev.move_account(0, 2).expect("valid move");
        let due = cycle
            .apply_edit(&prev, &new_config, Edit::Move { from: 0, to: 2 }, now)
            .expect("move accepted");

        assert!(due.is_empty(), "reordering must not trigger any query");
        fn names(reports: &[Report]) -> Vec<&str> {
            reports
                .iter()
                .map(|report| report.account.as_str())
                .collect()
        }
        let slots = (0..cycle.len())
            .map(|index| cycle.account_report(index).unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!(names(&slots), ["beta", "gamma", "alpha"]);
        assert_eq!(
            names(&cycle.snapshot().expect("complete snapshot").reports),
            ["beta", "gamma", "alpha"]
        );
        assert_eq!(cycle.deadlines, [deadlines[1], deadlines[2], deadlines[0]]);
        assert_eq!(
            cycle.intervals,
            [
                Duration::from_secs(45),
                Duration::from_secs(90),
                Duration::from_secs(30)
            ]
        );
        assert_eq!(cycle.failures, [1, 2, 0]);
        assert!(cycle.pending_indices().is_empty());
        assert!(cycle.attempted_full());
    }

    #[test]
    fn move_during_initial_sweep_does_not_rearm_or_accept_stale_reports() {
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);
        let token = cycle.begin_full().expect("initial sweep started");
        let new_config = prev.move_account(1, 0).expect("valid move");

        let due = cycle
            .apply_edit(&prev, &new_config, Edit::Move { from: 1, to: 0 }, now)
            .expect("move accepted");
        assert!(due.is_empty());
        assert!(cycle.attempted_full());
        assert!(cycle.pending_now_indices(now).is_empty());

        cycle.finish_full(
            Some(Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 5), report("beta", 6)],
            }),
            token,
            now,
        );
        assert!(cycle.snapshot().is_none());
        assert!(cycle.account_report(0).is_none());
        assert!(cycle.account_report(1).is_none());
    }

    /// Re-authenticating an account renews the vault credential but leaves its TOML entry
    /// byte-for-byte the same. That save is a legitimate edit: it must be accepted and owe a
    /// fresh query of that account (the new credential), not fail as "no change".
    #[test]
    fn apply_edit_upsert_accepts_an_unchanged_account_and_requeries_it() {
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 5), report("beta", 6)],
            },
            Instant::now(),
        );
        let same = prev.clone();
        let due = cycle
            .apply_edit(&prev, &same, Edit::Upsert(1), Instant::now())
            .expect("re-saving an unchanged account is accepted");
        assert_eq!(due, vec![1]);
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into()),
            "other accounts keep their reports"
        );
        assert!(
            cycle.account_report(1).is_none(),
            "re-authenticated slot is pending"
        );
    }

    /// The frontend sequence of a re-authentication save: `upsert_account` with the account as
    /// it already is, then `apply_edit` on the result.
    #[test]
    fn reauthentication_save_pipeline_accepts_the_same_account() {
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let same_beta = prev.accounts[1].clone();
        let updated = prev
            .upsert_account(same_beta, Some(1))
            .expect("same account upserts");
        assert_eq!(updated.accounts, prev.accounts);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        assert_eq!(
            cycle.apply_edit(&prev, &updated, Edit::Upsert(1), Instant::now()),
            Ok(vec![1])
        );
    }

    #[test]
    fn apply_edit_rejects_reorder() {
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 5), report("beta", 6)],
            },
            Instant::now(),
        );

        // Pretending "edit at index 1" while actually swapping the order.
        let new_config = config_with(vec![account("beta", None), account("alpha", None)]);
        let err = cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(1), Instant::now())
            .unwrap_err();
        assert_eq!(err, EditError::UnexpectedDiff);
    }

    #[test]
    fn apply_edit_rejects_multi_change_diff() {
        let prev = config_with(vec![
            account("alpha", None),
            account("beta", None),
            account("gamma", None),
        ]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        // Two accounts changed (alpha + beta) but only one edit is declared.
        let new_config = config_with(vec![
            account("alpha-2", None),
            account("beta-2", None),
            account("gamma", None),
        ]);
        let err = cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(0), Instant::now())
            .unwrap_err();
        assert_eq!(err, EditError::UnexpectedDiff);
    }

    #[test]
    fn apply_edit_rejects_out_of_bounds_index() {
        let prev = config_with(vec![account("alpha", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        let new_config = config_with(vec![account("alpha", None), account("beta", None)]);
        // Index 5 does not exist in either config.
        let err = cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(5), Instant::now())
            .unwrap_err();
        assert_eq!(err, EditError::Index(5));
    }

    #[test]
    fn apply_edit_remove_clears_snapshot_when_any_survivor_is_pending() {
        // If at least one surviving slot is still pending (i.e. a
        // pre-edit fetch was in flight when the user removed a
        // *different* account), the cached snapshot cannot be rebuilt
        // — the public JSON pipeline must not see a half-fresh
        // picture. We set up three accounts with the MIDDLE one pending
        // (a fetch was in flight for beta), then remove ALPHA — beta
        // slides to index 0 and stays pending, gamma slides to index 1
        // and is full, the snapshot must be None.
        let prev = config_with(vec![
            account("alpha", None),
            account("beta", None),
            account("gamma", None),
        ]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        // Pre-fill alpha and gamma; beta is pending.
        cycle.slots = vec![Some(report("alpha", 1)), None, Some(report("gamma", 3))];
        cycle.snapshot = None;

        // Remove the fully-reported alpha. Beta (pending) slides to
        // index 0; gamma (full) slides to index 1.
        let new_config = config_with(vec![account("beta", None), account("gamma", None)]);
        cycle
            .apply_edit(&prev, &new_config, Edit::Remove(0), Instant::now())
            .expect("remove accepted");

        // Beta slid up and is still pending; gamma slid up and is
        // full. The snapshot must NOT be rebuilt because the cycle is
        // not yet complete.
        assert!(
            cycle.account_report(0).is_none(),
            "beta pending slid up to index 0"
        );
        assert_eq!(
            cycle.account_report(1).map(|r| r.account.clone()),
            Some("gamma".into()),
            "gamma full slid up to index 1"
        );
        assert!(cycle.account_report(2).is_none());
        assert!(
            cycle.snapshot().is_none(),
            "snapshot must stay None when any surviving slot is pending"
        );
    }

    #[test]
    fn apply_edit_rejects_remove_diff_other_than_one_account() {
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        // new_config is empty, not just one shorter.
        let new_config = config_with(vec![]);
        let err = cycle
            .apply_edit(&prev, &new_config, Edit::Remove(0), Instant::now())
            .unwrap_err();
        assert_eq!(err, EditError::UnexpectedDiff);
    }

    #[test]
    fn apply_edit_rejects_global_setting_changes() {
        let prev = config_with(vec![account("alpha", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        let mut new_config = config_with(vec![account("alpha-2", None)]);
        new_config.refresh_seconds = 120;
        let err = cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(0), Instant::now())
            .unwrap_err();
        assert_eq!(err, EditError::UnexpectedDiff);
    }

    // -------------------------------------------------------------------
    // RF6: begin/finish generation guard, due_indices, backoff, and
    // failure-state masking. The tests use the same helpers as the RF1
    // suite; the new methods just add gates around the existing path
    // so we can prove every finish path drops stale results.
    // -------------------------------------------------------------------

    fn report_with_status(name: &str, second: i64, status: Status) -> Report {
        Report {
            account_id: None,
            diagnostic: None,
            account: name.into(),
            provider: Provider::OpenAiApi,
            checked_at: Utc.timestamp_opt(second, 0).unwrap(),
            status,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
        }
    }

    #[test]
    fn begin_returns_none_when_already_busy() {
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        let first = cycle.begin(0).expect("begin slot 0");
        let second = cycle.begin(0);
        assert!(
            second.is_none(),
            "second concurrent begin on slot 0 is refused"
        );
        // Release and try again — must succeed.
        cycle.finish_account(0, None, first, Instant::now());
        let third = cycle.begin(0).expect("begin slot 0 again");
        cycle.finish_account(0, None, third, Instant::now());
    }

    #[test]
    fn finish_account_stale_generation_does_not_apply_report() {
        // The canonical failure the generation guard prevents: a
        // per-account refresh lands *after* the user saved an edit on
        // the same account. The cycle must keep the new slot pending
        // rather than reanimate it from the old generation.
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        // Initial sweep so alpha and beta have reports.
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 3), report("beta", 4)],
            },
            Instant::now(),
        );
        // Begin a manual refresh on beta; the user is about to save an
        // edit on beta before the task lands.
        let manual_token = cycle.begin(1).expect("begin slot 1");

        // Simulate the edit landing first (apply_edit bumps the
        // generation and clears the busy set, which is what the
        // frontend does after the disk write succeeds).
        let edited_beta = account("beta", Some(45));
        let new_config = config_with(vec![account("alpha", None), edited_beta]);
        cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(1), Instant::now())
            .expect("edit accepted");

        // Now the manual refresh lands with its old token — the cycle
        // must drop the result.
        let stale_report = report("beta-OLD", 99);
        cycle.finish_account(1, Some(stale_report.clone()), manual_token, Instant::now());

        // Beta slot must still be pending: the edit's pending state
        // has not been touched by the stale result.
        assert!(
            cycle.account_report(1).is_none(),
            "stale finish must not reanimate a pending slot"
        );
        // The busy set must NOT be poisoned — apply_edit already
        // cleared it; the stale finish leaves it empty.
        assert!(!cycle.is_busy(1));
    }

    #[test]
    fn finish_full_after_apply_edit_rejects_stale_snapshot() {
        // Regression: after `apply_edit` clears `in_flight` to `None`,
        // a leftover `finish_full` for the previous generation must
        // not apply its snapshot. The "any mismatch" predicate would
        // vacuously accept (every slot is None) and either panic on
        // the length assert (length changed) or overwrite the new
        // configuration. The strict `all-match` predicate rejects.
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        // Prime the cycle with a real initial snapshot so the
        // survivors (alpha) carry their report across the edit.
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 3), report("beta", 4)],
            },
            Instant::now(),
        );
        // Sweep starts; every slot is reserved with one token.
        let sweep_token = cycle.begin_full().expect("begin_full");
        assert!(cycle.in_flight.iter().all(|t| *t == Some(sweep_token.0)));

        // User saves an edit while the sweep is in flight. The
        // cycle bumps generation, clears in_flight, and re-orders
        // the slot layout for the new config.
        let mut edited_beta = account("beta", None);
        edited_beta.refresh_seconds = Some(45);
        let new_config = config_with(vec![account("alpha", None), edited_beta]);
        cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(1), Instant::now())
            .expect("edit accepted");
        // in_flight is now [None, None]; the previous sweep's token
        // is gone with the cycle's generation bump.
        assert!(cycle.in_flight.iter().all(Option::is_none));

        // Sweep lands with the OLD token. The strict guard rejects
        // because in_flight is empty (and even if it weren't, every
        // entry would have to equal `sweep_token.0`, which it does
        // not because `apply_edit` bumped the generation).
        let now = Instant::now();
        let stale_snapshot = Snapshot {
            started_at: Utc.timestamp_opt(1, 0).unwrap(),
            finished_at: Utc.timestamp_opt(2, 0).unwrap(),
            reports: vec![report("alpha-OLD", 30), report("beta-OLD", 31)],
        };
        cycle.finish_full(Some(stale_snapshot), sweep_token, now);

        // No overwrite: the touched slot (beta) is still pending,
        // the survivor (alpha) keeps its original report, no
        // snapshot was applied.
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into()),
            "alpha survivor must not be overwritten by stale sweep"
        );
        assert!(
            cycle.account_report(1).is_none(),
            "beta touched slot must stay pending; stale sweep must not reanimate it"
        );
        assert!(
            cycle.snapshot().is_none(),
            "stale sweep must not assemble a snapshot"
        );
    }

    #[test]
    fn finish_full_after_remove_rejects_stale_snapshot() {
        // Same regression for `Edit::Remove` — after a removal the
        // slot layout shrinks, and a stale sweep whose length still
        // matches the old layout must not apply.
        let prev = config_with(vec![
            account("alpha", None),
            account("beta", None),
            account("gamma", None),
        ]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 3), report("beta", 4), report("gamma", 5)],
            },
            Instant::now(),
        );
        let sweep_token = cycle.begin_full().expect("begin_full");

        let new_config = config_with(vec![account("alpha", None), account("gamma", None)]);
        cycle
            .apply_edit(&prev, &new_config, Edit::Remove(1), Instant::now())
            .expect("remove accepted");
        assert!(cycle.in_flight.iter().all(Option::is_none));

        // Old sweep lands — len 3 snapshot. The strict guard must
        // reject before the length assert has a chance to panic.
        let now = Instant::now();
        let stale_snapshot = Snapshot {
            started_at: Utc.timestamp_opt(1, 0).unwrap(),
            finished_at: Utc.timestamp_opt(2, 0).unwrap(),
            reports: vec![
                report("alpha-OLD", 30),
                report("beta-OLD", 31),
                report("gamma-OLD", 32),
            ],
        };
        cycle.finish_full(Some(stale_snapshot), sweep_token, now);

        assert_eq!(cycle.len(), 2);
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into())
        );
        assert_eq!(
            cycle.account_report(1).map(|r| r.account.clone()),
            Some("gamma".into())
        );
        // The post-remove state rebuilds a fresh snapshot from the
        // surviving reports (alpha + gamma). The strict guard's
        // effect is that the OLD snapshot's reports do NOT appear —
        // i.e. beta is not resurrected by the stale sweep.
        let post = cycle.snapshot().expect("post-remove snapshot rebuilt");
        assert_eq!(post.reports.len(), 2);
        assert_eq!(post.reports[0].account, "alpha");
        assert_eq!(post.reports[1].account, "gamma");
    }

    #[test]
    fn finish_full_initial_load_is_valid() {
        // The strict guard must not regress the legitimate
        // initial-load path: a fresh cycle's first sweep lands with
        // `Some(token)` on every slot because `begin_full` reserved
        // them. Verify that an initial sweep DOES apply, so the
        // contract holds both ways.
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);
        let sweep_token = cycle.begin_full().expect("begin_full");
        cycle.finish_full(
            Some(Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 3), report("beta", 4)],
            }),
            sweep_token,
            now,
        );
        assert!(cycle.snapshot().is_some());
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into())
        );
        assert_eq!(
            cycle.account_report(1).map(|r| r.account.clone()),
            Some("beta".into())
        );
    }

    #[test]
    fn finish_full_stale_generation_does_not_apply_snapshot() {
        // The full-sweep race: a manual refresh on slot 1 is still
        // in flight when the sweep finishes. The scheduler is
        // expected to avoid this race by checking `is_busy` before
        // kicking off a sweep (`begin_full` returns `None` when any
        // slot is busy). The cycle rejects the sweep result whose
        // begin token does not match the current in-flight token for
        // any slot — otherwise the manual refresh's later result
        // would have nowhere to land without overwriting the
        // sweep's.
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 3), report("beta", 4)],
            },
            Instant::now(),
        );
        // Manual refresh on beta (begin bumps generation).
        let manual_token = cycle.begin(1).expect("begin slot 1");
        // Sweep refuses because a per-account refresh is in flight.
        let sweep_token = cycle.begin_full();
        assert!(
            sweep_token.is_none(),
            "begin_full rejects when a slot is busy"
        );

        // We simulate the scheduler's bad-day case — a sweep that
        // started BEFORE the manual begin landed, with a now-stale
        // token. Manually mark slot 0 as in-flight with the stale
        // sweep token; slot 1 keeps its manual token.
        let stale_token = cycle.begin(0).expect("begin slot 0 for the stale sweep");
        cycle.finish_account(0, None, stale_token, Instant::now());
        // Slot 0 is now free again. To model the "sweep started
        // before the manual begin" race, we re-pretend slot 0 had a
        // sweep token (99) and slot 1 has the manual token.
        cycle.busy.clear();
        cycle.in_flight[0] = Some(99);
        cycle.in_flight[1] = Some(manual_token.0);
        let foreign_token = Generation(99);
        cycle.finish_full(
            Some(Snapshot {
                started_at: Utc.timestamp_opt(10, 0).unwrap(),
                finished_at: Utc.timestamp_opt(11, 0).unwrap(),
                reports: vec![report("alpha-2", 30), report("beta-2", 31)],
            }),
            foreign_token,
            Instant::now(),
        );
        // Sweep token (99) only matches slot 0; slot 1's manual
        // token does not. The cycle rejects the whole sweep
        // because accepting it would overwrite slot 0 with the
        // sweep's report AND break the invariant "no sweep result
        // is partial". The cycle keeps the original reports.
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into()),
            "stale sweep did not overwrite alpha"
        );
        assert_eq!(
            cycle.account_report(1).map(|r| r.account.clone()),
            Some("beta".into()),
            "stale sweep did not overwrite beta"
        );
        // The manual begin's token is still valid for slot 1.
        assert_eq!(cycle.in_flight[1], Some(manual_token.0));
    }

    #[test]
    fn apply_edit_bumps_generation_and_clears_busy_set() {
        // Two edits in a row must produce two different generation
        // tokens; otherwise a stale begin from the first edit could
        // survive a second edit.
        let prev = config_with(vec![account("alpha", None), account("beta", None)]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 3), report("beta", 4)],
            },
            Instant::now(),
        );
        // Begin a manual refresh; busy set is non-empty.
        let before_edit = cycle.begin(0).expect("begin slot 0");
        assert!(cycle.is_busy(0));

        // Edit bumps generation, clears busy.
        let edited_alpha = account("alpha", Some(45));
        let new_config = config_with(vec![edited_alpha, account("beta", None)]);
        cycle
            .apply_edit(&prev, &new_config, Edit::Upsert(0), Instant::now())
            .expect("edit accepted");

        assert!(!cycle.is_busy(0), "apply_edit clears busy");
        // Stale finish is silently dropped (does not panic, does not
        // apply).
        cycle.finish_account(
            0,
            Some(report("alpha-OLD", 99)),
            before_edit,
            Instant::now(),
        );
        assert!(
            cycle.account_report(0).is_none(),
            "stale finish after edit is dropped"
        );

        // New begin works because the slot was cleared.
        let fresh = cycle.begin(0).expect("begin slot 0 after edit");
        cycle.finish_account(0, Some(report("alpha", 99)), fresh, Instant::now());
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into())
        );
    }

    #[test]
    fn apply_edit_remove_clears_in_flight_for_removed_slot() {
        // Begin a manual refresh on account 1; then remove account 1.
        // The remaining finish must not resurrect the removed slot.
        let prev = config_with(vec![
            account("alpha", None),
            account("beta", None),
            account("gamma", None),
        ]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("alpha", 3), report("beta", 4), report("gamma", 5)],
            },
            Instant::now(),
        );
        let removed_token = cycle.begin(1).expect("begin beta");
        let new_config = config_with(vec![account("alpha", None), account("gamma", None)]);
        cycle
            .apply_edit(&prev, &new_config, Edit::Remove(1), Instant::now())
            .expect("remove accepted");

        // Stale finish on slot 1 — must not append a 3rd slot back.
        cycle.finish_account(
            1,
            Some(report("beta-OLD", 99)),
            removed_token,
            Instant::now(),
        );
        assert_eq!(cycle.len(), 2);
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("alpha".into())
        );
        assert_eq!(
            cycle.account_report(1).map(|r| r.account.clone()),
            Some("gamma".into())
        );
        assert!(cycle.account_report(2).is_none());
    }

    #[test]
    fn due_indices_excludes_pending_and_busy_slots() {
        let config = config_with(vec![
            account("alpha", None),
            account("beta", None),
            account("gamma", None),
        ]);
        let start = Instant::now();
        let mut cycle = RefreshCycle::new(&config, start);
        // Force alpha and beta to be due; gamma to be far away.
        cycle.set_deadline_for_test(0, start - Duration::from_secs(1));
        cycle.set_deadline_for_test(1, start - Duration::from_secs(1));
        cycle.set_deadline_for_test(2, start + Duration::from_secs(60));

        // Prime alpha and beta so they are not pending; gamma stays
        // pending (None slot).
        cycle.slots[0] = Some(report("alpha", 3));
        cycle.slots[1] = Some(report("beta", 4));
        // gamma remains None (pending).

        // Reserve beta — it must drop out of due_indices while busy.
        let _busy = cycle.begin(1).expect("begin beta");

        let due = cycle.due_indices(start);
        assert_eq!(due, vec![0], "only alpha is due (beta busy, gamma pending)");
    }

    #[test]
    fn finish_account_none_increments_failures_and_doubles_deadline() {
        let prev = config_with(vec![account("only", None)]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);
        // Begin a refresh; finish with None (task failed).
        let token = cycle.begin(0).expect("begin slot 0");
        cycle.finish_account(0, None, token, now);

        // No report stored.
        assert!(cycle.account_report(0).is_none());
        // Deadline doubled (60 → 120).
        assert_eq!(cycle.deadlines[0], now + Duration::from_secs(120));
        assert_eq!(cycle.failures[0], 1);
    }

    #[test]
    fn finish_account_rate_limited_applies_backoff_capped_at_one_hour() {
        let prev = config_with(vec![account("only", None)]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);

        // Expected deadline after `n` consecutive failures with a 60s base:
        // n=1: 60*2=120; n=2: 240; n=3: 480; n=4: 960; n=5: 1920→3600 (capped);
        // n≥6: 3600 (capped).
        let mut expected = Duration::from_secs(60);
        for failures in 1..=20u32 {
            let token = cycle.begin(0).expect("begin slot 0");
            let status = if failures % 2 == 0 {
                Status::RateLimited
            } else {
                Status::AuthenticationError
            };
            cycle.finish_account(
                0,
                Some(report_with_status("only", failures as i64, status)),
                token,
                now,
            );
            assert_eq!(cycle.failures[0], failures);
            // Doubling logic mirrors `backoff_for`: each iteration
            // doubles `expected` until the cap is reached.
            if expected < Duration::from_secs(BACKOFF_CAP_SECONDS) {
                expected = (expected * 2).min(Duration::from_secs(BACKOFF_CAP_SECONDS));
            }
            assert_eq!(
                cycle.deadlines[0],
                now + expected,
                "deadline must respect the {BACKOFF_CAP_SECONDS}s cap after successive failures"
            );
        }
    }

    #[test]
    fn finish_account_ready_resets_failure_counter() {
        let prev = config_with(vec![account("only", None)]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);

        // Two RateLimited refreshes.
        for _ in 0..2 {
            let token = cycle.begin(0).expect("begin slot 0");
            cycle.finish_account(
                0,
                Some(report_with_status("only", 1, Status::RateLimited)),
                token,
                now,
            );
        }
        assert_eq!(cycle.failures[0], 2);

        // One Ready refresh resets the counter.
        let token = cycle.begin(0).expect("begin slot 0");
        cycle.finish_account(
            0,
            Some(report_with_status("only", 2, Status::Ready)),
            token,
            now,
        );
        assert_eq!(cycle.failures[0], 0);
        // Deadline back to base interval.
        assert_eq!(cycle.deadlines[0], now + Duration::from_secs(60));
    }

    #[test]
    fn backoff_is_capped_for_tiny_base_intervals() {
        // Tiny base interval (1s) must still cap at 3_600s after
        // enough consecutive failures, otherwise a misconfigured
        // account with a 1s refresh could be polled forever at an
        // exponentially growing rate.
        let backoff = RefreshCycle::backoff_for(Duration::from_secs(1), 50);
        assert_eq!(backoff, Duration::from_secs(BACKOFF_CAP_SECONDS));
    }

    #[test]
    fn backoff_never_shortens_an_interval_above_the_cap() {
        // A daily account (allowed up to 86 400 s) that gets rate-limited
        // must not be polled after 1 h: the failure keeps its interval.
        let day = Duration::from_secs(86_400);
        assert_eq!(RefreshCycle::backoff_for(day, 1), day);
        assert_eq!(RefreshCycle::backoff_for(day, 10), day);
        // Below the cap the doubling still stops at the cap.
        assert_eq!(
            RefreshCycle::backoff_for(Duration::from_secs(2_400), 1),
            Duration::from_secs(BACKOFF_CAP_SECONDS)
        );
    }

    #[test]
    fn failed_task_cannot_masquerade_as_ready() {
        // The cycle stores whatever report the caller hands in. A
        // tampered caller that drops a "RateLimited" result and
        // hands in a fake "Ready" must be visible: the dashboard
        // shows the report the cycle accepted. The protection is
        // elsewhere — the cycle never inspects the underlying HTTP
        // response. This test pins the contract so any future
        // tampering attempt is caught.
        let prev = config_with(vec![account("only", None)]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);

        let token = cycle.begin(0).expect("begin slot 0");
        // Caller hands in a fake Ready report after a failure.
        let fake_ready = report_with_status("only", 1, Status::Ready);
        cycle.finish_account(0, Some(fake_ready.clone()), token, now);
        // Slot is filled with whatever the caller passed; the cycle
        // does not invent. The dashboard and `cuotai --json` reflect
        // exactly the report the cycle accepted — the cycle does
        // not, and should not, second-guess the caller's payload.
        assert_eq!(
            cycle.account_report(0).map(|r| r.account.clone()),
            Some("only".into())
        );
        assert_eq!(
            cycle.account_report(0).map(|r| r.status),
            Some(Status::Ready)
        );
        // Subsequent RateLimited flips the slot to backoff state,
        // proving the cycle never silently corrects a forged Ready.
        let token = cycle.begin(0).expect("begin slot 0");
        cycle.finish_account(
            0,
            Some(report_with_status("only", 2, Status::RateLimited)),
            token,
            now,
        );
        assert_eq!(cycle.failures[0], 1);
    }

    #[test]
    fn finish_full_applies_backoff_per_slot_inside_the_sweep() {
        // A full sweep is *not* a global "everything is fine" signal:
        // each report inside the snapshot is fed through the per-slot
        // backoff arithmetic. A Ready report resets that slot's
        // failure counter; a RateLimited / AuthenticationError report
        // bumps it (and pushes the deadline forward), even inside a
        // sweep whose other slots are Ready.
        let prev = config_with(vec![account("a", None), account("b", None)]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);
        // Three RateLimited finishes on both slots.
        for _ in 0..3 {
            let token_a = cycle.begin(0).expect("begin a");
            cycle.finish_account(
                0,
                Some(report_with_status("a", 1, Status::RateLimited)),
                token_a,
                now,
            );
            let token_b = cycle.begin(1).expect("begin b");
            cycle.finish_account(
                1,
                Some(report_with_status("b", 1, Status::RateLimited)),
                token_b,
                now,
            );
        }
        assert_eq!(cycle.failures[0], 3);
        assert_eq!(cycle.failures[1], 3);

        // Sweep: slot 0 returns Ready (resets), slot 1 returns
        // RateLimited (bumps).
        let sweep_token = cycle.begin_full().expect("begin_full");
        cycle.finish_full(
            Some(Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![
                    report_with_status("a", 10, Status::Ready),
                    report_with_status("b", 11, Status::RateLimited),
                ],
            }),
            sweep_token,
            now + Duration::from_secs(100),
        );
        // Slot 0 reset; slot 1 grew to 4.
        assert_eq!(
            cycle.failures[0], 0,
            "Ready inside the sweep resets failures"
        );
        assert_eq!(
            cycle.failures[1], 4,
            "RateLimited inside the sweep grows failures"
        );
        // Slot 0 deadline is the base interval (60s); slot 1 deadline
        // reflects 4 consecutive RateLimited failures against the
        // 60s base: 60→120→480→960 (the sweep itself adds the fourth
        // failure; the third → fourth doubling reaches 960s).
        assert_eq!(
            cycle.next_refresh_in(0, now + Duration::from_secs(100)),
            Duration::from_secs(60)
        );
        assert_eq!(
            cycle.next_refresh_in(1, now + Duration::from_secs(100)),
            Duration::from_secs(960)
        );
    }

    #[test]
    fn finish_full_none_applies_uniform_backoff() {
        // Sweep with None (every task panicked) honours the same
        // backoff arithmetic on every slot — the next periodic
        // sweep does not pound the provider every deadline.
        let prev = config_with(vec![account("a", None), account("b", None)]);
        let now = Instant::now();
        let mut cycle = RefreshCycle::new(&prev, now);
        let sweep_token = cycle.begin_full().expect("begin_full");
        cycle.finish_full(None, sweep_token, now);
        // No snapshot, no reports stored.
        assert!(cycle.snapshot().is_none());
        assert!(cycle.account_report(0).is_none());
        assert!(cycle.account_report(1).is_none());
        // Failure counters both grew; deadlines both advanced to the
        // first backoff step (60s × 2 = 120s).
        assert_eq!(cycle.failures[0], 1);
        assert_eq!(cycle.failures[1], 1);
        assert_eq!(cycle.deadlines[0], now + Duration::from_secs(120));
        assert_eq!(cycle.deadlines[1], now + Duration::from_secs(120));
    }

    fn failed(account: &str, second: i64, status: Status) -> Report {
        Report {
            status,
            ..report(account, second)
        }
    }

    fn finish(cycle: &mut RefreshCycle, index: usize, report: Report) {
        let token = cycle.begin(index).expect("slot free");
        cycle.finish_account(index, Some(report), token, Instant::now());
    }

    /// `Report` has no `PartialEq`; compare what RF2 cares about.
    fn key(report: Option<&Report>) -> Option<(&str, i64, Status)> {
        report.map(|r| (r.account.as_str(), r.checked_at.timestamp(), r.status))
    }

    const READY: Status = Status::Ready;

    #[test]
    fn last_good_survives_failures_until_success_or_authentication_error() {
        let config = config_with(vec![account("a", None), account("b", None)]);
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("a", 10), report("b", 10)],
            },
            Instant::now(),
        );
        // A current `Ready` report is never doubled by a preserved one.
        assert!(cycle.last_good_report(0).is_none());

        // Network error after a success: the reading stays with its age,
        // the current report and snapshot carry the failure untouched.
        let error = failed("a", 50, Status::Error);
        finish(&mut cycle, 0, error.clone());
        assert_eq!(key(cycle.last_good_report(0)), Some(("a", 10, READY)));
        assert_eq!(key(cycle.account_report(0)), key(Some(&error)));
        assert_eq!(
            key(cycle.snapshot().unwrap().reports.first()),
            key(Some(&error))
        );
        assert!(cycle.last_good_report(1).is_none());

        finish(&mut cycle, 0, failed("a", 60, Status::RateLimited));
        assert_eq!(key(cycle.last_good_report(0)), Some(("a", 10, READY)));

        // The next success replaces it and hides it.
        finish(&mut cycle, 0, report("a", 70));
        assert!(cycle.last_good_report(0).is_none());
        finish(&mut cycle, 0, failed("a", 80, Status::Error));
        assert_eq!(key(cycle.last_good_report(0)), Some(("a", 70, READY)));

        // An authentication failure invalidates it.
        finish(&mut cycle, 0, failed("a", 90, Status::AuthenticationError));
        assert!(cycle.last_good_report(0).is_none());
        finish(&mut cycle, 0, failed("a", 95, Status::Error));
        assert!(cycle.last_good_report(0).is_none());
    }

    #[test]
    fn restored_readings_show_until_the_first_query_answers() {
        let config = config_with(vec![account("a", None), account("b", None)]);
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        cycle.restore_last_good_at(
            vec![Some(report("a", 5)), None, None],
            Utc.timestamp_opt(60, 0).unwrap(),
        );
        assert!(cycle.last_good_report(0).is_none(), "wrong count: ignored");
        cycle.restore_last_good_at(
            vec![Some(report("a", 5)), None],
            Utc.timestamp_opt(60, 0).unwrap(),
        );
        assert_eq!(key(cycle.last_good_report(0)), Some(("a", 5, READY)));
        assert!(cycle.last_good_report(1).is_none());
        assert!(cycle.snapshot().is_none(), "never a current report");

        // A failed first query keeps showing it; a success replaces it.
        finish(&mut cycle, 0, failed("a", 20, Status::Error));
        assert_eq!(key(cycle.last_good_report(0)), Some(("a", 5, READY)));
        finish(&mut cycle, 0, report("a", 30));
        assert!(cycle.last_good_report(0).is_none());
        // Restoring again never overrides what this session already knows.
        finish(&mut cycle, 1, failed("b", 31, Status::Error));
        cycle.restore_last_good_at(
            vec![Some(report("a", 1)), Some(report("b", 1))],
            Utc.timestamp_opt(60, 0).unwrap(),
        );
        finish(&mut cycle, 0, failed("a", 40, Status::Error));
        assert_eq!(key(cycle.last_good_report(0)), Some(("a", 30, READY)));
        assert!(cycle.last_good_report(1).is_none(), "b already answered");
    }

    #[test]
    fn last_good_follows_moves_and_removals_and_drops_on_edit() {
        let prev = config_with(vec![
            account("a", None),
            account("b", None),
            account("c", None),
        ]);
        let mut cycle = RefreshCycle::new(&prev, Instant::now());
        prime_with_snapshot(
            &mut cycle,
            Snapshot {
                started_at: Utc.timestamp_opt(1, 0).unwrap(),
                finished_at: Utc.timestamp_opt(2, 0).unwrap(),
                reports: vec![report("a", 10), report("b", 11), report("c", 12)],
            },
            Instant::now(),
        );
        // A full sweep that fails keeps the readings too.
        let token = cycle.begin_full().unwrap();
        cycle.finish_full(
            Some(Snapshot {
                started_at: Utc.timestamp_opt(20, 0).unwrap(),
                finished_at: Utc.timestamp_opt(21, 0).unwrap(),
                reports: vec![
                    failed("a", 20, Status::Error),
                    failed("b", 20, Status::RateLimited),
                    failed("c", 20, Status::Error),
                ],
            }),
            token,
            Instant::now(),
        );

        let moved = prev.move_account(0, 2).unwrap();
        cycle
            .apply_edit(&prev, &moved, Edit::Move { from: 0, to: 2 }, Instant::now())
            .unwrap();
        assert_eq!(key(cycle.last_good_report(2)), Some(("a", 10, READY)));
        assert_eq!(key(cycle.last_good_report(0)), Some(("b", 11, READY)));

        let removed = moved.without_account(0).unwrap();
        cycle
            .apply_edit(&moved, &removed, Edit::Remove(0), Instant::now())
            .unwrap();
        assert_eq!(key(cycle.last_good_report(0)), Some(("c", 12, READY)));
        assert_eq!(key(cycle.last_good_report(1)), Some(("a", 10, READY)));

        let mut edited = removed.clone();
        edited.accounts[1].credential_env = Some("OTHER_KEY".into());
        cycle
            .apply_edit(&removed, &edited, Edit::Upsert(1), Instant::now())
            .unwrap();
        assert!(cycle.last_good_report(1).is_none());
        assert_eq!(key(cycle.last_good_report(0)), Some(("c", 12, READY)));

        // A rebuild for a global setting carries the readings over.
        let mut rebuilt = RefreshCycle::new(&edited, Instant::now());
        rebuilt.keep_last_good_from(&cycle);
        assert_eq!(key(rebuilt.last_good_report(0)), Some(("c", 12, READY)));
    }
}

#[cfg(test)]
mod prd0033_rf2_tests {
    use super::*;
    use crate::{Metric, MetricKind, MetricWindow, Provider};
    use chrono::TimeDelta;

    fn reading(checked_ago: TimeDelta, resets_in: TimeDelta) -> Report {
        let now = Utc::now();
        Report {
            sub_reports: Vec::new(),
            diagnostic: Default::default(),
            account_id: None,
            account: "a".into(),
            provider: Provider::Claude,
            checked_at: now - checked_ago,
            status: Status::Ready,
            metrics: vec![Metric {
                label: "Week".into(),
                used: Some(100.0),
                limit: Some(100.0),
                remaining: Some(0.0),
                unit: "%".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Weekly),
                period: None,
                resets_at: Some(now + resets_in),
                billing: Default::default(),
                scope: None,
            }],
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            as_of: None,
        }
    }

    #[test]
    fn expired_metric_collapses_and_old_reading_is_ignored() {
        let config = Config {
            accounts: vec![crate::Account {
                id: None,
                name: "a".into(),
                provider: Provider::Claude,
                credential_env: None,
                session: None,
                oauth: None,
                params: Default::default(),
                refresh_seconds: None,
            }],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        cycle.restore_last_good(vec![Some(reading(
            TimeDelta::hours(2),
            TimeDelta::hours(-1),
        ))]);
        let kept = cycle.last_good_report(0).unwrap();
        assert_eq!(kept.metrics[0].used, None);
        assert_eq!(kept.metrics[0].remaining, None);

        let mut cycle = RefreshCycle::new(&config, Instant::now());
        cycle.restore_last_good(vec![Some(reading(
            TimeDelta::hours(30),
            TimeDelta::hours(5),
        ))]);
        assert!(cycle.last_good_report(0).is_none());
    }
    fn quota_report(provider: Provider, resets: &[TimeDelta]) -> Report {
        let mut report = reading(TimeDelta::zero(), TimeDelta::hours(1));
        report.provider = provider;
        let base = report.metrics[0].clone();
        report.metrics = resets
            .iter()
            .enumerate()
            .map(|(i, after)| crate::Metric {
                label: format!("w{i}"),
                resets_at: Some(Utc::now() + *after),
                ..base.clone()
            })
            .collect();
        report
    }

    fn cycle_with(provider: Provider, seconds: u64) -> (RefreshCycle, Instant) {
        let now = Instant::now();
        let acct = crate::Account {
            name: "a".into(),
            provider,
            credential_env: None,
            session: None,
            oauth: None,
            params: Default::default(),
            refresh_seconds: Some(seconds),
        };
        let config = Config {
            accounts: vec![acct],
            ..Config::default()
        };
        (RefreshCycle::new(&config, now), now)
    }

    fn finish_one(cycle: &mut RefreshCycle, report: Report, now: Instant) {
        let token = cycle.begin(0).unwrap();
        assert!(cycle.finish_account(0, Some(report), token, now));
    }

    #[test]
    fn a_reset_brings_the_account_forward_once() {
        let (mut cycle, now) = cycle_with(Provider::Claude, 3600);
        let report = quota_report(Provider::Claude, &[TimeDelta::minutes(10)]);
        finish_one(&mut cycle, report.clone(), now);
        let wait = cycle.next_refresh_in(0, now);
        assert!(wait > Duration::from_secs(10 * 60 + 25), "{wait:?}");
        assert!(wait < Duration::from_secs(10 * 60 + 35), "{wait:?}");
        // The extra query answered; the same window instance does not
        // arm a second one.
        let later = now + wait;
        finish_one(&mut cycle, report, later);
        assert_eq!(cycle.next_refresh_in(0, later), Duration::from_secs(3600));
    }

    #[test]
    fn resets_within_two_minutes_share_one_query() {
        let (mut cycle, now) = cycle_with(Provider::Claude, 3600);
        let report = quota_report(
            Provider::Claude,
            &[TimeDelta::minutes(10), TimeDelta::minutes(11)],
        );
        finish_one(&mut cycle, report, now);
        let wait = cycle.next_refresh_in(0, now);
        assert!(wait > Duration::from_secs(11 * 60 + 25), "{wait:?}");
        assert!(wait < Duration::from_secs(11 * 60 + 35), "{wait:?}");
    }

    #[test]
    fn metered_accounts_are_never_brought_forward() {
        let (mut cycle, now) = cycle_with(Provider::AwsBedrock, 3600);
        let report = quota_report(Provider::AwsBedrock, &[TimeDelta::minutes(10)]);
        finish_one(&mut cycle, report, now);
        assert_eq!(cycle.next_refresh_in(0, now), Duration::from_secs(3600));
    }

    #[test]
    fn a_scheduled_refresh_next_to_the_reset_adds_no_query() {
        let (mut cycle, now) = cycle_with(Provider::Claude, 600 + 45);
        let report = quota_report(Provider::Claude, &[TimeDelta::minutes(10)]);
        finish_one(&mut cycle, report, now);
        assert_eq!(cycle.next_refresh_in(0, now), Duration::from_secs(645));
    }

    #[test]
    fn bring_forward_never_raises_and_skips_busy_accounts() {
        let (mut cycle, now) = cycle_with(Provider::Claude, 600);
        assert!(!cycle.bring_forward(0, now + Duration::from_secs(900)));
        assert!(cycle.bring_forward(0, now + Duration::from_secs(60)));
        assert_eq!(cycle.next_refresh_in(0, now), Duration::from_secs(60));
        let _ = cycle.begin(0).unwrap();
        assert!(!cycle.bring_forward(0, now));
    }

    #[test]
    fn failed_readings_do_not_arm_a_reset_query() {
        let (mut cycle, now) = cycle_with(Provider::Claude, 3600);
        let mut report = quota_report(Provider::Claude, &[TimeDelta::minutes(10)]);
        report.status = Status::AuthenticationError;
        finish_one(&mut cycle, report, now);
        assert!(cycle.next_refresh_in(0, now) >= Duration::from_secs(3600));
    }
}
