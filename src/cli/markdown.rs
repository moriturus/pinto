//! Terminal Markdown rendering shared by `pinto show` and the Kanban details popup.
//!
//! PBI bodies are Markdown. Rendering them with `termimad` gives readable output
//! (styled headings, bullets, code, tables) instead of raw syntax. [`render_body`]
//! produces an ANSI-styled string for the CLI, and [`render_lines`] parses that
//! same output into ratatui [`Line`]s so the TUI popup shares one rendering path.
//!
//! Rendering is defensive: malformed Markdown is handled without panicking, so callers always
//! receive printable text.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::sync::LazyLock;
use termimad::{FmtText, MadSkin};

/// Coloured skin. `default()` picks gray levels that read on light or dark
/// terminals; used for the TUI popup and for `show` on a real terminal.
static COLOR_SKIN: LazyLock<MadSkin> = LazyLock::new(MadSkin::default);

/// Colourless skin: structural rendering (headings, bullets, tables) with no
/// ANSI escapes. Used for `show` when stdout is redirected so piped output and
/// files stay clean text instead of leaking escape sequences.
static PLAIN_SKIN: LazyLock<MadSkin> = LazyLock::new(MadSkin::no_style);

/// Render `markdown` to a string wrapped to `width` columns.
///
/// `color` selects the coloured skin (ANSI escapes) or the colourless one
/// (structural rendering only). Falls back to the raw Markdown if rendering panics, so callers
/// always receive printable text.
pub(crate) fn render_body(markdown: &str, width: usize, color: bool) -> String {
    // `termimad` only wraps when the width is at least 3 columns; below that it
    // renders unwrapped. Guard the width so it is always usable.
    let width = width.max(1);
    let skin = if color { &COLOR_SKIN } else { &PLAIN_SKIN };
    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        FmtText::from(skin, markdown, Some(width)).to_string()
    }));
    rendered.unwrap_or_else(|_| markdown.to_string())
}

/// Render `markdown` to ratatui [`Line`]s wrapped to `width`, for the TUI popup.
///
/// Always renders with colour (the popup re-emits the parsed styles through
/// ratatui) and shares [`render_body`]'s output so the popup and `pinto show`
/// render identically; each rendered line is parsed from ANSI into styled spans.
pub(crate) fn render_lines(markdown: &str, width: usize) -> Vec<Line<'static>> {
    render_body(markdown, width, true)
        .lines()
        .map(ansi_to_line)
        .collect()
}

/// Parse one line of ANSI-SGR-styled text into a ratatui [`Line`].
///
/// Recognises the SGR (`ESC [ … m`) sequences that `termimad`/`crossterm` emit —
/// attributes plus 4-bit, 8-bit and 24-bit colours — and drops any other escape
/// sequence. Text outside escapes becomes styled [`Span`]s.
fn ansi_to_line(src: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut text = String::new();
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            text.push(c);
            continue;
        }
        // Only CSI (`ESC [`) sequences carry styling; drop any other escape.
        if chars.peek() != Some(&'[') {
            continue;
        }
        chars.next(); // consume '['
        let mut params = String::new();
        let mut final_byte = None;
        for pc in chars.by_ref() {
            if pc.is_ascii_alphabetic() {
                final_byte = Some(pc);
                break;
            }
            params.push(pc);
        }
        // `m` is the "select graphic rendition" terminator; ignore cursor moves etc.
        if final_byte == Some('m') {
            if !text.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut text), style));
            }
            style = apply_sgr(style, &params);
        }
    }
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    if spans.is_empty() {
        // Preserve blank lines so vertical spacing (and popup scroll math) is kept.
        spans.push(Span::raw(String::new()));
    }
    Line::from(spans)
}

/// Update `style` from a `;`-separated SGR parameter list (the part before `m`).
fn apply_sgr(mut style: Style, params: &str) -> Style {
    // An empty parameter list means a full reset (`ESC [ m`).
    let codes: Vec<&str> = if params.is_empty() {
        vec!["0"]
    } else {
        params.split(';').collect()
    };
    let mut i = 0;
    while i < codes.len() {
        let Ok(code) = codes[i].parse::<u16>() else {
            i += 1;
            continue;
        };
        match code {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style = style.fg(basic_color(code - 30)),
            90..=97 => style = style.fg(bright_color(code - 90)),
            39 => style = style.fg(Color::Reset),
            40..=47 => style = style.bg(basic_color(code - 40)),
            100..=107 => style = style.bg(bright_color(code - 100)),
            49 => style = style.bg(Color::Reset),
            38 => {
                if let Some((color, advance)) = parse_extended(&codes[i + 1..]) {
                    style = style.fg(color);
                    i += advance;
                }
            }
            48 => {
                if let Some((color, advance)) = parse_extended(&codes[i + 1..]) {
                    style = style.bg(color);
                    i += advance;
                }
            }
            _ => {}
        }
        i += 1;
    }
    style
}

