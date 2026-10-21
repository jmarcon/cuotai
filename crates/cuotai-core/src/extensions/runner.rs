//! PRD 0017 RF3: isolated execution of an approved extension.
//!
//! [`Runner::run`] executes an [`Approved`] extension — the only type it
//! accepts, so an extension that was never approved, or whose manifest or
//! program changed since the approval ([`Approved::verify`]), never runs:
//!
//! - RF3.1: no shell; the manifest's argument vector is the child's `argv`,
//!   the working directory is the extension's folder and `stdin` is closed.
//! - RF3.2: the environment is reduced to `PATH`, `HOME` and `LANG` (plus
//!   `SystemRoot` on Windows, without which processes do not start) and
//!   only the variables the manifest lists in `env` **and** the user
//!   approved. Cuotai never injects vault tokens or other accounts'
//!   credentials; the approved values come from the environment Cuotai
//!   itself inherited, which the disclosure showed by name.
//! - RF3.3: the manifest's time limit, after which the whole process tree
//!   is killed (a process group on Unix, a kill-on-close Job Object on
//!   Windows); `stdout` captured up to the contract's
//!   [`output::MAX_BYTES`] (the rest is read and discarded, so a chatty
//!   extension never deadlocks) and `stderr` discarded unread — neither
//!   ever reaches a report, a log or an error; at most [`MAX_RUNNING`]
//!   extensions at once per [`Runner`]; and low OS priority. Network and
//!   files are **not** restricted: Cuotai offers no sandbox, only these
//!   mitigations, and the disclosure says so.
//! - RF3.4: one extension's failure or slowness never touches another's
//!   schedule. Each id has its own schedule: after a failure the next run
//!   waits the same backoff the `RefreshCycle` applies
//!   (`refresh::RefreshCycle::backoff_for`, capped at
//!   `refresh::BACKOFF_CAP_SECONDS`), and the concurrency cap only ever
//!   queues a run, never delays another extension's deadline.
//!
//! Dropping a `run` future (a cancelled cycle) kills the processes it
//! started: `kill_on_drop` covers the direct child, the tree guard the
//! rest.

use std::collections::BTreeMap;
#[cfg(not(windows))]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::io::AsyncRead;
use tokio::process::Command;
use tokio::sync::Semaphore;

use super::consent::Approved;
use super::output;
use crate::refresh::RefreshCycle;
use crate::text::Localized;
use crate::Status;

/// Extensions running at the same time, per [`Runner`].
pub const MAX_RUNNING: usize = 2;

/// The only variables always inherited from Cuotai's environment (besides
/// the approved manifest ones and `SystemRoot` on Windows).
pub const INHERITED_ENV: [&str; 3] = ["PATH", "HOME", "LANG"];

/// Note id of a run refused because the files changed since the approval.
pub(crate) const CHANGED_NOTE: &str = "extensions.run.changed";
/// Note id of a run whose program could not be started.
pub(crate) const NOT_STARTED_NOTE: &str = "extensions.run.not_started";
/// Note id of a run ended without an exit code (a signal).
pub(crate) const TERMINATED_NOTE: &str = "extensions.run.terminated";
/// Note id of a run killed at its time limit (the `timeout` failure
/// category of `events`).
pub(crate) const TIMED_OUT_NOTE: &str = "extensions.run.timed_out";
/// Note id of a run that exited with a code other than 0.
pub(crate) const FAILED_NOTE: &str = "extensions.run.failed";

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The process exited with [`Run::exit_code`].
    Exited,
    /// The process ended without an exit code (a signal).
    Terminated,
    /// The time limit ran out and the whole process tree was killed.
    TimedOut,
    /// The program could not be started (removed, not executable).
    NotStarted,
    /// The manifest or the program changed since the approval; nothing
    /// ran.
    Changed,
}

/// One run of one extension.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// The extension's id.
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub duration: Duration,
    pub outcome: Outcome,
    pub exit_code: Option<i32>,
    /// `stdout` up to [`output::MAX_BYTES`] + 1 bytes (the extra byte is
    /// what lets [`output::check`] recognize an oversized output); the
    /// rest was read and discarded. Never shown raw to the user.
    pub stdout: Vec<u8>,
    /// More output was written than was kept.
    pub truncated: bool,
}

