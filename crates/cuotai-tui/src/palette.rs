//! PRD 0022 RF16: the TUI's one palette. The design-system roles (docs/design-system.md section 3: fg, fg-muted,
//! accent, good, warn, bad, info, track) map to terminal colors here and nowhere else. `fg` and `bg` are the
//! terminal's own default colors. The role colors below are the Dracula dark values; on a light terminal
//! (`CUOTAI_TUI_PALETTE=light`, or a light background in `COLORFGBG`) every frame is remapped to the light
//! variant of the desktop Dracula theme, whose roles keep their contrast on a white background. With `NO_COLOR`
//! set to any non-empty value (<https://no-color.org>) every frame is drawn without foreground or background
//! colors: states stay readable through their text and the shared shapes (↓ → ↑ ✕ ↺).

use ratatui::{buffer::Buffer, style::Color};
use std::sync::OnceLock;

/// Window and popover background: the terminal's own.
pub(crate) const BG: Color = Color::Reset;
/// Primary text: the terminal's own foreground.
pub(crate) const FG: Color = Color::Reset;
/// Secondary text, captions and the unknown state.
pub(crate) const MUTED: Color = Color::Rgb(98, 114, 164);
/// Interaction and selection (headings, the selected item, keys).
pub(crate) const ACCENT: Color = Color::Rgb(139, 233, 253);
/// Quota level ≥ 70 % left and good pace.
pub(crate) const GOOD: Color = Color::Rgb(80, 250, 123);
/// Quota level 30–70 %, aging readings, warnings.
pub(crate) const WARN: Color = Color::Rgb(241, 250, 140);
/// Quota level < 30 %, exhaustion and failures.
pub(crate) const BAD: Color = Color::Rgb(255, 85, 85);
/// Neutral information and emphasis that is not a level (provider names, binding).
pub(crate) const INFO: Color = Color::Rgb(189, 147, 249);
/// Secondary emphasis (the dashboard's highlights).
pub(crate) const PINK: Color = Color::Rgb(255, 121, 198);

/// Ink for text drawn on a level fill (the gauge percentage): dark on the
/// bright dark-palette fills, white on the deep light-palette fills.
pub(crate) const ON_FILL: Color = Color::Rgb(33, 34, 44);

/// The RGB channels of the level roles, for the optional truecolor gauge blend.
pub(crate) const BAD_RGB: (u8, u8, u8) = (255, 85, 85);
pub(crate) const WARN_RGB: (u8, u8, u8) = (241, 250, 140);
pub(crate) const GOOD_RGB: (u8, u8, u8) = (80, 250, 123);

/// The level roles of the light palette (`css/themes/dracula.css`, light mode).
const LIGHT_BAD_RGB: (u8, u8, u8) = (0xc9, 0x2a, 0x2a);
const LIGHT_WARN_RGB: (u8, u8, u8) = (0x8a, 0x61, 0x00);
const LIGHT_GOOD_RGB: (u8, u8, u8) = (0x1f, 0x7a, 0x3a);

/// Each dark role and its light-palette value. `fg` and `bg` stay the
/// terminal's own, so only the roles move.
const LIGHT: [(Color, Color); 8] = [
    (MUTED, Color::Rgb(0x6b, 0x6b, 0x7b)),
    (ACCENT, Color::Rgb(0xb3, 0x4a, 0x08)),
    (GOOD, rgb(LIGHT_GOOD_RGB)),
    (WARN, rgb(LIGHT_WARN_RGB)),
    (BAD, rgb(LIGHT_BAD_RGB)),
    (INFO, Color::Rgb(0x0b, 0x6f, 0x94)),
    (PINK, Color::Rgb(0xa8, 0x2e, 0x6c)),
    (ON_FILL, Color::Rgb(255, 255, 255)),
];

/// Which palette the terminal gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scheme {
    Dark,
    Light,
}

