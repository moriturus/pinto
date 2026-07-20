use super::super::*;
use super::fixtures::{board, board_of, completed_item, item, item_with_deps, item_with_parent};
use pinto::backlog::Status;
use pinto::error::Error;

#[test]
fn new_selects_first_column_and_first_row() {
    let view = BoardView::new(board(&[("todo", &["T-1", "T-2"]), ("done", &["T-3"])]));
    assert_eq!(view.selected_col(), 0);
    assert_eq!(view.selected_row(), 0);
    assert_eq!(view.selected_item().unwrap().id.to_string(), "T-1");
    assert_eq!(view.col_offset(), 0);
}

#[test]
fn startup_can_select_a_named_column_and_enable_maximize() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"]), ("done", &["T-2"])]));

    assert!(view.select_column("done"));
    view.set_maximized(true);

    assert_eq!(view.selected_col(), 1);
    assert!(view.is_maximized());
}

#[test]
fn move_targets_skip_hidden_display_columns() {
    let mut todo = item("T-1", "Todo task");
    todo.status = Status::new("todo");
    let visible = board_of(&[("todo", vec![todo]), ("done", vec![])]);
    let display_columns = ["todo", "done"].into_iter().map(str::to_string).collect();
    let view = BoardView::new_with_scope(visible.clone(), visible, display_columns);

    assert_eq!(
        view.move_target(1),
        Some(("T-1".parse().unwrap(), "done".to_string()))
    );
    assert_eq!(view.move_target(-1), None);
}

#[test]
fn hidden_columns_remain_in_dependency_metadata() {
    let mut hidden_dependency = completed_item("T-1", "Completed dependency");
    hidden_dependency.status = Status::new("done");
    let mut visible_item = item_with_deps("T-2", "Ready task", &["T-1"]);
    visible_item.status = Status::new("ready");
    let full = board_of(&[
        ("done", vec![hidden_dependency]),
        ("ready", vec![visible_item.clone()]),
    ]);
    let display = board_of(&[("ready", vec![visible_item])]);
    let display_columns = ["ready".to_string()];
    let view = BoardView::new_with_scope(display, full, display_columns.to_vec());

    let index = view.dependency_index();
    let selected = view.selected_item().expect("visible item selected");

    assert!(!index.summary(selected).blocked);
}

#[test]
fn empty_board_has_no_selection() {
    let view = BoardView::new(board(&[]));
    assert!(view.selected_item().is_none());
}

#[test]
fn navigate_between_columns_clamps_row_to_shorter_column() {
    let mut view = BoardView::new(board(&[
        ("todo", &["T-1", "T-2", "T-3"]),
        ("done", &["T-9"]),
    ]));
    view.select_down();
    view.select_down();
    assert_eq!(view.selected_row(), 2);
    view.select_right();
    assert_eq!(view.selected_col(), 1);
    assert_eq!(view.selected_row(), 0);
    view.select_right();
    assert_eq!(view.selected_col(), 1);
    view.select_left();
    assert_eq!(view.selected_col(), 0);
    view.select_left();
    assert_eq!(view.selected_col(), 0);
}

#[test]
fn move_target_points_to_adjacent_column() {
    let view = BoardView::new(board(&[
        ("todo", &["T-1"]),
        ("doing", &["T-2"]),
        ("done", &[]),
    ]));
    assert_eq!(
        view.move_target(1),
        Some(("T-1".parse().unwrap(), "doing".to_string()))
    );
    assert_eq!(view.move_target(-1), None);
}

#[test]
fn reorder_target_up_targets_before_previous_and_down_after_next() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2", "T-3"])]));
    // There is no upward sorting for the first row.
    assert!(view.reorder_target(-1).is_none());
    view.select_down(); // T-2 (row=1).
    match view.reorder_target(-1) {
        Some((id, ReorderTarget::Before(reference))) => {
            assert_eq!(id.to_string(), "T-2");
            assert_eq!(reference.to_string(), "T-1");
        }
        other => panic!("expected Before(T-1), got {other:?}"),
    }
    match view.reorder_target(1) {
        Some((id, ReorderTarget::After(reference))) => {
            assert_eq!(id.to_string(), "T-2");
            assert_eq!(reference.to_string(), "T-3");
        }
        other => panic!("expected After(T-3), got {other:?}"),
    }
    view.select_down(); // T-3 (suffix).
    assert!(view.reorder_target(1).is_none());
}

#[test]
fn reorder_target_is_none_on_empty_column() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"]), ("done", &[])]));
    view.select_right();
    assert!(view.reorder_target(-1).is_none());
    assert!(view.reorder_target(1).is_none());
}

