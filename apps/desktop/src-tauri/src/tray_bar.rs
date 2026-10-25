//! PRD 0025 RF3: the menu-bar item drawn from the core's `tray::summary`.
//!
//! The backend hands every new state's accounts to [`update`]; this module
//! keeps the last ones (for the Settings preview and a preference change),
//! computes the summary with the stored preferences and changes the title,
//! the image and the tooltip only when the summary changed (RF3.5).
//!
//! macOS: a template image (it follows the light and dark menu bar) and the
//! title beside it; the outage badge is a cut-out notch; the summary is also
//! the item's accessibility description (RF7.1). Linux: the title where the
//! AppIndicator shows labels. Windows has no tray title, so the tightest
//! figure (two digits, "!" for an alert) is drawn into the icon and the
//! summary lives in the tooltip (RF3.3). The colored variant (RF3.6) uses a
//! non-template image whose meters take the level colors. Changes reach the
//! menu bar at most once per second (RF3.5). Per-platform behavior:
//! `docs/platforms.md`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use cuotai_core::tray::{
    self, AccountView, Badge, Level, TrayItem, TraySelection, TrayStyle, TraySummary,
};
use tauri::{image::Image, AppHandle};

use crate::settings::{self, TrayPrefs};

const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

#[derive(Default)]
struct State {
    accounts: Vec<AccountView>,
    /// The tooltip lines the interface sends with the menu (freshness, next notice).
    menu_tooltip: String,
    applied: Option<Applied>,
    /// RF3.5: when the menu bar last changed, and whether a delayed redraw waits.
    last_at: Option<Instant>,
    pending: bool,
}

#[derive(PartialEq, Clone)]
struct Applied {
    title: Option<String>,
    image: Vec<u8>,
    template: bool,
    tooltip: String,
}

static STATE: Mutex<State> = Mutex::new(State {
    accounts: Vec::new(),
    menu_tooltip: String::new(),
    applied: None,
    last_at: None,
    pending: false,
});

/// RF3.5: the menu-bar item changes at most once per second.
const MIN_GAP: Duration = Duration::from_secs(1);

/// How long a change must wait so two changes are at least [`MIN_GAP`]
/// apart; `None` when it can be applied now.
fn wait_before(last: Option<Instant>, now: Instant) -> Option<Duration> {
    let since = now.checked_duration_since(last?)?;
    (since < MIN_GAP).then(|| MIN_GAP - since)
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Local `HH:MM` (the title is narrow; the date is never needed there).
fn local_hhmm(at: chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&chrono::Local).format("%H:%M").to_string()
}

/// The summary of `accounts` for `style` and `selection`, now.
pub fn summarize(
    accounts: &[AccountView],
    style: TrayStyle,
    selection: &TraySelection,
) -> TraySummary {
    tray::summary(accounts, style, selection, chrono::Utc::now(), local_hhmm)
}

/// RF4.1: the preview the Settings page shows, from the current readings.
pub fn preview(style: TrayStyle, selection: &TraySelection) -> TraySummary {
    summarize(&lock().accounts, style, selection)
}

/// The current accounts' ids, dashboard order (for pruning and the account list).
pub fn account_keys() -> Vec<String> {
    lock().accounts.iter().map(|a| a.key.clone()).collect()
}

/// The current accounts as the Settings list shows them.
pub fn accounts() -> Vec<AccountView> {
    lock().accounts.clone()
}

/// A new state from the backend: remember the accounts and redraw.
pub fn update(app: &AppHandle, accounts: Vec<AccountView>) {
    let mut prefs = settings::tray_prefs();
    let keys: Vec<String> = accounts.iter().map(|a| a.key.clone()).collect();
    // RF2.2: removed accounts leave the selection when the state shows them gone.
    if !keys.is_empty() {
        if let Some(pruned) = prefs.pruned(&keys) {
            if settings::save_tray(&pruned).is_ok() {
                prefs = pruned;
            }
        }
    }
    lock().accounts = accounts;
    redraw(app, &prefs);
}

/// The interface's tooltip lines changed (language, freshness, next notice).
pub fn set_menu_tooltip(app: &AppHandle, text: String) {
    lock().menu_tooltip = text;
    redraw(app, &settings::tray_prefs());
}

