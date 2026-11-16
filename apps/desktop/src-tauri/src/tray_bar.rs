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
//! summary lives in the tooltip (RF3.3). Icon only draws the ring of the
//! tightest share (DS-CMP-MENU-2). Without template images (Windows,
//! Linux) a monochrome image is drawn in a neutral gray legible on light
//! and dark bars, and the outage notch keeps its shape there too. The
//! colored variant (RF3.6) uses fixed level colors that reach 3:1 on light
//! and dark bars (a color that does not falls back to the neutral gray);
//! a text style shows the ring in the level color, never the brand logo.
//! Every style carries the badge (V-04). Changes reach the
//! menu bar at most once per second (RF3.5). PRD 0043 RF4.2: on macOS the
//! **24-hour strip** style draws the tightest account's last 24 hours as
//! hourly bars ([`set_strip`] receives them from the backend, at most once
//! a minute); elsewhere that style falls back to the icon and the tooltip.
//! Per-platform behavior: `docs/platforms.md`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use cuotai_core::tray::{
    self, AccountView, Badge, Level, StripBar, TrayItem, TraySelection, TrayStyle, TraySummary,
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
    /// PRD 0043 RF4.2: the hourly bars of the strip style and when they
    /// were last computed.
    strip: Vec<StripBar>,
    strip_at: Option<Instant>,
}

#[derive(PartialEq, Clone)]
struct Applied {
    title: Option<String>,
    image: Vec<u8>,
    template: bool,
    tooltip: String,
    /// RF7.1: what VoiceOver reads: the summary alone (V-18).
    label: String,
}

static STATE: Mutex<State> = Mutex::new(State {
    accounts: Vec::new(),
    menu_tooltip: String::new(),
    applied: None,
    last_at: None,
    pending: false,
    strip: Vec::new(),
    strip_at: None,
});

/// PRD 0043 RF4.2: the strip is computed again at most this often.
const STRIP_EVERY: Duration = Duration::from_secs(60);

/// Whether the strip's bars are due for a new computation.
pub fn strip_due() -> bool {
    lock().strip_at.is_none_or(|at| at.elapsed() >= STRIP_EVERY)
}

/// PRD 0043 RF4.2: new hourly bars of the strip style (empty: no history
/// or no account); redraws when they changed.
pub fn set_strip(app: &AppHandle, bars: Vec<StripBar>) {
    {
        let mut state = lock();
        state.strip_at = Some(Instant::now());
        if state.strip == bars {
            return;
        }
        state.strip = bars;
    }
    redraw(app, &settings::tray_prefs());
}

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

/// The interface language stored in `desktop.json` (en-US by default).
fn current_locale() -> String {
    settings::settings_path()
        .and_then(|f| settings::read_locale(&f))
        .unwrap_or_else(|| "en-US".to_owned())
}

/// The summary of `accounts` for `style` and `selection`, now, with times
/// in the local zone and `locale` (the core owns the clock, V-02).
pub fn summarize(
    accounts: &[AccountView],
    style: TrayStyle,
    selection: &TraySelection,
    locale: &str,
) -> TraySummary {
    tray::summary(
        accounts,
        style,
        selection,
        chrono::Utc::now(),
        &chrono::Local,
        locale,
    )
}

/// RF4.1 (V-05): the preview the Settings page shows, from the current
/// readings: the summary and the image the shell would draw for it.
pub fn preview(style: TrayStyle, selection: &TraySelection, colored: bool) -> Preview {
    let locale = current_locale();
    let state = lock();
    let summary = summarize(&state.accounts, style, selection, &locale);
    let applied = render(
        &summary,
        colored,
        &locale,
        "",
        Platform::current(),
        &state.strip,
    );
    let (width, height) = image_size(&applied.image);
    Preview {
        image: PreviewImage {
            width,
            height,
            template: applied.template,
            rgba: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &applied.image[8..],
            ),
        },
        summary,
    }
}

/// RF4.1: the summary with the image the menu bar would show.
#[derive(serde::Serialize)]
pub struct Preview {
    #[serde(flatten)]
    pub summary: TraySummary,
    pub image: PreviewImage,
}