impl Run {
    /// Exited with code 0.
    pub fn succeeded(&self) -> bool {
        self.outcome == Outcome::Exited && self.exit_code == Some(0)
    }

    /// The reading of this run: the `stdout` checked against the output
    /// contract when the run exited 0; otherwise an `Error` reading whose
    /// only note says how the run ended, without any of its output.
    pub fn checked(&self) -> output::Checked {
        if self.succeeded() {
            return output::check(&self.id, &self.stdout);
        }
        let note = match (self.outcome, self.exit_code) {
            (Outcome::Changed, _) => Localized::fixed(
                CHANGED_NOTE,
                "O manifesto ou o programa da extensão mudou desde a aprovação; ela não foi \
                 executada.",
            ),
            (Outcome::NotStarted, _) => Localized::fixed(
                NOT_STARTED_NOTE,
                "O programa da extensão não pôde ser iniciado.",
            ),
            (Outcome::Terminated, _) => Localized::fixed(
                TERMINATED_NOTE,
                "A execução da extensão terminou sem código de saída.",
            ),
            (Outcome::TimedOut, _) => Localized::fixed(
                TIMED_OUT_NOTE,
                "A extensão excedeu o seu limite de tempo e foi encerrada.",
            ),
            (Outcome::Exited, code) => {
                let code = code.map_or_else(|| "?".to_string(), |code| code.to_string());
                Localized::with_param(
                    FAILED_NOTE,
                    "code",
                    code.as_str(),
                    format!("A extensão saiu com o código {code}."),
                )
            }
        };
        output::Checked {
            source: format!("extension:{}", self.id),
            status: Status::Error,
            metrics: Vec::new(),
            notes: vec![note],
            violations: Vec::new(),
        }
    }
}

/// The result of [`Runner::run`]: how the process ended and the reading
/// checked against the contract.
#[derive(Debug, Clone)]
pub struct Reading {
    pub run: Run,
    pub checked: output::Checked,
}

/// The per-extension schedule (RF3.4): consecutive failures and the
/// earliest next run. Each extension id has its own; nothing here is
/// shared between extensions.
#[derive(Debug, Default, Clone, Copy)]
struct Schedule {
    failures: u32,
    not_before: Option<Instant>,
}

impl Schedule {
    /// How long until this extension may run again at `now`.
    fn next_in(&self, now: Instant) -> Duration {
        self.not_before
            .map(|instant| instant.saturating_duration_since(now))
            .unwrap_or(Duration::ZERO)
    }

    /// Record a finished run: a usable reading resets the backoff, any
    /// failure doubles it (the `RefreshCycle`'s rule), and the next run
    /// waits one effective interval from `now`.
    fn finish(&mut self, interval: Duration, ok: bool, now: Instant) {
        self.failures = if ok {
            0
        } else {
            self.failures.saturating_add(1)
        };
        self.not_before = Some(now + RefreshCycle::backoff_for(interval, self.failures));
    }
}

/// Runs approved extensions under the RF3 limits. Share one instance (it
/// is cheap to clone) so [`MAX_RUNNING`] is global; the parent environment
/// is captured at construction. Must be used inside a Tokio runtime.
#[derive(Clone)]
pub struct Runner {
    slots: Arc<Semaphore>,
    parent: Arc<BTreeMap<OsString, OsString>>,
    schedules: Arc<Mutex<BTreeMap<String, Schedule>>>,
}

impl Runner {
    pub fn new() -> Self {
        Self::with_parent_env(std::env::vars_os())
    }

    /// [`Self::new`] reading the parent environment from `parent` instead
    /// of the process environment.
    fn with_parent_env(parent: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(MAX_RUNNING)),
            parent: Arc::new(parent.into_iter().collect()),
            schedules: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// RF3.4: how long until `id` may run again at `now` (zero for a
    /// never-run or healthy extension). Front ends use it to skip a
    /// backed-off extension without touching the others.
    pub fn next_run_in(&self, id: &str, now: Instant) -> Duration {
        self.schedules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .map_or(Duration::ZERO, |schedule| schedule.next_in(now))
    }

