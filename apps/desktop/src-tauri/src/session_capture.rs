//! PRD 0048: "Sign in and capture session". On the user's click, a
//! Cuotai-owned window opens the provider's sign-in page in a private,
//! throw-away cookie store; the user signs in there; Rust polls that
//! store, keeps only the descriptor's cookies and hands them to the
//! account dialog's paste box, which runs the usual preview and test.
//!
//! PRD 0056: a capture whose sign-in offers Google
//! ([`user_session::CaptureSpec::shared`]) runs in one shared window
//! instead, which is hidden and kept alive between captures so the next
//! "Sign in with Google" reuses the Google sign-in. Between captures that
//! window holds only Google's cookies (every other cookie is deleted, or the
//! window is destroyed), shows `about:blank`, and lives at most
//! [`SHARED_IDLE`] without use, until the user forgets it, or until the app
//! quits. Nothing of it is written to disk by Cuotai.
//!
//! Safety properties (RF2–RF5 of PRD 0048):
//! - neither window is listed in `capabilities/default.json` and no
//!   capability has `remote` URLs, so Tauri rejects every IPC call of their
//!   pages;
//! - nothing is injected into the provider's pages (no initialisation
//!   script, no `eval`); the cookies are read from the native store;
//! - navigation is limited to [`SessionDescriptor::capture_allows`] of the
//!   capture in progress (the parked shared window: `about:blank` only);
//!   `window.open` never opens a webview (HTTPS links go to the browser,
//!   except a Google sign-in popup of a shared capture, which opens in the
//!   same window, PRD 0056 RF5), downloads are refused;
//! - the store is non-persistent (`incognito`, plus a fresh data directory
//!   on Windows and Linux) and is cleared and removed when the window ends;
//! - cookie values are never logged nor kept outside the returned paste.

use cuotai_core::user_session::{self, SessionDescriptor, SharedSignIn};
use cuotai_core::Provider;
use parking_lot::Mutex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::webview::NewWindowResponse;
use tauri::{
    AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

/// The private window's label. It must stay out of every capability (see
/// tests).
pub const LABEL: &str = "session-capture";
/// PRD 0056: the shared Google window's label; out of every capability too.
pub const SHARED_LABEL: &str = "session-capture-google";
/// RF6.1: how long the window waits for a finished sign-in.
pub const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// RF4.1: how often the cookie store is read.
pub const POLL: Duration = Duration::from_secs(1);
/// PRD 0056 RF2.3: how long a parked shared window lives without a capture.
pub const SHARED_IDLE: Duration = Duration::from_secs(30 * 60);
/// PRD 0056 RF3: how long a parking window waits after leaving the
/// provider's page before it deletes the cookies (a page being unloaded may
/// still set one).
const PARK_SETTLE: Duration = Duration::from_millis(500);
/// Prefix of the throw-away data directories (Windows and Linux).
const DIR_PREFIX: &str = "cuotai-session-capture-";

/// How a capture ended, as the account dialog reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// The required cookies were found; `paste` is the reduced
    /// `name=value; …` list, for the dialog's paste box only.
    Captured { paste: String },
    /// The user closed the window or the dialog.
    Cancelled,
    /// RF6.1: no finished sign-in before [`TIMEOUT`].
    TimedOut,
    /// RF7.2: this platform's webview could not read its cookie store;
    /// the paste box stays available.
    Unavailable,
}

/// One cookie as read from the store (the fields the reduction needs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

impl StoreCookie {
    fn of(cookie: &tauri::webview::Cookie<'_>) -> Self {
        Self {
            name: cookie.name().to_owned(),
            value: cookie.value().to_owned(),
            domain: cookie.domain().unwrap_or_default().to_owned(),
            path: cookie.path().unwrap_or_default().to_owned(),
        }
    }
}

/// RF4.2: whether a cookie of `domain` is sent to `host` (RFC 6265
/// domain-match: the host itself or a parent domain; case-insensitive).
pub fn domain_matches(host: &str, domain: &str) -> bool {
    let domain = domain.trim_start_matches('.').to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    !domain.is_empty()
        && (host == domain
            || (host.ends_with(&domain) && host[..host.len() - domain.len()].ends_with('.')))
}

/// RF4.2: the descriptor's required and optional cookies of `store` that
/// `host` receives, as a `Cookie` header list in the descriptor's order;
/// `None` while a required cookie, or one of the capture's `done_when`
/// cookies, is missing or empty. Every other cookie
/// is dropped. A name the store holds several times (a host and a
/// parent-domain cookie) keeps the most specific one: the longest domain,
/// then the longest path.
pub fn reduce_cookies(capture: Capture, host: &str, store: &[StoreCookie]) -> Option<String> {
    let pick = |name: &str| {
        store
            .iter()
            .filter(|c| c.name == name && !c.value.is_empty() && domain_matches(host, &c.domain))
            .max_by_key(|c| (c.domain.trim_start_matches('.').len(), c.path.len()))
    };
    let Capture { descriptor, spec } = capture;
    if descriptor
        .required
        .iter()
        .chain(spec.done_when)
        .any(|name| pick(name).is_none())
    {
        return None;
    }
    let pairs: Vec<String> = descriptor
        .required
        .iter()
        .chain(descriptor.optional)
        .filter_map(|name| pick(name).map(|c| format!("{}={}", c.name, c.value)))
        .collect();
    (!pairs.is_empty()).then(|| pairs.join("; "))
}

/// RF6: one polling step. `read` is the store's content, or `Err` when the
/// platform cannot read it. `None` keeps waiting.
pub fn step(
    capture: Capture,
    host: &str,
    elapsed: Duration,
    cancelled: bool,
    read: Result<&[StoreCookie], ()>,
) -> Option<Outcome> {
    if cancelled {
        return Some(Outcome::Cancelled);
    }
    let store = match read {
        Ok(store) => store,
        Err(()) => return Some(Outcome::Unavailable),
    };
    if let Some(paste) = reduce_cookies(capture, host, store) {
        return Some(Outcome::Captured { paste });
    }
    (elapsed >= TIMEOUT).then_some(Outcome::TimedOut)
}

