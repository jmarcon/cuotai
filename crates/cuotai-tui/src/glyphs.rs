//! PRD 0035 RF6.3 and RF6.7: glyphs the terminal may not draw in one cell.
//!
//! The TUI lays out text with the narrow Unicode width table. A CJK terminal
//! usually draws the East Asian *Ambiguous* characters (the pace and status
//! shapes `↓ → ↑ ✕ ◆ ▲`, the middle dot, the box drawing of every border)
//! two cells wide, which shifts every column after them. A terminal whose
//! locale is not UTF-8 cannot draw them at all. Both cases are fixed in one
//! place: at the end of each frame, every cell whose symbol is in [`TABLE`]
//! is replaced by its one-cell ASCII equivalent, so columns and borders stay
//! where the layout put them and the shape still carries the state (color
//! only repeats it).
//!
//! - `CUOTAI_AMBIGUOUS_WIDTH=narrow|wide` overrides the width. Without it,
//!   the ambiguous glyphs are wide under the zh-CN, ja and ko locales.
//! - `CUOTAI_GLYPHS=unicode|ascii` overrides the glyph set. Without it, the
//!   set is ASCII when the effective locale variable (`LC_ALL`, then
//!   `LC_CTYPE`, then `LANG`) is set and does not name UTF-8.

use ratatui::buffer::Buffer;
use unicode_width::UnicodeWidthChar;

/// How the frame's glyphs are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Glyphs {
    /// The terminal draws every glyph in the width the layout assumed.
    Unicode,
    /// Ambiguous-width glyphs are two cells wide: only those are replaced.
    AmbiguousWide,
    /// The terminal cannot draw them: every glyph of the table is replaced.
    Ascii,
}

/// PRD 0035 RF6.7: shapes that are not East Asian Ambiguous in the width
/// table but that the common CJK terminal fonts draw two cells wide; they
/// are replaced in the same wide mode.
const CJK_FONT_WIDE: &[char] = &['\u{2715}'];

/// The one mapping from a drawn glyph to its ASCII equivalent.
pub(crate) const TABLE: &[(char, char)] = &[
    // Pace and status shapes (RF6.7).
    ('↓', 'v'),
    ('→', '>'),
    ('↑', '^'),
    ('←', '<'),
    ('✕', 'x'),
    ('×', 'x'),
    ('◆', '*'),
    ('▲', '!'),
    ('▼', 'v'),
    ('↺', '@'),
    ('✓', '+'),
    ('⚠', '!'),
    ('●', '*'),
    ('○', 'o'),
    ('★', '*'),
    // Bullets and punctuation.
    ('·', '.'),
    ('•', '*'),
    ('…', '~'),
    ('—', '-'),
    ('–', '-'),
    ('≈', '~'),
    ('«', '<'),
    ('‘', '\''),
    ('’', '\''),
    ('“', '"'),
    ('”', '"'),
    ('°', 'o'),
    ('±', '+'),
    ('¿', '?'),
    ('¡', '!'),
    ('§', 'S'),
    ('¶', 'P'),
    ('»', '>'),
    // Box drawing (every border).
    ('─', '-'),
    ('━', '-'),
    ('═', '='),
    ('│', '|'),
    ('┃', '|'),
    ('║', '|'),
    ('┌', '+'),
    ('┐', '+'),
    ('└', '+'),
    ('┘', '+'),
    ('├', '+'),
    ('┤', '+'),
    ('┬', '+'),
    ('┴', '+'),
    ('┼', '+'),
    ('╭', '+'),
    ('╮', '+'),
    ('╰', '+'),
    ('╯', '+'),
    ('╔', '+'),
    ('╗', '+'),
    ('╚', '+'),
    ('╝', '+'),
    // Gauges and sparklines.
    ('█', '#'),
    ('▉', '#'),
    ('▊', '#'),
    ('▋', '#'),
    ('▌', '#'),
    ('▍', '#'),
    ('▎', '#'),
    ('▏', '#'),
    ('▓', '#'),
    ('▒', ':'),
    ('░', '.'),
    ('▁', '_'),
    ('▂', '_'),
    ('▃', '.'),
    ('▄', ':'),
    ('▅', ':'),
    ('▆', '|'),
    ('▇', '|'),
];

