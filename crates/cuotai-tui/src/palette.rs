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

/// Called at the end of every frame: strips the colors when `NO_COLOR` is set.
pub(crate) fn finish(buffer: &mut Buffer) {
    if no_color() {
        strip(buffer);
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
