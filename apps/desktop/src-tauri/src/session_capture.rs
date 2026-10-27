//! PRD 0048: "Sign in and capture session". On the user's click, a
//! Cuotai-owned window opens the provider's sign-in page in a private,
//! throw-away cookie store; the user signs in there; Rust polls that
//! store, keeps only the descriptor's cookies and hands them to the
//! account dialog's paste box, which runs the usual preview and test.
//!
//! Safety properties (RF2–RF5):
//! - the window is not listed in `capabilities/default.json` and has no
//!   `remote` capability, so Tauri rejects every IPC call of its pages;
//! - nothing is injected into the provider's pages (no initialisation
//!   script, no `eval`); the cookies are read from the native store;
//! - navigation is limited to [`SessionDescriptor::capture_allows`];
//!   `window.open` never opens a webview (HTTPS links go to the browser),
//!   downloads are refused;
//! - the store is non-persistent (`incognito`, plus a fresh data directory
//!   on Windows and Linux) and is cleared and removed when the window ends;
//! - cookie values are never logged nor kept outside the returned paste.

use cuotai_core::user_session::{self, SessionDescriptor};
use cuotai_core::Provider;
use parking_lot::Mutex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// The window label. It must stay out of every capability (see tests).
pub const LABEL: &str = "session-capture";
/// RF6.1: how long the window waits for a finished sign-in.
pub const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// RF4.1: how often the cookie store is read.
pub const POLL: Duration = Duration::from_secs(1);
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

/// The capture in progress, at most one per app.
#[derive(Default)]
pub struct State {
    active: Mutex<Option<Arc<AtomicBool>>>,
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
/// webview still held when its window ended).
fn sweep_dirs() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(DIR_PREFIX) {
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
        if let Some(window) = app.get_webview_window(LABEL) {
            let _ = window.set_focus();
        }
        return Err("busy".into());
    };
    let outcome = capture(app, target, &cancel).await;
    state.end();
    outcome
}

async fn capture<R: Runtime>(
    app: &AppHandle<R>,
    target: Capture,
    cancel: &Arc<AtomicBool>,
) -> Result<Outcome, String> {
    let Capture { descriptor, spec } = target;
    let start = tauri::Url::parse(spec.start_url).map_err(|_| "not_capturable")?;
    let cookie_url = tauri::Url::parse(spec.cookie_url).map_err(|_| "not_capturable")?;
    let host = cookie_url.host_str().ok_or("not_capturable")?.to_owned();
    let host = host.as_str();
    sweep_dirs();
    let dir = fresh_dir();
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(start))
        .title(descriptor.site)
        .inner_size(480.0, 720.0)
        .center()
        .incognito(true)
        .disable_drag_drop_handler()
        .on_navigation(move |url| descriptor.capture_allows_with(spec, url.as_str()))
        .on_new_window(|url, _| {
            if url.scheme() == "https" {
                let _ = crate::about::open_dashboard(url.as_str());
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .on_download(|_, _| false);
    // RF2.2: WebKitGTK and WebView2 keep their store under a data
    // directory; a fresh one per capture keeps it apart from the main
    // window's even where `incognito` is unsupported (old WebView2).
    if cfg!(not(target_os = "macos")) {
        builder = builder.data_directory(dir.clone());
    }
    let window = builder.build().map_err(|_| "unavailable")?;
    {
        let cancel = Arc::clone(cancel);
        window.on_window_event(move |event| {
            if matches!(
                event,
                WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
            ) {
                cancel.store(true, Ordering::SeqCst);
            }
        });
    }
    let began = Instant::now();
    let outcome = loop {
        tokio::time::sleep(POLL).await;
        let cancelled = cancel.load(Ordering::SeqCst);
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
        if let Some(outcome) = step(target, host, began.elapsed(), cancelled, read) {
            break outcome;
        }
    };
    // RF2.3: wipe the store, close the window, remove its directory.
    let _ = window.clear_all_browsing_data();
    let _ = window.destroy();
    if cfg!(not(target_os = "macos")) {
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(outcome)
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

    /// RF5.1: the capture window gets no capability, so Tauri rejects IPC
    /// from its remote pages; no capability may grant remote origins.
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
                assert!(w != LABEL && !w.contains('*'), "{w}");
            }
            assert!(json.get("webviews").is_none());
        }
    }
}
