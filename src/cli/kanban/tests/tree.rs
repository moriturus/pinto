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