/// The palette for the environment: `CUOTAI_TUI_PALETTE=dark|light` first
/// (any other value falls through), then the background index that
/// `COLORFGBG` reports (its last `;` field: 7 and 9–15 are light), then dark.
/// Terminal.app does not set `COLORFGBG`, hence the explicit override.
pub(crate) fn scheme_from(var: impl Fn(&str) -> Option<String>) -> Scheme {
    match var("CUOTAI_TUI_PALETTE")
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("light") => return Scheme::Light,
        Some("dark") => return Scheme::Dark,
        _ => {}
    }
    let background = var("COLORFGBG").and_then(|value| {
        value
            .rsplit(';')
            .next()
            .and_then(|field| field.trim().parse::<u8>().ok())
    });
    match background {
        Some(7 | 9..=15) => Scheme::Light,
        _ => Scheme::Dark,
    }
}

pub(crate) fn scheme() -> Scheme {
    static SCHEME: OnceLock<Scheme> = OnceLock::new();
    *SCHEME.get_or_init(|| scheme_from(|key| std::env::var(key).ok()))
}

/// The bad, warn and good channels the truecolor gauge blends between.
pub(crate) fn levels(scheme: Scheme) -> [(u8, u8, u8); 3] {
    match scheme {
        Scheme::Dark => [BAD_RGB, WARN_RGB, GOOD_RGB],
        Scheme::Light => [LIGHT_BAD_RGB, LIGHT_WARN_RGB, LIGHT_GOOD_RGB],
    }
}

/// The light-palette value of a dark role. Any other color (the terminal's
/// own, ANSI names, a gauge blend already built from the light levels) is
/// returned unchanged.
pub(crate) fn light(color: Color) -> Color {
    LIGHT
        .iter()
        .find(|(dark, _)| *dark == color)
        .map_or(color, |(_, light)| *light)
}

/// Replaces every dark role of a drawn frame with its light value.
pub(crate) fn lighten(buffer: &mut Buffer) {
    let area = buffer.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buffer[(x, y)];
            let (fg, bg) = (light(cell.fg), light(cell.bg));
            cell.set_fg(fg);
            cell.set_bg(bg);
        }
    }
}

/// A truecolor value built from channels (the gauge blend between two level roles).
pub(crate) const fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

/// `NO_COLOR` as the environment sets it (any non-empty value).
pub(crate) fn no_color_from(var: impl Fn(&str) -> Option<String>) -> bool {
    var("NO_COLOR").is_some_and(|v| !v.is_empty())
}

fn no_color() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| no_color_from(|key| std::env::var(key).ok()))
}

/// Removes every color from a drawn frame (foreground, background and underline), keeping text and modifiers.
pub(crate) fn strip(buffer: &mut Buffer) {
    let area = buffer.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buffer[(x, y)];
            cell.set_fg(Color::Reset);
            cell.set_bg(Color::Reset);
        }
    }
}

/// Whether the environment names a terminal limited to the 16 ANSI colors:
/// no truecolor advertised (`COLORTERM`, the gauge gradient's signal) and a
/// `TERM` without `256color`, `direct` or `truecolor` (`linux`, `xterm`,
/// `screen`, `vt100`). An unset `TERM` (Windows) is not limited.
pub(crate) fn limited_colors_from(var: impl Fn(&str) -> Option<String>) -> bool {
    if crate::motion::truecolor(&var) {
        return false;
    }
    var("TERM").is_some_and(|term| {
        !["256color", "direct", "truecolor"]
            .iter()
            .any(|rich| term.contains(rich))
    })
}

fn limited_colors() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| limited_colors_from(|key| std::env::var(key).ok()))
}

/// The ANSI color of a palette role, for a terminal that does not advertise
/// truecolor: a 16-color terminal or the Linux console would otherwise
/// ignore the RGB sequences, and the named colors follow the user's own
/// scheme (so a light theme keeps its contrast). A blended gauge value maps
/// to the nearest level role.
pub(crate) fn ansi_fallback(color: Color) -> Color {
    let Color::Rgb(r, g, b) = color else {
        return color;
    };
    const ROLES: [((u8, u8, u8), Color); 8] = [
        ((98, 114, 164), Color::DarkGray),
        ((139, 233, 253), Color::Cyan),
        (GOOD_RGB, Color::Green),
        (WARN_RGB, Color::Yellow),
        (BAD_RGB, Color::Red),
        ((189, 147, 249), Color::Magenta),
        ((255, 121, 198), Color::LightMagenta),
        ((33, 34, 44), Color::Black),
    ];
    let distance = |(cr, cg, cb): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(cr, r) + d(cg, g) + d(cb, b)
    };
    ROLES
        .iter()
        .min_by_key(|(channels, _)| distance(*channels))
        .map_or(color, |(_, ansi)| *ansi)
}