    /// Run `approved` once under the RF3 limits and check its output
    /// against the contract. Waits for a free slot (never longer than the
    /// runs ahead of it allow) and records the outcome in the extension's
    /// own schedule.
    pub async fn run(&self, approved: &Approved) -> Reading {
        let permit = Arc::clone(&self.slots)
            .acquire_owned()
            .await
            .expect("the runner semaphore stays open");
        let run = execute(approved, &self.parent).await;
        let checked = run.checked();
        let ok = checked.status == Status::Ready;
        let interval = Duration::from_secs(approved.manifest().interval_seconds);
        self.schedules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(run.id.clone())
            .or_default()
            .finish(interval, ok, Instant::now());
        drop(permit);
        Reading { run, checked }
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

/// The environment of the child: `PATH`, `HOME` and `LANG` plus, of the
/// names the user approved, only those the manifest still asks for and the
/// parent has. Nothing else — never a credential Cuotai holds.
fn child_env(
    parent: &BTreeMap<OsString, OsString>,
    approved: &Approved,
) -> Vec<(OsString, OsString)> {
    let mut env = Vec::new();
    let mut push = |name: &str| {
        if let Some(value) = lookup(parent, name) {
            env.push((OsString::from(name), value));
        }
    };
    for name in INHERITED_ENV {
        push(name);
    }
    for name in approved.env() {
        if approved.manifest().env.contains(name) {
            push(name);
        }
    }
    env
}

/// The parent's value of `name`. Unix variable names are exact, as the
/// manifest validates them; on Windows they are case-insensitive (the
/// inherited `PATH` is usually spelled `Path`, and the OS reports the
/// spelling it holds), so the lookup folds ASCII case. The child's name is
/// the requested one — the OS matches it case-insensitively too.
#[cfg(windows)]
fn lookup(parent: &BTreeMap<OsString, OsString>, name: &str) -> Option<OsString> {
    parent
        .iter()
        .find(|(key, _)| key.as_encoded_bytes().eq_ignore_ascii_case(name.as_bytes()))
        .map(|(_, value)| value.clone())
}

/// [`lookup`] on Unix: the exact name, nothing else.
#[cfg(not(windows))]
fn lookup(parent: &BTreeMap<OsString, OsString>, name: &str) -> Option<OsString> {
    parent.get(OsStr::new(name)).cloned()
}

/// The whole process tree of `child`, killed together (best effort) on
/// timeout, on normal exit (orphans left behind) and on drop (a cancelled
/// run). On Unix the child leads its own process group, killed with
/// `killpg`; on Windows it belongs to a kill-on-close Job Object.
struct Tree {
    #[cfg(unix)]
    pgid: u32,
    #[cfg(windows)]
    job: Option<Job>,
}

#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
// A Job Object handle is process-global state, safe to use from any
// thread; `Tree` never leaves the run task anyway.
unsafe impl Send for Job {}

#[cfg(windows)]
impl Job {
    /// A Job Object that kills its processes when terminated or closed,
    /// with `child` assigned. `None` when the assignment failed (the
    /// direct child is still covered by `kill_on_drop`); a grandchild
    /// spawned before the assignment escapes it — a documented race of the
    /// best-effort tree kill.
    fn new(child: &tokio::process::Child) -> Option<Self> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        let handle = child.raw_handle()?;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size,
            ) != 0;
            if !configured || AssignProcessToJobObject(job, handle as _) == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Self(job))
        }
    }

    fn kill(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        self.kill();
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

impl Tree {
    fn new(child: &tokio::process::Child) -> Self {
        #[cfg(unix)]
        {
            // The child leads its own process group (`process_group(0)`),
            // so its id is the group id.
            Self {
                pgid: child.id().unwrap_or(0),
            }
        }
        #[cfg(windows)]
        {
            Self {
                job: Job::new(child),
            }
        }
        // Other platforms keep the direct child's `kill_on_drop` only.
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Self {}
        }
    }

    /// Kill the whole tree; errors (an empty group, a failed assignment)
    /// are ignored: the direct child is always handled by the caller.
    fn kill(&self) {
        #[cfg(unix)]
        unsafe {
            // A zero pgid would be our own process group: never.
            if self.pgid != 0 {
                libc::killpg(self.pgid as libc::pid_t, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.kill();
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

impl Captured {
    fn push(&mut self, chunk: &[u8]) {
        // One byte past the contract's limit, so an oversized output is
        // recognizable as such instead of silently truncated into one
        // that could still parse.
        let limit = output::MAX_BYTES + 1;
        let room = limit.saturating_sub(self.bytes.len());
        if chunk.len() > room {
            self.truncated = true;
        }
        self.bytes
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
}

enum Waited {
    Exited(io::Result<std::process::ExitStatus>),
    TimedOut,
}

async fn execute(approved: &Approved, parent: &BTreeMap<OsString, OsString>) -> Run {
    let manifest = approved.manifest();
    let started_at = Utc::now();
    let started = Instant::now();
    let run = |outcome, exit_code, captured: Captured| Run {
        id: manifest.id.clone(),
        started_at,
        duration: started.elapsed(),
        outcome,
        exit_code,
        stdout: captured.bytes,
        truncated: captured.truncated,
    };
    // RF4.2: never start over files that changed since the approval.
    if !approved.verify() {
        return run(Outcome::Changed, None, Captured::default());
    }
    let mut command = Command::new(&manifest.program);
    command
        .args(&manifest.command[1..])
        .env_clear()
        .envs(child_env(parent, approved))
        .current_dir(&manifest.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // RF3.3: the child runs in its own process group, at low priority.
    #[cfg(unix)]
    unsafe {
        command.process_group(0);
        use std::os::unix::process::CommandExt;
        command.as_std_mut().pre_exec(|| {
            // Best effort: the run matters more than the niceness.
            libc::setpriority(libc::PRIO_PROCESS as _, 0, 10);
            Ok(())
        });
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        command
            .as_std_mut()
            .creation_flags(CREATE_NEW_PROCESS_GROUP | BELOW_NORMAL_PRIORITY_CLASS);
    }
    // Windows processes need SystemRoot to start at all; it is a bootstrap
    // requirement of the OS, not user data.
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return run(Outcome::NotStarted, None, Captured::default()),
    };
    let tree = Tree::new(&child);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(manifest.timeout_seconds);
    let captured = Mutex::new(Captured::default());
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let io = async {
        tokio::join!(drain_keep(stdout, &captured), drain_drop(stderr));
    };
    let mut io = std::pin::pin!(io);
    let mut io_done = false;
    let waited = loop {
        tokio::select! {
            status = child.wait() => break Waited::Exited(status),
            () = &mut io, if !io_done => io_done = true,
            () = tokio::time::sleep_until(deadline) => break Waited::TimedOut,
        }
    };
    let (outcome, exit_code) = match waited {
        Waited::Exited(Ok(status)) => {
            // Orphans the extension left behind hold no pipes for long.
            tree.kill();
            if !io_done {
                // Output still arriving is read until the time limit only.
                let _ = tokio::time::timeout_at(deadline, &mut io).await;
            }
            match status.code() {
                Some(code) => (Outcome::Exited, Some(code)),
                None => (Outcome::Terminated, None),
            }
        }
        Waited::Exited(Err(_)) => {
            tree.kill();
            let _ = child.kill().await;
            (Outcome::Terminated, None)
        }
        Waited::TimedOut => {
            tree.kill();
            let _ = child.kill().await;
            (Outcome::TimedOut, None)
        }
    };
    let duration = started.elapsed();
    let captured = std::mem::take(&mut *captured.lock().unwrap_or_else(PoisonError::into_inner));
    Run {
        duration,
        ..run(outcome, exit_code, captured)
    }
}

/// Read `pipe` to its end, keeping what fits in the capture.
async fn drain_keep(pipe: Option<impl AsyncRead + Unpin>, captured: &Mutex<Captured>) {
    let Some(mut pipe) = pipe else {
        return;
    };
    let mut buffer = [0u8; 4096];
    loop {
        match tokio::io::AsyncReadExt::read(&mut pipe, &mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => captured
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(&buffer[..read]),
        }
    }
}

/// Read `pipe` to its end, discarding everything: `stderr` never reaches a
/// report, a log or an error, but must be drained so a verbose extension
/// never deadlocks on a full pipe.
async fn drain_drop(pipe: Option<impl AsyncRead + Unpin>) {
    let Some(mut pipe) = pipe else {
        return;
    };
    let mut buffer = [0u8; 4096];
    while matches!(
        tokio::io::AsyncReadExt::read(&mut pipe, &mut buffer).await,
        Ok(read) if read > 0
    ) {}
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