/// A session descriptor and the capture chosen for the account's region.
#[derive(Debug, Clone, Copy)]
pub struct Capture {
    pub descriptor: &'static SessionDescriptor,
    pub spec: &'static user_session::CaptureSpec,
}

/// The capture of `slot` for an account of `region` (`params.region`, the
/// dialog's choice; PRD 0048 RF3.4), refused when it declares no capture.
pub fn capture_descriptor(
    provider: Provider,
    slot: &str,
    region: Option<&str>,
) -> Result<Capture, &'static str> {
    let descriptor = user_session::descriptor(provider, slot).ok_or("not_capturable")?;
    let spec = descriptor.capture_in(region).ok_or("not_capturable")?;
    Ok(Capture { descriptor, spec })
}

/// PRD 0056 RF1.2: the window a capture runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// PRD 0048: a private window, destroyed when the capture ends.
    Private,
    /// PRD 0056: the window shared by the captures of this sign-in.
    Shared(SharedSignIn),
}

/// PRD 0056 RF1.2: the route of `capture`, from the core's declaration.
pub fn route(capture: Capture) -> Route {
    capture.spec.shared.map_or(Route::Private, Route::Shared)
}

/// RF3 (both PRDs): whether the window may load `url`. `target` is the
/// capture in progress; a window without one (the parked shared window)
/// loads only `about:blank`.
pub fn navigation_allowed(target: Option<Capture>, url: &str) -> bool {
    match target {
        Some(Capture { descriptor, spec }) => descriptor.capture_allows_with(spec, url),
        None => url == "about:blank",
    }
}

/// What a page's `window.open` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popup {
    /// PRD 0056 RF5: the shared window itself goes there (a Google sign-in
    /// popup the capture allows).
    InPlace,
    /// PRD 0048 RF3: an HTTPS page opens in the user's browser.
    Browser,
    /// Anything else is dropped.
    Drop,
}

/// RF5: the fate of a popup to `url` while `target` is in progress.
pub fn popup(target: Option<Capture>, url: &str) -> Popup {
    if let Some(Capture { descriptor, spec }) = target {
        if let Some(sign_in) = spec.shared {
            if sign_in.opens_in_place(url) && descriptor.capture_allows_with(spec, url) {
                return Popup::InPlace;
            }
        }
    }
    if url.starts_with("https://") {
        Popup::Browser
    } else {
        Popup::Drop
    }
}

/// PRD 0056 RF3: the positions in `store` of the cookies a parked window
/// must not keep (everything but the shared sign-in's own).
pub fn to_forget(sign_in: SharedSignIn, store: &[StoreCookie]) -> Vec<usize> {
    store
        .iter()
        .enumerate()
        .filter(|(_, c)| !sign_in.keeps_cookie(&c.domain))
        .map(|(i, _)| i)
        .collect()
}

/// PRD 0056 RF2: where the shared window is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// No window.
    #[default]
    Absent,
    /// A capture runs in it (visible).
    Busy,
    /// Hidden on `about:blank` with only the sign-in's cookies, waiting for
    /// the next capture; `generation` tells its idle timer apart from a
    /// later parking.
    Parked { generation: u64 },
}

/// PRD 0056 RF2: how a capture in the shared window ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Captured,
    TimedOut,
    /// The dialog closed, changed provider or pressed Cancel: the window
    /// itself was not refused, so it is kept.
    DialogCancelled,
    /// The user closed the window: its sign-in is forgotten.
    WindowClosed,
    /// The store could not be read: the window is useless.
    Unavailable,
}

/// PRD 0056 RF2: how a capture in the shared window starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Create,
    Reuse,
}

/// PRD 0056 RF2: what to do with the shared window after a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Park { generation: u64 },
    Destroy,
}

/// PRD 0056 RF2: the shared window's state machine (no I/O).
#[derive(Debug, Default)]
pub struct Life {
    phase: Phase,
    generation: u64,
}

impl Life {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// A capture starts: reuse a parked window, else create one; `None`
    /// while one runs.
    pub fn begin(&mut self) -> Option<Start> {
        let start = match self.phase {
            Phase::Busy => return None,
            Phase::Absent => Start::Create,
            Phase::Parked { .. } => Start::Reuse,
        };
        self.phase = Phase::Busy;
        Some(start)
    }

    /// A capture ended: park the window when its sign-in may still serve,
    /// destroy it when the user closed it or it cannot read its store.
    pub fn end(&mut self, ending: Ending) -> After {
        if self.phase != Phase::Busy {
            self.phase = Phase::Absent;
            return After::Destroy;
        }
        match ending {
            Ending::Captured | Ending::TimedOut | Ending::DialogCancelled => {
                self.generation += 1;
                self.phase = Phase::Parked {
                    generation: self.generation,
                };
                After::Park {
                    generation: self.generation,
                }
            }
            Ending::WindowClosed | Ending::Unavailable => {
                self.phase = Phase::Absent;
                After::Destroy
            }
        }
    }

    /// The idle limit of the parking `generation` passed: `true` when the
    /// window must be destroyed now (still parked since then).
    pub fn idle(&mut self, generation: u64) -> bool {
        if self.phase == (Phase::Parked { generation }) {
            self.phase = Phase::Absent;
            true
        } else {
            false
        }
    }

    /// RF2.4 "Forget": `Ok(true)` when a parked window must be destroyed,
    /// `Ok(false)` without one, `Err(())` while a capture runs in it.
    pub fn forget(&mut self) -> Result<bool, ()> {
        match self.phase {
            Phase::Busy => Err(()),
            Phase::Absent => Ok(false),
            Phase::Parked { .. } => {
                self.phase = Phase::Absent;
                Ok(true)
            }
        }
    }

