//! "Start at login" (PRD 0022, audit #37), through the official `tauri-plugin-autostart`.
//!
//! The operating system owns the state (a LaunchAgent on macOS, the `Run` registry key on Windows, an XDG
//! autostart entry on Linux); nothing about it is kept in `desktop.json`, so the switch always shows what the
//! system will do. The web UI never calls the plugin: it asks the two commands below, so no `autostart:`
//! capability permission is granted to any window. An entry started by the system carries [`ARG`], and the
//! app then stays in the tray without opening its main window.
//!
//! Errors are the code `autostart_failed`; the operating system's own message (which can carry paths) is
//! never passed on.

use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

/// The argument the login entry starts the app with.
pub const ARG: &str = "--autostart";

/// The error code the UI translates.
pub const FAILED: &str = "autostart_failed";

/// Whether the process was started by the login entry (and so should stay in the tray).
pub fn launched_at_login<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|a| a.as_ref() == ARG)
}

/// What the plugin offers, as the module needs it (a fake stands in for it in the tests).
pub trait Login {
    fn is_enabled(&self) -> Result<bool, String>;
    fn enable(&self) -> Result<(), String>;
    fn disable(&self) -> Result<(), String>;
}

impl<R: Runtime> Login for AppHandle<R> {
    fn is_enabled(&self) -> Result<bool, String> {
        self.autolaunch().is_enabled().map_err(|e| e.to_string())
    }
    fn enable(&self) -> Result<(), String> {
        self.autolaunch().enable().map_err(|e| e.to_string())
    }
    fn disable(&self) -> Result<(), String> {
        self.autolaunch().disable().map_err(|e| e.to_string())
    }
}

/// The current state, as the system reports it.
pub fn status(login: &dyn Login) -> Result<bool, String> {
    login.is_enabled().map_err(|_| FAILED.to_owned())
}

/// Makes the system entry match `on` (touching it only when it differs) and returns what the system reports
/// afterwards, so a refusal is visible instead of assumed.
pub fn apply(login: &dyn Login, on: bool) -> Result<bool, String> {
    let now = status(login)?;
    if now != on {
        let done = if on { login.enable() } else { login.disable() };
        done.map_err(|_| FAILED.to_owned())?;
    }
    status(login)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Fake {
        on: Cell<bool>,
        calls: RefCell<Vec<&'static str>>,
        fail: Cell<bool>,
        ignore: Cell<bool>,
    }

    impl Login for Fake {
        fn is_enabled(&self) -> Result<bool, String> {
            Ok(self.on.get())
        }
        fn enable(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("enable");
            if self.fail.get() {
                return Err("/Users/someone/Library/LaunchAgents: denied".into());
            }
            self.on.set(!self.ignore.get());
            Ok(())
        }
        fn disable(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("disable");
            self.on.set(false);
            Ok(())
        }
    }

    #[test]
    fn the_login_argument_is_recognised_only_exactly() {
        assert!(launched_at_login(["Cuotai", "--autostart"]));
        assert!(!launched_at_login(["Cuotai"]));
        assert!(!launched_at_login(["Cuotai", "--autostart=1", "autostart"]));
    }

    #[test]
    fn enabling_and_disabling_touch_the_system_once_and_report_its_state() {
        let fake = Fake::default();
        assert_eq!(apply(&fake, true), Ok(true));
        assert_eq!(apply(&fake, true), Ok(true));
        assert_eq!(apply(&fake, false), Ok(false));
        assert_eq!(apply(&fake, false), Ok(false));
        assert_eq!(*fake.calls.borrow(), ["enable", "disable"]);
    }

    #[test]
    fn a_refusal_is_a_code_without_the_system_message() {
        let fake = Fake::default();
        fake.fail.set(true);
        assert_eq!(apply(&fake, true), Err(FAILED.to_owned()));
    }

    #[test]
    fn a_system_that_ignores_the_request_is_reported_as_it_is() {
        let fake = Fake::default();
        fake.ignore.set(true);
        assert_eq!(apply(&fake, true), Ok(false));
    }
}