/// The drawn image: RGBA pixels (base64), and whether the system tints it
/// (a template follows the bar's text color).
#[derive(serde::Serialize)]
pub struct PreviewImage {
    pub width: u32,
    pub height: u32,
    pub template: bool,
    pub rgba: String,
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
    let locale = current_locale();
    let mut state = lock();
    let summary = summarize(&state.accounts, prefs.style, &prefs.selection(), &locale);
    let next = render(
        &summary,
        prefs.colored,
        &locale,
        &state.menu_tooltip,
        Platform::current(),
        &state.strip,
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
        let label = next.label.clone();
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
    /// PRD 0043 RF4.2: a wide image for the 24-hour strip (macOS menu bar).
    strip: bool,
    /// S-03: the tooltip's limit in UTF-16 units, if the platform cuts it
    /// (the classic Windows tray tip holds 127 plus the terminator).
    tip_units: Option<usize>,
}

impl Platform {
    const MACOS: Self = Self {
        template: true,
        title: true,
        strip: true,
        tip_units: None,
    };
    const LINUX: Self = Self {
        template: false,
        title: true,
        strip: false,
        tip_units: None,
    };
    const WINDOWS: Self = Self {
        template: false,
        title: false,
        strip: false,
        tip_units: Some(127),
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
    strip: &[StripBar],
) -> Applied {
    // The template image only exists on macOS, and the colored variant is
    // never one.
    let template = platform.template && !colored;
    let ink = Ink { template, colored };
    let tightest = tightest_item(summary);
    let (mut rgba, w, h) = match summary.style {
        TrayStyle::Meters if !summary.items.is_empty() => meters(&summary.items, ink),
        TrayStyle::HourlyStrip if platform.strip && !summary.items.is_empty() => {
            let level = summary.items.first().and_then(|item| item.level);
            strip_image(strip, ink.rgb(level))
        }
        // DS-CMP-MENU-2: Icon only is the ring of the tightest share; the
        // colored variant shows the ring for the text styles too (RF3.6,
        // never the brand logo).
        TrayStyle::IconOnly => ring(tightest, ink),
        TrayStyle::TightestPercent | TrayStyle::LogosPercent if colored => ring(tightest, ink),
        _ => logo(),
    };
    // RF3.3/RF3.4: without a title, a text style draws its tightest figure into
    // the icon; Meters and the strip fall back to the icon and the tooltip.
    let figure = if platform.title {
        None
    } else {
        icon_figure(summary)
    };
    // V-04: every image carries the badge, a monochrome notch; on Windows
    // it moves to the top corner when the figure's plate takes the bottom.
    if summary.badge != Badge::None {
        cut_notch(
            &mut rgba,
            w,
            h,
            summary.badge == Badge::Alert,
            ink.mono(),
            figure.is_some(),
        );
    }
    if let Some(figure) = &figure {
        draw_figure(&mut rgba, w, h, figure);
    }
    let label = summary.accessible_label.resolve(locale);
    let summary_tip = summary.tooltip.resolve(locale);
    let tooltip = match platform.tip_units {
        // S-03: the freshness lines first, then the summary, cut inside the
        // platform's limit at a character boundary.
        Some(units) => cut_units(&join_lines(menu_tooltip, &summary_tip), units),
        None => join_lines(&summary_tip, menu_tooltip),
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
        label,
    }
}

fn join_lines(first: &str, second: &str) -> String {
    match (first.is_empty(), second.is_empty()) {
        (_, true) => first.to_owned(),
        (true, false) => second.to_owned(),
        (false, false) => format!("{first}\n{second}"),
    }
}

/// `text` within `units` UTF-16 units, cut at a character boundary and
/// ended with "…" (never a split surrogate pair).
fn cut_units(text: &str, units: usize) -> String {
    if text.encode_utf16().count() <= units {
        return text.to_owned();
    }
    let mut used = 1; // the ellipsis
    let mut out = String::new();
    for c in text.chars() {
        used += c.len_utf16();
        if used > units {
            break;
        }
        out.push(c);
    }
    format!("{}…", out.trim_end())
}

/// The tightest selected item (stale readings count only when every
/// reading is stale; an exhausted item counts as zero), as the title picks
/// it.
fn tightest_item(summary: &TraySummary) -> Option<&TrayItem> {
    let share = |i: &TrayItem| if i.exhausted { Some(0.0) } else { i.remaining };
    let pick = |stale: bool| {
        summary
            .items
            .iter()
            .filter(|i| i.stale == stale)
            .filter_map(|i| share(i).map(|s| (s, i)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i)| i)
    };
    pick(false).or_else(|| pick(true))
}

/// RF3.3: the figure drawn into the icon of a text style: "!" for an active
/// alert, else the tightest remaining share in up to two digits (stale
/// readings count only when every reading is stale; a positive share below
/// 0.5 % is "1", never the "0" of an exhausted account), "-" when unknown;
/// `None` for Icon only or an empty selection.
fn icon_figure(summary: &TraySummary) -> Option<String> {
    if !matches!(
        summary.style,
        TrayStyle::TightestPercent | TrayStyle::LogosPercent
    ) || summary.items.is_empty()
    {
        return None;
    }
    if summary.items.iter().any(|i| i.alert) {
        return Some("!".into());
    }
    Some(match tightest_item(summary) {
        None => "-".into(),
        Some(i) if i.exhausted => "0".into(),
        Some(i) => {
            let r = i.remaining.unwrap_or(0.0);
            let n = (r * 100.0).round().clamp(0.0, 99.0) as u8;
            format!("{}", if r > 0.0 { n.max(1) } else { n })
        }
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

/// The app mark.
fn logo() -> (Vec<u8>, u32, u32) {
    let Ok(icon) = Image::from_bytes(TRAY_PNG) else {
        return (vec![0; 4], 1, 1);
    };
    (icon.rgba().to_vec(), icon.width(), icon.height())
}

/// How an image is inked: black in a template (the system tints it), the
/// neutral gray elsewhere, or the level colors of the colored variant.
#[derive(Clone, Copy)]
pub(crate) struct Ink {
    template: bool,
    colored: bool,
}

impl Ink {
    /// The monochrome ink: black for a template, else [`NEUTRAL`].
    fn mono(self) -> [u8; 3] {
        if self.template {
            [0, 0, 0]
        } else {
            NEUTRAL
        }
    }

    /// The ink of an item at `level`.
    fn rgb(self, level: Option<Level>) -> [u8; 3] {
        if self.colored {
            level_rgb(level)
        } else {
            self.mono()
        }
    }
}

/// The monochrome ink of a non-template image: a gray that reaches 3:1
/// against every bar in [`BARS`], so Meters and the ring read on a light or
/// a dark taskbar (V-03).
const NEUTRAL: [u8; 3] = [0x7a, 0x7a, 0x7a];

/// The menu bars and taskbars a non-template image must read on: macOS
/// light and dark, Windows 11 light and dark.
const BARS: [[u8; 3]; 4] = [
    [0xec, 0xec, 0xec],
    [0x1e, 0x1e, 0x1e],
    [0xf3, 0xf3, 0xf3],
    [0x20, 0x20, 0x20],
];

/// WCAG relative luminance of an sRGB color.
fn luminance(rgb: [u8; 3]) -> f64 {
    let channel = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

/// WCAG contrast ratio of two colors.
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `rgb` when it reaches 3:1 on every bar (DS-CMP-MENU-4), else the
/// monochrome [`NEUTRAL`].
fn on_any_bar(rgb: [u8; 3]) -> [u8; 3] {
    if BARS.iter().all(|bar| contrast(rgb, *bar) >= 3.0) {
        rgb
    } else {
        NEUTRAL
    }
}

/// RF3.6: the fixed DS-ID-5 level colors of the colored variant, chosen to
/// reach 3:1 on light and dark bars at once; unknown is the neutral gray.
fn level_rgb(level: Option<Level>) -> [u8; 3] {
    on_any_bar(match level {
        Some(Level::Good) => [0x2e, 0x8b, 0x3e],
        Some(Level::Warn) => [0xa8, 0x70, 0x00],
        Some(Level::Bad) => [0xd0, 0x3a, 0x3a],
        None => NEUTRAL,
    })
}

/// RF3.1 / DS-CMP-MENU-2: the badge as a notch, so it reads by shape on any
/// bar: a cut-out ring around a solid dot in `ink` (an alert leaves the dot
/// hollow), in the bottom-right corner, or the top-right one when `top`
/// (the Windows figure's plate takes the bottom). Never a color dot.
pub(crate) fn cut_notch(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    hollow: bool,
    ink: [u8; 3],
    top: bool,
) {
    let size = width.min(height) as f32;
    let radius = size / 6.0;
    let gap = (size / 14.0).max(1.0);
    let cx = width as f32 - radius - gap;
    let cy = if top {
        radius + gap
    } else {
        height as f32 - radius - gap
    };
    for y in 0..height {
        for x in 0..width {
            let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
            let at = ((y * width + x) * 4) as usize;
            if d <= radius && !(hollow && d <= radius * 0.5) {
                rgba[at..at + 4].copy_from_slice(&[ink[0], ink[1], ink[2], 0xff]);
            } else if d <= radius + gap {
                rgba[at + 3] = 0;
            }
        }
    }
}

/// DS-CMP-MENU-2: the ring of the tightest share, 44 px: a faint track and
/// an arc from the top, clockwise, as long as the remaining share (a short
/// arc below 1 %); an exhausted item is a thin full outline, an unknown one
/// the track only.
pub(crate) fn ring(item: Option<&TrayItem>, ink: Ink) -> (Vec<u8>, u32, u32) {
    const S: u32 = 44;
    let rgb = ink.rgb(item.and_then(|i| i.level));
    let (outer, inner) = (19.0_f32, 13.0_f32);
    let c = S as f32 / 2.0;
    let share = item
        .filter(|i| !i.exhausted)
        .and_then(|i| i.remaining)
        .map(|r| {
            let r = r.clamp(0.0, 1.0) as f32;
            if r > 0.0 {
                r.max(0.04)
            } else {
                0.0
            }
        });
    let exhausted = item.is_some_and(|i| i.exhausted);
    let mut rgba = vec![0u8; (S * S * 4) as usize];
    for y in 0..S {
        for x in 0..S {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let d = dx.hypot(dy);
            if d > outer || d < inner {
                continue;
            }
            // 0 at the top, growing clockwise, in [0, 1).
            let turn = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
            let alpha = if exhausted {
                if d >= outer - 1.5 || d <= inner + 1.5 {
                    0xff
                } else {
                    0
                }
            } else if share.is_some_and(|s| turn < s) {
                0xff
            } else {
                0x55
            };
            if alpha == 0 {
                continue;
            }
            let at = ((y * S + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    (rgba, S, S)
}

/// PRD 0043 RF4.2: the 24-hour strip, one column per hour (oldest left),
/// each as tall as its consumption against the busiest hour; an hour with
/// no use is a solid baseline, a gap (no reading) only a faint one, and a
/// reset hour carries a dot at the top. Monochrome (a template image)
/// in `rgb` (black in a template, the neutral gray or a level color
/// elsewhere). 36 px high like the meters.
pub(crate) fn strip_image(bars: &[StripBar], rgb: [u8; 3]) -> (Vec<u8>, u32, u32) {
    const H: u32 = 36;
    const BAR: u32 = 2;
    const GAP: u32 = 1;
    let n = bars.len().clamp(1, 24) as u32;
    let w = n * (BAR + GAP) - GAP;
    let mut rgba = vec![0u8; (w * H * 4) as usize];
    let busiest = bars
        .iter()
        .filter_map(|bar| bar.consumption)
        .filter(|c| c.is_finite())
        .fold(0.0_f64, f64::max);
    // The column area: 2 px at the top for the reset dot, 1 px between.
    let area = H - 4;
    let mut put = |x: u32, y: u32, alpha: u8| {
        let at = ((y * w + x) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
    };
    for (i, bar) in bars.iter().take(n as usize).enumerate() {
        let x0 = i as u32 * (BAR + GAP);
        let (height, alpha) = match bar.consumption.filter(|c| c.is_finite()) {
            Some(c) if c > 0.0 && busiest > 0.0 => (
                ((c / busiest) * f64::from(area)).round().max(2.0) as u32,
                0xff,
            ),
            Some(_) => (1, 0xff),
            None => (1, 0x55),
        };
        for y in H - height.min(area)..H {
            for x in x0..x0 + BAR {
                put(x, y, alpha);
            }
        }
        if bar.reset {
            for y in 0..2 {
                for x in x0..x0 + BAR {
                    put(x, y, 0xff);
                }
            }
        }
    }
    (rgba, w, H)
}

/// RF3.2: up to four continuous bars, one per item, each the remaining share
/// (a sliver below 1 %), an exhausted bar as an outline and an unknown one as
/// its faint track only, in `ink` (V-03: the neutral gray where there is no
/// template). 36 px high (18 pt at 2×).
pub(crate) fn meters(items: &[TrayItem], ink: Ink) -> (Vec<u8>, u32, u32) {
    const W: u32 = 44;
    const H: u32 = 36;
    let n = items.len().clamp(1, tray::AUTO_METERS) as u32;
    let gap = 3u32;
    let bar = ((H - gap * (n + 1)) / n).clamp(4, 10);
    let top = (H - (bar * n + gap * (n - 1))) / 2;
    let mut rgba = vec![0u8; (W * H * 4) as usize];
    for (i, item) in items.iter().take(n as usize).enumerate() {
        let y0 = top + i as u32 * (bar + gap);
        let rgb = ink.rgb(item.level);
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

    const MONO: Ink = Ink {
        template: true,
        colored: false,
    };
    const PLAIN: Ink = Ink {
        template: false,
        colored: false,
    };
    const COLORED: Ink = Ink {
        template: false,
        colored: true,
    };

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

    /// Every opaque color of `rgba`.
    fn opaque_colors(rgba: &[u8]) -> Vec<[u8; 3]> {
        let mut c: Vec<[u8; 3]> = rgba
            .chunks(4)
            .filter(|p| p[3] == 0xff)
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        c.sort();
        c.dedup();
        c
    }

    fn summary_of(accounts: &[AccountView], style: TrayStyle) -> TraySummary {
        summarize(accounts, style, &TraySelection::Auto, "en-US")
    }

    #[test]
    fn meters_fill_by_length_and_keep_a_sliver() {
        let (rgba, w, h) = meters(&[item(Some(0.5), false), item(Some(0.004), false)], MONO);
        assert_eq!((w, h), (44, 36));
        // Monochrome template: every painted pixel is black.
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
        let (rgba, w, h) = meters(&[item(Some(0.0), true), item(None, false)], MONO);
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
        let (rgba, _, _) = meters(&[item(Some(0.9), false)], COLORED);
        assert!(rgba
            .chunks(4)
            .any(|p| p[3] == 0xff && p[..3] == level_rgb(Some(Level::Good))));
    }

    /// V-05 / DS-CMP-MENU-4: the fixed level colors and the neutral gray
    /// reach 3:1 on light and dark bars, computed; a color that does not
    /// falls back to the monochrome gray.
    #[test]
    fn level_colors_reach_three_to_one_on_light_and_dark_bars() {
        for level in [Some(Level::Good), Some(Level::Warn), Some(Level::Bad), None] {
            let rgb = level_rgb(level);
            for bar in BARS {
                let ratio = contrast(rgb, bar);
                assert!(ratio >= 3.0, "{level:?} on {bar:?}: {ratio:.2}");
            }
        }
        for bar in BARS {
            assert!(contrast(NEUTRAL, bar) >= 3.0, "neutral on {bar:?}");
        }
        assert_ne!(level_rgb(Some(Level::Good)), NEUTRAL, "a real color");
        // The earlier bright green fails on a light bar: monochrome instead.
        assert!(contrast([0x50, 0xfa, 0x7b], [0xec, 0xec, 0xec]) < 3.0);
        assert_eq!(on_any_bar([0x50, 0xfa, 0x7b]), NEUTRAL);
        assert_eq!(on_any_bar([0xff, 0xff, 0xff]), NEUTRAL);
        assert!((contrast([0, 0, 0], [0xff, 0xff, 0xff]) - 21.0).abs() < 0.01);
    }

    /// V-03: without a template, monochrome images use the neutral gray,
    /// legible on a light and a dark taskbar; Meters on Windows is real
    /// meters, not a black image.
    #[test]
    fn non_template_monochrome_images_read_on_both_bars() {
        let accounts = work(Some(0.22));
        for style in [TrayStyle::Meters, TrayStyle::IconOnly] {
            let s = summary_of(&accounts, style);
            for p in [Platform::LINUX, Platform::WINDOWS] {
                let a = render(&s, false, "en-US", "", p, &[]);
                assert!(!a.template);
                let colors = opaque_colors(&a.image[8..]);
                assert!(!colors.is_empty(), "{style:?} {p:?}");
                for c in colors {
                    for bar in BARS {
                        assert!(contrast(c, bar) >= 3.0, "{style:?} {p:?} {c:?} on {bar:?}");
                    }
                }
            }
        }
        let meters_win = render(
            &summary_of(&accounts, TrayStyle::Meters),
            false,
            "en-US",
            "",
            Platform::WINDOWS,
            &[],
        );
        assert_eq!(image_size(&meters_win.image), (44, 36), "real meters");
    }

    #[test]
    fn template_badge_is_a_cut_out_never_a_color() {
        let (w, h) = (44u32, 44u32);
        let mut rgba = vec![0xff; (w * h * 4) as usize];
        cut_notch(&mut rgba, w, h, false, [0, 0, 0], false);
        assert!(rgba.chunks(4).any(|p| p[3] == 0), "a transparent ring");
        assert!(rgba
            .chunks(4)
            .all(|p| p[3] == 0 || p[..3] == [0, 0, 0] || p[..3] == [0xff, 0xff, 0xff]));
    }

    /// V-04: the notch sits bottom-right, or top-right when the Windows
    /// figure's plate takes the bottom.
    #[test]
    fn the_notch_moves_to_the_top_when_asked() {
        let (w, h) = (44u32, 44u32);
        let blank = vec![0u8; (w * h * 4) as usize];
        let painted = |top: bool| {
            let mut rgba = blank.clone();
            cut_notch(&mut rgba, w, h, false, NEUTRAL, top);
            let px = |x: u32, y: u32| rgba[((y * w + x) * 4 + 3) as usize];
            (px(36, 36), px(36, 7))
        };
        assert_eq!(painted(false), (0xff, 0));
        assert_eq!(painted(true), (0, 0xff));
    }

    /// V-04: an outage and an alert change the image of every style on
    /// every platform (Meters and the strip included), and on Windows the
    /// badge stays clear of the figure's plate.
    #[test]
    fn every_style_and_platform_carries_the_badge() {
        let bars = vec![bar(Some(1.0), false); 24];
        for style in TrayStyle::ALL {
            for p in [Platform::MACOS, Platform::LINUX, Platform::WINDOWS] {
                for colored in [false, true] {
                    let plain = render(
                        &summary_of(&work(Some(0.5)), style),
                        colored,
                        "en-US",
                        "",
                        p,
                        &bars,
                    );
                    for (alert, incident) in [(false, true), (true, false)] {
                        let mut accounts = work(Some(0.5));
                        accounts[0].alert = alert;
                        accounts[0].incident = incident;
                        let marked = render(
                            &summary_of(&accounts, style),
                            colored,
                            "en-US",
                            "",
                            p,
                            &bars,
                        );
                        assert_ne!(
                            plain.image, marked.image,
                            "{style:?} {p:?} colored={colored} alert={alert}"
                        );
                    }
                }
            }
        }
        // Windows, Tightest %: the plate keeps the bottom-right corner and
        // the notch goes to the top-right one.
        let mut down = work(Some(0.5));
        down[0].incident = true;
        let win = render(
            &summary_of(&down, TrayStyle::TightestPercent),
            false,
            "en-US",
            "",
            Platform::WINDOWS,
            &[],
        );
        let px = |x: u32, y: u32| {
            let at = 8 + ((y * 44 + x) * 4) as usize;
            win.image[at..at + 4].to_vec()
        };
        assert_eq!(px(43, 43), vec![0x1e, 0x1e, 0x24, 0xff], "the plate");
        assert_eq!(px(36, 7)[..3], NEUTRAL, "the notch, top-right");
    }

    /// V-05: the colored variant never swaps in the brand logo: text styles
    /// show the ring in the level color.
    #[test]
    fn colored_text_styles_show_the_ring_not_the_brand_logo() {
        let (brand, _, _) = logo();
        let brand_colors = opaque_colors(&brand);
        for style in [TrayStyle::TightestPercent, TrayStyle::LogosPercent] {
            let a = render(
                &summary_of(&work(Some(0.9)), style),
                true,
                "en-US",
                "",
                Platform::MACOS,
                &[],
            );
            let colors = opaque_colors(&a.image[8..]);
            assert_eq!(colors, vec![level_rgb(Some(Level::Good))], "{style:?}");
            assert!(colors.iter().all(|c| !brand_colors.contains(c)));
        }
    }

    /// DS-CMP-MENU-2 (V-06): Icon only is the ring of the tightest share;
    /// its arc grows with the share, unknown is the track only.
    #[test]
    fn icon_only_is_a_ring_of_the_tightest_share() {
        let full = |rgba: &[u8]| rgba.chunks(4).filter(|p| p[3] == 0xff).count();
        let (low, w, h) = ring(Some(&item(Some(0.05), false)), MONO);
        let (high, _, _) = ring(Some(&item(Some(0.95), false)), MONO);
        let (unknown, _, _) = ring(Some(&item(None, false)), MONO);
        let (out, _, _) = ring(Some(&item(Some(0.0), true)), MONO);
        assert_eq!((w, h), (44, 44));
        assert!(full(&high) > 10 * full(&low), "the arc follows the share");
        assert!(full(&low) > 0, "a short arc below 1 % too");
        assert_eq!(full(&unknown), 0, "unknown: the track only");
        assert!(full(&out) > 0, "exhausted: an outline");
        // The arc starts at the top.
        let alpha = |rgba: &[u8], x: u32, y: u32| rgba[((y * w + x) * 4 + 3) as usize];
        assert_eq!(alpha(&low, 23, 4), 0xff);
        assert_eq!(alpha(&low, 20, 4), 0x55, "left of the top is track");
        let s = summary_of(&work(Some(0.5)), TrayStyle::IconOnly);
        let a = render(&s, false, "en-US", "", Platform::MACOS, &[]);
        assert!(a.template);
        assert_eq!(a.image[8..], ring(s.items.first(), MONO).0[..]);
        assert_eq!(
            opaque_colors(&ring(s.items.first(), PLAIN).0),
            vec![NEUTRAL]
        );
    }

    #[test]
    fn summary_maps_to_title_template_tooltip_and_label() {
        let accounts = work(Some(0.22));
        let s = summary_of(&accounts, TrayStyle::TightestPercent);
        let p = Platform::current();
        let a = render(&s, false, "en-US", "oldest reading: 2 min ago", p, &[]);
        assert_eq!(a.template, cfg!(target_os = "macos"));
        if cfg!(target_os = "windows") {
            assert_eq!(a.title, None);
        } else {
            assert_eq!(a.title.as_deref(), Some("22% · Claude"));
            assert_eq!(
                a.tooltip,
                "Cuotai — Claude Work 22% left\noldest reading: 2 min ago"
            );
        }
        // V-18: VoiceOver reads the summary alone.
        assert_eq!(a.label, "Claude Work 22% left");
        let colored = render(&s, true, "en-US", "", p, &[]);
        assert!(!colored.template);
        let m = render(
            &summary_of(&accounts, TrayStyle::Meters),
            false,
            "en-US",
            "",
            p,
            &[],
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
    /// icon.
    #[test]
    fn each_platform_gets_its_title_image_and_tooltip() {
        let s = summary_of(&work(Some(0.22)), TrayStyle::TightestPercent);
        let mac = render(&s, false, "en-US", "", Platform::MACOS, &[]);
        assert!(mac.template);
        assert_eq!(mac.title.as_deref(), Some("22% · Claude"));
        let linux = render(&s, false, "en-US", "", Platform::LINUX, &[]);
        assert!(!linux.template);
        assert_eq!(linux.title.as_deref(), Some("22% · Claude"));
        let win = render(&s, false, "en-US", "", Platform::WINDOWS, &[]);
        assert_eq!(win.title, None);
        assert!(!win.template);
        assert_ne!(win.image, linux.image, "the figure is drawn into the icon");
        assert_eq!(win.tooltip, "Cuotai — Claude Work 22% left");
        // The colored variant is never a template, on any platform.
        for p in [Platform::MACOS, Platform::LINUX, Platform::WINDOWS] {
            assert!(!render(&s, true, "en-US", "", p, &[]).template);
        }
    }

    /// S-03: on Windows the freshness line comes first and the tooltip is
    /// cut inside 127 UTF-16 units at a character boundary.
    #[test]
    fn the_windows_tooltip_puts_freshness_first_and_fits_the_tip() {
        let accounts: Vec<AccountView> = (0..6)
            .map(|i| AccountView {
                key: format!("k{i}"),
                provider: "claude".into(),
                provider_label: "Claude".into(),
                name: format!("Account number {i} 😀"),
                remaining: Some(0.5),
                ..AccountView::default()
            })
            .collect();
        let s = summary_of(&accounts, TrayStyle::TightestPercent);
        let win = render(
            &s,
            false,
            "en-US",
            "oldest reading: 2 min ago",
            Platform::WINDOWS,
            &[],
        );
        assert!(win
            .tooltip
            .starts_with("oldest reading: 2 min ago\nCuotai — "));
        assert!(win.tooltip.encode_utf16().count() <= 127, "{}", win.tooltip);
        assert!(win.tooltip.ends_with('…'));
        assert_eq!(cut_units("ab😀", 3), "ab…");
        assert_eq!(cut_units("ab😀", 4), "ab😀");
        let mac = render(
            &s,
            false,
            "en-US",
            "oldest reading: 2 min ago",
            Platform::MACOS,
            &[],
        );
        assert!(mac.tooltip.ends_with("\noldest reading: 2 min ago"));
    }

    #[test]
    fn icon_figure_is_two_digits_an_alert_or_a_dash() {
        let fig = |accounts: Vec<AccountView>, style| icon_figure(&summary_of(&accounts, style));
        assert_eq!(
            fig(work(Some(0.224)), TrayStyle::TightestPercent).as_deref(),
            Some("22")
        );
        assert_eq!(
            fig(work(Some(1.0)), TrayStyle::LogosPercent).as_deref(),
            Some("99")
        );
        // V-17: a positive share below 0.5 % is "1", never the exhausted "0".
        assert_eq!(
            fig(work(Some(0.004)), TrayStyle::TightestPercent).as_deref(),
            Some("1")
        );
        let mut out = work(Some(0.0));
        out[0].exhausted = true;
        assert_eq!(fig(out, TrayStyle::TightestPercent).as_deref(), Some("0"));
        assert_eq!(
            fig(work(None), TrayStyle::TightestPercent).as_deref(),
            Some("-")
        );
        assert_eq!(fig(work(Some(0.5)), TrayStyle::IconOnly), None);
        assert_eq!(fig(work(Some(0.5)), TrayStyle::Meters), None);
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

    fn bar(consumption: Option<f64>, reset: bool) -> StripBar {
        StripBar { consumption, reset }
    }

    /// PRD 0043 RF4.2: one column per hour scaled to the busiest one, a
    /// faint baseline for a gap, a solid one for an hour without use, a dot
    /// for a reset; monochrome unless colored.
    #[test]
    fn the_strip_draws_hours_gaps_and_resets() {
        let mut bars = vec![bar(None, false); 24];
        bars[0] = bar(Some(4.0), false);
        bars[1] = bar(Some(2.0), true);
        bars[2] = bar(Some(0.0), false);
        let (rgba, w, h) = strip_image(&bars, [0, 0, 0]);
        assert_eq!((w, h), (71, 36));
        let alpha = |x: u32, y: u32| rgba[((y * w + x) * 4 + 3) as usize];
        let column = |x: u32| (0..h).filter(|y| alpha(x, *y) == 0xff).count();
        assert_eq!(column(0), 32, "the busiest hour fills the column area");
        assert_eq!(column(3), 16 + 2, "half as tall, plus the reset dot");
        assert_eq!(alpha(3, 0), 0xff);
        assert_eq!(column(6), 1, "an hour without use is a solid baseline");
        assert_eq!(alpha(9, h - 1), 0x55, "a gap is only a faint baseline");
        assert_eq!(column(9), 0);
        assert!(rgba.chunks(4).all(|p| p[3] == 0 || p[..3] == [0, 0, 0]));
        let bad = level_rgb(Some(Level::Bad));
        let (colored, _, _) = strip_image(&bars, bad);
        assert!(colored.chunks(4).any(|p| p[3] == 0xff && p[..3] == bad));
    }

    /// PRD 0043 RF4.2: the strip is a macOS menu-bar style; Windows and
    /// Linux keep the icon (and the tooltip names the account).
    #[test]
    fn the_strip_style_draws_on_macos_only() {
        let s = summary_of(&work(Some(0.4)), TrayStyle::HourlyStrip);
        let bars = vec![bar(Some(1.0), false); 24];
        let mac = render(&s, false, "en-US", "", Platform::MACOS, &bars);
        assert_eq!(image_size(&mac.image), (71, 36));
        assert!(mac.template);
        assert_eq!(mac.title, None);
        assert_eq!(
            mac.tooltip,
            "Cuotai — Claude Work 40% left; the last 24 hours as hourly bars"
        );
        for p in [Platform::LINUX, Platform::WINDOWS] {
            let other = render(&s, false, "en-US", "", p, &bars);
            assert_eq!(image_size(&other.image), (44, 44), "{p:?}");
        }
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
        let mut down = work(Some(0.5));
        down[0].incident = true;
        let plain = render(
            &summary_of(&work(Some(0.5)), TrayStyle::TightestPercent),
            false,
            "en-US",
            "",
            Platform::MACOS,
            &[],
        );
        let outage = render(
            &summary_of(&down, TrayStyle::TightestPercent),
            false,
            "en-US",
            "",
            Platform::MACOS,
            &[],
        );
        assert!(plain.template && outage.template);
        assert_ne!(plain.image, outage.image);
        let before = opaque_colors(&plain.image[8..]);
        assert!(
            opaque_colors(&outage.image[8..])
                .iter()
                .all(|c| before.contains(c) || *c == [0, 0, 0]),
            "no new color"
        );
        assert_eq!(image_size(&plain.image), (44, 44));
    }
}