    /// The window is gone (destroyed, a failed park, the app quitting).
    pub fn gone(&mut self) {
        self.phase = Phase::Absent;
    }
}

/// PRD 0056: the shared window's bookkeeping.
#[derive(Default)]
struct SharedSlot {
    life: Life,
    /// The capture in progress, read by the window's navigation and popup
    /// hooks; `None` while parked.
    target: Arc<Mutex<Option<Capture>>>,
    /// Set by the window's close or destroy event.
    closed: Arc<AtomicBool>,
    /// The window's data directory (Windows and Linux).
    dir: Option<PathBuf>,
}

/// The capture in progress (at most one per app) and the shared window.
#[derive(Default)]
pub struct State {
    active: Mutex<Option<Arc<AtomicBool>>>,
    shared: Mutex<SharedSlot>,
}

impl State {
    /// RF6.2: the dialog's cancel; a no-op without a capture.
    pub fn cancel(&self) {
        if let Some(flag) = self.active.lock().as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    fn begin(&self) -> Option<Arc<AtomicBool>> {
        let mut active = self.active.lock();
        if active.is_some() {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&flag));
        Some(flag)
    }

    fn end(&self) {
        *self.active.lock() = None;
    }

    /// PRD 0056: the shared window's phase (tests and diagnostics).
    pub fn shared_phase(&self) -> Phase {
        self.shared.lock().life.phase()
    }

    fn shared_dir(&self) -> Option<PathBuf> {
        self.shared.lock().dir.clone()
    }
}

/// A fresh, unique data directory for one capture window.
fn fresh_dir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "{DIR_PREFIX}{}-{nanos}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ))
}

/// RF2.3: remove leftover capture directories (a crash, or a file the
/// webview still held when its window ended), except `keep`, the parked
/// shared window's own.
fn sweep_dirs(keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(DIR_PREFIX)
            && keep != Some(entry.path().as_path())
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// RF1–RF6: open the capture window for `slot` of `provider` and wait for
/// its outcome. Errors: `not_capturable`, `busy`, `unavailable`.
pub async fn run<R: Runtime>(
    app: &AppHandle<R>,
    provider: Provider,
    slot: &str,
    region: Option<&str>,
) -> Result<Outcome, String> {
    let target = capture_descriptor(provider, slot, region)?;
    let state = app.state::<State>();
    let Some(cancel) = state.begin() else {
        let label = if state.shared_phase() == Phase::Busy {
            SHARED_LABEL
        } else {
            LABEL
        };
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_focus();
        }
        return Err("busy".into());
    };
    let outcome = match route(target) {
        Route::Private => private_capture(app, target, &cancel).await,
        Route::Shared(sign_in) => shared_capture(app, target, sign_in, &cancel).await,
    };
    state.end();
    outcome
}

/// The host whose cookies a capture reads, and its first page.
fn urls(target: Capture) -> Result<(tauri::Url, String), String> {
    let start = tauri::Url::parse(target.spec.start_url).map_err(|_| "not_capturable")?;
    let cookie_url = tauri::Url::parse(target.spec.cookie_url).map_err(|_| "not_capturable")?;
    let host = cookie_url.host_str().ok_or("not_capturable")?.to_owned();
    Ok((start, host))
}

/// RF4/RF6: poll `window`'s store until the capture ends. `cancel` is the
/// dialog's flag, `closed` the window's.
async fn poll<R: Runtime>(
    window: &WebviewWindow<R>,
    target: Capture,
    host: &str,
    cancel: &AtomicBool,
    closed: &AtomicBool,
) -> Outcome {
    let began = Instant::now();
    let mut seen: Vec<String> = Vec::new();
    loop {
        tokio::time::sleep(POLL).await;
        let cancelled = cancel.load(Ordering::SeqCst) || closed.load(Ordering::SeqCst);
        // The cookie read runs on this async task, never on the main
        // thread (WebView2 deadlocks there, Tauri's known issue). The whole
        // private store is read and matched here: WKWebView's
        // `cookies_for_url` drops parent-domain cookies (exact domain only).
        let read = if cancelled {
            Ok(Vec::new())
        } else {
            window
                .cookies()
                .map(|cookies| cookies.iter().map(StoreCookie::of).collect::<Vec<_>>())
                .map_err(|_| ())
        };
        let read = read.as_deref().map_err(|_| ());
        if cfg!(debug_assertions) {
            if let Ok(store) = read {
                if !store.is_empty() {
                    let mut names: Vec<String> = store
                        .iter()
                        .map(|c| format!("{}@{}", c.name, c.domain))
                        .collect();
                    names.sort();
                    names.dedup();
                    seen = names;
                }
            }
        }
        if let Some(outcome) = step(target, host, began.elapsed(), cancelled, read) {
            // Debug builds only: the cookie names and domains the store last
            // held, never a value, to diagnose a capture that ends early or
            // never ends.
            if cfg!(debug_assertions) {
                eprintln!("session capture ({host}): cookie names {seen:?}");
            }
            return outcome;
        }
    }
}

/// Debug builds only: the host and path of each page or frame load (never
/// the query, which carries sign-in codes and state). The webview's
/// navigation hook does not see the page's fetch/XHR calls; those need the
/// browser's DevTools.
fn log_navigation(allowed: bool, url: &tauri::Url) {
    if cfg!(debug_assertions) {
        eprintln!(
            "session capture: navigation {} {}{}",
            if allowed { "allowed" } else { "refused" },
            url.host_str().unwrap_or("-"),
            url.path()
        );
    }
}

