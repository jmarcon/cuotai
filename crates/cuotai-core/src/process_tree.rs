//! The process tree of a child Cuotai starts on the user's behalf (an
//! extension, PRD 0017 RF3.3, or a hook, PRD 0013 RF3.4), killed together:
//! on Unix the child leads its own process group (`process_group(0)`),
//! killed with `killpg`; on Windows it is assigned to a kill-on-close Job
//! Object. [`own_group`] prepares the command, [`Tree::new`] takes the
//! spawned child, and dropping the [`Tree`] kills whatever is left of it,
//! so a timed-out, cancelled or finished child never leaves grandchildren
//! running outside the caller's concurrency limit.
//!
//! Best effort by nature: a grandchild that starts its own session or
//! process group (`setsid`) on Unix, or one spawned on Windows before the
//! child was assigned to the Job Object, escapes the kill. The direct
//! child stays covered by the caller's `kill_on_drop`.

use tokio::process::Command;

/// Make the child of `command` lead its own process group (Unix) or new
/// process group (Windows), so [`Tree`] can kill it with everything it
/// started. `low_priority` also lowers its scheduling priority (niceness
/// 10 on Unix, below-normal priority class on Windows), best effort.
pub(crate) fn own_group(command: &mut Command, low_priority: bool) {
    #[cfg(unix)]
    {
        command.process_group(0);
        if low_priority {
            use std::os::unix::process::CommandExt;
            // SAFETY: `setpriority` is async-signal-safe and touches only
            // the child being started.
            unsafe {
                command.as_std_mut().pre_exec(|| {
                    // Best effort: the run matters more than the niceness.
                    libc::setpriority(libc::PRIO_PROCESS as _, 0, 10);
                    Ok(())
                });
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        let priority = if low_priority {
            BELOW_NORMAL_PRIORITY_CLASS
        } else {
            0
        };
        command
            .as_std_mut()
            .creation_flags(CREATE_NEW_PROCESS_GROUP | priority);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (command, low_priority);
    }
}

/// The whole process tree of a child started with [`own_group`], killed
/// together (best effort) by [`Self::kill`] and on drop.
pub(crate) struct Tree {
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
        // SAFETY: plain Win32 calls on handles this function owns or that
        // `child` keeps alive for the call; `limits` is a fully initialized
        // struct passed with its own size, and a job that fails to configure
        // is closed before it is dropped on the floor.
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
        // SAFETY: `self.0` is the job handle `new` created and still owns.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        self.kill();
        // SAFETY: the handle is owned by `self` and closed exactly once, here.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

impl Tree {
    pub(crate) fn new(child: &tokio::process::Child) -> Self {
        #[cfg(unix)]
        {
            // The child leads its own process group (`own_group`), so its
            // id is the group id.
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
    pub(crate) fn kill(&self) {
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
