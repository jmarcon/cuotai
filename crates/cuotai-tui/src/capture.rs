//! Test-only: turn a drawn `TestBackend` buffer into an SVG terminal
//! capture, for the documentation screenshots (`docs/assets/screenshots/
//! tui-*.png`). Every cell is placed on a fixed grid, so the capture shows
//! exactly the columns the TUI laid out. Convert with
//! `rsvg-convert -z 2 tui-dashboard.svg -o tui-dashboard.png`.

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use std::fmt::Write as _;

const CELL_W: f64 = 9.0;
const CELL_H: f64 = 19.0;
const PAD: f64 = 12.0;
const BG: &str = "#22252e";
const FG: &str = "#e6e6e6";

fn hex(color: Color, default: &str) -> String {
    match color {
        Color::Reset => default.to_owned(),
        color if crate::palette::channels(color).is_some() => {
            let (r, g, b) = crate::palette::channels(color).unwrap_or_default();
            format!("#{r:02x}{g:02x}{b:02x}")
        }
        Color::Black => "#21222c".into(),
        Color::Red | Color::LightRed => "#ff5555".into(),
        Color::Green | Color::LightGreen => "#50fa7b".into(),
        Color::Yellow | Color::LightYellow => "#f1fa8c".into(),
        Color::Blue | Color::LightBlue => "#6272a4".into(),
        Color::Magenta | Color::LightMagenta => "#ff79c6".into(),
        Color::Cyan | Color::LightCyan => "#8be9fd".into(),
        Color::Gray | Color::White => "#f8f8f2".into(),
        Color::DarkGray => "#6272a4".into(),
        _ => default.to_owned(),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The SVG of `buffer`: a dark terminal window, one text node per cell.
pub(crate) fn svg(buffer: &Buffer) -> String {
    let area = buffer.area;
    let width = f64::from(area.width) * CELL_W + 2.0 * PAD;
    let height = f64::from(area.height) * CELL_H + 2.0 * PAD;
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    let _ = writeln!(
        out,
        r#"<rect width="100%" height="100%" rx="8" fill="{BG}"/>"#
    );
    let _ = writeln!(
        out,
        r#"<g font-family="Menlo, 'DejaVu Sans Mono', 'Noto Sans Mono', monospace" font-size="15">"#
    );
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            let left = PAD + f64::from(x - area.left()) * CELL_W;
            let top = PAD + f64::from(y - area.top()) * CELL_H;
            let (fg, bg) = if cell.modifier.contains(Modifier::REVERSED) {
                (hex(cell.bg, BG), hex(cell.fg, FG))
            } else {
                (hex(cell.fg, FG), hex(cell.bg, BG))
            };
            let fg = if bg != BG && fg == FG {
                BG.to_owned()
            } else {
                fg
            };
            if bg != BG {
                let _ = writeln!(
                    out,
                    r#"<rect x="{left}" y="{top}" width="{CELL_W}" height="{CELL_H}" fill="{bg}"/>"#
                );
            }
            let symbol = cell.symbol();
            if symbol.trim().is_empty() {
                continue;
            }
            let weight = if cell.modifier.contains(Modifier::BOLD) {
                r#" font-weight="bold""#
            } else {
                ""
            };
            let baseline = top + CELL_H * 0.75;
            let _ = writeln!(
                out,
                r#"<text x="{left}" y="{baseline}" fill="{fg}"{weight}>{}</text>"#,
                escape(symbol)
            );
        }
    }
    out.push_str("</g>\n</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn cells_land_on_the_grid_with_their_colors() {
        let mut buffer = Buffer::with_lines(["a<b", "  c"]);
        buffer[(0, 0)].set_style(Style::default().fg(crate::palette::BAD));
        buffer[(2, 1)].set_style(Style::default().bg(Color::Green));
        let svg = svg(&buffer);
        assert!(svg.contains(r##"fill="#ff5555">a</text>"##), "{svg}");
        assert!(svg.contains("&lt;"));
        assert!(svg.contains(r##"fill="#50fa7b"/>"##));
        assert_eq!(svg.matches("<text").count(), 4);
    }
}