/// PRD 0048: the private, one-shot window.
async fn private_capture<R: Runtime>(
    app: &AppHandle<R>,
    target: Capture,
    cancel: &Arc<AtomicBool>,
) -> Result<Outcome, String> {
    let (start, host) = urls(target)?;
    let state = app.state::<State>();
    sweep_dirs(state.shared_dir().as_deref());
    let dir = fresh_dir();
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(start))
        .title(target.descriptor.site)
        .inner_size(480.0, 720.0)
        .center()
        .incognito(true)
        .disable_drag_drop_handler()
        .on_navigation(move |url| {
            let allowed = navigation_allowed(Some(target), url.as_str());
            log_navigation(allowed, url);
            allowed
        })
        .on_new_window(move |url, _| {
            if popup(Some(target), url.as_str()) == Popup::Browser {
                let _ = crate::about::open_dashboard(url.as_str());
            }
            NewWindowResponse::Deny
        })
        .on_download(|_, _| false);
    // RF2.2: WebKitGTK and WebView2 keep their store under a data
    // directory; a fresh one per capture keeps it apart from the main
    // window's even where `incognito` is unsupported (old WebView2).
    if cfg!(not(target_os = "macos")) {
        builder = builder.data_directory(dir.clone());
    }
    let window = builder.build().map_err(|_| "unavailable")?;
    let closed = Arc::new(AtomicBool::new(false));
    {
        let closed = Arc::clone(&closed);
        window.on_window_event(move |event| {
            if matches!(
                event,
                WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
            ) {
                closed.store(true, Ordering::SeqCst);
            }
        });
    }
    let outcome = poll(&window, target, &host, cancel, &closed).await;
    // RF2.3: wipe the store, close the window, remove its directory.
    let _ = window.clear_all_browsing_data();
    let _ = window.destroy();
    if cfg!(not(target_os = "macos")) {
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(outcome)
}

/// PRD 0056 RF2.1: create the shared window at `start`. Its hooks read the
/// capture in progress from `target` at each call, so a reused window
/// follows the next provider's allow-list.
fn build_shared<R: Runtime>(
    app: &AppHandle<R>,
    start: tauri::Url,
    site: &str,
    target: &Arc<Mutex<Option<Capture>>>,
    closed: &Arc<AtomicBool>,
    dir: Option<&Path>,
) -> Result<WebviewWindow<R>, String> {
    let nav_target = Arc::clone(target);
    let popup_target = Arc::clone(target);
    let popup_app = app.clone();
    let mut builder = WebviewWindowBuilder::new(app, SHARED_LABEL, WebviewUrl::External(start))
        .title(site)
        .inner_size(480.0, 720.0)
        .center()
        .incognito(true)
        .disable_drag_drop_handler()
        .on_navigation(move |url| {
            let current = *nav_target.lock();
            let allowed = navigation_allowed(current, url.as_str());
            log_navigation(allowed, url);
            allowed
        })
        .on_new_window(move |url, _| {
            let current = *popup_target.lock();
            match popup(current, url.as_str()) {
                Popup::InPlace => {
                    // RF5: the Google sign-in goes on in this window. The
                    // navigation runs after this hook returns and passes
                    // the same allow-list again.
                    if let Some(window) = popup_app.get_webview_window(SHARED_LABEL) {
                        tauri::async_runtime::spawn(async move {
                            let _ = window.navigate(url);
                        });
                    }
                }
                Popup::Browser => {
                    let _ = crate::about::open_dashboard(url.as_str());
                }
                Popup::Drop => {}
            }
            NewWindowResponse::Deny
        })
        .on_download(|_, _| false);
    if let Some(dir) = dir {
        builder = builder.data_directory(dir.to_path_buf());
    }
    let window = builder.build().map_err(|_| "unavailable")?;
    let closed = Arc::clone(closed);
    window.on_window_event(move |event| {
        if matches!(
            event,
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
        ) {
            closed.store(true, Ordering::SeqCst);
        }
    });
    Ok(window)
}

/// PRD 0056 RF2.1: show the parked window at `start` for the next capture;
/// `None` when it is gone.
fn reuse_shared<R: Runtime>(
    app: &AppHandle<R>,
    start: tauri::Url,
    site: &str,
    closed: &AtomicBool,
) -> Option<WebviewWindow<R>> {
    if closed.load(Ordering::SeqCst) {
        return None;
    }
    let window = app.get_webview_window(SHARED_LABEL)?;
    let _ = window.set_title(site);
    window.navigate(start).ok()?;
    window.show().ok()?;
    let _ = window.set_focus();
    Some(window)
}

