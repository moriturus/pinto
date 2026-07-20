use super::super::*;
use super::fixtures::{board, board_of, item, item_with_parent};
use pinto::kanban_keys::KeyBindings;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn default_keymap() -> super::super::keymap::KeyMap {
    super::super::keymap::KeyMap::from_bindings(&KeyBindings::default()).expect("default keymap")
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
