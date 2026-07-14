//! Text layout helpers for the Kanban view: display width and word wrapping.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display width (East Asian Width). Treats combining characters and zero width as 0 digits.
pub(crate) fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Display width of characters. Undeterminable characters (such as control characters) are treated as 0 digits.
pub(crate) fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Wrap `text` to display width `width`. Whitespace priority - If a word runs out, force a line break at the character boundary.
///
/// In order to take full-width (East Asian Width) into account, the display width is counted instead of the number of digits. Blank like CJK
/// Missing strings are wrapped character by character. It does not depend on drawing so that it can be unit tested as a pure function.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut line_w = 0usize;
    for word in text.split_whitespace() {
        place_word(word, width, &mut lines, &mut line, &mut line_w);
    }
    // Last line (one line is returned even if an empty string is input).
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// One word placement of [`wrap`]. Append if it fits on the current line, break a line if it doesn't, force split if it's too long.
fn place_word(
    word: &str,
    width: usize,
    lines: &mut Vec<String>,
    line: &mut String,
    line_w: &mut usize,
) {
    let ww = display_width(word);
    if line.is_empty() {
        if ww <= width {
            line.push_str(word);
            *line_w = ww;
        } else {
            let mut chunks = hard_break(word, width);
            let last = chunks.pop().unwrap_or_default();
            lines.extend(chunks);
            *line_w = display_width(&last);
            *line = last;
        }
    } else if *line_w + 1 + ww <= width {
        line.push(' ');
        line.push_str(word);
        *line_w += 1 + ww;
    } else {
        lines.push(std::mem::take(line));
        *line_w = 0;
        place_word(word, width, lines, line, line_w); // Since the line is empty, recurse only one step.
    }
}

/// Divide one word into pieces of display width `width` or less at character boundaries.
fn hard_break(word: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut w = 0usize;
    for c in word.chars() {
        let cw = char_width(c);
        if w + cw > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            w = 0;
        }
        cur.push(c);
        w += cw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
