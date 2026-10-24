//! PRD 0020 RF1: the dedicated Settings window. A regular window, label
//! `settings`, loading `settings.html`, created on demand and destroyed when
//! closed. Opening it while it is open focuses it (single instance); it is
//! not modal, so the dashboard stays usable. The first opening centers it over
//! the main window; afterwards it comes back with its last size and position
//! (`desktop.json`, `settings_window`).
//!
//! Every entry point (the gear of the main window and the tray popover, the
//! tray menu, the macOS app menu with `⌘,`, `Ctrl+,` elsewhere, and the
//! "open settings" links inside other screens) calls [`open`] with an
//! optional page and anchor. The request is kept here until the window takes
//! it ([`take_target`]), so a window that is still loading and one that is
//! already open read it the same way; an open window is told to read it by
//! [`NAVIGATE_EVENT`].
use crate::settings::{self, WindowGeometry};
use serde::Serialize;
use std::{path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// The window label, also listed in `capabilities/default.json`.
pub const LABEL: &str = "settings";
/// RF1.1: default inner size, in logical pixels.
pub const DEFAULT_SIZE: (f64, f64) = (860.0, 640.0);
/// RF1.1: minimum inner size, in logical pixels.
pub const MIN_SIZE: (f64, f64) = (640.0, 460.0);
/// Tells an open Settings window that a new page/anchor request is waiting.
pub const NAVIGATE_EVENT: &str = "settings-navigate";
/// The page served in the window.
const PAGE: &str = "settings.html";
/// The longest page or anchor id accepted.
const MAX_TOKEN: usize = 40;

/// RF1.3: where the window should land: a page of the sidebar registry and an
/// anchor inside it (for example `cost` and `cost-sources`). Unknown ids are
/// resolved by the interface (an unknown page falls back to the last one).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Target {
    pub page: Option<String>,
    pub anchor: Option<String>,
}

impl Target {
    /// The request from a caller; any id that is not a plain token is
    /// dropped, so nothing but `[a-z0-9-]` ever reaches the page.
    pub fn new(page: Option<String>, anchor: Option<String>) -> Self {
        Self {
            page: token(page),
            anchor: token(anchor),
        }
    }
}

/// `Some(id)` when `value` is 1 to [`MAX_TOKEN`] characters of lowercase
/// ASCII letters, digits and `-`.
fn token(value: Option<String>) -> Option<String> {
    value.filter(|v| {
        !v.is_empty()
            && v.len() <= MAX_TOKEN
            && v.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// The shell's state for the window: the request waiting to be read, the
/// localized title for the next creation, the geometry seen last and the
/// `desktop.json` that keeps it between openings.
#[derive(Default)]
pub struct State {
    target: Mutex<Option<Target>>,
    title: Mutex<Option<String>>,
    tracker: Mutex<Tracker>,
    file: Option<PathBuf>,
}

impl State {
    /// The geometry is kept in `file` (`None`: not kept).
    pub fn new(file: Option<PathBuf>) -> Self {
        Self {
            file,
            ..Self::default()
        }
    }

    /// The window title in the interface language, as the interface last
    /// reported it (the tray menu payload); `Cuotai` until then.
    pub fn set_title(&self, title: &str) {
        let title = clean_title(title);
        if !title.is_empty() {
            *self.title.lock().unwrap_or_else(|e| e.into_inner()) = Some(title);
        }
    }

    fn title(&self) -> String {
        self.title
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_else(|| "Cuotai".into())
    }
}

/// A window title: control characters removed, at most 80 characters.
pub fn clean_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// RF1.2/RF1.3: open the Settings window on `target`, or focus it when it is
/// already open and tell it to read the new target.
pub fn open<R: Runtime>(app: &AppHandle<R>, target: Target) -> tauri::Result<()> {
    let state = app.state::<State>();
    *state.target.lock().unwrap_or_else(|e| e.into_inner()) = Some(target);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return app.emit(NAVIGATE_EVENT, ());
    }
    let saved = state
        .file
        .as_deref()
        .and_then(|file| settings::read_settings_window(file, MIN_SIZE))
        .filter(|g| on_screen(g, &monitor_rects(app)));
    let size = saved.map_or(DEFAULT_SIZE, |g| (g.width, g.height));
    // Centered over a small main window near a screen edge, the title bar could
    // fall off the screen: then the window is centered on the screen instead.
    let position = saved.map(|g| (g.x, g.y)).or_else(|| {
        let (x, y) = centered_over(main_rect(app)?, size);
        let placed = WindowGeometry {
            x,
            y,
            width: size.0,
            height: size.1,
        };
        on_screen(&placed, &monitor_rects(app)).then_some((x, y))
    });
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(PAGE.into()))
        .title(state.title())
        .inner_size(size.0, size.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .resizable(true)
        .disable_drag_drop_handler();
    builder = match position {
        Some((x, y)) => builder.position(x, y),
        None => builder.center(),
    };
    // RF1.4: the overlay title bar with the traffic lights, as the main window;
    // Windows and Linux keep the native frame.
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }
    let window = builder.build()?;
    // The starting geometry is read here, outside the event handler; afterwards
    // only the events' own values are used.
    let mut start = Tracker {
        scale: window.scale_factor().unwrap_or(1.0),
        ..Tracker::default()
    };
    if let Ok(p) = window.outer_position() {
        start.moved(p.x as f64, p.y as f64);
    }
    if let Ok(s) = window.inner_size() {
        start.resized(s.width as f64, s.height as f64);
    }
    *state.tracker.lock().unwrap_or_else(|e| e.into_inner()) = start;
    let app = app.clone();
    window.on_window_event(move |event| {
        let state = app.state::<State>();
        let mut tracker = state.tracker.lock().unwrap_or_else(|e| e.into_inner());
        match event {
            WindowEvent::Moved(p) => tracker.moved(p.x as f64, p.y as f64),
            WindowEvent::Resized(s) => tracker.resized(s.width as f64, s.height as f64),
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
                ..
            } => {
                tracker.scale = *scale_factor;
                tracker.resized(new_inner_size.width as f64, new_inner_size.height as f64);
            }
            WindowEvent::Destroyed => {
                if let (Some(g), Some(file)) = (tracker.geometry(), state.file.as_deref()) {
                    let _ = settings::write_settings_window(file, &g);
                }
            }
            _ => {}
        }
    });
    let _ = window.set_focus();
    Ok(())
}

