use super::super::*;
use super::fixtures::board;
use pinto::service::{SearchFilter, SearchMode};

#[test]
fn begin_search_enters_input_mode_with_an_empty_buffer() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    assert!(!view.is_searching());
    view.begin_search(SearchMode::Contains);
    assert!(view.is_searching());
    assert_eq!(view.search_input_mode(), Some(SearchMode::Contains));
    assert_eq!(view.search_input_buffer(), "");
}

#[test]
fn typing_builds_the_query_and_backspace_erases() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.begin_search(SearchMode::Regex);
    view.push_search_char('a');
    view.push_search_char('b');
    view.push_search_char('c');
    assert_eq!(view.search_input_buffer(), "abc");
    view.pop_search_char();
    assert_eq!(view.search_input_buffer(), "ab");
}

#[test]
fn begin_search_snapshots_the_active_filter_for_restore() {
    // Incremental search edits the applied filter live, so cancelling must roll back to the
    // filter that was active when the prompt opened.
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.set_search(Some(SearchFilter::new("keep", false).unwrap()));
    view.begin_search(SearchMode::Contains);
    // Simulate an incremental edit replacing the applied filter while typing.
    view.set_search(Some(SearchFilter::new("k", false).unwrap()));
    let restore = view.take_search_restore();
    assert!(!view.is_searching());
    assert_eq!(restore.as_ref().map(SearchFilter::pattern), Some("keep"));
}

#[test]
fn take_search_restore_yields_none_when_no_filter_was_active() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.begin_search(SearchMode::Contains);
    view.push_search_char('x');
    assert!(view.take_search_restore().is_none());
    assert!(!view.is_searching());
}

#[test]
fn navigation_characters_are_typed_literally_without_moving_selection() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    view.begin_search(SearchMode::Contains);
    for c in "jjl".chars() {
        view.push_search_char(c);
    }
    assert_eq!(view.search_input_buffer(), "jjl");
    assert_eq!(view.selected_row(), 0);
}

#[test]
fn validation_error_is_surfaced_then_cleared_on_the_next_edit() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.begin_search(SearchMode::Regex);
    view.push_search_char('[');
    view.set_search_input_error("invalid search pattern".to_string());
    assert_eq!(view.search_input_error(), Some("invalid search pattern"));
    // Editing the query invalidates the stale error so the prompt reads cleanly again.
    view.push_search_char(']');
    assert_eq!(view.search_input_error(), None);
    view.begin_search(SearchMode::Regex);
    view.set_search_input_error("boom".to_string());
    view.pop_search_char();
    assert_eq!(view.search_input_error(), None);
}

#[test]
fn ending_search_clears_the_input_without_cancelling() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.begin_search(SearchMode::Regex);
    view.push_search_char('z');
    view.end_search();
    assert!(!view.is_searching());
}
