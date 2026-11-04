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

use std::path::{Path, PathBuf};

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

/// PRD 0054 RF4.3: rewrite an existing entry so it starts this copy of the app (after the app or its
/// AppImage moved). An entry the user removed in the system's settings stays removed: without one this
/// changes nothing and reports `false`.
pub fn repair(login: &dyn Login) -> Result<bool, String> {
    if !status(login)? {
        return Ok(false);
    }
    login.disable().map_err(|_| FAILED.to_owned())?;
    login.enable().map_err(|_| FAILED.to_owned())?;
    status(login)
}

/// PRD 0054 RF4.3: whether the login entry starts a program that is gone or is not this copy of the app.
/// `registered` is the program the entry names (`None`: no entry, or one Cuotai cannot read, as on
/// Windows), `expected` the one the plugin would write now.
pub fn is_stale(
    registered: Option<&Path>,
    expected: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> bool {
    let Some(registered) = registered else {
        return false;
    };
    !exists(registered) || expected.is_some_and(|expected| expected != registered)
}

/// The program the plugin writes into the entry: the app's executable (resolved through links on
/// macOS), or the AppImage file on Linux when the app runs from one.
pub fn expected_target(appimage: Option<PathBuf>) -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        if let Some(appimage) = appimage {
            return Some(appimage);
        }
    }
    let exe = std::env::current_exe().ok()?;
    if cfg!(target_os = "macos") {
        exe.canonicalize().ok()
    } else {
        Some(exe)
    }
}

/// The program the entry named `app_name` starts, read from the file the plugin writes: the LaunchAgent
/// plist on macOS, the XDG autostart file on Linux. `None` on Windows (the `Run` registry value is not
/// read) and when there is no readable entry.
pub fn registered_target(app_name: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    if cfg!(target_os = "macos") {
        let file = home
            .join("Library/LaunchAgents")
            .join(format!("{app_name}.plist"));
        plist_program(&std::fs::read_to_string(file).ok()?).map(PathBuf::from)
    } else if cfg!(target_os = "linux") {
        let file = home
            .join(".config/autostart")
            .join(format!("{app_name}.desktop"));
        desktop_exec(&std::fs::read_to_string(file).ok()?).map(PathBuf::from)
    } else {
        None
    }
}

/// The first `ProgramArguments` string of a LaunchAgent plist.
fn plist_program(text: &str) -> Option<String> {
    let after = &text[text.find("<key>ProgramArguments</key>")?..];
    let array = &after[after.find("<array>")? + "<array>".len()..after.find("</array>")?];
    let start = array.find("<string>")? + "<string>".len();
    let end = start + array[start..].find("</string>")?;
    let value = array[start..end]
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    (!value.is_empty()).then_some(value)
}

/// The program of an XDG autostart file's `Exec=` line, as the plugin writes it (`Exec=<path> --autostart`,
/// the path unquoted and possibly with spaces).
fn desktop_exec(text: &str) -> Option<String> {
    let line = text.lines().find_map(|line| line.strip_prefix("Exec="))?;
    let line = line.trim_end();
    let program = line
        .strip_suffix(ARG)
        .map(str::trim_end)
        .unwrap_or(line)
        .trim_matches('"');
    (!program.is_empty()).then(|| program.to_owned())
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

    #[test]
    fn repair_rewrites_an_existing_entry_and_never_creates_a_removed_one() {
        let fake = Fake::default();
        assert_eq!(repair(&fake), Ok(false));
        assert!(fake.calls.borrow().is_empty(), "nothing re-enabled");
        fake.on.set(true);
        assert_eq!(repair(&fake), Ok(true));
        assert_eq!(*fake.calls.borrow(), ["disable", "enable"]);
        fake.fail.set(true);
        assert_eq!(repair(&fake), Err(FAILED.to_owned()));
    }

    #[test]
    fn an_entry_is_stale_when_its_program_moved_or_is_gone() {
        let here = Path::new("/Applications/Cuotai.app/Contents/MacOS/Cuotai");
        let moved = Path::new("/Users/someone/Downloads/Cuotai.app/Contents/MacOS/Cuotai");
        let present = |p: &Path| p == here;
        assert!(!is_stale(None, Some(here), present), "no entry");
        assert!(!is_stale(Some(here), Some(here), present));
        assert!(is_stale(Some(moved), Some(here), |_| true), "another copy");
        assert!(is_stale(Some(moved), Some(here), present), "gone");
        assert!(
            is_stale(Some(here), None, |_| false),
            "gone, nothing expected"
        );
        assert!(!is_stale(Some(here), None, present));
    }

    #[test]
    fn the_program_is_read_from_the_entries_the_plugin_writes() {
        let plist = "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n  <dict>\n  <key>Label</key>\n  <string>Cuotai</string>\n  <key>AssociatedBundleIdentifiers</key>\n  <array><string>dev.cuotai.desktop</string></array>\n  <key>ProgramArguments</key>\n  <array><string>/Applications/R&amp;D/Cuotai.app/Contents/MacOS/Cuotai</string><string>--autostart</string></array>\n  <key>RunAtLoad</key>\n  <true/>\n  </dict>\n</plist>";
        assert_eq!(
            plist_program(plist).as_deref(),
            Some("/Applications/R&D/Cuotai.app/Contents/MacOS/Cuotai")
        );
        assert_eq!(plist_program("<plist><dict></dict></plist>"), None);
        let desktop = "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Cuotai\nComment=Cuotai startup script\nExec=/home/someone/Apps/My Apps/Cuotai.AppImage --autostart\nStartupNotify=false\nTerminal=false";
        assert_eq!(
            desktop_exec(desktop).as_deref(),
            Some("/home/someone/Apps/My Apps/Cuotai.AppImage")
        );
        assert_eq!(
            desktop_exec("Exec=\"/usr/bin/cuotai-desktop\"").as_deref(),
            Some("/usr/bin/cuotai-desktop")
        );
        assert_eq!(desktop_exec("[Desktop Entry]\nName=Cuotai"), None);
    }
}
