//! PRD 0054 RF7.1/RF7.2: the main window comes back where it was.
//!
//! `desktop.json` keeps `main_window { x, y, width, height }`: the outer
//! position in the virtual desktop's physical pixels and the inner size in
//! logical pixels. The window's own events feed a [`Tracker`] (Windows'
//! minimized parking place and zero sizes are ignored); the geometry is
//! written a short while after the last move or resize, when the window is
//! hidden and when the app exits, and never while it is minimized,
//! maximized or full screen (only the normal geometry is kept).
//!
//! Before the window is first shown, [`placement`] picks the monitor its
//! title bar overlaps most in physical pixels, clamps the size to that
//! monitor and keeps at least 120 × 24 logical pixels of the title bar on
//! it; when no saved monitor remains it centers the window on the primary
//! one. Each monitor's scale converts only that monitor's sizes: monitor
//! origins are never divided by their own scale into one shared space.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewWindow, WindowEvent,
};

use crate::settings::{self, WindowGeometry};

/// The window label in `tauri.conf.json`.
pub const LABEL: &str = "main";
/// `tauri.conf.json`'s size, in logical pixels.
pub const DEFAULT_SIZE: (f64, f64) = (1200.0, 820.0);
/// `tauri.conf.json`'s minimum size, in logical pixels.
pub const MIN_SIZE: (f64, f64) = (520.0, 480.0);
/// The part of the title bar that must stay on a screen, in logical pixels.
const TITLE_BAR: (f64, f64) = (120.0, 24.0);
/// How long the window must stay still before its geometry is written.
const DEBOUNCE: Duration = Duration::from_secs(2);

/// A monitor as the OS reports it: origin and size in physical pixels,
/// with its own scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

impl Screen {
    fn scale(&self) -> f64 {
        if self.scale > 0.0 && self.scale.is_finite() {
            self.scale
        } else {
            1.0
        }
    }

    fn overlap(&self, (x, y, w, h): (f64, f64, f64, f64)) -> f64 {
        let width = (x + w).min(self.x + self.width) - x.max(self.x);
        let height = (y + h).min(self.y + self.height) - y.max(self.y);
        width.max(0.0) * height.max(0.0)
    }
}

impl<'a> From<&'a tauri::Monitor> for Screen {
    fn from(m: &'a tauri::Monitor) -> Self {
        let (p, s) = (m.position(), m.size());
        Self {
            x: p.x as f64,
            y: p.y as f64,
            width: s.width as f64,
            height: s.height as f64,
            scale: m.scale_factor(),
        }
    }
}

/// Where the window goes: outer position in physical pixels and inner size
/// in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// RF7.2: where to show a window saved as `saved` on `screens`, `primary`
/// being the index of the primary monitor. `None` leaves the window where
/// the OS put it: nothing saved and nothing to center on.
pub fn placement(
    saved: Option<WindowGeometry>,
    screens: &[Screen],
    primary: Option<usize>,
) -> Option<Placement> {
    let saved = saved.and_then(|g| g.checked(MIN_SIZE));
    // The monitor the saved title bar overlaps most (each measured with its
    // own scale), if any.
    let home = saved.and_then(|g| {
        screens
            .iter()
            .map(|screen| {
                let s = screen.scale();
                let bar = (g.x, g.y, TITLE_BAR.0 * s, TITLE_BAR.1 * s);
                (screen, screen.overlap(bar))
            })
            .filter(|(_, area)| *area > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(screen, _)| *screen)
    });
    let size = saved.map_or(DEFAULT_SIZE, |g| (g.width, g.height));
    match (home, saved) {
        (Some(screen), Some(g)) => {
            let s = screen.scale();
            let (width, height) = fit(size, &screen);
            let (bar_w, bar_h) = (TITLE_BAR.0 * s, TITLE_BAR.1 * s);
            let outer_w = width * s;
            let x = clamp(
                g.x,
                screen.x + bar_w - outer_w,
                screen.x + screen.width - bar_w,
            );
            let y = clamp(g.y, screen.y, screen.y + screen.height - bar_h);
            Some(Placement {
                x,
                y,
                width,
                height,
            })
        }
        _ => {
            let screen = primary
                .and_then(|i| screens.get(i))
                .or_else(|| screens.first())?;
            let s = screen.scale();
            let (width, height) = fit(size, screen);
            Some(Placement {
                x: screen.x + (screen.width - width * s) / 2.0,
                y: screen.y + (screen.height - height * s) / 2.0,
                width,
                height,
            })
        }
    }
}

