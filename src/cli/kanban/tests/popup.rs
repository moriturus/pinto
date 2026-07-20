use super::super::*;
use super::fixtures::{
    board, board_of, completed_item, item, item_with_parent, item_with_points_assignee,
};
use pinto::rank::Rank;

#[test]
fn popup_is_closed_by_default() {
    let view = BoardView::new(board(&[("todo", &["T-1"])]));
    assert!(!view.is_popup_open());
}

#[test]
fn open_popup_shows_it_when_an_item_is_selected() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.open_popup();
    assert!(view.is_popup_open());
}

#[test]
fn open_popup_is_a_noop_on_an_empty_board() {
    let mut view = BoardView::new(board(&[]));
    view.open_popup();
    assert!(!view.is_popup_open(), "no item selected → nothing to show");
}

#[test]
fn close_popup_hides_it_again() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.open_popup();
    view.close_popup();
    assert!(!view.is_popup_open());
}

#[test]
fn open_popup_resets_scroll_to_top() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.open_popup();
    view.scroll_popup(5, 100);
    assert_eq!(view.popup_scroll(), 5);
    view.close_popup();
    view.open_popup();
    assert_eq!(view.popup_scroll(), 0, "reopening starts scrolled to top");
}

#[test]
fn closing_and_reopening_preserves_the_selected_item() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    view.select_down();
    view.open_popup();
    assert_eq!(view.selected_item().unwrap().id.to_string(), "T-2");
    view.close_popup();
    assert_eq!(
        view.selected_item().unwrap().id.to_string(),
        "T-2",
        "selection is untouched by opening/closing the popup"
    );
    assert_eq!(view.selected_col(), 0);
    assert_eq!(view.selected_row(), 1);
}

#[test]
fn scroll_popup_moves_down_and_up_within_bounds() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.open_popup();
    view.scroll_popup(1, 10);
    assert_eq!(view.popup_scroll(), 1);
    view.scroll_popup(1, 10);
    assert_eq!(view.popup_scroll(), 2);
    view.scroll_popup(-1, 10);
    assert_eq!(view.popup_scroll(), 1);
}

#[test]
fn scroll_popup_does_not_go_below_zero() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.open_popup();
    view.scroll_popup(-3, 10);
    assert_eq!(view.popup_scroll(), 0);
}

#[test]
fn scroll_popup_clamps_to_the_given_maximum() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.open_popup();
    view.scroll_popup(100, 3);
    assert_eq!(view.popup_scroll(), 3, "cannot scroll past the max");
}

#[test]
fn popup_content_carries_the_core_fields() {
    let board = board_of(&[("todo", vec![item_with_parent("T-2", "child title", "T-1")])]);
    let mut view = BoardView::new(board);
    view.open_popup();
    let content = view.popup_content().expect("popup is open");
    assert_eq!(content.id.to_string(), "T-2");
    assert_eq!(content.title, "child title");
    assert_eq!(content.status.to_string(), "todo");
    assert_eq!(content.parent.as_deref(), Some("T-1"));
}

#[test]
fn popup_content_carries_linked_commits() {
    let mut selected = item("T-1", "linked item");
    selected.commits = vec!["abc123456789".to_string(), "def987654321".to_string()];
    let board = board_of(&[("todo", vec![selected])]);
    let mut view = BoardView::new(board);
    view.open_popup();

    let content = view.popup_content().expect("popup is open");
    assert_eq!(content.commits, ["abc123456789", "def987654321"]);
}

#[test]
fn popup_content_carries_points_and_assignee() {
    let board = board_of(&[(
        "todo",
        vec![item_with_points_assignee(
            "T-1",
            "sized",
            Some(5),
            Some("alice"),
        )],
    )]);
    let mut view = BoardView::new(board);
    view.open_popup();
    let content = view.popup_content().expect("popup is open");
    assert_eq!(content.points, Some(5));
    assert_eq!(content.assignee.as_deref(), Some("alice"));
}

#[test]
fn popup_content_reports_rank_ordinal_within_column() {
    let first = item("T-1", "first");
    let mut second = item("T-2", "second");
    second.rank = Rank::after(Some(&first.rank));
    let board = board_of(&[("todo", vec![first, second])]);
    let mut view = BoardView::new(board);
    view.select_down(); // Select T-2 (the lower-ranked card).
    view.open_popup();
    let content = view.popup_content().expect("popup is open");
    assert_eq!(content.id.to_string(), "T-2");
    assert_eq!(content.rank_ordinal, 2);
}

#[test]
fn popup_content_carries_lifecycle_timestamps() {
    let done = completed_item("T-1", "finished");
    let created = done.created;
    let done_at = done.done_at;
    let board = board_of(&[("done", vec![done])]);
    let mut view = BoardView::new(board);
    view.open_popup();
    let content = view.popup_content().expect("popup is open");
    assert_eq!(content.done_at, done_at);
    assert_eq!(content.created, created);
}

#[test]
fn popup_content_lists_children_from_the_whole_board() {
    // Even if parent T-1 (doing column) and child T-2 (todo column) are separated into different columns, the parent-child relationship can be detected.
    let board = board_of(&[
        ("todo", vec![item_with_parent("T-2", "c", "T-1")]),
        ("doing", vec![item("T-1", "p")]),
    ]);
    let mut view = BoardView::new(board);
    view.select_right(); // Select T-1 (doing).
    view.open_popup();
    let content = view.popup_content().expect("popup is open");
    assert_eq!(content.id.to_string(), "T-1");
    assert_eq!(
        content
            .children
            .iter()
            .map(ItemId::to_string)
            .collect::<Vec<_>>(),
        vec!["T-2".to_string()]
    );
}

#[test]
fn popup_content_is_none_when_closed() {
    let view = BoardView::new(board(&[("todo", &["T-1"])]));
    assert!(view.popup_content().is_none());
}

#[test]
fn navigating_rows_while_popup_open_follows_the_selection() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    view.open_popup();
    assert_eq!(view.popup_content().unwrap().id.to_string(), "T-1");
    view.select_down();
    assert_eq!(
        view.popup_content().unwrap().id.to_string(),
        "T-2",
        "popup content follows the newly selected row"
    );
}

#[test]
fn navigating_columns_while_popup_open_follows_the_selection() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"]), ("doing", &["T-2"])]));
    view.open_popup();
    view.select_right();
    assert_eq!(
        view.popup_content().unwrap().id.to_string(),
        "T-2",
        "popup content follows the newly selected column"
    );
}

#[test]
fn navigating_while_popup_open_resets_scroll_to_top() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    view.open_popup();
    view.scroll_popup(5, 100);
    assert_eq!(view.popup_scroll(), 5);
    view.select_down();
    assert_eq!(
        view.popup_scroll(),
        0,
        "moving to another item scrolls back to top"
    );
}

#[test]
fn navigating_while_popup_closed_leaves_scroll_untouched() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    view.open_popup();
    view.scroll_popup(5, 100);
    view.close_popup();
    view.select_down();
    assert_eq!(
        view.popup_scroll(),
        5,
        "navigation only resets popup scroll while the popup is open"
    );
}
