//! PRD 0020 re-validation S-01: the windows a destructive command may come
//! from.
//!
//! The three webviews (`main`, `tray`, `settings`) share one capability, so
//! Tauri lets each of them invoke every command. A destructive command whose
//! confirmation lives in its own window's markup therefore checks the label
//! of the window that called it, first, before it touches anything:
//!
//! - the account dialogs live in the main window ([`MAIN`]);
//! - a restore is started from the main window or Settings
//!   ([`MAIN_OR_SETTINGS`]);
//! - every destructive action of a Settings page is Settings-only
//!   ([`SETTINGS_ONLY`]).
//!
//! The tray popover may call none of them. A refusal is the fixed,
//! translated `desktop.error.wrong_window` error, and nothing is done.

use tauri::{Runtime, WebviewWindow};

/// The main window's label.
pub const MAIN: &str = crate::main_window::LABEL;
/// The Settings window's label.
pub const SETTINGS: &str = crate::settings_window::LABEL;
/// The tray popover's label (never allowed here; named for the tests).
#[cfg(test)]
pub const TRAY: &str = "tray";

/// Commands of the account dialogs.
pub const MAIN_ONLY: &[&str] = &[MAIN];
/// Commands of a Settings page.
pub const SETTINGS_ONLY: &[&str] = &[SETTINGS];
/// Restore, offered by the main window and by Settings › Files.
pub const MAIN_OR_SETTINGS: &[&str] = &[MAIN, SETTINGS];

/// `Ok` when `label` is one of `allowed`; otherwise the fixed refusal.
pub fn check(label: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&label) {
        Ok(())
    } else {
        Err(crate::desk_text::wrong_window())
    }
}

/// [`check`] for the window that invoked the command.
pub fn require<R: Runtime>(window: &WebviewWindow<R>, allowed: &[&str]) -> Result<(), String> {
    check(window.label(), allowed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // security-invariant: SEC-WINDOW-SCOPE
    #[test]
    fn each_scope_admits_only_its_windows_and_never_the_tray() {
        for (allowed, admitted) in [
            (MAIN_ONLY, &[MAIN][..]),
            (SETTINGS_ONLY, &[SETTINGS][..]),
            (MAIN_OR_SETTINGS, &[MAIN, SETTINGS][..]),
        ] {
            for label in [MAIN, SETTINGS, TRAY, "", "settings ", "Main"] {
                let result = check(label, allowed);
                assert_eq!(
                    result.is_ok(),
                    admitted.contains(&label),
                    "{label:?} in {allowed:?}"
                );
                if let Err(e) = result {
                    assert!(e.contains("desktop.error.wrong_window"), "{e}");
                }
            }
        }
    }

    // security-invariant: SEC-WINDOW-SCOPE
    #[test]
    fn a_window_that_is_not_allowed_is_refused_by_its_label() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        use tauri::{WebviewUrl, WebviewWindowBuilder};
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("test app");
        let tray = WebviewWindowBuilder::new(&app, TRAY, WebviewUrl::default())
            .build()
            .expect("tray window");
        let settings = WebviewWindowBuilder::new(&app, SETTINGS, WebviewUrl::default())
            .build()
            .expect("settings window");
        for allowed in [MAIN_ONLY, SETTINGS_ONLY, MAIN_OR_SETTINGS] {
            assert!(require(&tray, allowed).is_err(), "{allowed:?}");
        }
        assert!(require(&settings, SETTINGS_ONLY).is_ok());
        assert!(require(&settings, MAIN_OR_SETTINGS).is_ok());
        assert!(require(&settings, MAIN_ONLY).is_err());
    }
}