/// Redraw with `prefs` (after a change in Settings).
pub fn redraw(app: &AppHandle, prefs: &TrayPrefs) {
    let locale = settings::settings_path()
        .and_then(|f| settings::read_locale(&f))
        .unwrap_or_else(|| "en-US".to_owned());
    let mut state = lock();
    let summary = summarize(&state.accounts, prefs.style, &prefs.selection());
    let next = render(
        &summary,
        prefs.colored,
        &locale,
        &state.menu_tooltip,
        Platform::current(),
    );
    if state.applied.as_ref() == Some(&next) {
        return;
    }
    let now = Instant::now();
    if let Some(wait) = wait_before(state.last_at, now) {
        // A burst of changes: the last one is drawn once the second is over.
        if !state.pending {
            state.pending = true;
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(wait);
                lock().pending = false;
                redraw(&app, &settings::tray_prefs());
            });
        }
        return;
    }
    state.applied = Some(next.clone());
    state.last_at = Some(now);
    drop(state);
    apply(app, next);
}

fn apply(app: &AppHandle, next: Applied) {
    let Some(item) = app.tray_by_id("cuotai") else {
        return;
    };
    let (w, h) = image_size(&next.image);
    let image = Image::new_owned(next.image[8..].to_vec(), w, h);
    let _ = item.set_icon_with_as_template(Some(image), next.template);
    // Windows has no tray title (RF3.3): the tooltip carries the summary.
    #[cfg(not(target_os = "windows"))]
    let _ = item.set_title(next.title.as_deref());
    let _ = item.set_tooltip(Some(next.tooltip.as_str()));
    // RF7.1: VoiceOver reads the summary, not only the hover tooltip.
    #[cfg(target_os = "macos")]
    {
        let label = next.tooltip.clone();
        let _ = item.with_inner_tray_icon(move |tray| {
            let Some(mtm) = objc2::MainThreadMarker::new() else {
                return;
            };
            if let Some(button) = tray.ns_status_item().and_then(|s| s.button(mtm)) {
                use objc2_app_kit::NSAccessibility as _;
                button.setAccessibilityLabel(Some(&objc2_foundation::NSString::from_str(&label)));
            }
        });
    }
}

/// What the platform's menu bar or tray can show (RF3.1, RF3.3, RF3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Platform {
    /// A template image that follows the light and dark menu bar (macOS).
    template: bool,
    /// A text title beside the icon (macOS, Linux AppIndicator labels).
    title: bool,
}

impl Platform {
    const MACOS: Self = Self {
        template: true,
        title: true,
    };
    const LINUX: Self = Self {
        template: false,
        title: true,
    };
    const WINDOWS: Self = Self {
        template: false,
        title: false,
    };

    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MACOS
        } else if cfg!(target_os = "windows") {
            Self::WINDOWS
        } else {
            Self::LINUX
        }
    }
}

/// Images travel as `[width u32 LE][height u32 LE][rgba…]` so `Applied` compares cheaply.
fn image_size(buf: &[u8]) -> (u32, u32) {
    let w = u32::from_le_bytes(buf[0..4].try_into().unwrap_or_default());
    let h = u32::from_le_bytes(buf[4..8].try_into().unwrap_or_default());
    (w, h)
}

fn pack(rgba: Vec<u8>, w: u32, h: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(rgba.len() + 8);
    buf.extend_from_slice(&w.to_le_bytes());
    buf.extend_from_slice(&h.to_le_bytes());
    buf.extend(rgba);
    buf
}

/// RF3: what to set for `summary` on `platform`.
fn render(
    summary: &TraySummary,
    colored: bool,
    locale: &str,
    menu_tooltip: &str,
    platform: Platform,
) -> Applied {
    // The template image only exists on macOS; elsewhere the colored logo stays.
    let template = platform.template && !colored;
    let (mut rgba, w, h) = match summary.style {
        TrayStyle::Meters if !summary.items.is_empty() => meters(&summary.items, colored),
        _ => logo(summary.badge, template),
    };
    // RF3.3/RF3.4: without a title, a text style draws its tightest figure into
    // the icon; Meters falls back to the icon and the tooltip.
    if !platform.title && summary.style != TrayStyle::Meters {
        if let Some(figure) = icon_figure(summary) {
            draw_figure(&mut rgba, w, h, &figure);
        }
    }
    let label = summary.tooltip.resolve(locale);
    let tooltip = if menu_tooltip.is_empty() {
        label
    } else {
        format!("{label}\n{menu_tooltip}")
    };
    let title = summary
        .title
        .clone()
        .filter(|t| !t.is_empty() && platform.title);
    Applied {
        title,
        image: pack(rgba, w, h),
        template,
        tooltip,
    }
}

