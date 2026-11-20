//! PRD 0056 RF3 (PRD 0048 S-3): erase every kind of website data but
//! cookies from a capture window's store. A parked shared Google window
//! keeps Google's cookies (its sign-in) and nothing else: the cookie passes
//! of `session_capture::park` delete the other sites' cookies, and this
//! module erases local storage, IndexedDB, caches, service workers and the
//! rest, for every site, Google included.
//!
//! The erase is store-wide by data type, the way wry builds its own
//! `clear_all_browsing_data`, through the platform webview that Tauri's
//! `with_webview` hands over:
//! - macOS: `WKWebsiteDataStore.removeDataOfTypes:modifiedSince:` with
//!   every type of `allWebsiteDataTypes` except `WKWebsiteDataTypeCookies`;
//! - Linux: `WebsiteDataManager::clear` of the web view's (ephemeral)
//!   context with `WebsiteDataTypes::ALL` minus `COOKIES`;
//! - Windows: `ICoreWebView2Profile2::ClearBrowsingData` with every
//!   individual data kind except `COOKIES` (the aggregate kinds `ALL_SITE`
//!   and `ALL_PROFILE` are single flags that include cookies, so they are
//!   never used).
//!
//! [`erase_all_but_cookies`] waits for the platform's completion for at
//! most [`ERASE_TIMEOUT`]; an error, a time-out or a completion that never
//! comes reads as a failure, and the caller then destroys the window (fail
//! closed).

use std::time::Duration;
use tauri::{Runtime, WebviewWindow};

/// How long the erase may take before it counts as failed.
pub const ERASE_TIMEOUT: Duration = Duration::from_secs(5);

/// The completion: `true` when the platform reported success. Dropping it
/// uncalled counts as a failure.
pub type Done = Box<dyn FnOnce(bool) + Send + 'static>;

/// A runtime whose windows can erase their website data but cookies.
pub trait SiteData: Runtime {
    /// Start the erase in `window`'s store; `done` receives the result.
    /// `Err` when it could not even start (`done` is then dropped).
    fn start_erase_all_but_cookies(window: &WebviewWindow<Self>, done: Done) -> Result<(), ()>;
}

/// Erase every website data type but cookies from `window`'s store and wait
/// for it (at most [`ERASE_TIMEOUT`]); `false` on any failure.
pub async fn erase_all_but_cookies<R: SiteData>(window: &WebviewWindow<R>) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let done: Done = Box::new(move |ok| {
        let _ = tx.send(ok);
    });
    if R::start_erase_all_but_cookies(window, done).is_err() {
        return false;
    }
    matches!(tokio::time::timeout(ERASE_TIMEOUT, rx).await, Ok(Ok(true)))
}

impl SiteData for tauri::Wry {
    fn start_erase_all_but_cookies(window: &WebviewWindow<Self>, done: Done) -> Result<(), ()> {
        window
            .with_webview(move |webview| platform::erase(webview, done))
            .map_err(|_| ())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Done;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSDate, NSSet, NSString};
    use objc2_web_kit::{WKWebView, WKWebsiteDataStore, WKWebsiteDataTypeCookies};
    use std::cell::Cell;

    /// The types of `all` to erase: every one but `cookies`.
    pub(super) fn erased_types(
        all: Vec<Retained<NSString>>,
        cookies: &NSString,
    ) -> Vec<Retained<NSString>> {
        all.into_iter()
            .filter(|kind| !kind.isEqualToString(cookies))
            .collect()
    }