/// `size` (logical) no larger than `screen` and no smaller than the minimum.
fn fit((width, height): (f64, f64), screen: &Screen) -> (f64, f64) {
    let s = screen.scale();
    (
        width.min(screen.width / s).max(MIN_SIZE.0),
        height.min(screen.height / s).max(MIN_SIZE.1),
    )
}

/// `value` within `lo..=hi`, preferring `lo` when the range is empty.
fn clamp(value: f64, lo: f64, hi: f64) -> f64 {
    value.min(hi).max(lo)
}

/// What the window last reported about itself, from its own events (no
/// window getter runs inside the event handler).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tracker {
    /// Outer position, physical pixels.
    position: Option<(f64, f64)>,
    /// Inner size, physical pixels.
    size: Option<(f64, f64)>,
    scale: f64,
}

impl Tracker {
    /// Windows parks a minimized window at (-32000, -32000).
    pub fn moved(&mut self, x: f64, y: f64) {
        if x > -32_000.0 || y > -32_000.0 {
            self.position = Some((x, y));
        }
    }

    /// A minimized window reports a zero size on Windows.
    pub fn resized(&mut self, width: f64, height: f64) {
        if width > 0.0 && height > 0.0 {
            self.size = Some((width, height));
        }
    }

    pub fn scaled(&mut self, scale: f64) {
        self.scale = scale;
    }

    /// The physical outer position and the logical inner size, when both
    /// are known and valid.
    pub fn geometry(&self) -> Option<WindowGeometry> {
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        let ((x, y), (width, height)) = (self.position?, self.size?);
        WindowGeometry {
            x,
            y,
            width: width / scale,
            height: height / scale,
        }
        .checked(MIN_SIZE)
    }
}

/// The shell's state: the tracker, the file and a counter that makes only
/// the last of several quick moves write.
#[derive(Default)]
pub struct State {
    tracker: Mutex<Tracker>,
    file: Option<PathBuf>,
    changes: AtomicU64,
}

impl State {
    pub fn new(file: Option<PathBuf>) -> Self {
        Self {
            file,
            ..Self::default()
        }
    }
}

/// RF7.2: size and place `window` from `desktop.json` before it is shown.
pub fn restore<R: Runtime>(window: &WebviewWindow<R>) {
    let state = window.state::<Arc<State>>();
    let saved = state
        .file
        .as_deref()
        .and_then(|file| settings::read_main_window(file, MIN_SIZE));
    let monitors = window.available_monitors().unwrap_or_default();
    let screens: Vec<Screen> = monitors.iter().map(Screen::from).collect();
    let primary = window.primary_monitor().ok().flatten().and_then(|p| {
        let p = Screen::from(&p);
        screens.iter().position(|s| s.x == p.x && s.y == p.y)
    });
    // Nothing saved: the window keeps the place the OS gave it.
    if saved.is_none() {
        return;
    }
    if let Some(place) = placement(saved, &screens, primary) {
        let _ = window.set_size(LogicalSize::new(place.width, place.height));
        let _ = window.set_position(PhysicalPosition::new(place.x, place.y));
    }
}