/// RF3.3: the figure drawn into the icon of a text style: "!" for an active
/// alert, else the tightest remaining share in up to two digits (stale
/// readings count only when every reading is stale), "-" when unknown;
/// `None` for Icon only or an empty selection.
fn icon_figure(summary: &TraySummary) -> Option<String> {
    if summary.style == TrayStyle::IconOnly || summary.items.is_empty() {
        return None;
    }
    if summary.items.iter().any(|i| i.alert) {
        return Some("!".into());
    }
    let fresh: Vec<&TrayItem> = summary.items.iter().filter(|i| !i.stale).collect();
    let pool = if fresh.is_empty() {
        summary.items.iter().collect()
    } else {
        fresh
    };
    let tightest = pool
        .iter()
        .filter_map(|i| if i.exhausted { Some(0.0) } else { i.remaining })
        .fold(None, |m: Option<f64>, r| Some(m.map_or(r, |m| m.min(r))));
    Some(match tightest {
        None => "-".into(),
        Some(r) => format!("{}", (r * 100.0).round().clamp(0.0, 99.0) as u8),
    })
}

/// 3×5 glyphs of the icon figure, one row per entry (4 = left column).
fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        '!' => [2, 2, 2, 0, 2],
        _ => [0, 0, 7, 0, 0],
    }
}

/// RF3.3: the figure in white on a dark plate in the icon's lower-right
/// corner, legible on light and dark taskbars.
pub(crate) fn draw_figure(rgba: &mut [u8], width: u32, height: u32, text: &str) {
    let chars: Vec<char> = text.chars().take(2).collect();
    if chars.is_empty() {
        return;
    }
    let n = chars.len() as u32;
    // Glyphs about half the icon's height.
    let scale = (height / 2 / 5).max(1);
    let (gw, gh, gap) = (3 * scale, 5 * scale, scale);
    let text_w = n * gw + (n - 1) * gap;
    let pad = scale;
    let plate_w = (text_w + 2 * pad).min(width);
    let plate_h = (gh + 2 * pad).min(height);
    let (x0, y0) = (width - plate_w, height - plate_h);
    let mut put = |x: u32, y: u32, px: [u8; 4]| {
        if x < width && y < height {
            let at = ((y * width + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&px);
        }
    };
    for y in y0..height {
        for x in x0..width {
            put(x, y, [0x1e, 0x1e, 0x24, 0xff]);
        }
    }
    for (i, c) in chars.iter().enumerate() {
        let gx = x0 + pad + i as u32 * (gw + gap);
        for (row, bits) in glyph(*c).iter().enumerate() {
            for col in 0..3u32 {
                if bits & (4 >> col) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        put(
                            gx + col * scale + dx,
                            y0 + pad + row as u32 * scale + dy,
                            [0xff, 0xff, 0xff, 0xff],
                        );
                    }
                }
            }
        }
    }
}

/// The app mark with the badge.
fn logo(badge: Badge, template: bool) -> (Vec<u8>, u32, u32) {
    let Ok(icon) = Image::from_bytes(TRAY_PNG) else {
        return (vec![0; 4], 1, 1);
    };
    let (w, h) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    if badge != Badge::None {
        if template {
            cut_notch(&mut rgba, w, h, badge == Badge::Alert);
        } else {
            paint_dot(&mut rgba, w, h, badge == Badge::Alert);
        }
    }
    (rgba, w, h)
}

/// The DS-ID-5 level colors of the default theme's status roles (colored variant).
fn level_rgb(level: Option<Level>) -> [u8; 3] {
    match level {
        Some(Level::Good) => [0x50, 0xfa, 0x7b],
        Some(Level::Warn) => [0xf1, 0xa9, 0x3b],
        Some(Level::Bad) => [0xff, 0x55, 0x55],
        None => [0x80, 0x80, 0x80],
    }
}

