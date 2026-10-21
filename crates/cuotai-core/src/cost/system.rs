//! RF1.4/RF2.2: what the scan asks of the operating system: whether the
//! computer is saving energy (the worker then pauses), a background
//! priority for the scanning thread (lower CPU and disk I/O), and on Unix
//! the user the process runs as (only that user's files are read). Nothing
//! here elevates privileges or works around a refused access.

/// Whether the computer is in an energy-saving mode: Low Power Mode on
/// macOS (`pmset -g`), the `low-power` platform profile on Linux, battery
/// saver on Windows. Unknown counts as not saving.
pub fn energy_saving() -> bool {
    imp::energy_saving()
}

/// Put the calling thread in the background: on macOS the darwin
/// background band (throttled CPU and I/O), on Linux the lowest nice value
/// of the thread (its I/O priority follows), on Windows the background
/// processing mode. Other systems keep the default; the scan still paces
/// itself.
pub(crate) fn lower_thread_priority() {
    imp::lower_thread_priority();
}

/// Whether `pmset -g` reports Low Power Mode on.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn pmset_saving(output: &str) -> bool {
    output.lines().any(|line| {
        let mut words = line.split_whitespace();
        matches!(
            (words.next(), words.next()),
            (Some("lowpowermode"), Some("1")) | (Some("powermode"), Some("1"))
        )
    })
}

/// Whether a Linux `platform_profile` asks for low power.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn profile_saving(profile: &str) -> bool {
    profile.trim() == "low-power"
}

#[cfg(unix)]
pub(crate) fn effective_uid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: `geteuid` takes no arguments, cannot fail and only reads the
    // process credentials.
    unsafe { geteuid() }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Command, Stdio};

    pub fn energy_saving() -> bool {
        Command::new("/usr/bin/pmset")
            .arg("-g")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .is_some_and(|out| super::pmset_saving(&String::from_utf8_lossy(&out.stdout)))
    }

    pub fn lower_thread_priority() {
        const PRIO_DARWIN_THREAD: i32 = 3;
        const PRIO_DARWIN_BG: i32 = 0x1000;
        extern "C" {
            fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        }
        // SAFETY: plain integers; `who = 0` is the calling thread. A
        // failure leaves the default priority.
        unsafe {
            setpriority(PRIO_DARWIN_THREAD, 0, PRIO_DARWIN_BG);
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub fn energy_saving() -> bool {
        std::fs::read_to_string("/sys/firmware/acpi/platform_profile")
            .is_ok_and(|profile| super::profile_saving(&profile))
    }

    pub fn lower_thread_priority() {
        const PRIO_PROCESS: i32 = 0;
        extern "C" {
            fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        }
        // SAFETY: plain integers; on Linux the nice value is per thread and
        // `who = 0` is the calling one. A failure leaves the default.
        unsafe {
            setpriority(PRIO_PROCESS, 0, 19);
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    /// SYSTEM_POWER_STATUS; only `system_status_flag` is read.
    #[repr(C)]
    #[allow(dead_code)]
    struct SystemPowerStatus {
        ac_line_status: u8,
        battery_flag: u8,
        battery_life_percent: u8,
        system_status_flag: u8,
        battery_life_time: u32,
        battery_full_life_time: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
        fn GetCurrentThread() -> *mut c_void;
        fn SetThreadPriority(thread: *mut c_void, priority: i32) -> i32;
    }

    pub fn energy_saving() -> bool {
        let mut status = SystemPowerStatus {
            ac_line_status: 0,
            battery_flag: 0,
            battery_life_percent: 0,
            system_status_flag: 0,
            battery_life_time: 0,
            battery_full_life_time: 0,
        };
        // SAFETY: `status` is a valid, writable SYSTEM_POWER_STATUS.
        let ok = unsafe { GetSystemPowerStatus(&mut status) } != 0;
        // SystemStatusFlag 1: battery saver is on.
        ok && status.system_status_flag == 1
    }

    pub fn lower_thread_priority() {
        const THREAD_MODE_BACKGROUND_BEGIN: i32 = 0x0001_0000;
        // SAFETY: the pseudo-handle of the calling thread needs no closing.
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    pub fn energy_saving() -> bool {
        false
    }

    pub fn lower_thread_priority() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_power_mode_is_read_from_pmset_and_the_platform_profile() {
        let on =
            "System-wide power settings:\nCurrently in use:\n lowpowermode         1\n sleep 1\n";
        let off = "Currently in use:\n lowpowermode         0\n powermode            2\n";
        assert!(pmset_saving(on));
        assert!(pmset_saving(" powermode 1\n"));
        assert!(!pmset_saving(off));
        assert!(!pmset_saving(""));
        assert!(profile_saving("low-power\n"));
        assert!(!profile_saving("balanced\n"));
    }
}
