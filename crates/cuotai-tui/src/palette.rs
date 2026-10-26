//! PRD 0022 RF16: the TUI's one palette. The design-system roles (docs/design-system.md section 3: fg, fg-muted,
//! accent, good, warn, bad, info, track) map to terminal colors here and nowhere else. `fg` and `bg` are the
//! terminal's own default colors, so light terminals stay readable. With `NO_COLOR` set to any non-empty value
//! (<https://no-color.org>) every frame is drawn without foreground or background colors: states stay readable
//! through their text and the shared shapes (↓ → ↑ ✕ ↺).

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

/// The RGB channels of the level roles, for the optional truecolor gauge blend.
pub(crate) const BAD_RGB: (u8, u8, u8) = (255, 85, 85);
pub(crate) const WARN_RGB: (u8, u8, u8) = (241, 250, 140);
pub(crate) const GOOD_RGB: (u8, u8, u8) = (80, 250, 123);

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
    const ROLES: [((u8, u8, u8), Color); 7] = [
        ((98, 114, 164), Color::DarkGray),
        ((139, 233, 253), Color::Cyan),
        (GOOD_RGB, Color::Green),
        (WARN_RGB, Color::Yellow),
        (BAD_RGB, Color::Red),
        ((189, 147, 249), Color::Magenta),
        ((255, 121, 198), Color::LightMagenta),
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
/// set, and otherwise downgrades them to ANSI names on a terminal without
/// truecolor.
pub(crate) fn finish(buffer: &mut Buffer) {
    if no_color() {
        strip(buffer);
    } else if limited_colors() {
        downgrade(buffer);
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