/// Parse the sub-parameters after a `38`/`48` extended-colour introducer.
///
/// Returns the colour and how many further codes it consumed (`5;n` → 8-bit,
/// `2;r;g;b` → 24-bit), or `None` if the sequence is malformed.
fn parse_extended(rest: &[&str]) -> Option<(Color, usize)> {
    match rest.first()?.parse::<u16>().ok()? {
        5 => Some((Color::Indexed(rest.get(1)?.parse().ok()?), 2)),
        2 => Some((
            Color::Rgb(
                rest.get(1)?.parse().ok()?,
                rest.get(2)?.parse().ok()?,
                rest.get(3)?.parse().ok()?,
            ),
            4,
        )),
        _ => None,
    }
}

/// Map a 4-bit standard colour index (0..=7) to a ratatui [`Color`].
fn basic_color(index: u16) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        _ => Color::Gray,
    }
}

/// Map a 4-bit bright colour index (0..=7) to a ratatui [`Color`].
fn bright_color(index: u16) -> Color {
    match index {
        0 => Color::DarkGray,
        1 => Color::LightRed,
        2 => Color::LightGreen,
        3 => Color::LightYellow,
        4 => Color::LightBlue,
        5 => Color::LightMagenta,
        6 => Color::LightCyan,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_body_strips_heading_syntax() {
        let out = render_body("# Hello", 80, true);
        assert!(out.contains("Hello"), "keeps heading text: {out:?}");
        assert!(!out.contains('#'), "drops markdown syntax: {out:?}");
    }

    #[test]
    fn render_body_strips_emphasis_syntax() {
        let out = render_body("**bold** and `code`", 80, true);
        assert!(out.contains("bold"), "keeps bold text: {out:?}");
        assert!(!out.contains("**"), "drops emphasis markers: {out:?}");
        assert!(out.contains("code"), "keeps code text: {out:?}");
    }

    #[test]
    fn render_body_falls_back_without_panicking_on_malformed_markdown() {
        // Unbalanced fences / table pipes must not crash rendering.
        let malformed = "```rust\nfn main() {\n| a | b |\n| - |\n> quote";
        let out = render_body(malformed, 40, true);
        assert!(!out.is_empty(), "renders something: {out:?}");
        assert!(out.contains("quote"), "preserves content: {out:?}");
    }

    #[test]
    fn ansi_to_line_plain_text_is_single_span() {
        let line = ansi_to_line("just text");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "just text");
        assert_eq!(line.spans[0].style, Style::default());
    }

    #[test]
    fn ansi_to_line_parses_bold_then_reset() {
        let line = ansi_to_line("\u{1b}[1mBold\u{1b}[0m plain");
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content, "Bold");
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[1].content, " plain");
        assert!(!line.spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn ansi_to_line_parses_8bit_foreground() {
        let line = ansi_to_line("\u{1b}[38;5;240mX\u{1b}[39mY");
        assert_eq!(line.spans[0].style.fg, Some(Color::Indexed(240)));
        assert_eq!(line.spans[1].style.fg, Some(Color::Reset));
    }

    #[test]
    fn ansi_to_line_parses_24bit_background() {
        let line = ansi_to_line("\u{1b}[48;2;10;20;30mX");
        assert_eq!(line.spans[0].style.bg, Some(Color::Rgb(10, 20, 30)));
    }

    #[test]
    fn ansi_to_line_handles_all_supported_sgr_attributes_and_malformed_sequences() {
        let source = concat!(
            "\u{1b}[1;2;3;4;7;9mattrs",
            "\u{1b}[22;23;24;27;29mremoved",
            "\u{1b}[30;31;32;33;34;35;36;37mcolors",
            "\u{1b}[90;91;92;93;94;95;96;97mbright",
            "\u{1b}[39;40;41;42;43;44;45;46;47mbackground",
            "\u{1b}[100;101;102;103;104;105;106;107mbright-bg",
            "\u{1b}[49;38;5;123mindexed",
            "\u{1b}[48;2;1;2;3mrgb",
            "\u{1b}[38;5;not-a-numbermbad",
            "\u{1b}[38;2;1;2mshort",
            "\u{1b}[999munknown",
            "\u{1b}[2Jcursor",
            "\u{1b}7other",
        );
        let line = ansi_to_line(source);
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains("attrs"));
        assert!(text.contains("indexed"));
        assert!(text.contains("cursor"));
    }

    #[test]
    fn ansi_to_line_preserves_blank_line() {
        let line = ansi_to_line("");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "");
    }

    #[test]
    fn render_lines_renders_heading_text() {
        let lines = render_lines("# Title\n\nbody text", 80);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(joined.contains("Title"), "has heading: {joined:?}");
        assert!(joined.contains("body text"), "has body: {joined:?}");
    }
}