#[test]
fn reorder_target_stays_within_the_sibling_group() {
    // Column: parent T-1 with children T-2, T-3, then unrelated root T-9.
    let items = vec![
        item("T-1", "parent"),
        item_with_parent("T-2", "c1", "T-1"),
        item_with_parent("T-3", "c2", "T-1"),
        item("T-9", "root"),
    ];
    let mut view = BoardView::new(board_of(&[("todo", items)]));
    view.toggle_expand(); // expand T-1 so its children are visible

    view.select_down(); // T-2, the first child
    assert!(
        view.reorder_target(-1).is_none(),
        "first child cannot move up past its parent into another group"
    );

    view.select_down(); // T-3, the second child
    match view.reorder_target(-1) {
        Some((id, ReorderTarget::Before(reference))) => {
            assert_eq!(id.to_string(), "T-3");
            assert_eq!(reference.to_string(), "T-2", "moves before its sibling");
        }
        other => panic!("expected Before(T-2), got {other:?}"),
    }
    assert!(
        view.reorder_target(1).is_none(),
        "last child cannot move down into the unrelated root T-9"
    );
}

#[test]
fn scroll_follows_selection_and_clamps_to_end() {
    // 6 rows, 2 rows of display windows.
    let cols: Vec<(&str, &[&str])> = vec![
        ("c0", &[]),
        ("c1", &[]),
        ("c2", &[]),
        ("c3", &[]),
        ("c4", &[]),
        ("c5", &[]),
    ];
    let mut view = BoardView::new(board(&cols));
    view.scroll_to_visible(2);
    assert_eq!(view.col_offset(), 0);
    for _ in 0..3 {
        view.select_right();
    }
    view.scroll_to_visible(2); // Selected column 3 → window [2,3].
    assert_eq!(view.col_offset(), 2);
    for _ in 0..2 {
        view.select_right();
    }
    view.scroll_to_visible(2); // Selected column 5 (last) → window [4,5].
    assert_eq!(view.col_offset(), 4);
    view.select_left();
    view.select_left();
    view.select_left();
    view.scroll_to_visible(2); // Selection column 2 → to the left edge of the window.
    assert_eq!(view.col_offset(), 2);
}

#[test]
fn select_id_repositions_to_the_matching_item() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"]), ("done", &["T-9"])]));
    view.select_id(&"T-9".parse().unwrap());
    assert_eq!(view.selected_col(), 1);
    assert_eq!(view.selected_row(), 0);
    view.select_id(&"T-404".parse().unwrap());
    assert_eq!(view.selected_col(), 1);
}

#[test]
fn toggle_maximize_flips_the_flag_back_and_forth() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    assert!(!view.is_maximized(), "starts un-maximized");
    view.toggle_maximize();
    assert!(view.is_maximized(), "first toggle maximizes");
    view.toggle_maximize();
    assert!(!view.is_maximized(), "second toggle restores normal view");
}

#[test]
fn add_form_collects_title_then_body_and_preserves_the_selected_item() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.begin_add();
    assert_eq!(view.input_mode(), Some(InputMode::AddTitle));

    for c in "New item".chars() {
        view.push_input_char(c);
    }
    assert_eq!(
        view.submit_input().expect("title accepted"),
        InputSubmission::AddTitle {
            title: "New item".to_string()
        }
    );
    assert_eq!(view.input_mode(), Some(InputMode::AddBody));

    for c in "body text".chars() {
        view.push_input_char(c);
    }
    assert_eq!(
        view.submit_input().expect("body accepted"),
        InputSubmission::AddStep
    );
    assert_eq!(view.input_mode(), Some(InputMode::AddParent));
    assert_eq!(
        view.submit_input().expect("parent skipped"),
        InputSubmission::AddStep
    );
    assert_eq!(view.input_mode(), Some(InputMode::AddDependencies));
    assert_eq!(
        view.submit_input().expect("relationships accepted"),
        InputSubmission::Add {
            title: "New item".to_string(),
            body: "body text".to_string(),
            parent: None,
            depends_on: vec![],
        }
    );
    assert_eq!(
        view.selected_item().map(|item| item.id.to_string()),
        Some("T-1".to_string())
    );
    view.end_input();
    assert!(!view.is_input_active());
}

#[test]
fn add_form_collects_parent_and_dependencies_after_body() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2", "T-3"])]));
    view.begin_add();
    for c in "New item".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("title accepted");
    for c in "body text".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("body accepted");
    assert_eq!(view.input_mode(), Some(InputMode::AddParent));

    view.select_down();
    view.submit_input()
        .expect("cursor-selected parent accepted");
    assert_eq!(view.input_mode(), Some(InputMode::AddDependencies));

    for c in "T-1 T-3".chars() {
        view.push_input_char(c);
    }
    assert_eq!(
        view.submit_input().expect("dependencies accepted"),
        InputSubmission::Add {
            title: "New item".to_string(),
            body: "body text".to_string(),
            parent: Some("T-2".parse().unwrap()),
            depends_on: vec!["T-1".parse().unwrap(), "T-3".parse().unwrap()],
        }
    );
}

