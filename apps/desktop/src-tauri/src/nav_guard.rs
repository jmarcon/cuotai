//! PRD 0022 re-validation S-04: the app's own windows stay on the bundle.
//!
//! The main window, the tray popover and Settings only ever show the bundled
//! pages; every outside address opens in the browser through the shell's
//! allow-list (`about::open_external`). So a top-level navigation of one of
//! them is allowed only to the app origin, and a new window (`window.open`,
//! `target="_blank"`) is always refused. This is defense in depth: should
//! markup ever be injected into a page, a link in it cannot take the window
//! away from the bundle.
//!
//! The session-capture windows load a provider's site and keep their own
//! handlers (`session_capture.rs`); this guard leaves every other label alone.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::webview::NewWindowResponse;
use tauri::{Runtime, Url};

/// The tray popover's label (declared in `tauri.conf.json`).
pub const TRAY: &str = "tray";
/// The windows this guard keeps on the bundle.
pub const GUARDED: &[&str] = &[
    crate::main_window::LABEL,
    TRAY,
    crate::settings_window::LABEL,
];

/// Whether `url` is the bundle's own origin: `tauri://localhost` (macOS and
/// Linux) or `http(s)://tauri.localhost` (Windows). `about:blank` is allowed
/// too, since a webview may pass through it while it starts.
pub fn is_app_origin(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost") && url.port().is_none(),
        "http" | "https" => url.host_str() == Some("tauri.localhost") && url.port().is_none(),
        "about" => url.path() == "blank",
        _ => false,
    }
}

/// The navigation decision for the window labelled `label`.
pub fn navigation_allowed(label: &str, url: &Url) -> bool {
    !GUARDED.contains(&label) || is_app_origin(url)
}

/// The global navigation guard, registered once on the app builder. It sees
/// the windows `tauri.conf.json` creates as well as those built in code.
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("cuotai-nav-guard")
        .on_navigation(|webview, url| navigation_allowed(webview.label(), url))
        .build()
}

/// The answer to a new-window request (`window.open`, `target="_blank"`)
/// from the window labelled `label`: always refused for a guarded window,
/// whose links go through `open_external` instead.
pub fn new_window_response<R: Runtime>(label: &str) -> NewWindowResponse<R> {
    if GUARDED.contains(&label) {
        NewWindowResponse::Deny
    } else {
        NewWindowResponse::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("url")
    }

    // security-invariant: SEC-WEBVIEW-NAV
    #[test]
    fn the_app_windows_navigate_only_to_the_bundle() {
        for allowed in [
            "tauri://localhost/index.html",
            "tauri://localhost/tray.html#stats",
            "http://tauri.localhost/settings.html",
            "https://tauri.localhost/index.html?x=1",
            "about:blank",
        ] {
            for label in GUARDED {
                assert!(
                    navigation_allowed(label, &url(allowed)),
                    "{label} {allowed}"
                );
            }
        }
        for refused in [
            "https://example.com/",
            "http://localhost:6738/",
            "tauri://example.com/index.html",
            "https://tauri.localhost.example.com/",
            "https://tauri.localhost:8443/",
            "http://evil.tauri.localhost/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<b>x</b>",
            "about:srcdoc",
        ] {
            for label in GUARDED {
                assert!(
                    !navigation_allowed(label, &url(refused)),
                    "{label} {refused}"
                );
            }
        }
    }

    // security-invariant: SEC-WEBVIEW-NAV
    #[test]
    fn other_windows_keep_their_own_handlers() {
        let site = url("https://example.com/login");
        assert!(navigation_allowed(crate::session_capture::LABEL, &site));
        assert!(navigation_allowed(
            crate::session_capture::SHARED_LABEL,
            &site
        ));
        // A label close to a guarded one is not guarded by accident, and a guarded one is matched exactly.
        assert!(navigation_allowed("Main", &site));
        assert!(!navigation_allowed("main", &site));
    }

    // security-invariant: SEC-WEBVIEW-NAV
    #[test]
    fn a_guarded_window_never_opens_a_new_window() {
        type R = tauri::test::MockRuntime;
        for label in GUARDED {
            assert!(
                matches!(new_window_response::<R>(label), NewWindowResponse::Deny),
                "{label}"
            );
        }
        assert!(matches!(
            new_window_response::<R>(crate::session_capture::LABEL),
            NewWindowResponse::Allow
        ));
    }
}
