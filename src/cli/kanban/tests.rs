//! Unit tests for the Kanban view model, layout, wrapping, and rendering.

#[cfg(test)]
mod fixtures {
    use pinto::backlog::{BacklogItem, ItemId, Status};
    use pinto::rank::Rank;
    use pinto::service::{Board, BoardColumn};

    /// PBI for testing (only ID and title are meaningful).
    pub(super) fn item(id: &str, title: &str) -> BacklogItem {
        BacklogItem::new(
            id.parse::<ItemId>().unwrap(),
            title.to_string(),
            Status::new("todo"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            chrono::Utc::now(),
        )
        .unwrap()
    }

    /// Test PBI with parent ID set.
    pub(super) fn item_with_parent(id: &str, title: &str, parent: &str) -> BacklogItem {
        let mut it = item(id, title);
        it.parent = Some(parent.parse::<ItemId>().unwrap());
        it
    }

    /// Test PBI with dependencies (`depends_on`) set.
    pub(super) fn item_with_deps(id: &str, title: &str, deps: &[&str]) -> BacklogItem {
        let mut it = item(id, title);
        it.depends_on = deps.iter().map(|d| d.parse::<ItemId>().unwrap()).collect();
        it
    }

    /// Test PBI with story points and/or an assignee set.
    pub(super) fn item_with_points_assignee(
        id: &str,
        title: &str,
        points: Option<u32>,
        assignee: Option<&str>,
    ) -> BacklogItem {
        let mut it = item(id, title);
        it.points = points;
        it.assignee = assignee.map(str::to_string);
        it
    }

    /// Completed (`done_at` setting) test PBI.
    pub(super) fn completed_item(id: &str, title: &str) -> BacklogItem {
        let mut it = item(id, title);
        it.done_at = Some(chrono::Utc::now());
        it
    }

    /// Each element of `columns` is (state name, PBI ID group of that column). The title is generated from the ID.
    pub(super) fn board(columns: &[(&str, &[&str])]) -> Board {
        Board {
            columns: columns
                .iter()
                .map(|(name, ids)| BoardColumn {
                    status: Status::new(*name),
                    items: ids
                        .iter()
                        .map(|id| item(id, &format!("title {id}")))
                        .collect(),
                })
                .collect(),
            orphaned: Vec::new(),
        }
    }

    /// Create a Board from each column (state name, card group) (cards with parent/child/dependency can be passed directly).
    pub(super) fn board_of(columns: &[(&str, Vec<BacklogItem>)]) -> Board {
        Board {
            columns: columns
                .iter()
                .map(|(name, items)| BoardColumn {
                    status: Status::new(*name),
                    items: items.clone(),
                })
                .collect(),
            orphaned: Vec::new(),
        }
    }
}

#[cfg(test)]
mod display_scope_tests {
    use super::super::*;

    #[test]
    fn default_scope_keeps_every_workflow_column() {
        let workflow = ["backlog", "ready", "in-progress", "review", "done"].map(str::to_string);

        let visible = resolve_display_columns(&workflow, &[], None).expect("scope resolves");

        assert_eq!(visible, workflow);
    }

    #[test]
    fn hidden_columns_are_removed_in_workflow_order() {
        let workflow = ["backlog", "ready", "in-progress", "review", "done"].map(str::to_string);

        let visible = resolve_display_columns(&workflow, &["backlog".to_string()], None)
            .expect("scope resolves");

        assert_eq!(visible, ["ready", "in-progress", "review", "done"]);
    }

    #[test]
    fn explicit_columns_override_hidden_columns_and_keep_workflow_order() {
        let workflow = ["backlog", "ready", "in-progress", "review", "done"].map(str::to_string);
        let requested = ["done".to_string(), "backlog".to_string()];

        let visible =
            resolve_display_columns(&workflow, &["backlog".to_string()], Some(&requested))
                .expect("scope resolves");

        assert_eq!(visible, ["backlog", "done"]);
    }

    #[test]
    fn explicit_unknown_column_is_rejected_before_terminal_startup() {
        let workflow = ["todo".to_string(), "done".to_string()];

        let error = resolve_display_columns(
            &workflow,
            &[],
            Some(&["todo".to_string(), "missing".to_string()]),
        )
        .expect_err("unknown column rejected");

        assert!(error.to_string().contains("missing"));
    }
}

#[cfg(test)]
mod view_tests {
    use super::super::*;
    use super::fixtures::{
        board, board_of, completed_item, item, item_with_deps, item_with_parent,
    };
    use pinto::backlog::Status;

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
}

#[cfg(test)]
mod popup_tests {
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
}

#[cfg(test)]
mod tree_tests {
    use super::super::*;
    use super::fixtures::{board_of, item, item_with_parent};

