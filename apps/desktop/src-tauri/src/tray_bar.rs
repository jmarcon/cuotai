//! PRD 0025 RF3: the menu-bar item drawn from the core's `tray::summary`.
//!
//! The backend hands every new state's accounts to [`update`]; this module
//! keeps the last ones (for the Settings preview and a preference change),
//! computes the summary with the stored preferences and changes the title,
//! the image and the tooltip only when the summary changed (RF3.5).
//!
//! macOS: a template image (it follows the light and dark menu bar) and the
//! title beside it; the outage badge is a cut-out notch. Linux: the title
//! where the AppIndicator shows labels. Windows has no tray title, so the
//! summary lives in the tooltip (RF3.3). The colored variant (RF3.6) uses a
//! non-template image whose meters take the level colors.

use std::sync::Mutex;

use cuotai_core::tray::{self, AccountView, Badge, Level, TrayItem, TraySelection, TrayStyle, TraySummary};
use tauri::{image::Image, AppHandle};

use crate::settings::{self, TrayPrefs};

const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

#[derive(Default)]
struct State {
    accounts: Vec<AccountView>,
    /// The tooltip lines the interface sends with the menu (freshness, next notice).
    menu_tooltip: String,
    applied: Option<Applied>,
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
});

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Local `HH:MM` (the title is narrow; the date is never needed there).
fn local_hhmm(at: chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&chrono::Local).format("%H:%M").to_string()
}

/// The summary of `accounts` for `style` and `selection`, now.
pub fn summarize(accounts: &[AccountView], style: TrayStyle, selection: &TraySelection) -> TraySummary {
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
    let next = render(&summary, prefs.colored, &locale, &state.menu_tooltip);
    if state.applied.as_ref() == Some(&next) {
        return;
    }
    state.applied = Some(next.clone());
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

/// RF3: what to set for `summary` on this platform.
fn render(summary: &TraySummary, colored: bool, locale: &str, menu_tooltip: &str) -> Applied {
    // The template image only exists on macOS; elsewhere the colored logo stays.
    let template = cfg!(target_os = "macos") && !colored;
    let (rgba, w, h) = match summary.style {
        TrayStyle::Meters if !summary.items.is_empty() => meters(&summary.items, colored),
        _ => logo(summary.badge, template),
    };
    let label = summary.tooltip.resolve(locale);
    let tooltip = if menu_tooltip.is_empty() {
        label
    } else {
        format!("{label}\n{menu_tooltip}")
    };
    let title = summary.title.clone().filter(|t| !t.is_empty());
    // On Windows the title is part of the tooltip's first line.
    let title = if cfg!(target_os = "windows") { None } else { title };
    Applied {
        title,
        image: pack(rgba, w, h),
        template,
        tooltip,
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
        let rgb = if colored { level_rgb(item.level) } else { [0, 0, 0] };
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
                    if edge { 0xff } else { 0 }
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
        (0..w).map(|x| rgba[((y * w + x) * 4 + 3) as usize]).collect()
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
        assert!(rgba.chunks(4).any(|p| p[3] == 0xff && p[..3] == level_rgb(Some(Level::Good))));
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
        let a = render(&s, false, "en-US", "oldest reading: 2 min ago");
        assert_eq!(a.template, cfg!(target_os = "macos"));
        if cfg!(target_os = "windows") {
            assert_eq!(a.title, None);
        } else {
            assert_eq!(a.title.as_deref(), Some("22% · Work"));
        }
        assert_eq!(a.tooltip, "Cuotai — Work 22% left\noldest reading: 2 min ago");
        let colored = render(&s, true, "en-US", "");
        assert!(!colored.template);
        let m = render(
            &summarize(&accounts, TrayStyle::Meters, &TraySelection::Auto),
            false,
            "en-US",
            "",
        );
        assert_eq!(m.title, None);
        assert_eq!(image_size(&m.image), (44, 36));
    }
}
