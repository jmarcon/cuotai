//! PRD 0054 RF5: a window-only fallback when the tray cannot be created, and
//! a visible startup failure.
//!
//! The tray icon is built apart from the rest of the setup. When the system
//! refuses it (GNOME without an AppIndicator extension, for example) the app
//! continues in [`WindowMode::NoTray`]: the main window is shown and stays in
//! the Dock or taskbar, a banner that cannot be dismissed says why there is
//! no tray icon, and closing the window quits instead of hiding a process
//! nobody could reach. A launch by the login entry keeps the window hidden
//! only while the tray works (RF4.2).
//!
//! A failure that stops the app altogether (RF5.3) goes through
//! `startup_error` (PRD 0052 RF2.2): logged, shown in a native dialog that
//! names the log, and a nonzero exit.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the tray icon exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    /// The tray icon was created: closing the window hides it.
    Tray,
    /// No tray icon: the window is the only way in, and closing it quits.
    NoTray,
}

static NO_TRAY: AtomicBool = AtomicBool::new(false);

/// Record the mode chosen at setup.
pub fn set(mode: WindowMode) {
    NO_TRAY.store(mode == WindowMode::NoTray, Ordering::SeqCst);
}

/// The mode chosen at setup ([`WindowMode::Tray`] before it).
pub fn current() -> WindowMode {
    if NO_TRAY.load(Ordering::SeqCst) {
        WindowMode::NoTray
    } else {
        WindowMode::Tray
    }
}

/// RF4.2/RF5.2: whether the main window is shown when setup ends: always,
/// except for a launch by the login entry while the tray works.
pub fn show_main_at_start(mode: WindowMode, launched_at_login: bool) -> bool {
    mode == WindowMode::NoTray || !launched_at_login
}

/// RF5.1: whether closing the main window quits the app (otherwise it only
/// hides).
pub fn close_quits(mode: WindowMode) -> bool {
    mode == WindowMode::NoTray
}

/// RF5.1: whether the app must show a Dock icon whatever the preference
/// (macOS): without a tray it is the only way back to a hidden window.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn force_dock(mode: WindowMode) -> bool {
    mode == WindowMode::NoTray
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_launch_stays_hidden_only_with_a_tray() {
        assert!(show_main_at_start(WindowMode::Tray, false));
        assert!(!show_main_at_start(WindowMode::Tray, true));
        assert!(show_main_at_start(WindowMode::NoTray, false));
        assert!(show_main_at_start(WindowMode::NoTray, true));
    }

    #[test]
    fn without_a_tray_closing_quits_and_the_dock_icon_stays() {
        assert!(!close_quits(WindowMode::Tray));
        assert!(close_quits(WindowMode::NoTray));
        assert!(!force_dock(WindowMode::Tray));
        assert!(force_dock(WindowMode::NoTray));
    }

    #[test]
    fn the_mode_is_recorded_once_set() {
        assert_eq!(current(), WindowMode::Tray);
        set(WindowMode::NoTray);
        assert_eq!(current(), WindowMode::NoTray);
        set(WindowMode::Tray);
        assert_eq!(current(), WindowMode::Tray);
    }
}