    fn expanded(ids: &[&str]) -> HashSet<ItemId> {
        ids.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn childless_items_are_flat_roots() {
        let items = vec![item("T-1", "a"), item("T-2", "b")];
        let rows = column_display_rows(&items, &HashSet::new());
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|r| r.depth == 0 && r.child_count == 0 && r.parent_index.is_none())
        );
    }

    #[test]
    fn children_hidden_when_collapsed_but_counted() {
        let items = vec![
            item("T-1", "p"),
            item_with_parent("T-2", "c1", "T-1"),
            item_with_parent("T-3", "c2", "T-1"),
        ];
        let rows = column_display_rows(&items, &HashSet::new());
        assert_eq!(rows.len(), 1, "only the parent is visible when collapsed");
        assert_eq!(rows[0].item_index, 0);
        assert_eq!(
            rows[0].child_count, 2,
            "collapsed parent knows its child count"
        );
        assert!(!rows[0].expanded);
    }

    #[test]
    fn children_shown_indented_when_expanded() {
        let items = vec![
            item("T-1", "p"),
            item_with_parent("T-2", "c1", "T-1"),
            item_with_parent("T-3", "c2", "T-1"),
        ];
        let rows = column_display_rows(&items, &expanded(&["T-1"]));
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].depth, rows[0].item_index), (0, 0));
        assert_eq!((rows[1].depth, rows[1].item_index), (1, 1));
        assert_eq!((rows[2].depth, rows[2].item_index), (1, 2));
        assert_eq!(rows[1].parent_index, Some(0));
        assert!(rows[0].expanded);
    }

    #[test]
    fn grandchildren_follow_nested_expansion() {
        let items = vec![
            item("T-1", "p"),
            item_with_parent("T-2", "c", "T-1"),
            item_with_parent("T-3", "g", "T-2"),
        ];
        // Expand only T-1: display p and c and hide grandchildren (c is collapsed).
        let rows = column_display_rows(&items, &expanded(&["T-1"]));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].child_count, 1, "child knows it has a grandchild");
        // Both deployed: 3 levels deep.
        let rows = column_display_rows(&items, &expanded(&["T-1", "T-2"]));
        assert_eq!(
            rows.iter().map(|r| r.depth).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn child_whose_parent_is_in_another_column_is_a_flat_root() {
        // Parent T-9 does not exist in this column → T-2 is displayed flat as root.
        let items = vec![item("T-1", "a"), item_with_parent("T-2", "b", "T-9")];
        let rows = column_display_rows(&items, &HashSet::new());
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|r| r.depth == 0 && r.parent_index.is_none())
        );
    }

    #[test]
    fn every_item_is_emitted_even_if_parent_links_form_a_cycle() {
        // Even if the data is invalid (both parents), it is sent as the root without missing anything (no failure).
        let a = item_with_parent("T-1", "a", "T-2");
        let b = item_with_parent("T-2", "b", "T-1");
        let rows = column_display_rows(&[a, b], &expanded(&["T-1", "T-2"]));
        let shown: HashSet<usize> = rows.iter().map(|r| r.item_index).collect();
        assert_eq!(shown, HashSet::from([0, 1]), "no item is dropped");
    }

    #[test]
    fn toggle_expand_reveals_and_hides_children() {
        let board = board_of(&[(
            "todo",
            vec![item("T-1", "p"), item_with_parent("T-2", "c", "T-1")],
        )]);
        let mut view = BoardView::new(board);
        assert_eq!(view.selected_item().unwrap().id.to_string(), "T-1");
        // While collapsed, there is only one visible row, so it stays at the parent even if you move down.
        view.select_down();
        assert_eq!(view.selected_row(), 0);
        view.toggle_expand();
        view.select_down();
        assert_eq!(view.selected_item().unwrap().id.to_string(), "T-2");
        // Refolding rounds the selection to its parent.
        view.select_up();
        view.toggle_expand();
        assert_eq!(view.selected_row(), 0);
        assert_eq!(view.selected_item().unwrap().id.to_string(), "T-1");
    }

    #[test]
    fn toggle_on_childless_item_is_a_noop() {
        let board = board_of(&[("todo", vec![item("T-1", "a")])]);
        let mut view = BoardView::new(board);
        view.toggle_expand();
        assert_eq!(view.visible_rows(0).len(), 1);
    }

    #[test]
    fn expand_state_survives_board_replacement() {
        let cards = || vec![item("T-1", "p"), item_with_parent("T-2", "c", "T-1")];
        let mut view = BoardView::new(board_of(&[("todo", cards())]));
        view.toggle_expand(); // Deploy T-1.
        assert_eq!(view.visible_rows(0).len(), 2);
        // The development is maintained even when reloading (replacing the board with the same content).
        let board = board_of(&[("todo", cards())]);
        view.set_boards(board.clone(), board);
        assert_eq!(
            view.visible_rows(0).len(),
            2,
            "expansion persists across reload"
        );
    }

    #[test]
    fn select_id_expands_ancestors_to_reveal_a_collapsed_child() {
        let board = board_of(&[(
            "todo",
            vec![item("T-1", "p"), item_with_parent("T-2", "c", "T-1")],
        )]);
        let mut view = BoardView::new(board);
        assert_eq!(view.visible_rows(0).len(), 1, "child collapsed initially");
        view.select_id(&"T-2".parse().unwrap());
        assert_eq!(view.selected_item().unwrap().id.to_string(), "T-2");
        assert_eq!(view.visible_rows(0).len(), 2, "ancestor auto-expanded");
    }

    #[test]
    fn reorder_is_scoped_to_visible_siblings() {
        // Parent T-1 deployment, child T-2 (grandchild T-3 deployment), T-4. Down T-2 → Behind brother T-4 (grandchild does not straddle).
        let board = board_of(&[(
            "todo",
            vec![
                item("T-1", "p"),
                item_with_parent("T-2", "c1", "T-1"),
                item_with_parent("T-3", "g1", "T-2"),
                item_with_parent("T-4", "c2", "T-1"),
            ],
        )]);
        let mut view = BoardView::new(board);
        view.toggle_expand(); // T-1 deployment.
        view.select_id(&"T-2".parse().unwrap());
        view.toggle_expand(); // T-2 deployment (grandson T-3 wedged between T-2 and T-4).
        view.select_id(&"T-2".parse().unwrap());
        // Sort T-2 downwards → behind brother T-4 (grandchild T-3 does not straddle).
        let (id, target) = view.reorder_target(1).expect("sibling below exists");
        assert_eq!(id.to_string(), "T-2");
        match target {
            ReorderTarget::After(r) => assert_eq!(r.to_string(), "T-4"),
            other => panic!("expected After(T-4), got {other:?}"),
        }
    }
}