/// What the window last reported about itself, in physical pixels with its
/// scale, taken from the window events (no window getter runs inside the
/// event handler).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tracker {
    position: Option<(f64, f64)>,
    size: Option<(f64, f64)>,
    scale: f64,
}

impl Tracker {
    /// Windows parks a minimized window at (-32000, -32000): not a place to
    /// come back to.
    fn moved(&mut self, x: f64, y: f64) {
        if x > -32_000.0 || y > -32_000.0 {
            self.position = Some((x, y));
        }
    }

    /// A minimized window reports a zero size on Windows; that is ignored.
    fn resized(&mut self, width: f64, height: f64) {
        if width > 0.0 && height > 0.0 {
            self.size = Some((width, height));
        }
    }

    /// The outer position and inner size in logical pixels, when both are
    /// known and valid for the window.
    fn geometry(&self) -> Option<WindowGeometry> {
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        let ((x, y), (width, height)) = (self.position?, self.size?);
        WindowGeometry {
            x: x / scale,
            y: y / scale,
            width: width / scale,
            height: height / scale,
        }
        .checked(MIN_SIZE)
    }
}

/// The request waiting for the window, taken once.
pub fn take_target<R: Runtime>(app: &AppHandle<R>) -> Option<Target> {
    app.state::<State>()
        .target
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

/// A rectangle `(x, y, width, height)` in logical pixels.
type Rect = (f64, f64, f64, f64);

/// Every screen in logical pixels (each with its own scale).
fn monitor_rects<R: Runtime>(app: &AppHandle<R>) -> Vec<Rect> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let scale = m.scale_factor();
            let (p, s) = (m.position(), m.size());
            (
                p.x as f64 / scale,
                p.y as f64 / scale,
                s.width as f64 / scale,
                s.height as f64 / scale,
            )
        })
        .collect()
}

/// The main window's outer rectangle in logical pixels, while it is shown.
fn main_rect<R: Runtime>(app: &AppHandle<R>) -> Option<Rect> {
    let main = app.get_webview_window("main")?;
    if !main.is_visible().unwrap_or(false) || main.is_minimized().unwrap_or(false) {
        return None;
    }
    let scale = main.scale_factor().ok()?;
    let p = main.outer_position().ok()?.to_logical::<f64>(scale);
    let s = main.outer_size().ok()?.to_logical::<f64>(scale);
    Some((p.x, p.y, s.width, s.height))
}

/// Where a window of `size` goes to be centered over `main`.
fn centered_over((x, y, width, height): Rect, size: (f64, f64)) -> (f64, f64) {
    (x + (width - size.0) / 2.0, y + (height - size.1) / 2.0)
}

/// Whether a saved window would still be reachable: the top of its title bar
/// (the first 120 × 24 logical pixels) overlaps a screen by at least a
/// third. With no screen information the saved place is trusted.
fn on_screen(g: &WindowGeometry, screens: &[Rect]) -> bool {
    if screens.is_empty() {
        return true;
    }
    let bar = (g.x, g.y, g.width.min(120.0), 24.0);
    screens
        .iter()
        .any(|&screen| overlap(bar, screen) * 3.0 >= bar.2 * bar.3)
}