#[test]
fn add_form_does_not_reuse_selected_parent_when_dependencies_are_skipped() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    view.begin_add();
    for c in "New item".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("title accepted");
    for c in "body text".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("body accepted");

    view.select_down();
    view.submit_input()
        .expect("cursor-selected parent accepted");
    assert_eq!(view.input_mode(), Some(InputMode::AddDependencies));

    assert_eq!(
        view.submit_input().expect("dependencies skipped"),
        InputSubmission::Add {
            title: "New item".to_string(),
            body: "body text".to_string(),
            parent: Some("T-2".parse().unwrap()),
            depends_on: vec![],
        }
    );
}

#[test]
fn add_form_accepts_a_direct_parent_id_and_multiple_dependency_ids() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2", "T-3"])]));
    view.begin_add();
    for c in "New item".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("title accepted");
    for c in "body text".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("body accepted");

    for c in "T-2".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("direct parent accepted");

    for c in "T-1 T-3".chars() {
        view.push_input_char(c);
    }
    assert_eq!(
        view.submit_input().expect("direct dependencies accepted"),
        InputSubmission::Add {
            title: "New item".to_string(),
            body: "body text".to_string(),
            parent: Some("T-2".parse().unwrap()),
            depends_on: vec!["T-1".parse().unwrap(), "T-3".parse().unwrap()],
        }
    );
}

#[test]
fn dependency_form_targets_the_current_item_and_supports_add_and_remove() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    view.begin_dependency_add();
    assert_eq!(view.input_mode(), Some(InputMode::DependencyAdd));
    for c in "T-2".chars() {
        view.push_input_char(c);
    }
    assert_eq!(
        view.submit_input().expect("dependency accepted"),
        InputSubmission::Dependency {
            source: "T-1".parse().unwrap(),
            dependency: "T-2".to_string(),
            remove: false,
        }
    );
    view.end_input();

    view.begin_dependency_remove();
    assert_eq!(view.input_mode(), Some(InputMode::DependencyRemove));
    for c in "T-2".chars() {
        view.push_input_char(c);
    }
    assert_eq!(
        view.submit_input().expect("dependency removal accepted"),
        InputSubmission::Dependency {
            source: "T-1".parse().unwrap(),
            dependency: "T-2".to_string(),
            remove: true,
        }
    );
}

#[test]
fn parent_form_sets_a_parent_and_blank_input_clears_it() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    assert!(view.begin_parent());
    assert_eq!(view.input_mode(), Some(InputMode::Parent));
    for c in "T-2".chars() {
        view.push_input_char(c);
    }
    assert_eq!(
        view.submit_input().expect("parent accepted"),
        InputSubmission::Parent {
            source: "T-1".parse().unwrap(),
            parent: Some("T-2".to_string()),
        }
    );
    view.end_input();

    assert!(view.begin_parent());
    assert_eq!(
        view.submit_input().expect("blank parent clears"),
        InputSubmission::Parent {
            source: "T-1".parse().unwrap(),
            parent: None,
        }
    );
}

#[test]
fn relation_forms_use_the_selected_card_as_target() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));

    assert!(view.begin_dependency_add());
    view.select_down();
    assert_eq!(
        view.submit_input().expect("selected dependency accepted"),
        InputSubmission::Dependency {
            source: "T-1".parse().unwrap(),
            dependency: "T-2".to_string(),
            remove: false,
        }
    );
    view.end_input();

    view.select_up();
    assert!(view.begin_dependency_remove());
    view.select_down();
    assert_eq!(
        view.submit_input()
            .expect("selected dependency removal accepted"),
        InputSubmission::Dependency {
            source: "T-1".parse().unwrap(),
            dependency: "T-2".to_string(),
            remove: true,
        }
    );
    view.end_input();

    view.select_up();
    assert!(view.begin_parent());
    view.select_down();
    assert_eq!(
        view.submit_input().expect("selected parent accepted"),
        InputSubmission::Parent {
            source: "T-1".parse().unwrap(),
            parent: Some("T-2".to_string()),
        }
    );
}

#[test]
fn cancelling_a_form_keeps_selection_and_rejects_an_empty_title() {
    let mut view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
    view.select_down();
    view.begin_add();
    assert!(
        view.submit_input().is_err(),
        "empty title stays in the form"
    );
    assert!(view.is_input_active());
    view.push_input_char('x');
    view.end_input();
    assert!(!view.is_input_active());
    assert_eq!(
        view.selected_item().map(|item| item.id.to_string()),
        Some("T-2".to_string())
    );
}

#[test]
fn invalid_relationship_ids_keep_the_structured_error_for_the_runtime_boundary() {
    let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
    view.begin_add();
    for c in "New item".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("title accepted");
    for c in "body".chars() {
        view.push_input_char(c);
    }
    view.submit_input().expect("body accepted");
    for c in "bad".chars() {
        view.push_input_char(c);
    }

    match view.submit_input() {
        Err(InputValidation::InvalidItemId(Error::InvalidItemId(value))) => {
            assert_eq!(value, "bad")
        }
        other => panic!("expected structured invalid ID error, got {other:?}"),
    }
}
