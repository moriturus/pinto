use super::super::*;

#[test]
fn short_text_fits_on_one_line() {
    assert_eq!(wrap("hello world", 20), vec!["hello world"]);
}

#[test]
fn wraps_on_word_boundaries() {
    assert_eq!(
        wrap("alpha beta gamma", 11),
        vec!["alpha beta".to_string(), "gamma".to_string()]
    );
}

#[test]
fn hard_breaks_a_word_longer_than_width() {
    assert_eq!(wrap("abcdefgh", 3), vec!["abc", "def", "gh"]);
}

#[test]
fn counts_east_asian_width() {
    // Full-width is 2 digits. Width 4 means 2 characters each.
    assert_eq!(
        wrap("あいうえ", 4),
        vec!["あい".to_string(), "うえ".to_string()]
    );
}

#[test]
fn empty_text_yields_one_empty_line() {
    assert_eq!(wrap("", 5), vec![String::new()]);
}