/// PRD 0056: a capture in the shared window.
async fn shared_capture<R: Runtime>(
    app: &AppHandle<R>,
    target: Capture,
    sign_in: SharedSignIn,
    cancel: &Arc<AtomicBool>,
) -> Result<Outcome, String> {
    let (start, host) = urls(target)?;
    let state = app.state::<State>();
    let site = target.descriptor.site;
    let (begin, target_slot, closed) = {
        let mut slot = state.shared.lock();
        let Some(begin) = slot.life.begin() else {
            return Err("busy".into());
        };
        *slot.target.lock() = Some(target);
        (begin, Arc::clone(&slot.target), Arc::clone(&slot.closed))
    };
    let reused = match begin {
        Start::Reuse => reuse_shared(app, start.clone(), site, &closed),
        Start::Create => None,
    };
    let window = match reused {
        Some(window) => window,
        None => {
            // A parked window that vanished is replaced by a new one.
            if begin == Start::Reuse {
                destroy_shared(app);
                let mut slot = state.shared.lock();
                slot.life.gone();
                slot.life.begin();
                *slot.target.lock() = Some(target);
            }
            sweep_dirs(None);
            let dir = cfg!(not(target_os = "macos")).then(fresh_dir);
            let closed = Arc::new(AtomicBool::new(false));
            {
                let mut slot = state.shared.lock();
                slot.closed = Arc::clone(&closed);
                slot.dir = dir.clone();
            }
            match build_shared(app, start, site, &target_slot, &closed, dir.as_deref()) {
                Ok(window) => window,
                Err(e) => {
                    let mut slot = state.shared.lock();
                    slot.life.gone();
                    *slot.target.lock() = None;
                    if let Some(dir) = slot.dir.take() {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                    return Err(e);
                }
            }
        }
    };
    let closed = Arc::clone(&state.shared.lock().closed);
    let outcome = poll(&window, target, &host, cancel, &closed).await;
    let ending = match &outcome {
        Outcome::Captured { .. } => Ending::Captured,
        Outcome::TimedOut => Ending::TimedOut,
        Outcome::Unavailable => Ending::Unavailable,
        Outcome::Cancelled if closed.load(Ordering::SeqCst) => Ending::WindowClosed,
        Outcome::Cancelled => Ending::DialogCancelled,
    };
    // From here the window serves no capture: its hooks allow only
    // `about:blank` and open no popup in place.
    *target_slot.lock() = None;
    let after = state.shared.lock().life.end(ending);
    match after {
        After::Park { generation } => {
            if park(&window, sign_in).await {
                schedule_idle(app, generation);
            } else {
                state.shared.lock().life.gone();
                destroy_shared(app);
            }
        }
        After::Destroy => destroy_shared(app),
    }
    Ok(outcome)
}

/// PRD 0056 RF3: hide the window on `about:blank` and delete every cookie
/// but the sign-in's own; `false` when the store cannot be read or still
/// holds another cookie afterwards (the caller then destroys the window).
async fn park<R: Runtime>(window: &WebviewWindow<R>, sign_in: SharedSignIn) -> bool {
    let Ok(blank) = tauri::Url::parse("about:blank") else {
        return false;
    };
    if window.navigate(blank).is_err() {
        return false;
    }
    let _ = window.hide();
    tokio::time::sleep(PARK_SETTLE).await;
    for _ in 0..2 {
        let Ok(cookies) = window.cookies() else {
            return false;
        };
        let store: Vec<StoreCookie> = cookies.iter().map(StoreCookie::of).collect();
        let forget = to_forget(sign_in, &store);
        if forget.is_empty() {
            return true;
        }
        for i in forget {
            if window.delete_cookie(cookies[i].clone()).is_err() {
                return false;
            }
        }
    }
    // Verify once more after the second pass.
    window.cookies().is_ok_and(|cookies| {
        let store: Vec<StoreCookie> = cookies.iter().map(StoreCookie::of).collect();
        to_forget(sign_in, &store).is_empty()
    })
}

/// PRD 0056 RF2.3: destroy the parking `generation` after [`SHARED_IDLE`]
/// unless a capture used the window since.
fn schedule_idle<R: Runtime>(app: &AppHandle<R>, generation: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SHARED_IDLE).await;
        let expired = app.state::<State>().shared.lock().life.idle(generation);
        if expired {
            destroy_shared(&app);
        }
    });
}

/// Wipe and destroy the shared window and remove its directory. The state
/// lock is never held across the window calls.
fn destroy_shared<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(SHARED_LABEL) {
        let _ = window.clear_all_browsing_data();
        let _ = window.destroy();
    }
    let dir = {
        let state = app.state::<State>();
        let mut slot = state.shared.lock();
        *slot.target.lock() = None;
        slot.dir.take()
    };
    if let Some(dir) = dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// PRD 0056 RF2.4: the dialog's "Forget the Google sign-in". `Ok(true)`
/// when a parked window was destroyed, `Ok(false)` without one; `busy`
/// while a capture runs in it.
pub fn forget_shared<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
    let forget = app.state::<State>().shared.lock().life.forget();
    match forget {
        Err(()) => Err("busy".into()),
        Ok(false) => Ok(false),
        Ok(true) => {
            destroy_shared(app);
            Ok(true)
        }
    }
}