/// RF7.1: follow `window`'s geometry and write it after it settles.
pub fn track<R: Runtime>(window: &WebviewWindow<R>) {
    let state = Arc::clone(&window.state::<Arc<State>>());
    {
        let mut tracker = state.tracker.lock().unwrap_or_else(PoisonError::into_inner);
        tracker.scaled(window.scale_factor().unwrap_or(1.0));
        if let Ok(p) = window.outer_position() {
            tracker.moved(p.x as f64, p.y as f64);
        }
        if let Ok(s) = window.inner_size() {
            tracker.resized(s.width as f64, s.height as f64);
        }
    }
    let handle = window.app_handle().clone();
    window.on_window_event(move |event| {
        let changed = {
            let mut tracker = state.tracker.lock().unwrap_or_else(PoisonError::into_inner);
            match event {
                WindowEvent::Moved(p) => {
                    tracker.moved(p.x as f64, p.y as f64);
                    true
                }
                WindowEvent::Resized(s) => {
                    tracker.resized(s.width as f64, s.height as f64);
                    true
                }
                WindowEvent::ScaleFactorChanged {
                    scale_factor,
                    new_inner_size,
                    ..
                } => {
                    tracker.scaled(*scale_factor);
                    tracker.resized(new_inner_size.width as f64, new_inner_size.height as f64);
                    true
                }
                _ => false,
            }
        };
        if changed {
            let change = state.changes.fetch_add(1, Ordering::SeqCst) + 1;
            let state = Arc::clone(&state);
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(DEBOUNCE).await;
                if state.changes.load(Ordering::SeqCst) == change {
                    save(&handle);
                }
            });
        }
    });
}

