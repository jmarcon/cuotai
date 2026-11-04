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
//! A failure that stops the app altogether is shown in a native dialog
//! ([`startup_failed`]) with a fixed, translated text and its category, and
//! the process exits nonzero; the error itself (which can carry paths) is
//! never shown.

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

/// RF5.3: the fixed category of a fatal startup failure, shown in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The windowing system, the web view or the setup could not start.
    Start,
}

impl Failure {
    fn code(self) -> &'static str {
        match self {
            Failure::Start => "startup",
        }
    }
}

/// The UI catalog of `locale` (compiled in), for the dialog's text before
/// any web view exists.
fn catalog(locale: &str) -> &'static str {
    match locale {
        "pt-BR" => include_str!("../../ui/locales/pt-BR.json"),
        "es" => include_str!("../../ui/locales/es.json"),
        "it" => include_str!("../../ui/locales/it.json"),
        "fr" => include_str!("../../ui/locales/fr.json"),
        "de" => include_str!("../../ui/locales/de.json"),
        "ja" => include_str!("../../ui/locales/ja.json"),
        "ko" => include_str!("../../ui/locales/ko.json"),
        "zh-CN" => include_str!("../../ui/locales/zh-CN.json"),
        "hi-IN" => include_str!("../../ui/locales/hi-IN.json"),
        "id-ID" => include_str!("../../ui/locales/id-ID.json"),
        "ru" => include_str!("../../ui/locales/ru.json"),
        "tr" => include_str!("../../ui/locales/tr.json"),
        "vi" => include_str!("../../ui/locales/vi.json"),
        _ => include_str!("../../ui/locales/en-US.json"),
    }
}

/// The dialog's title and text in `locale` (English for an unknown one).
pub fn startup_text(locale: Option<&str>, failure: Failure) -> (String, String) {
    let english: serde_json::Value = serde_json::from_str(catalog("en-US")).unwrap_or_default();
    let chosen: serde_json::Value =
        serde_json::from_str(catalog(locale.unwrap_or("en-US"))).unwrap_or_default();
    let text = |key: &str| {
        chosen[key]
            .as_str()
            .or_else(|| english[key].as_str())
            .unwrap_or(key)
            .to_owned()
    };
    let body = text("startup.failed").replace("{category}", failure.code());
    (text("startup.failedTitle"), body)
}

/// RF5.3: show the native dialog for a fatal failure and exit nonzero.
pub fn startup_failed(failure: Failure) -> ! {
    let locale = crate::settings::settings_path()
        .as_deref()
        .and_then(crate::settings::read_locale);
    let (title, body) = startup_text(locale.as_deref(), failure);
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title.as_str())
        .set_description(body.as_str())
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    std::process::exit(1)
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

    #[test]
    fn the_startup_dialog_is_translated_with_a_fixed_category() {
        let (title, body) = startup_text(None, Failure::Start);
        assert_eq!(title, "Cuotai could not start");
        assert!(body.contains("startup") && !body.contains('{'), "{body}");
        for locale in [
            "pt-BR", "es", "it", "fr", "de", "ja", "ko", "zh-CN", "hi-IN", "id-ID", "ru", "tr",
            "vi",
        ] {
            let (title, body) = startup_text(Some(locale), Failure::Start);
            assert_ne!(title, "Cuotai could not start", "{locale}");
            assert!(
                body.contains("startup") && !body.contains('{'),
                "{locale}: {body}"
            );
        }
        // An unknown language reads English.
        assert_eq!(
            startup_text(Some("xx"), Failure::Start).0,
            "Cuotai could not start"
        );
    }
}