    pub(super) fn erase(webview: tauri::webview::PlatformWebview, done: Done) {
        // `with_webview` runs this on the main thread.
        let Some(mtm) = MainThreadMarker::new() else {
            return done(false);
        };
        let view = webview.inner().cast::<WKWebView>();
        if view.is_null() {
            return done(false);
        }
        // SAFETY: `inner` is the window's live WKWebView, used on the main
        // thread for the duration of this call; the arguments are the
        // types the selectors expect, and the completion block is retained
        // by WebKit until it runs.
        unsafe {
            let view = &*view;
            let store = view.configuration().websiteDataStore();
            let all = WKWebsiteDataStore::allWebsiteDataTypes(mtm);
            let types =
                NSSet::from_retained_slice(&erased_types(all.to_vec(), WKWebsiteDataTypeCookies));
            let since = NSDate::dateWithTimeIntervalSince1970(0.0);
            let done = Cell::new(Some(done));
            let handler = RcBlock::new(move || {
                if let Some(done) = done.take() {
                    done(true);
                }
            });
            store.removeDataOfTypes_modifiedSince_completionHandler(&types, &since, &handler);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::Done;
    use webkit2gtk::{WebContextExt, WebViewExt, WebsiteDataManagerExtManual, WebsiteDataTypes};

    /// Every website data type but cookies.
    pub(super) fn erased_types() -> WebsiteDataTypes {
        let mut types = WebsiteDataTypes::ALL;
        types.remove(WebsiteDataTypes::COOKIES);
        types
    }

    pub(super) fn erase(webview: tauri::webview::PlatformWebview, done: Done) {
        // Under `incognito` wry gives the web view an ephemeral context;
        // its manager holds the window's whole store.
        let Some(manager) = webview
            .inner()
            .context()
            .and_then(|context| context.website_data_manager())
        else {
            return done(false);
        };
        manager.clear(
            erased_types(),
            webkit2gtk::glib::TimeSpan::from_seconds(0),
            None::<&webkit2gtk::gio::Cancellable>,
            move |result| done(result.is_ok()),
        );
    }
}

#[cfg(windows)]
mod platform {
    use super::Done;
    use webview2_com::ClearBrowsingDataCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Profile2, ICoreWebView2_13, COREWEBVIEW2_BROWSING_DATA_KINDS,
        COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY,
        COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_DOWNLOAD_HISTORY,
        COREWEBVIEW2_BROWSING_DATA_KINDS_FILE_SYSTEMS,
        COREWEBVIEW2_BROWSING_DATA_KINDS_GENERAL_AUTOFILL,
        COREWEBVIEW2_BROWSING_DATA_KINDS_INDEXED_DB,
        COREWEBVIEW2_BROWSING_DATA_KINDS_LOCAL_STORAGE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS,
        COREWEBVIEW2_BROWSING_DATA_KINDS_SETTINGS, COREWEBVIEW2_BROWSING_DATA_KINDS_WEB_SQL,
    };
    use windows_core::Interface;

    /// Every individual data kind but cookies. `ALL_SITE` and
    /// `ALL_PROFILE` are aggregate flags that include cookies, so the
    /// kinds are listed one by one.
    pub(super) fn erased_kinds() -> COREWEBVIEW2_BROWSING_DATA_KINDS {
        COREWEBVIEW2_BROWSING_DATA_KINDS_FILE_SYSTEMS
            | COREWEBVIEW2_BROWSING_DATA_KINDS_INDEXED_DB
            | COREWEBVIEW2_BROWSING_DATA_KINDS_LOCAL_STORAGE
            | COREWEBVIEW2_BROWSING_DATA_KINDS_WEB_SQL
            | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE
            | COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE
            | COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE
            | COREWEBVIEW2_BROWSING_DATA_KINDS_DOWNLOAD_HISTORY
            | COREWEBVIEW2_BROWSING_DATA_KINDS_GENERAL_AUTOFILL
            | COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE
            | COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY
            | COREWEBVIEW2_BROWSING_DATA_KINDS_SETTINGS
            | COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS
    }

    pub(super) fn erase(webview: tauri::webview::PlatformWebview, done: Done) {
        // A failure drops the handler and with it `done` (a failure for
        // the caller).
        let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
            done(result.is_ok());
            Ok(())
        }));
        // SAFETY: COM calls on the window's live WebView2 controller, on
        // the thread `with_webview` runs this on (the UI thread).
        let _ = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_13>())
                .and_then(|core| core.Profile())
                .and_then(|profile| profile.cast::<ICoreWebView2Profile2>())
                .and_then(|profile| profile.ClearBrowsingData(erased_kinds(), &handler))
        };
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod platform {
    use super::Done;

    /// No store-wide erase here: fail closed.
    pub(super) fn erase(_webview: tauri::webview::PlatformWebview, done: Done) {
        done(false);
    }
}

/// Tests: the mock runtime's `with_webview` never runs its closure, so the
/// erase's result comes from the app's [`MockErase`] state (success when
/// absent).
#[cfg(test)]
pub struct MockErase(pub parking_lot::Mutex<Option<bool>>);

#[cfg(test)]
impl SiteData for tauri::test::MockRuntime {
    fn start_erase_all_but_cookies(window: &WebviewWindow<Self>, done: Done) -> Result<(), ()> {
        use tauri::Manager;
        let result = window
            .try_state::<MockErase>()
            .map_or(Some(true), |mock| *mock.0.lock());
        // `None`: the completion never comes (dropped).
        if let Some(ok) = result {
            done(ok);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_erases_every_type_but_cookies() {
        use objc2_foundation::NSString;
        use objc2_web_kit::{
            WKWebsiteDataTypeCookies, WKWebsiteDataTypeIndexedDBDatabases,
            WKWebsiteDataTypeLocalStorage,
        };
        // SAFETY: WebKit's constant strings, linked with the app.
        let (cookies, local, indexed) = unsafe {
            (
                WKWebsiteDataTypeCookies,
                WKWebsiteDataTypeLocalStorage,
                WKWebsiteDataTypeIndexedDBDatabases,
            )
        };
        let all = vec![
            NSString::from_str(&local.to_string()),
            NSString::from_str(&cookies.to_string()),
            NSString::from_str(&indexed.to_string()),
        ];
        let erased = platform::erased_types(all, cookies);
        assert_eq!(erased.len(), 2);
        assert!(erased.iter().all(|kind| !kind.isEqualToString(cookies)));
        assert!(erased.iter().any(|kind| kind.isEqualToString(local)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_erases_every_type_but_cookies() {
        use webkit2gtk::WebsiteDataTypes;
        let erased = platform::erased_types();
        assert!(!erased.contains(WebsiteDataTypes::COOKIES));
        assert!(erased.contains(WebsiteDataTypes::LOCAL_STORAGE));
        assert!(erased.contains(WebsiteDataTypes::INDEXEDDB_DATABASES));
        assert!(erased.contains(WebsiteDataTypes::SERVICE_WORKER_REGISTRATIONS));
    }

    #[cfg(windows)]
    #[test]
    fn windows_erases_every_kind_but_cookies() {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE,
            COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE, COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES,
            COREWEBVIEW2_BROWSING_DATA_KINDS_LOCAL_STORAGE,
            COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS,
        };
        let erased = platform::erased_kinds();
        // The aggregate kinds include cookies: none of them may be set.
        for kind in [
            COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES,
            COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE,
            COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE,
        ] {
            assert_eq!((erased & kind).0, 0);
        }
        assert!(erased.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_LOCAL_STORAGE));
        assert!(erased.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS));
    }
}