#[cfg(test)]
mod search_input_tests {
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
}

#[cfg(test)]
mod wrap_tests {
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
}

#[cfg(test)]
mod render_tests {
    use super::super::*;
    use super::fixtures::{board, board_of, item, item_with_parent};
    use pinto::kanban_keys::KeyBindings;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn default_keymap() -> super::super::keymap::KeyMap {
        super::super::keymap::KeyMap::from_bindings(&KeyBindings::default())
            .expect("default keymap")
    }

    fn draw(view: &BoardView, confirming: bool, w: u16, h: u16) -> String {
        let keymap = default_keymap();
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|frame| runtime::render(frame, view, confirming, &keymap))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    /// Returns the buffer as a row-major string. To inspect header lines and body text separately.
    fn draw_rows(view: &BoardView, w: u16, h: u16) -> Vec<String> {
        let keymap = default_keymap();
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|frame| runtime::render(frame, view, false, &keymap))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(w as usize)
            .map(|row| row.iter().map(ratatui::buffer::Cell::symbol).collect())
            .collect()
    }

    #[test]
    fn render_shows_ids_and_titles_and_footer() {
        let mut view = BoardView::new(board(&[("todo", &["T-1"]), ("done", &["T-3"])]));
        view.scroll_to_visible(runtime::capacity_for(80));
        let text = draw(&view, false, 80, 20);
        assert!(text.contains("T-1"), "item id shown");
        assert!(text.contains("todo"), "column title shown");
        // Footer key guide (starts with ASCII, so you can avoid the gap cell problem with wide characters).
        assert!(text.contains("h/l"), "footer key hints shown");
    }

    #[test]
    fn render_empty_board_is_safe() {
        let view = BoardView::new(board(&[]));
        let _ = draw(&view, false, 40, 10);
    }

    #[test]
    fn status_message_replaces_footer_hints() {
        let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
        // Normally, a key guide will appear.
        assert!(
            draw(&view, false, 80, 20).contains("h/l"),
            "hints by default"
        );
        // When setting the status, the footer changes to a warning and the key guide is hidden.
        // (Since wide characters have gap cells between them, the inspection is performed on continuous ASCII parts.)
        view.set_status_message("WIP over: doing".to_string());
        let text = draw(&view, false, 80, 20);
        assert!(text.contains("WIP over"), "warning shown in footer");
        assert!(
            !text.contains("h/l"),
            "key hints hidden while warning shown"
        );
        // Clear to return to the original key guide.
        view.clear_status_message();
        assert!(draw(&view, false, 80, 20).contains("h/l"), "hints restored");
    }

    #[test]
    fn selected_item_key_cell_shares_highlight_background() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;

        // T-1 is selected (first column/first row).
        let view = BoardView::new(board(&[("todo", &["T-1", "T-2"])]));
        let keymap = default_keymap();
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| runtime::render(frame, &view, false, &keymap))
            .unwrap();
        let buffer = terminal.backend().buffer();
        // The cell with the key ('T' in "T-1") has the highlighted background color of the selection (there is no variation due to inversion).
        let key_cell = buffer
            .content()
            .iter()
            .find(|c| c.symbol() == "T" && c.bg == Color::Cyan);
        assert!(
            key_cell.is_some(),
            "selected item's key cell shares the highlight background"
        );
    }

    #[test]
    fn render_quit_popup_overlays_confirmation() {
        let view = BoardView::new(board(&[("todo", &["T-1"])]));
        let text = draw(&view, true, 60, 20);
        // Check the presence in the ASCII part (`Esc`) of the popup body.
        assert!(text.contains("Esc"), "quit popup shown");
    }

    #[test]
    fn render_many_columns_shows_scroll_indicator() {
        let cols: Vec<(&str, &[&str])> = (0..8).map(|_| ("in-progress", &[] as &[&str])).collect();
        let mut view = BoardView::new(board(&cols));
        // If the width is 80, only 2 columns will fit → the right direction indicator will appear.
        view.scroll_to_visible(runtime::capacity_for(80));
        let text = draw(&view, false, 80, 12);
        assert!(text.contains('▶'), "right scroll indicator shown");
    }

    #[test]
    fn render_collapsed_parent_hides_children_but_shows_fold_marker() {
        let board = board_of(&[(
            "todo",
            vec![
                item("T-1", "parent"),
                item_with_parent("T-2", "childXY", "T-1"),
            ],
        )]);
        let mut view = BoardView::new(board);
        view.scroll_to_visible(runtime::capacity_for(80));
        let text = draw(&view, false, 80, 12);
        assert!(text.contains('▸'), "collapsed fold marker shown");
        assert!(text.contains("parent"), "parent visible");
        assert!(
            !text.contains("childXY"),
            "child hidden while collapsed: {text:?}"
        );
    }

    #[test]
    fn render_expanded_parent_shows_child_and_expanded_marker() {
        let board = board_of(&[(
            "todo",
            vec![
                item("T-1", "parent"),
                item_with_parent("T-2", "childXY", "T-1"),
            ],
        )]);
        let mut view = BoardView::new(board);
        view.toggle_expand(); // Deploy T-1.
        view.scroll_to_visible(runtime::capacity_for(80));
        let text = draw(&view, false, 80, 12);
        assert!(text.contains('▾'), "expanded fold marker shown");
        assert!(text.contains("childXY"), "child visible while expanded");
    }

    #[test]
    fn render_footer_documents_the_expand_key() {
        let view = BoardView::new(board(&[("todo", &["T-1"])]));
        let text = draw(&view, false, 100, 12);
        assert!(text.contains("Space"), "footer documents the expand key");
    }

    #[test]
    fn render_card_shows_dependency_markers() {
        let board = board_of(&[(
            "todo",
            vec![
                super::fixtures::item("T-1", "alpha"),
                super::fixtures::item_with_deps("T-2", "beta", &["T-1"]),
            ],
        )]);
        let mut view = BoardView::new(board);
        view.scroll_to_visible(runtime::capacity_for(80));
        // Inspect the main text (from row 1 onward) to distinguish it from the header legend.
        let body = draw_rows(&view, 80, 12)[1..].join("\n");
        assert!(body.contains('⊸'), "depends-on marker on card: {body:?}");
        assert!(body.contains('⊷'), "dependents marker on card: {body:?}");
    }

    #[test]
    fn render_card_without_deps_has_no_markers() {
        let view = BoardView::new(board(&[("todo", &["T-1"])]));
        // Dependency markers do not appear in the main text (excluding header lines with legends).
        let body = draw_rows(&view, 80, 12)[1..].join("\n");
        assert!(
            !body.contains('⊸') && !body.contains('⊷'),
            "no card markers: {body:?}"
        );
    }

    #[test]
    fn render_card_shows_points_and_assignee() {
        let board = board_of(&[(
            "todo",
            vec![super::fixtures::item_with_points_assignee(
                "T-1",
                "alpha",
                Some(5),
                Some("alice"),
            )],
        )]);
        let mut view = BoardView::new(board);
        view.scroll_to_visible(runtime::capacity_for(80));
        // Inspect the main text (from row 1 onward) to distinguish it from the header.
        let body = draw_rows(&view, 80, 12)[1..].join("\n");
        assert!(body.contains('◆'), "points marker on card: {body:?}");
        assert!(body.contains('5'), "points value on card: {body:?}");
        assert!(body.contains("@alice"), "assignee on card: {body:?}");
    }

    #[test]
    fn render_card_shows_points_without_assignee() {
        let board = board_of(&[(
            "todo",
            vec![super::fixtures::item_with_points_assignee(
                "T-1",
                "alpha",
                Some(8),
                None,
            )],
        )]);
        let mut view = BoardView::new(board);
        view.scroll_to_visible(runtime::capacity_for(80));
        let body = draw_rows(&view, 80, 12)[1..].join("\n");
        assert!(body.contains('◆'), "points marker on card: {body:?}");
        assert!(body.contains('8'), "points value on card: {body:?}");
        assert!(!body.contains('@'), "no assignee marker: {body:?}");
    }

    #[test]
    fn render_card_shows_assignee_without_points() {
        let board = board_of(&[(
            "todo",
            vec![super::fixtures::item_with_points_assignee(
                "T-1",
                "alpha",
                None,
                Some("bob"),
            )],
        )]);
        let mut view = BoardView::new(board);
        view.scroll_to_visible(runtime::capacity_for(80));
        let body = draw_rows(&view, 80, 12)[1..].join("\n");
        assert!(body.contains("@bob"), "assignee on card: {body:?}");
        assert!(!body.contains('◆'), "no points marker: {body:?}");
    }

    #[test]
    fn render_card_without_points_or_assignee_has_no_meta_line() {
        // Unestimated / unassigned cards keep their previous single-line layout.
        let view = BoardView::new(board(&[("todo", &["T-1"])]));
        let body = draw_rows(&view, 80, 12)[1..].join("\n");
        assert!(
            !body.contains('◆') && !body.contains('@'),
            "no meta markers: {body:?}"
        );
    }

    #[test]
    fn render_header_shows_dependency_legend() {
        // The legend text uses the same localized wording as the board.
        // Full-width characters are broken into continuation cells on the buffer, so check strictly by checking the span contents of the Line.
        let view = BoardView::new(board(&[("todo", &["T-1"])]));
        let line = runtime::header(&view, 120);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            content.contains(&super::super::dependency_legend(pinto::i18n::current())),
            "shared legend in header: {content:?}"
        );
    }

    #[test]
    fn render_header_shows_active_search_filter() {
        // When a search filter is active, the header must surface it so hidden items look
        // explained rather than like a rendering bug.
        let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
        view.set_search(Some(
            pinto::service::SearchFilter::new("parser", false).unwrap(),
        ));
        let line = runtime::header(&view, 120);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            content.contains("parser"),
            "header surfaces the active filter pattern: {content:?}"
        );
    }

    #[test]
    fn render_header_omits_filter_when_search_inactive() {
        let view = BoardView::new(board(&[("todo", &["T-1"])]));
        let line = runtime::header(&view, 120);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !content.to_lowercase().contains("filter"),
            "no filter indicator without an active search: {content:?}"
        );
    }

    #[test]
    fn render_header_hides_the_indicator_while_the_prompt_is_open() {
        use pinto::service::SearchMode;
        // The bottom prompt echoes the query while typing, so the header must not double it up.
        let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
        view.set_search(Some(
            pinto::service::SearchFilter::new("keep", false).unwrap(),
        ));
        view.begin_search(SearchMode::Contains);
        let line = runtime::header(&view, 120);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !content.contains("filter:"),
            "header indicator suppressed while searching: {content:?}"
        );
    }

    #[test]
    fn footer_shows_a_vim_style_search_prompt_while_typing() {
        use pinto::service::SearchMode;
        let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
        view.begin_search(SearchMode::Contains);
        for c in "foo".chars() {
            view.push_search_char(c);
        }
        let rows = draw_rows(&view, 80, 20);
        let bottom = rows.last().expect("footer row");
        assert!(
            bottom.contains("/foo"),
            "vim-style contains prompt on the bottom line: {bottom:?}"
        );
        // Still inside the kanban display — the header is intact while searching.
        assert!(rows[0].contains("kanban"), "kanban header still rendered");
    }

    #[test]
    fn footer_uses_the_question_prefix_for_regex_search() {
        use pinto::service::SearchMode;
        let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
        view.begin_search(SearchMode::Regex);
        for c in "ba".chars() {
            view.push_search_char(c);
        }
        let rows = draw_rows(&view, 80, 20);
        let bottom = rows.last().expect("footer row");
        assert!(
            bottom.contains("?ba"),
            "vim-style regex prompt on the bottom line: {bottom:?}"
        );
    }

    #[test]
    fn render_help_overlay_includes_secondary_operations_and_clear_filter() {
        use pinto::service::SearchFilter;

        let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
        view.open_help();
        let plain = draw(&view, false, 100, 24);
        assert!(
            plain.contains("add"),
            "help lists secondary operations: {plain:?}"
        );

        view.set_search(Some(SearchFilter::new("task", false).expect("filter")));
        let filtered = draw(&view, false, 100, 24);
        assert!(
            filtered.contains("clear") || filtered.contains("filter"),
            "help explains clearing an active filter: {filtered:?}"
        );
    }

    #[test]
    fn render_input_prompt_and_validation_error() {
        let mut view = BoardView::new(board(&[("todo", &["T-1"])]));
        view.begin_add();
        view.push_input_char('x');
        let rows = draw_rows(&view, 80, 20);
        assert!(rows.last().is_some_and(|row| row.contains('x')));

        view.set_input_error("invalid input".to_string());
        let rows = draw_rows(&view, 80, 20);
        assert!(rows.iter().any(|row| row.contains("invalid input")));
    }

    #[test]
    fn blocked_dependency_marker_is_red_with_bang() {
        use ratatui::style::Color;
        // T-2 depends on incomplete T-1 → Blocking. T-2 is not selected, so it cannot be destroyed by emphasis.
        // To make it easy to distinguish even in environments where colors cannot be used, the marker can also be used as a character string, `⊸!` (same symbol as board).
        let board = board_of(&[(
            "todo",
            vec![
                super::fixtures::item("T-1", "alpha"),
                super::fixtures::item_with_deps("T-2", "beta", &["T-1"]),
            ],
        )]);
        let mut view = BoardView::new(board);
        view.scroll_to_visible(runtime::capacity_for(80));
        let keymap = default_keymap();
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| runtime::render(frame, &view, false, &keymap))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let red = buffer
            .content()
            .iter()
            .find(|c| c.symbol() == "⊸" && c.fg == Color::Red);
        assert!(red.is_some(), "blocked depends-on marker rendered red");
        // Check for `⊸!` in the body excluding the header (row 0 legend).
        let body = draw_rows(&view, 80, 12)[1..].join("\n");
        assert!(
            body.contains("⊸! T-1"),
            "blocked marker uses '⊸!' string: {body:?}"
        );
    }

    #[test]
    fn resolved_dependency_marker_has_no_bang() {
        // Dependency destination (T-1) has completed → Block removed. The marker is `⊸` (without `!`).
        let board = board_of(&[
            (
                "done",
                vec![super::fixtures::completed_item("T-1", "alpha")],
            ),
            (
                "todo",
                vec![super::fixtures::item_with_deps("T-2", "beta", &["T-1"])],
            ),
        ]);
        let mut view = BoardView::new(board);
        view.scroll_to_visible(runtime::capacity_for(80));
        let body = draw_rows(&view, 80, 12)[1..].join("\n");
        assert!(
            body.contains("⊸ T-1") && !body.contains("⊸! T-1"),
            "resolved marker has no '!': {body:?}"
        );
    }

    #[test]
    fn effective_capacity_matches_capacity_for_when_not_maximized() {
        assert_eq!(
            runtime::effective_capacity(80, false),
            runtime::capacity_for(80)
        );
    }

    #[test]
    fn effective_capacity_is_always_one_when_maximized() {
        assert_eq!(runtime::effective_capacity(80, true), 1);
        assert_eq!(
            runtime::effective_capacity(240, true),
            1,
            "wide terminal still yields a single column while maximized"
        );
        assert_eq!(runtime::effective_capacity(24, true), 1);
    }

    #[test]
    fn maximize_shows_only_the_selected_column() {
        let mut view = BoardView::new(board(&[
            ("todo", &["T-1"]),
            ("doing", &["T-2"]),
            ("done", &["T-3"]),
        ]));
        // Move the selection to the center column (doing) and then maximize it.
        view.select_right();
        view.toggle_maximize();
        view.scroll_to_visible(runtime::effective_capacity(80, view.is_maximized()));
        let text = draw(&view, false, 80, 20);
        assert!(text.contains("doing"), "selected column title shown");
        assert!(!text.contains("todo"), "left column hidden while maximized");
        assert!(
            !text.contains("done"),
            "right column hidden while maximized"
        );
    }

    #[test]
    fn popup_is_not_shown_when_closed() {
        let view = BoardView::new(board(&[("todo", &["T-1"])]));
        let text = draw(&view, false, 80, 24);
        assert!(
            !text.contains("詳細"),
            "popup title hidden while popup is closed"
        );
    }

    #[test]
    fn popup_on_empty_column_still_shows_a_placeholder() {
        // Open the popup on T-1, then navigate to the empty `done` column. The selection becomes
        // empty, but the popup must stay visible so it does not look like a return to normal mode.
        let mut view = BoardView::new(board(&[("todo", &["T-1"]), ("done", &[])]));
        view.open_popup();
        view.select_right();
        assert!(view.is_popup_open(), "popup stays open on the empty column");
        assert!(view.popup_content().is_none(), "no item is selected");
        let text = draw(&view, false, 60, 20);
        assert!(
            text.contains("Details"),
            "detail mode is still indicated by the popup title: {text:?}"
        );
        assert!(
            text.contains("No item selected"),
            "placeholder explains that nothing is selected: {text:?}"
        );
    }

    /// Render a single-column board whose card title is a long run of `glyph`, open the popup, and
    /// assert no full-width glyph sits in the column immediately left of the popup's left border.
    /// The board draws its own bordered column at x=0, so the popup corner is the first `┌` at x>0.
    fn assert_popup_left_border_intact(glyph: &str) {
        // Width 90 places the popup's left border on an even column, where a full-width glyph's
        // *first* half lands (the card's `T-1␣␣` prefix starts the title on an even column), so the
        // artifact is actually reproduced rather than a harmless continuation cell.
        const W: u16 = 90;
        const H: u16 = 24;
        let board = board_of(&[("todo", vec![item("T-1", &glyph.repeat(200))])]);
        let mut view = BoardView::new(board);
        view.open_popup();
        let keymap = default_keymap();
        let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
        terminal
            .draw(|frame| runtime::render(frame, &view, false, &keymap))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let at = |x: u16, y: u16| buffer.cell((x, y)).unwrap().symbol().to_string();
        // Popup top-left corner: the first `┌` in a column other than 0 (the board owns x=0).
        let mut corner = None;
        'find: for y in 0..H {
            for x in 1..W {
                if at(x, y) == "┌" {
                    corner = Some((x, y));
                    break 'find;
                }
            }
        }
        let (px, top) = corner.expect("popup top-left corner drawn at x>0");
        let bottom = (top + 1..H)
            .find(|&y| at(px, y) == "└")
            .expect("popup bottom-left corner drawn");
        for y in top..=bottom {
            let sym = at(px - 1, y);
            assert!(
                display_width(&sym) <= 1,
                "full-width glyph {sym:?} overlaps the left border at row {y} (glyph {glyph:?})"
            );
        }
    }

    #[test]
    fn popup_left_border_is_not_broken_by_wide_cjk_text() {
        assert_popup_left_border_intact("あ");
    }

    #[test]
    fn popup_left_border_is_not_broken_by_wide_emoji() {
        // The repair is width-based (East Asian Width), so any full-width glyph is covered — an
        // emoji here — not just CJK ideographs.
        assert_popup_left_border_intact("😀");
    }

    #[test]
    fn popup_shows_core_fields() {
        let board = board_of(&[(
            "todo",
            vec![super::fixtures::item_with_deps(
                "T-1",
                "alpha title",
                &["T-2"],
            )],
        )]);
        let mut view = BoardView::new(board);
        view.open_popup();
        let text = draw(&view, false, 80, 24);
        assert!(text.contains("T-1"), "id shown");
        assert!(text.contains("alpha title"), "title shown");
        assert!(text.contains("todo"), "status shown");
        assert!(text.contains("Depends on"), "depends-on label shown");
        assert!(text.contains("T-2"), "depends-on id shown");
    }

    #[test]
    fn popup_shows_linked_commits() {
        let mut it = item("T-1", "alpha");
        it.commits = vec!["abc123456789".to_string()];
        let board = board_of(&[("todo", vec![it])]);
        let mut view = BoardView::new(board);
        view.open_popup();
        let text = draw(&view, false, 80, 24);

        assert!(text.contains("Commits"), "commits label shown");
        assert!(text.contains("abc123456789"), "linked commit shown");
    }

    #[test]
    fn popup_shows_body_text() {
        // Wide characters (CJK) are drawn across the gap cell, so check the existence of each character.
        // (Same style as other drawing tests).
        let mut it = item("T-1", "alpha");
        it.body = "本文の内容をここに書く".to_string();
        let board = board_of(&[("todo", vec![it])]);
        let mut view = BoardView::new(board);
        view.open_popup();
        // Tall enough to fit the metadata header and the body without scrolling.
        let text = draw(&view, false, 80, 40);
        assert!(text.contains('本'), "body text shown: {text:?}");
        assert!(text.contains('容'), "body text shown: {text:?}");
    }

    #[test]
    fn popup_shows_parent_and_children() {
        let board = board_of(&[(
            "todo",
            vec![
                item("T-1", "parent"),
                item_with_parent("T-2", "child", "T-1"),
            ],
        )]);
        let mut view = BoardView::new(board);
        view.open_popup(); // Selecting T-1 (parent).
        let text = draw(&view, false, 80, 24);
        assert!(text.contains("Children"), "children label shown");
        assert!(text.contains("T-2"), "child id listed on parent's popup");

        view.close_popup();
        view.toggle_expand();
        view.select_down();
        view.open_popup(); // Selecting child T-2.
        let text = draw(&view, false, 80, 24);
        assert!(text.contains("Parent"), "parent label shown");
        assert!(text.contains("T-1"), "parent id listed on child's popup");
    }

    #[test]
    fn popup_does_not_break_layout_on_a_tiny_terminal() {
        let mut it = item("T-1", "alpha");
        it.body = "a b c d e f g h i j k l m n o p q r s t".to_string();
        let board = board_of(&[("todo", vec![it])]);
        let mut view = BoardView::new(board);
        view.open_popup();
        // You can draw without panic even on extremely small terminals.
        let _ = draw(&view, false, 12, 6);
        let _ = draw(&view, false, 1, 1);
    }

    #[test]
    fn popup_scroll_changes_visible_body_window() {
        // Make the text too long to fit, and make sure that the visible window changes when you scroll.
        let mut it = item("T-1", "alpha");
        it.body = (1..=60)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let board = board_of(&[("todo", vec![it])]);
        let mut view = BoardView::new(board);
        view.open_popup();
        // Wide enough that the metadata header does not wrap, so the body starts within the viewport.
        let (w, h) = (80, 40);
        let before = draw(&view, false, w, h);
        assert!(before.contains("line 1 "), "first line visible initially");
        assert!(
            !before.contains("line 60"),
            "far tail not yet visible: {before:?}"
        );

        let keymap = default_keymap();
        let max = runtime::popup_max_scroll(&view, Some(ratatui::layout::Size::new(w, h)), &keymap);
        assert!(max > 0, "content should overflow the popup viewport");
        view.scroll_popup(i32::from(max), max);
        let after = draw(&view, false, w, h);
        assert!(
            after.contains("line 60"),
            "last line visible after scrolling to the max: {after:?}"
        );
        assert!(
            !after.contains("line 1 "),
            "top of the body scrolled out of view: {after:?}"
        );
    }

    #[test]
    fn popup_max_scroll_is_zero_when_closed() {
        let view = BoardView::new(board(&[("todo", &["T-1"])]));
        let keymap = default_keymap();
        assert_eq!(runtime::popup_max_scroll(&view, None, &keymap), 0);
    }
}