/// The ASCII equivalent of `glyph` under `mode`, when it must be replaced.
pub(crate) fn fallback(glyph: char, mode: Glyphs) -> Option<char> {
    let replace = match mode {
        Glyphs::Unicode => false,
        Glyphs::Ascii => true,
        Glyphs::AmbiguousWide => {
            glyph.width_cjk() != glyph.width() || CJK_FONT_WIDE.contains(&glyph)
        }
    };
    if !replace {
        return None;
    }
    TABLE
        .iter()
        .find(|(from, _)| *from == glyph)
        .map(|(_, to)| *to)
        // The braille spinner and sparklines draw narrow everywhere; only a
        // terminal that cannot draw them gets a plain mark.
        .or_else(|| ('\u{2800}'..='\u{28FF}').contains(&glyph).then_some('*'))
}

/// `CUOTAI_AMBIGUOUS_WIDTH`, else wide for the CJK locales.
pub(crate) fn ambiguous_wide(override_: Option<&str>, locale: &str) -> bool {
    match override_.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("wide") => true,
        Some(value) if value.eq_ignore_ascii_case("narrow") => false,
        _ => matches!(locale, "zh-CN" | "ja" | "ko"),
    }
}

/// Whether the terminal's locale can draw the Unicode glyphs:
/// `CUOTAI_GLYPHS`, else the effective locale variable names UTF-8 (an
/// unset locale, as on Windows, counts as UTF-8).
pub(crate) fn unicode_capable(var: impl Fn(&str) -> Option<String>) -> bool {
    match var("CUOTAI_GLYPHS").as_deref().map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("ascii") => return false,
        Some(value) if value.eq_ignore_ascii_case("unicode") => return true,
        _ => {}
    }
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .find_map(|key| var(key).filter(|value| !value.is_empty()));
    locale.is_none_or(|value| {
        let value = value.to_ascii_lowercase();
        value.contains("utf-8") || value.contains("utf8")
    })
}

/// The mode from the environment and the TUI locale.
pub(crate) fn mode_from(var: impl Fn(&str) -> Option<String>, locale: &str) -> Glyphs {
    if !unicode_capable(&var) {
        Glyphs::Ascii
    } else if ambiguous_wide(var("CUOTAI_AMBIGUOUS_WIDTH").as_deref(), locale) {
        Glyphs::AmbiguousWide
    } else {
        Glyphs::Unicode
    }
}

#[cfg(not(test))]
fn mode() -> Glyphs {
    use std::sync::OnceLock;
    static MODE: OnceLock<Glyphs> = OnceLock::new();
    *MODE.get_or_init(|| mode_from(|key| std::env::var(key).ok(), crate::ui::tui_locale()))
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_MODE: std::cell::Cell<Glyphs> = const { std::cell::Cell::new(Glyphs::Unicode) };
}

#[cfg(test)]
fn mode() -> Glyphs {
    TEST_MODE.with(std::cell::Cell::get)
}

/// Replace, in a drawn frame, every glyph the terminal would misdraw.
pub(crate) fn apply(buffer: &mut Buffer, mode: Glyphs) {
    if mode == Glyphs::Unicode {
        return;
    }
    let area = buffer.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buffer[(x, y)];
            let mut chars = cell.symbol().chars();
            if let (Some(glyph), None) = (chars.next(), chars.next()) {
                if let Some(ascii) = fallback(glyph, mode) {
                    cell.set_char(ascii);
                }
            }
        }
    }
}