/// Write the tracked geometry now, unless the window is minimized,
/// maximized or full screen (its normal geometry is the one to keep).
pub fn save<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let abnormal = window.is_minimized().unwrap_or(false)
        || window.is_maximized().unwrap_or(false)
        || window.is_fullscreen().unwrap_or(false);
    if abnormal {
        return;
    }
    let state = window.state::<Arc<State>>();
    let geometry = state
        .tracker
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .geometry();
    if let (Some(g), Some(file)) = (geometry, state.file.as_deref()) {
        if settings::read_main_window(file, MIN_SIZE) != Some(g) {
            let _ = settings::write_main_window(file, &g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(x: f64, y: f64, width: f64, height: f64) -> WindowGeometry {
        WindowGeometry {
            x,
            y,
            width,
            height,
        }
    }

    fn screen(x: f64, y: f64, width: f64, height: f64, scale: f64) -> Screen {
        Screen {
            x,
            y,
            width,
            height,
            scale,
        }
    }

    /// A 2× laptop (3024 × 1964 physical) with a 1× monitor on its left
    /// (negative origin) and one on its right.
    fn desk() -> Vec<Screen> {
        vec![
            screen(0.0, 0.0, 3024.0, 1964.0, 2.0),
            screen(-2560.0, -200.0, 2560.0, 1440.0, 1.0),
            screen(3024.0, 0.0, 1920.0, 1080.0, 1.0),
        ]
    }

    #[test]
    fn a_saved_place_on_a_present_monitor_comes_back_as_it_was() {
        let screens = desk();
        let on_left = placement(Some(g(-2000.0, 100.0, 1000.0, 700.0)), &screens, Some(0));
        assert_eq!(
            on_left,
            Some(Placement {
                x: -2000.0,
                y: 100.0,
                width: 1000.0,
                height: 700.0
            })
        );
        let on_laptop = placement(Some(g(400.0, 300.0, 1200.0, 820.0)), &screens, Some(0));
        assert_eq!(on_laptop.map(|p| (p.x, p.y)), Some((400.0, 300.0)));
    }

    #[test]
    fn the_size_follows_the_chosen_monitor_scale_only() {
        // 1700 × 1200 logical does not fit the 2× laptop (1512 × 982 logical).
        let screens = desk();
        let p = placement(Some(g(100.0, 100.0, 1700.0, 1200.0)), &screens, Some(0)).unwrap();
        assert_eq!((p.width, p.height), (1512.0, 982.0));
        // The same size on the 1× right monitor is clamped to 1920 × 1080.
        let p = placement(Some(g(3100.0, 50.0, 2400.0, 1200.0)), &screens, Some(0)).unwrap();
        assert_eq!((p.width, p.height), (1920.0, 1080.0));
        // Too wide for the right monitor: pulled back so the title bar shows.
        assert!(p.x <= 3024.0 + 1920.0 - 120.0 && p.x >= 3024.0 + 120.0 - 1920.0);
    }

    #[test]
    fn the_title_bar_stays_reachable() {
        let screens = desk();
        // Mostly above the laptop's top edge: brought down to it.
        let p = placement(Some(g(400.0, -10.0, 800.0, 600.0)), &screens, Some(0)).unwrap();
        assert_eq!(p.y, 0.0);
        // Hanging off the right of the right monitor: 120 logical px stay.
        let p = placement(Some(g(4900.0, 100.0, 800.0, 600.0)), &screens, Some(0)).unwrap();
        assert_eq!(p.x, 3024.0 + 1920.0 - 120.0);
        // At the bottom of the left monitor: the bar's 24 px stay.
        let p = placement(Some(g(-2400.0, 1235.0, 800.0, 600.0)), &screens, Some(0)).unwrap();
        assert_eq!(p.y, -200.0 + 1440.0 - 24.0);
    }

    #[test]
    fn an_unplugged_monitor_centers_the_window_on_the_primary_one() {
        let laptop_only = vec![screen(0.0, 0.0, 3024.0, 1964.0, 2.0)];
        let p = placement(
            Some(g(-2000.0, 100.0, 1000.0, 700.0)),
            &laptop_only,
            Some(0),
        )
        .unwrap();
        assert_eq!((p.width, p.height), (1000.0, 700.0));
        assert_eq!(
            (p.x, p.y),
            ((3024.0 - 2000.0) / 2.0, (1964.0 - 1400.0) / 2.0)
        );
        // The primary is not always the first one listed.
        let screens = vec![
            screen(-1920.0, 0.0, 1920.0, 1080.0, 1.0),
            screen(0.0, 0.0, 2560.0, 1440.0, 1.0),
        ];
        let p = placement(Some(g(9000.0, 9000.0, 800.0, 600.0)), &screens, Some(1)).unwrap();
        assert_eq!((p.x, p.y), (880.0, 420.0));
    }

    #[test]
    fn oversize_or_invalid_values_never_produce_an_unreachable_window() {
        let laptop = vec![screen(0.0, 0.0, 3024.0, 1964.0, 2.0)];
        // Larger than the screen: clamped to it.
        let p = placement(Some(g(0.0, 0.0, 9000.0, 9000.0)), &laptop, Some(0)).unwrap();
        assert_eq!((p.width, p.height), (1512.0, 982.0));
        // Below the minimum or not a number: as if nothing were saved.
        assert_eq!(
            placement(Some(g(0.0, 0.0, 100.0, 100.0)), &laptop, Some(0)).map(|p| p.width),
            Some(1200.0)
        );
        assert_eq!(
            placement(Some(g(f64::NAN, 0.0, 800.0, 600.0)), &laptop, Some(0)).map(|p| p.width),
            Some(1200.0)
        );
        // No monitor information at all: left to the OS.
        assert_eq!(placement(Some(g(0.0, 0.0, 800.0, 600.0)), &[], None), None);
    }

    #[test]
    fn the_tracker_keeps_the_last_normal_geometry() {
        let mut t = Tracker::default();
        t.scaled(2.0);
        assert_eq!(t.geometry(), None);
        t.moved(400.0, 200.0);
        t.resized(2400.0, 1640.0);
        let kept = g(400.0, 200.0, 1200.0, 820.0);
        assert_eq!(t.geometry(), Some(kept));
        // Minimized on Windows: parked with a zero size; both ignored.
        t.moved(-32_000.0, -32_000.0);
        t.resized(0.0, 0.0);
        assert_eq!(t.geometry(), Some(kept));
        // Moved to a 1× monitor on the left: physical position as reported.
        t.scaled(1.0);
        t.moved(-2000.0, 100.0);
        t.resized(1200.0, 820.0);
        assert_eq!(t.geometry(), Some(g(-2000.0, 100.0, 1200.0, 820.0)));
    }
}