/// Replaces every RGB color of a drawn frame with its ANSI fallback.
pub(crate) fn downgrade(buffer: &mut Buffer) {
    let area = buffer.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buffer[(x, y)];
            let (fg, bg) = (ansi_fallback(cell.fg), ansi_fallback(cell.bg));
            cell.set_fg(fg);
            cell.set_bg(bg);
        }
    }
}

/// Called at the end of every frame: strips the colors when `NO_COLOR` is
/// set, otherwise downgrades them to ANSI names on a terminal without
/// truecolor (the named colors already follow the user's scheme), and
/// otherwise remaps the roles to the light palette on a light terminal.
pub(crate) fn finish(buffer: &mut Buffer) {
    crate::glyphs::finish(buffer);
    if no_color() {
        strip(buffer);
    } else if limited_colors() {
        downgrade(buffer);
    } else if scheme() == Scheme::Light {
        lighten(buffer);
    }
}

/// The RGB channels of a truecolor value (test captures).
#[cfg(test)]
pub(crate) fn channels(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn no_color_needs_a_non_empty_value() {
        assert!(no_color_from(|k| (k == "NO_COLOR").then(|| "1".into())));
        assert!(!no_color_from(|k| (k == "NO_COLOR").then(String::new)));
        assert!(!no_color_from(|_| None));
    }

    #[test]
    fn strip_leaves_no_color_attributes() {
        let mut buffer = Buffer::with_lines(["ab"]);
        buffer.set_style(buffer.area, Style::default().fg(BAD).bg(rgb((40, 42, 54))));
        strip(&mut buffer);
        for x in 0..2 {
            assert_eq!(buffer[(x, 0)].fg, Color::Reset);
            assert_eq!(buffer[(x, 0)].bg, Color::Reset);
        }
        assert_eq!(buffer[(0, 0)].symbol(), "a");
    }

    #[test]
    fn limited_colors_follow_term_unless_truecolor_is_advertised() {
        let env = |pairs: &'static [(&str, &str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert!(limited_colors_from(env(&[("TERM", "linux")])));
        assert!(limited_colors_from(env(&[("TERM", "xterm")])));
        assert!(limited_colors_from(env(&[("TERM", "screen")])));
        assert!(!limited_colors_from(env(&[("TERM", "xterm-256color")])));
        assert!(!limited_colors_from(env(&[("TERM", "xterm-direct")])));
        assert!(!limited_colors_from(env(&[
            ("TERM", "xterm"),
            ("COLORTERM", "truecolor")
        ])));
        // Windows: no TERM, and the console speaks 24-bit.
        assert!(!limited_colors_from(env(&[])));
    }

    #[test]
    fn ansi_fallback_maps_roles_and_blends_to_named_colors() {
        assert_eq!(ansi_fallback(GOOD), Color::Green);
        assert_eq!(ansi_fallback(WARN), Color::Yellow);
        assert_eq!(ansi_fallback(BAD), Color::Red);
        assert_eq!(ansi_fallback(ACCENT), Color::Cyan);
        assert_eq!(ansi_fallback(MUTED), Color::DarkGray);
        assert_eq!(ansi_fallback(INFO), Color::Magenta);
        assert_eq!(ansi_fallback(PINK), Color::LightMagenta);
        assert_eq!(ansi_fallback(ON_FILL), Color::Black);
        // A gauge blend between warn and good lands on the nearer role.
        assert_eq!(ansi_fallback(rgb((120, 250, 127))), Color::Green);
        assert_eq!(ansi_fallback(Color::Reset), Color::Reset);
        assert_eq!(ansi_fallback(Color::Blue), Color::Blue);
        let mut buffer = Buffer::with_lines(["ab"]);
        buffer.set_style(buffer.area, Style::default().fg(BAD).bg(MUTED));
        downgrade(&mut buffer);
        assert_eq!(buffer[(1, 0)].fg, Color::Red);
        assert_eq!(buffer[(1, 0)].bg, Color::DarkGray);
        assert_eq!(buffer[(1, 0)].symbol(), "b");
    }

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn scheme_prefers_the_override_then_colorfgbg_then_dark() {
        use Scheme::{Dark, Light};
        assert_eq!(scheme_from(env(&[])), Dark);
        assert_eq!(scheme_from(env(&[("COLORFGBG", "0;15")])), Light);
        assert_eq!(scheme_from(env(&[("COLORFGBG", "15;0")])), Dark);
        assert_eq!(scheme_from(env(&[("COLORFGBG", "12;7")])), Light);
        assert_eq!(scheme_from(env(&[("COLORFGBG", "0;default;15")])), Light);
        assert_eq!(scheme_from(env(&[("COLORFGBG", "7;8")])), Dark);
        assert_eq!(scheme_from(env(&[("COLORFGBG", "garbage")])), Dark);
        assert_eq!(scheme_from(env(&[("COLORFGBG", "")])), Dark);
        assert_eq!(
            scheme_from(env(&[
                ("CUOTAI_TUI_PALETTE", "light"),
                ("COLORFGBG", "15;0")
            ])),
            Light
        );
        assert_eq!(
            scheme_from(env(&[
                ("CUOTAI_TUI_PALETTE", "DARK"),
                ("COLORFGBG", "0;15")
            ])),
            Dark
        );
        // An unknown override falls through to the automatic choice.
        assert_eq!(
            scheme_from(env(&[
                ("CUOTAI_TUI_PALETTE", "solar"),
                ("COLORFGBG", "0;15")
            ])),
            Light
        );
    }

    /// WCAG 2 relative luminance and contrast ratio.
    fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
        let lum = |(r, g, b): (u8, u8, u8)| {
            let lin = |c: u8| {
                let c = f64::from(c) / 255.0;
                if c <= 0.03928 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
        };
        let (la, lb) = (lum(a), lum(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn every_role_has_a_light_value_and_others_stay() {
        for role in [MUTED, ACCENT, GOOD, WARN, BAD, INFO, PINK, ON_FILL] {
            assert_ne!(light(role), role, "{role:?} keeps its dark value");
            assert!(channels(light(role)).is_some());
        }
        assert_eq!(light(Color::Reset), Color::Reset);
        assert_eq!(light(Color::Green), Color::Green);
        let blend = rgb((150, 80, 30));
        assert_eq!(light(blend), blend, "a light-level blend is left alone");
        let mut buffer = Buffer::with_lines(["ab"]);
        buffer.set_style(buffer.area, Style::default().fg(ON_FILL).bg(GOOD));
        lighten(&mut buffer);
        assert_eq!(buffer[(1, 0)].fg, Color::Rgb(255, 255, 255));
        assert_eq!(buffer[(1, 0)].bg, rgb(LIGHT_GOOD_RGB));
        assert_eq!(buffer[(1, 0)].symbol(), "b");
    }

    #[test]
    fn light_roles_meet_the_contrast_targets_on_white() {
        const WHITE: (u8, u8, u8) = (255, 255, 255);
        for (role, target) in [
            (MUTED, 4.5),
            (GOOD, 4.5),
            (WARN, 4.5),
            (BAD, 4.5),
            (INFO, 4.5),
            (PINK, 4.5),
            (ACCENT, 3.0),
        ] {
            let ratio = contrast(channels(light(role)).unwrap(), WHITE);
            assert!(ratio >= target, "{role:?}: {ratio:.2} < {target}");
        }
    }

    #[test]
    fn on_fill_ink_is_legible_on_every_fill_in_both_palettes() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let ink = match scheme {
                Scheme::Dark => channels(ON_FILL).unwrap(),
                Scheme::Light => channels(light(ON_FILL)).unwrap(),
            };
            let [bad, warn, good] = levels(scheme);
            // The level fills and the truecolor blends between them.
            for (a, b) in [(bad, warn), (warn, good)] {
                for step in 0..=10 {
                    let t = f64::from(step) / 10.0;
                    let ch = |x: u8, y: u8| {
                        (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8
                    };
                    let fill = (ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2));
                    let ratio = contrast(ink, fill);
                    assert!(ratio >= 4.5, "{scheme:?} {fill:?}: {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn rgb_literals_live_only_in_this_module() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs")
                || path.file_name().is_some_and(|n| n == "palette.rs")
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains("Color::Rgb"),
                "{} uses Color::Rgb outside the palette",
                path.display()
            );
        }
    }
}