/// Called at the end of every frame (through `palette::finish`).
pub(crate) fn finish(buffer: &mut Buffer) {
    apply(buffer, mode());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn cjk_locales_default_to_wide_and_the_switch_overrides() {
        for locale in ["zh-CN", "ja", "ko"] {
            assert!(ambiguous_wide(None, locale));
            assert!(!ambiguous_wide(Some("narrow"), locale));
        }
        for locale in ["en-US", "de", "pt-BR", "hi-IN", "vi"] {
            assert!(!ambiguous_wide(None, locale));
            assert!(ambiguous_wide(Some(" WIDE "), locale));
        }
        assert!(ambiguous_wide(Some("bogus"), "ja"));
    }

    #[test]
    fn a_non_utf8_locale_selects_ascii() {
        assert!(unicode_capable(env(&[])));
        assert!(unicode_capable(env(&[("LANG", "ja_JP.UTF-8")])));
        assert!(unicode_capable(env(&[("LC_ALL", "C.utf8"), ("LANG", "C")])));
        assert!(!unicode_capable(env(&[("LANG", "C")])));
        assert!(!unicode_capable(env(&[("LC_CTYPE", "POSIX")])));
        assert!(!unicode_capable(env(&[("CUOTAI_GLYPHS", "ascii")])));
        assert!(unicode_capable(env(&[
            ("CUOTAI_GLYPHS", "unicode"),
            ("LANG", "C")
        ])));
        assert_eq!(mode_from(env(&[("LANG", "C")]), "en-US"), Glyphs::Ascii);
        assert_eq!(mode_from(env(&[]), "ja"), Glyphs::AmbiguousWide);
        assert_eq!(mode_from(env(&[]), "fr"), Glyphs::Unicode);
    }

    #[test]
    fn every_table_entry_is_one_ascii_cell_and_the_shapes_are_covered() {
        for (from, to) in TABLE {
            assert!(to.is_ascii() && !to.is_ascii_control(), "{from}");
            assert_eq!(from.width(), Some(1), "{from}");
        }
        let mut seen = std::collections::BTreeSet::new();
        assert!(
            TABLE.iter().all(|(from, _)| seen.insert(*from)),
            "duplicate"
        );
        for shape in ['↓', '→', '↑', '✕', '◆', '▲'] {
            assert!(fallback(shape, Glyphs::Ascii).is_some(), "{shape}");
            let ambiguous = shape.width_cjk() != shape.width() || shape == '✕';
            assert_eq!(fallback(shape, Glyphs::AmbiguousWide).is_some(), ambiguous);
        }
        assert_eq!(fallback('↓', Glyphs::Unicode), None);
        assert_eq!(fallback('a', Glyphs::Ascii), None);
        assert_eq!(fallback('⠋', Glyphs::Ascii), Some('*'));
        assert_eq!(fallback('⠋', Glyphs::AmbiguousWide), None);
        assert_eq!(fallback('│', Glyphs::AmbiguousWide), Some('|'));
    }

    #[test]
    fn wide_mode_keeps_cjk_text_and_replaces_only_ambiguous_glyphs() {
        let mut buffer = Buffer::with_lines(["┌ 会话 ↓ · ✓┐"]);
        apply(&mut buffer, Glyphs::AmbiguousWide);
        let line: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol().to_owned())
            .collect();
        assert!(line.starts_with("+ 会"), "{line}");
        assert!(line.contains("v . ✓+"), "{line}");
    }

    #[test]
    fn the_cross_mark_is_replaced_under_cjk_locales() {
        assert_eq!(fallback('\u{2715}', Glyphs::AmbiguousWide), Some('x'));
        assert_eq!(fallback('\u{2715}', Glyphs::Unicode), None);
        let mut buffer = Buffer::with_lines(["✕ 失败"]);
        apply(&mut buffer, Glyphs::AmbiguousWide);
        assert_eq!(buffer[(0, 0)].symbol(), "x");
    }
}