/// PRD 0056 RF2.3: the app quits. The non-persistent store ends with the
/// process; the data directory (Windows and Linux) is removed here, and
/// any leftover by the next capture's sweep.
pub fn on_exit<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<State>();
    let dir = {
        let mut slot = state.shared.lock();
        slot.life.gone();
        *slot.target.lock() = None;
        slot.dir.take()
    };
    if let Some(dir) = dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, value: &str, domain: &str, path: &str) -> StoreCookie {
        StoreCookie {
            name: name.into(),
            value: value.into(),
            domain: domain.into(),
            path: path.into(),
        }
    }

    const HOST: &str = "platform.xiaomimimo.com";

    #[test]
    fn domain_match_follows_rfc_6265() {
        assert!(domain_matches(HOST, "platform.xiaomimimo.com"));
        assert!(domain_matches(HOST, ".xiaomimimo.com"));
        assert!(domain_matches(HOST, "XiaomiMimo.com"));
        assert!(!domain_matches(HOST, "mimo.com"));
        assert!(!domain_matches(HOST, "omimo.com"));
        assert!(!domain_matches(HOST, ".account.xiaomi.com"));
        assert!(!domain_matches(HOST, "aistudio.xiaomimimo.com"));
        assert!(!domain_matches(HOST, ""));
        assert!(!domain_matches(HOST, "."));
    }

    #[test]
    fn reduction_ignores_other_sites_cookies() {
        let store = [
            cookie("userId", "7", ".account.xiaomi.com", "/"),
            cookie(
                "api-platform_serviceToken",
                "tok",
                "platform.xiaomimimo.com",
                "/",
            ),
        ];
        assert_eq!(reduce_cookies(platform(), HOST, &store), None);
    }

    fn platform() -> Capture {
        capture_descriptor(Provider::XiaomiMimo, "platform_session_env", None).unwrap()
    }

    #[test]
    fn reduction_keeps_only_the_descriptor_cookies() {
        let store = [
            cookie("_ga", "tracker", ".xiaomimimo.com", "/"),
            cookie("userId", "7", ".xiaomimimo.com", "/"),
            cookie(
                "api-platform_serviceToken",
                "tok",
                "platform.xiaomimimo.com",
                "/",
            ),
            cookie("api-platform_slh", "other", "platform.xiaomimimo.com", "/"),
        ];
        assert_eq!(
            reduce_cookies(platform(), HOST, &store).as_deref(),
            Some("api-platform_serviceToken=tok; userId=7")
        );
    }

    #[test]
    fn reduction_waits_for_every_required_cookie() {
        assert_eq!(reduce_cookies(platform(), HOST, &[]), None);
        let partial = [cookie("userId", "7", ".xiaomimimo.com", "/")];
        assert_eq!(reduce_cookies(platform(), HOST, &partial), None);
        let empty = [
            cookie("userId", "7", ".xiaomimimo.com", "/"),
            cookie(
                "api-platform_serviceToken",
                "",
                "platform.xiaomimimo.com",
                "/",
            ),
        ];
        assert_eq!(reduce_cookies(platform(), HOST, &empty), None);
    }

    #[test]
    fn reduction_prefers_the_most_specific_duplicate() {
        let store = [
            cookie("userId", "parent", ".xiaomimimo.com", "/"),
            cookie("userId", "host", "platform.xiaomimimo.com", "/"),
            cookie(
                "api-platform_serviceToken",
                "root",
                "platform.xiaomimimo.com",
                "/",
            ),
            cookie(
                "api-platform_serviceToken",
                "deep",
                "platform.xiaomimimo.com",
                "/api",
            ),
        ];
        assert_eq!(
            reduce_cookies(platform(), HOST, &store).as_deref(),
            Some("api-platform_serviceToken=deep; userId=host")
        );
    }

    #[test]
    fn captured_paste_passes_the_core_reduction() {
        let store = [
            cookie("userId", "7", ".xiaomimimo.com", "/"),
            cookie(
                "api-platform_serviceToken",
                "tok",
                "platform.xiaomimimo.com",
                "/",
            ),
        ];
        let paste = reduce_cookies(platform(), HOST, &store).unwrap();
        let reduced = user_session::reduce(
            Provider::XiaomiMimo,
            "platform_session_env",
            &paste,
            chrono::Utc::now(),
        )
        .unwrap();
        assert_eq!(reduced.kept(), ["api-platform_serviceToken", "userId"]);
        assert_eq!(reduced.discarded(), 0);
    }

    #[test]
    fn state_machine_success_cancel_timeout_and_unavailable() {
        let d = platform();
        let ready = [
            cookie("userId", "7", ".xiaomimimo.com", "/"),
            cookie(
                "api-platform_serviceToken",
                "tok",
                "platform.xiaomimimo.com",
                "/",
            ),
        ];
        let signed_out = [cookie("_ga", "x", ".xiaomimimo.com", "/")];
        let second = Duration::from_secs(1);
        assert_eq!(step(d, HOST, second, false, Ok(&signed_out)), None);
        assert_eq!(
            step(d, HOST, second, false, Ok(&ready)),
            Some(Outcome::Captured {
                paste: "api-platform_serviceToken=tok; userId=7".into()
            })
        );
        // Cancel wins over a ready store: the user's close is final.
        assert_eq!(
            step(d, HOST, second, true, Ok(&ready)),
            Some(Outcome::Cancelled)
        );
        assert_eq!(
            step(d, HOST, TIMEOUT, false, Ok(&signed_out)),
            Some(Outcome::TimedOut)
        );
        // A sign-in finished on the last poll is still captured.
        assert!(matches!(
            step(d, HOST, TIMEOUT, false, Ok(&ready)),
            Some(Outcome::Captured { .. })
        ));
        assert_eq!(
            step(d, HOST, second, false, Err(())),
            Some(Outcome::Unavailable)
        );
    }

    #[test]
    fn only_declared_captures_start() {
        assert_eq!(
            capture_descriptor(Provider::XiaomiMimo, "session_env", None).err(),
            Some("not_capturable")
        );
        assert_eq!(
            capture_descriptor(Provider::NineRouter, "auth_session_env", None).err(),
            Some("not_capturable")
        );
        assert!(capture_descriptor(Provider::XiaomiMimo, "platform_session_env", None).is_ok());
        assert!(capture_descriptor(Provider::AlibabaTokenPlan, "session_env", None).is_ok());
        assert!(capture_descriptor(Provider::AlibabaCodingPlan, "session_env", None).is_ok());
        assert!(capture_descriptor(Provider::AlibabaCodingPlan, "session_env", Some("cn")).is_ok());
        assert!(capture_descriptor(Provider::Meta, "session_env", None).is_ok());
    }

    /// PRD 0049 RF7: the dev.meta.ai capture ends only on the console
    /// session `llama_dev_sess`: the anonymous `datr` and the sign-in
    /// helpers never end it, and only `llama_dev_sess` is kept.
    #[test]
    fn meta_console_capture_waits_for_the_console_session() {
        let d = capture_descriptor(Provider::Meta, "session_env", None).unwrap();
        let host = "dev.meta.ai";
        let anonymous = [
            cookie("datr", "anon", ".meta.ai", "/"),
            cookie("llama_dev_csrf", "c", "dev.meta.ai", "/"),
            cookie("llama_dev_sess", "s", ".facebook.com", "/"),
        ];
        assert_eq!(reduce_cookies(d, host, &anonymous), None);
        let signed_in = [
            cookie("datr", "anon", ".meta.ai", "/"),
            cookie("llama_dev_sess", "s", "dev.meta.ai", "/"),
            cookie("llama_auto_login", "1", "dev.meta.ai", "/"),
        ];
        assert_eq!(
            reduce_cookies(d, host, &signed_in).as_deref(),
            Some("llama_dev_sess=s")
        );
    }

    /// RF3.4/RF4.3: the Coding Plan capture reads the region's own console
    /// cookies; anonymous `cna` never ends it, and the other region's
    /// cookies never match.
    #[test]
    fn coding_plan_capture_waits_for_the_sign_in_of_its_region() {
        for (region, domain, other) in [
            (None, ".alibabacloud.com", ".aliyun.com"),
            (Some("cn"), ".aliyun.com", ".alibabacloud.com"),
        ] {
            let c = capture_descriptor(Provider::AlibabaCodingPlan, "session_env", region).unwrap();
            let host = tauri::Url::parse(c.spec.cookie_url).unwrap();
            let host = host.host_str().unwrap();
            let anonymous = [
                cookie("cna", "anon", domain, "/"),
                cookie("login_aliyunid_pk", "1", other, "/"),
                cookie("login_aliyunid_ticket", "t", other, "/"),
            ];
            assert_eq!(reduce_cookies(c, host, &anonymous), None, "{region:?}");
            let signed_in = [
                cookie("cna", "anon", domain, "/"),
                cookie("_ga", "x", domain, "/"),
                cookie("login_aliyunid_ticket", "t", domain, "/"),
                cookie("login_aliyunid_pk", "1", domain, "/"),
                cookie("login_aliyunid_csrf", "c", domain, "/"),
            ];
            assert_eq!(
                reduce_cookies(c, host, &signed_in).as_deref(),
                Some(
                    "login_aliyunid_ticket=t; login_aliyunid_pk=1; login_aliyunid_csrf=c; cna=anon"
                ),
                "{region:?}"
            );
        }
    }

    /// RF4.2: the Qwen Cloud session has only optional cookies; anonymous
    /// ones (`cna`) do not end the capture, the sign-in's account id does,
    /// and `.qwencloud.com` cookies are kept for `cs-data.qwencloud.com`.
    #[test]
    fn qwen_cloud_capture_waits_for_the_sign_in() {
        let d = capture_descriptor(Provider::AlibabaTokenPlan, "session_env", None).unwrap();
        let host = "home.qwencloud.com";
        let anonymous = [
            cookie("cna", "anon", ".qwencloud.com", "/"),
            cookie("login_aliyunid_pk", "1", ".alibabacloud.com", "/"),
        ];
        assert_eq!(reduce_cookies(d, host, &anonymous), None);
        let signed_in = [
            cookie("cna", "anon", ".qwencloud.com", "/"),
            cookie("_ga", "x", ".qwencloud.com", "/"),
            cookie("login_aliyunid_ticket", "t", ".qwencloud.com", "/"),
            cookie("login_aliyunid_pk", "1", ".qwencloud.com", "/"),
        ];
        assert_eq!(
            reduce_cookies(d, host, &signed_in).as_deref(),
            Some("login_aliyunid_ticket=t; login_aliyunid_pk=1; cna=anon")
        );
    }

    #[test]
    fn one_capture_at_a_time_and_cancel_sets_the_flag() {
        let state = State::default();
        state.cancel();
        let flag = state.begin().unwrap();
        assert!(state.begin().is_none());
        state.cancel();
        assert!(flag.load(Ordering::SeqCst));
        state.end();
        assert!(state.begin().is_some());
    }

    #[test]
    fn outcome_serialises_without_extra_fields() {
        assert_eq!(
            serde_json::to_string(&Outcome::TimedOut).unwrap(),
            r#"{"status":"timed_out"}"#
        );
    }

    /// PRD 0056 RF1.2: the captures whose sign-in offers Google share one
    /// window; the mainland Coding Plan console keeps a private one.
    #[test]
    fn google_captures_route_to_the_shared_window() {
        let google = Route::Shared(SharedSignIn::Google);
        for (provider, slot) in [
            (Provider::XiaomiMimo, "platform_session_env"),
            (Provider::AlibabaTokenPlan, "session_env"),
            (Provider::AlibabaCodingPlan, "session_env"),
            (Provider::Meta, "session_env"),
        ] {
            let c = capture_descriptor(provider, slot, None).unwrap();
            assert_eq!(route(c), google, "{provider:?}");
        }
        let cn =
            capture_descriptor(Provider::AlibabaCodingPlan, "session_env", Some("cn")).unwrap();
        assert_eq!(route(cn), Route::Private);
        // Another region value keeps the default (shared) capture.
        let other =
            capture_descriptor(Provider::AlibabaCodingPlan, "session_env", Some("intl")).unwrap();
        assert_eq!(route(other), google);
    }

    /// PRD 0056 RF3/RF5: a reused window follows the capture in progress;
    /// parked, it loads only `about:blank`.
    #[test]
    fn navigation_follows_the_capture_in_progress() {
        let qwen = capture_descriptor(Provider::AlibabaTokenPlan, "session_env", None).unwrap();
        let meta = capture_descriptor(Provider::Meta, "session_env", None).unwrap();
        assert!(navigation_allowed(
            Some(qwen),
            "https://home.qwencloud.com/x"
        ));
        assert!(!navigation_allowed(
            Some(meta),
            "https://home.qwencloud.com/x"
        ));
        assert!(navigation_allowed(Some(meta), "https://dev.meta.ai/usage"));
        for both in [Some(qwen), Some(meta)] {
            assert!(navigation_allowed(
                both,
                "https://accounts.google.com/signin"
            ));
        }
        assert!(navigation_allowed(None, "about:blank"));
        for refused in [
            "https://accounts.google.com/signin",
            "https://home.qwencloud.com/",
            "about:srcdoc",
        ] {
            assert!(!navigation_allowed(None, refused), "{refused}");
        }
    }

    /// PRD 0056 RF5: a Google sign-in popup opens in the shared window;
    /// other HTTPS popups go to the browser; the rest is dropped.
    #[test]
    fn popups_open_in_place_only_for_google_sign_in_of_a_shared_capture() {
        let qwen = capture_descriptor(Provider::AlibabaTokenPlan, "session_env", None).unwrap();
        let cn =
            capture_descriptor(Provider::AlibabaCodingPlan, "session_env", Some("cn")).unwrap();
        let google = "https://accounts.google.com/o/oauth2/v2/auth?client_id=x";
        assert_eq!(popup(Some(qwen), google), Popup::InPlace);
        // Parked, or a private capture: the browser, as before.
        assert_eq!(popup(None, google), Popup::Browser);
        assert_eq!(popup(Some(cn), google), Popup::Browser);
        assert_eq!(
            popup(Some(qwen), "https://github.com/login/oauth/authorize"),
            Popup::Browser
        );
        assert_eq!(
            popup(Some(qwen), "https://myaccount.google.com/"),
            Popup::Browser
        );
        assert_eq!(
            popup(Some(qwen), "http://accounts.google.com/"),
            Popup::Drop
        );
        assert_eq!(popup(Some(qwen), "javascript:alert(1)"), Popup::Drop);
    }

    /// PRD 0056 RF3: parking keeps Google's cookies and forgets every
    /// provider's.
    #[test]
    fn parking_forgets_every_cookie_but_googles() {
        let store = [
            cookie("SID", "g", ".google.com", "/"),
            cookie("login_aliyunid_ticket", "t", ".qwencloud.com", "/"),
            cookie("__Host-GAPS", "g", "accounts.google.com", "/"),
            cookie("llama_dev_sess", "s", "dev.meta.ai", "/"),
            cookie("x", "y", "google.com.evil.example", "/"),
        ];
        assert_eq!(to_forget(SharedSignIn::Google, &store), vec![1, 3, 4]);
        assert!(to_forget(SharedSignIn::Google, &store[..1]).is_empty());
    }

    /// PRD 0056 RF2: create, park, reuse, idle and forget.
    #[test]
    fn shared_window_life() {
        let mut life = Life::default();
        assert_eq!(life.phase(), Phase::Absent);
        assert_eq!(life.forget(), Ok(false));
        assert_eq!(life.begin(), Some(Start::Create));
        // One capture at a time.
        assert_eq!(life.begin(), None);
        assert_eq!(life.forget(), Err(()));
        assert_eq!(life.end(Ending::Captured), After::Park { generation: 1 });
        assert_eq!(life.phase(), Phase::Parked { generation: 1 });
        // The next capture reuses it; the old idle timer no longer applies.
        assert_eq!(life.begin(), Some(Start::Reuse));
        assert!(!life.idle(1));
        assert_eq!(
            life.end(Ending::DialogCancelled),
            After::Park { generation: 2 }
        );
        assert!(!life.idle(1));
        assert!(life.idle(2));
        assert_eq!(life.phase(), Phase::Absent);
        // A time-out parks too; forgetting destroys.
        assert_eq!(life.begin(), Some(Start::Create));
        assert_eq!(life.end(Ending::TimedOut), After::Park { generation: 3 });
        assert_eq!(life.forget(), Ok(true));
        assert_eq!(life.phase(), Phase::Absent);
        assert!(!life.idle(3));
        // Closing the window or an unreadable store destroys it.
        for ending in [Ending::WindowClosed, Ending::Unavailable] {
            assert_eq!(life.begin(), Some(Start::Create));
            assert_eq!(life.end(ending), After::Destroy);
            assert_eq!(life.phase(), Phase::Absent);
        }
        // An end without a capture never parks.
        assert_eq!(life.end(Ending::Captured), After::Destroy);
        // The app quitting or a failed park leaves nothing.
        assert_eq!(life.begin(), Some(Start::Create));
        life.end(Ending::Captured);
        life.gone();
        assert_eq!(life.phase(), Phase::Absent);
    }

    /// PRD 0056 RF2 with Tauri's mock runtime (no real webview, no
    /// network): a dialog cancel parks the shared window, the next capture
    /// reuses it, forgetting destroys it and the app's exit clears it.
    #[test]
    fn shared_capture_parks_reuses_and_forgets_with_a_mock_runtime() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        let app = mock_builder()
            .manage(State::default())
            .build(mock_context(noop_assets()))
            .expect("test app");
        let handle = app.handle().clone();
        let state = handle.state::<State>();
        let run_cancelled = |handle: &AppHandle<tauri::test::MockRuntime>| {
            // The dialog cancels at once: the first poll ends the capture.
            let cancel_soon = {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        if handle.state::<State>().active.lock().is_some() {
                            handle.state::<State>().cancel();
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                })
            };
            let outcome = tauri::async_runtime::block_on(run(
                handle,
                Provider::AlibabaTokenPlan,
                "session_env",
                None,
            ));
            cancel_soon.join().unwrap();
            outcome
        };
        assert_eq!(run_cancelled(&handle), Ok(Outcome::Cancelled));
        assert!(matches!(state.shared_phase(), Phase::Parked { .. }));
        assert!(handle.get_webview_window(SHARED_LABEL).is_some());
        assert!(handle.get_webview_window(LABEL).is_none());
        // Parked: the hooks see no capture.
        assert!(state.shared.lock().target.lock().is_none());
        assert_eq!(run_cancelled(&handle), Ok(Outcome::Cancelled));
        assert!(matches!(
            state.shared_phase(),
            Phase::Parked { generation: 2 }
        ));
        assert_eq!(forget_shared(&handle), Ok(true));
        assert_eq!(state.shared_phase(), Phase::Absent);
        assert_eq!(forget_shared(&handle), Ok(false));
        on_exit(&handle);
        assert_eq!(state.shared_phase(), Phase::Absent);
    }

    /// RF5.1: the capture windows get no capability, so Tauri rejects IPC
    /// from their remote pages; no capability may grant remote origins.
    #[test]
    fn capture_window_has_no_capability() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
        for entry in std::fs::read_dir(dir).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert!(json.get("remote").is_none());
            let windows = json["windows"].as_array().unwrap();
            for w in windows {
                let w = w.as_str().unwrap();
                assert!(w != LABEL && w != SHARED_LABEL && !w.contains('*'), "{w}");
                assert!(!w.starts_with("session-capture"), "{w}");
            }
            assert!(json.get("webviews").is_none());
        }
    }
}