/// The area two rectangles share.
fn overlap(a: Rect, b: Rect) -> f64 {
    let w = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
    let h = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
    w.max(0.0) * h.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_and_anchor_must_be_plain_tokens() {
        let t = Target::new(Some("cost".into()), Some("cost-sources".into()));
        assert_eq!(t.page.as_deref(), Some("cost"));
        assert_eq!(t.anchor.as_deref(), Some("cost-sources"));
        for bad in [
            "",
            "Cost",
            "cost sources",
            "../index",
            "a\"b",
            "<script>",
            "x?y=1",
            &"a".repeat(41),
        ] {
            assert_eq!(token(Some(bad.into())), None, "{bad}");
        }
        assert_eq!(Target::new(None, None), Target::default());
    }

    #[test]
    fn first_opening_is_centered_over_the_main_window() {
        assert_eq!(
            centered_over((100.0, 50.0, 1200.0, 820.0), DEFAULT_SIZE),
            (100.0 + 170.0, 50.0 + 90.0)
        );
        // A main window smaller than Settings still centers it on the same axis.
        let (x, y) = centered_over((0.0, 0.0, 520.0, 480.0), DEFAULT_SIZE);
        assert_eq!((x, y), (-170.0, -80.0));
        // ...which would put its title bar above the screen: that place is
        // refused (the window is then centered on the screen).
        let placed = WindowGeometry {
            x,
            y,
            width: DEFAULT_SIZE.0,
            height: DEFAULT_SIZE.1,
        };
        assert!(!on_screen(&placed, &[(0.0, 0.0, 1512.0, 982.0)]));
    }

    #[test]
    fn a_saved_place_off_every_screen_is_not_restored() {
        let screens = [(0.0, 0.0, 1512.0, 982.0), (1512.0, 0.0, 2560.0, 1440.0)];
        let at = |x: f64, y: f64| WindowGeometry {
            x,
            y,
            width: 860.0,
            height: 640.0,
        };
        assert!(on_screen(&at(100.0, 100.0), &screens));
        // On the second screen.
        assert!(on_screen(&at(2000.0, 300.0), &screens));
        // Its title bar mostly above the top edge or past the right edge.
        assert!(!on_screen(&at(100.0, -30.0), &screens));
        assert!(!on_screen(&at(4100.0, 100.0), &screens));
        // A screen that was unplugged.
        assert!(!on_screen(&at(-2000.0, 100.0), &screens));
        // Without screen information the saved place is kept.
        assert!(on_screen(&at(-2000.0, 100.0), &[]));
    }

    #[test]
    fn the_geometry_kept_is_the_last_real_one_in_logical_pixels() {
        let mut t = Tracker {
            scale: 2.0,
            ..Tracker::default()
        };
        assert_eq!(t.geometry(), None);
        t.moved(400.0, 200.0);
        t.resized(1720.0, 1280.0);
        let g = WindowGeometry {
            x: 200.0,
            y: 100.0,
            width: 860.0,
            height: 640.0,
        };
        assert_eq!(t.geometry(), Some(g));
        // Minimized on Windows: parked at -32000 with a zero size; both ignored.
        t.moved(-32_000.0, -32_000.0);
        t.resized(0.0, 0.0);
        assert_eq!(t.geometry(), Some(g));
        // Moved to a screen on the left (negative x is a real place).
        t.moved(-2000.0, 200.0);
        assert_eq!(t.geometry().map(|g| g.x), Some(-1000.0));
        // Below the minimum size (a bad report): nothing to keep.
        t.resized(600.0, 400.0);
        assert_eq!(t.geometry(), None);
    }

    #[test]
    fn titles_lose_control_characters_and_are_capped() {
        assert_eq!(clean_title("  Settings\n"), "Settings");
        assert_eq!(clean_title(&"x".repeat(200)).chars().count(), 80);
        let state = State::default();
        assert_eq!(state.title(), "Cuotai");
        state.set_title("\u{7}");
        assert_eq!(state.title(), "Cuotai");
        state.set_title("Configurações");
        assert_eq!(state.title(), "Configurações");
    }

    #[test]
    fn one_window_opens_and_a_second_request_focuses_it_with_the_new_target() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        let dir = tempfile::tempdir().unwrap();
        let app = mock_builder()
            .manage(State::new(Some(dir.path().join("desktop.json"))))
            .build(mock_context(noop_assets()))
            .expect("test app");
        let handle = app.handle();
        open(
            handle,
            Target::new(Some("cost".into()), Some("cost-sources".into())),
        )
        .unwrap();
        assert!(handle.get_webview_window(LABEL).is_some());
        assert_eq!(
            take_target(handle),
            Some(Target::new(
                Some("cost".into()),
                Some("cost-sources".into())
            ))
        );
        // Taken once.
        assert_eq!(take_target(handle), None);
        open(handle, Target::new(Some("about".into()), None)).unwrap();
        assert_eq!(handle.webview_windows().len(), 1);
        assert_eq!(
            take_target(handle).and_then(|t| t.page),
            Some("about".into())
        );
        // The gear without a page: the window keeps its last page.
        open(handle, Target::default()).unwrap();
        assert_eq!(take_target(handle), Some(Target::default()));
    }
}
