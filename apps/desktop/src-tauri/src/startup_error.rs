//! PRD 0052 RF2.2 — a startup that fails is reported, never a panic: the
//! sanitized failure goes to the app log, and a platform-native modal
//! (independent of the webview, which may be what failed) says that Cuotai
//! could not start and names the app log. The raw Tauri error is never
//! shown; when the dialog itself cannot be shown, stderr receives the same
//! fixed text.

use std::path::Path;

use cuotai_core::diagnostics::logging::{self, startup_failed, startup_failed_title, Field, Level};

/// What shows the modal. [`Native`] in the app; a fake in tests.
pub trait Dialog {
    /// Show `title`/`text`; `false` when it could not be shown.
    fn show(&self, title: &str, text: &str) -> bool;
}

/// The platform's own message box (`rfd`: AppKit, Win32, GTK/portal).
pub struct Native;

impl Dialog for Native {
    fn show(&self, title: &str, text: &str) -> bool {
        let (title, text) = (title.to_string(), text.to_string());
        std::panic::catch_unwind(move || {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title(title)
                .set_description(text)
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
        })
        .is_ok()
    }
}

/// Report a failed start: one `ERROR` record with the failure's class
/// (never its text), the native modal in `locale`, and stderr when the
/// modal fails. Returns the process status to exit with (always non-zero).
pub fn report(class: &'static str, locale: &str, log: Option<&Path>, dialog: &dyn Dialog) -> u8 {
    logging::event(
        Level::Error,
        "desktop.startup_failed",
        &[("class", Field::Text(class))],
    );
    let (title, text) = (
        startup_failed_title().resolve(locale),
        startup_failed(log).resolve(locale),
    );
    if !dialog.show(&title, &text) {
        eprintln!("{text}");
    }
    1
}

/// The class of a Tauri build/run error: its variant name only.
pub fn class_of(error: &tauri::Error) -> &'static str {
    match error {
        tauri::Error::Setup(_) => "setup",
        tauri::Error::Runtime(_) => "runtime",
        tauri::Error::WebviewNotFound | tauri::Error::WindowNotFound => "window_missing",
        tauri::Error::Io(_) => "io",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        shown: RefCell<Vec<(String, String)>>,
        works: bool,
    }

    impl Dialog for Fake {
        fn show(&self, title: &str, text: &str) -> bool {
            self.shown.borrow_mut().push((title.into(), text.into()));
            self.works
        }
    }

    #[test]
    fn a_failed_start_shows_the_fixed_modal_and_exits_non_zero() {
        let fake = Fake {
            shown: RefCell::new(Vec::new()),
            works: true,
        };
        let log = logging::default_log_path();
        let code = report("setup", "en-US", log.as_deref(), &fake);
        assert_eq!(code, 1);
        let shown = fake.shown.borrow();
        assert_eq!(shown.len(), 1, "one native modal");
        assert_eq!(shown[0].0, "Cuotai could not start");
        assert!(shown[0].1.starts_with("Cuotai could not start."));
        assert!(shown[0].1.contains("cuotai.log"));
        if let Some(home) = dirs::home_dir() {
            assert!(
                !shown[0].1.contains(&*home.to_string_lossy()),
                "home abbreviated"
            );
        }
    }

    #[test]
    fn a_failed_modal_still_reports_and_without_a_log_says_so() {
        let fake = Fake {
            shown: RefCell::new(Vec::new()),
            works: false,
        };
        assert_eq!(report("other", "en-US", None, &fake), 1);
        assert!(fake.shown.borrow()[0].1.contains("no app log"));
    }

    #[test]
    fn the_class_never_carries_the_error_text() {
        let error = tauri::Error::Setup(
            Box::<dyn std::error::Error>::from("Bearer sk-secret-token-value").into(),
        );
        assert_eq!(class_of(&error), "setup");
    }
}
