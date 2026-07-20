//! Text width measurement and truncation helpers for terminal output.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Default number of terminal cells when the output width cannot be determined.
pub(crate) const DEFAULT_TERM_WIDTH: usize = 80;

/// Minimum title width, preserving a readable title even on very narrow terminals.
pub(super) const MIN_TITLE_WIDTH: usize = 10;

/// Return the terminal display width of one character.
///
/// [`unicode_width`] uses East Asian Width properties; control and undefined characters count as
/// zero cells.
fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Return the number of terminal cells occupied by `s`.
pub(super) fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate `s` so its display width is at most `max` cells.
///
/// If truncation is needed, replace the final cell with an ellipsis (`…`). Full-width characters
/// count as two cells, so they cannot overflow the limit. Return an empty string when `max` is zero.
pub(super) fn truncate(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    // Reserve one cell for the ellipsis.
    let budget = max - 1;
    let mut width = 0;
    let mut out = String::new();
    for c in s.chars() {
        let w = char_width(c);
        if width + w > budget {
            break;
        }
        width += w;
        out.push(c);
    }
    out.push('…');
    out
}