/// PRD 0011 RF4.2 (non-template): a dot with a light ring in the bottom-right
/// corner; red for an outage, amber for an alert.
pub(crate) fn paint_dot(rgba: &mut [u8], width: u32, height: u32, alert: bool) {
    let size = width.min(height) as f32;
    let radius = size / 6.0;
    let ring = (size / 22.0).max(1.0);
    let (cx, cy) = (width as f32 - radius - ring, height as f32 - radius - ring);
    let fill = if alert {
        [0xf1, 0xa9, 0x3b, 0xff]
    } else {
        [0xe5, 0x48, 0x4d, 0xff]
    };
    for y in 0..height {
        for x in 0..width {
            let distance = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
            let color = if distance <= radius {
                fill
            } else if distance <= radius + ring {
                [0xff, 0xff, 0xff, 0xff]
            } else {
                continue;
            };
            let at = ((y * width + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&color);
        }
    }
}

/// RF3.1 (template): a cut-out ring around a solid dot in the bottom-right
/// corner, so the badge reads by shape on any menu bar; an alert leaves the
/// dot hollow.
pub(crate) fn cut_notch(rgba: &mut [u8], width: u32, height: u32, hollow: bool) {
    let size = width.min(height) as f32;
    let radius = size / 6.0;
    let gap = (size / 14.0).max(1.0);
    let (cx, cy) = (width as f32 - radius - gap, height as f32 - radius - gap);
    for y in 0..height {
        for x in 0..width {
            let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
            let at = ((y * width + x) * 4) as usize;
            if d <= radius && !(hollow && d <= radius * 0.5) {
                rgba[at..at + 4].copy_from_slice(&[0, 0, 0, 0xff]);
            } else if d <= radius + gap {
                rgba[at + 3] = 0;
            }
        }
    }
}

/// RF3.2: up to four continuous bars, one per item, each the remaining share
/// (a sliver below 1 %), an exhausted bar as an outline and an unknown one as
/// its faint track only. Monochrome unless `colored`. 36 px high (18 pt at 2×).
pub(crate) fn meters(items: &[TrayItem], colored: bool) -> (Vec<u8>, u32, u32) {
    const W: u32 = 44;
    const H: u32 = 36;
    let n = items.len().clamp(1, tray::AUTO_METERS) as u32;
    let gap = 3u32;
    let bar = ((H - gap * (n + 1)) / n).clamp(4, 10);
    let top = (H - (bar * n + gap * (n - 1))) / 2;
    let mut rgba = vec![0u8; (W * H * 4) as usize];
    for (i, item) in items.iter().take(n as usize).enumerate() {
        let y0 = top + i as u32 * (bar + gap);
        let rgb = if colored {
            level_rgb(item.level)
        } else {
            [0, 0, 0]
        };
        let share = item.remaining.unwrap_or(0.0).clamp(0.0, 1.0);
        let mut fill = (share * f64::from(W)).round() as u32;
        if share > 0.0 {
            fill = fill.max(bar); // the sliver: a dot of the bar's height
        }
        if share < 1.0 {
            fill = fill.min(W - 1);
        }
        let r = bar as f32 / 2.0;
        for y in y0..y0 + bar {
            for x in 0..W {
                // Pill mask over the whole track.
                let cy = y0 as f32 + r;
                let px = (x as f32 + 0.5).clamp(r, W as f32 - r);
                let inside = (x as f32 + 0.5 - px).hypot(y as f32 + 0.5 - cy) <= r;
                if !inside {
                    continue;
                }
                let edge = y == y0 || y == y0 + bar - 1 || x == 0 || x == W - 1;
                let alpha = if item.exhausted {
                    if edge {
                        0xff
                    } else {
                        0
                    }
                } else if item.remaining.is_some() && x < fill {
                    0xff
                } else {
                    0x55
                };
                let at = ((y * W + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
            }
        }
    }
    (rgba, W, H)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(remaining: Option<f64>, exhausted: bool) -> TrayItem {
        TrayItem {
            key: "acc_a".into(),
            provider: "claude".into(),
            name: "Work".into(),
            remaining,
            level: remaining.map(Level::of),
            stale: false,
            exhausted,
            alert: false,
            on_extra: false,
            figure: String::new(),
        }
    }

    fn alpha_row(rgba: &[u8], w: u32, y: u32) -> Vec<u8> {
        (0..w)
            .map(|x| rgba[((y * w + x) * 4 + 3) as usize])
            .collect()
    }

    #[test]
    fn meters_fill_by_length_and_keep_a_sliver() {
        let (rgba, w, h) = meters(&[item(Some(0.5), false), item(Some(0.004), false)], false);
        assert_eq!((w, h), (44, 36));
        // Monochrome: every painted pixel is black.
        assert!(rgba.chunks(4).all(|p| p[3] == 0 || p[..3] == [0, 0, 0]));
        let n = 2u32;
        let bar = ((h - 3 * (n + 1)) / n).clamp(4, 10);
        let top = (h - (bar * n + 3 * (n - 1))) / 2;
        let mid = |i: u32| top + i * (bar + 3) + bar / 2;
        let full = |row: Vec<u8>| row.iter().filter(|a| **a == 0xff).count();
        let half = full(alpha_row(&rgba, w, mid(0)));
        assert!((20..=24).contains(&half), "{half}");
        // Below 1 % still shows a visible sliver.
        assert!(full(alpha_row(&rgba, w, mid(1))) >= 4);
    }

    #[test]
    fn exhausted_is_an_outline_and_unknown_has_no_fill() {
        let (rgba, w, h) = meters(&[item(Some(0.0), true), item(None, false)], false);
        let n = 2u32;
        let bar = ((h - 3 * (n + 1)) / n).clamp(4, 10);
        let top = (h - (bar * n + 3 * (n - 1))) / 2;
        let row0 = alpha_row(&rgba, w, top + bar / 2);
        assert_eq!(row0[w as usize / 2], 0, "hollow inside");
        let row1 = alpha_row(&rgba, w, top + bar + 3 + bar / 2);
        assert!(row1.iter().all(|a| *a != 0xff), "no fill for unknown");
    }

    #[test]
    fn colored_meters_use_the_level_colors() {
        let (rgba, _, _) = meters(&[item(Some(0.9), false)], true);
        assert!(rgba
            .chunks(4)
            .any(|p| p[3] == 0xff && p[..3] == level_rgb(Some(Level::Good))));
    }

    #[test]
    fn outage_dot_marks_only_the_bottom_right_corner() {
        let (width, height) = (44, 44);
        let blank = vec![0u8; (width * height * 4) as usize];
        let mut rgba = blank.clone();
        paint_dot(&mut rgba, width, height, false);
        let pixel = |x: u32, y: u32| {
            let at = ((y * width + x) * 4) as usize;
            <[u8; 4]>::try_from(&rgba[at..at + 4]).unwrap()
        };
        assert_eq!(pixel(35, 35), [0xe5, 0x48, 0x4d, 0xff]);
        assert_eq!(pixel(43, 35)[..3], [0xff, 0xff, 0xff]);
        for (x, y) in [(0, 0), (22, 22), (43, 0), (0, 43), (22, 30)] {
            assert_eq!(pixel(x, y), [0, 0, 0, 0], "({x}, {y})");
        }
        assert_ne!(rgba, blank);
    }

    #[test]
    fn template_badge_is_a_cut_out_never_a_color() {
        let (w, h) = (44u32, 44u32);
        let mut rgba = vec![0xff; (w * h * 4) as usize];
        cut_notch(&mut rgba, w, h, false);
        assert!(rgba.chunks(4).any(|p| p[3] == 0), "a transparent ring");
        assert!(rgba
            .chunks(4)
            .all(|p| p[3] == 0 || p[..3] == [0, 0, 0] || p[..3] == [0xff, 0xff, 0xff]));
    }

    #[test]
    fn summary_maps_to_title_template_and_tooltip() {
        let accounts = vec![AccountView {
            key: "acc_a".into(),
            provider: "claude".into(),
            provider_label: "Claude".into(),
            name: "Work".into(),
            remaining: Some(0.22),
            ..AccountView::default()
        }];
        let s = summarize(&accounts, TrayStyle::TightestPercent, &TraySelection::Auto);
        let p = Platform::current();
        let a = render(&s, false, "en-US", "oldest reading: 2 min ago", p);
        assert_eq!(a.template, cfg!(target_os = "macos"));
        if cfg!(target_os = "windows") {
            assert_eq!(a.title, None);
        } else {
            assert_eq!(a.title.as_deref(), Some("22% · Work"));
        }
        assert_eq!(
            a.tooltip,
            "Cuotai — Work 22% left\noldest reading: 2 min ago"
        );
        let colored = render(&s, true, "en-US", "", p);
        assert!(!colored.template);
        let m = render(
            &summarize(&accounts, TrayStyle::Meters, &TraySelection::Auto),
            false,
            "en-US",
            "",
            p,
        );
        assert_eq!(m.title, None);
        assert_eq!(image_size(&m.image), (44, 36));
    }

    fn work(remaining: Option<f64>) -> Vec<AccountView> {
        vec![AccountView {
            key: "acc_a".into(),
            provider: "claude".into(),
            provider_label: "Claude".into(),
            name: "Work".into(),
            remaining,
            ..AccountView::default()
        }]
    }

    /// RF3.1/RF3.3/RF3.4 per platform: macOS and Linux keep the title (macOS in
    /// a template image); Windows has no title and draws the figure in the
    /// icon, Meters falling back to the icon and the tooltip.
    #[test]
    fn each_platform_gets_its_title_image_and_tooltip() {
        let s = summarize(
            &work(Some(0.22)),
            TrayStyle::TightestPercent,
            &TraySelection::Auto,
        );
        let mac = render(&s, false, "en-US", "", Platform::MACOS);
        assert!(mac.template);
        assert_eq!(mac.title.as_deref(), Some("22% · Work"));
        let linux = render(&s, false, "en-US", "", Platform::LINUX);
        assert!(!linux.template);
        assert_eq!(linux.title.as_deref(), Some("22% · Work"));
        let win = render(&s, false, "en-US", "", Platform::WINDOWS);
        assert_eq!(win.title, None);
        assert!(!win.template);
        assert_ne!(win.image, linux.image, "the figure is drawn into the icon");
        assert_eq!(win.tooltip, "Cuotai — Work 22% left");
        let meters = summarize(&work(Some(0.22)), TrayStyle::Meters, &TraySelection::Auto);
        assert_eq!(
            render(&meters, false, "en-US", "", Platform::WINDOWS).image,
            render(&meters, false, "en-US", "", Platform::LINUX).image,
            "Meters falls back to the icon and the tooltip"
        );
        // The colored variant is never a template, on any platform.
        for p in [Platform::MACOS, Platform::LINUX, Platform::WINDOWS] {
            assert!(!render(&s, true, "en-US", "", p).template);
        }
    }

    #[test]
    fn icon_figure_is_two_digits_an_alert_or_a_dash() {
        let fig = |accounts: Vec<AccountView>, style| {
            icon_figure(&summarize(&accounts, style, &TraySelection::Auto))
        };
        assert_eq!(
            fig(work(Some(0.224)), TrayStyle::TightestPercent).as_deref(),
            Some("22")
        );
        assert_eq!(
            fig(work(Some(1.0)), TrayStyle::LogosPercent).as_deref(),
            Some("99")
        );
        assert_eq!(
            fig(work(None), TrayStyle::TightestPercent).as_deref(),
            Some("-")
        );
        assert_eq!(fig(work(Some(0.5)), TrayStyle::IconOnly), None);
        let mut alert = work(Some(0.5));
        alert[0].alert = true;
        assert_eq!(fig(alert, TrayStyle::TightestPercent).as_deref(), Some("!"));
    }

    #[test]
    fn the_figure_is_white_on_a_dark_plate_in_the_corner() {
        let (w, h) = (44u32, 44u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        draw_figure(&mut rgba, w, h, "22");
        let px = |x: u32, y: u32| {
            let at = ((y * w + x) * 4) as usize;
            rgba[at..at + 4].to_vec()
        };
        assert_eq!(px(0, 0), vec![0, 0, 0, 0], "the top-left stays the mark");
        assert_eq!(px(w - 1, h - 1), vec![0x1e, 0x1e, 0x24, 0xff]);
        assert!(rgba.chunks(4).any(|p| p == [0xff, 0xff, 0xff, 0xff]));
    }

    /// RF3.5: at most one change per second.
    #[test]
    fn changes_wait_for_the_second_to_pass() {
        let now = Instant::now();
        assert_eq!(wait_before(None, now), None);
        assert_eq!(wait_before(Some(now), now), Some(MIN_GAP));
        let later = now + Duration::from_millis(400);
        assert_eq!(
            wait_before(Some(now), later),
            Some(Duration::from_millis(600))
        );
        assert_eq!(wait_before(Some(now), now + MIN_GAP), None);
    }

    /// PRD 0022 RF17.8: the default macOS build uses a template image, and the
    /// outage state changes the template image's shape, not its colors.
    #[test]
    fn outage_changes_the_template_image_not_its_color() {
        let (plain, w, h) = logo(Badge::None, true);
        let (outage, _, _) = logo(Badge::Outage, true);
        assert_ne!(plain, outage);
        let colors = |rgba: &[u8]| {
            let mut c: Vec<[u8; 3]> = rgba
                .chunks(4)
                .filter(|p| p[3] != 0)
                .map(|p| [p[0], p[1], p[2]])
                .collect();
            c.sort();
            c.dedup();
            c
        };
        let before = colors(&plain);
        assert!(
            colors(&outage)
                .iter()
                .all(|c| before.contains(c) || *c == [0, 0, 0]),
            "no new color"
        );
        assert_eq!((w, h), (44, 44));
        let s = summarize(&work(Some(0.5)), TrayStyle::IconOnly, &TraySelection::Auto);
        assert!(render(&s, false, "en-US", "", Platform::MACOS).template);
    }
}
