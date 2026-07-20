mod quit_intent_tests {
    use super::super::*;

    fn intent(code: KeyCode, confirm_quit: bool) -> QuitIntent {
        let keymap = KeyMap::from_bindings(&KeyBindings::default()).expect("default keymap");
        quit_intent(
            &keymap,
            event::KeyEvent::new(code, KeyModifiers::NONE),
            confirm_quit,
        )
    }

    #[test]
    fn uppercase_q_leaves_for_shell_when_disabled() {
        assert_eq!(
            intent(KeyCode::Char('Q'), false),
            QuitIntent::Leave(ExitMode::Shell)
        );
    }

    #[test]
    fn uppercase_q_confirms_for_shell_when_enabled() {
        assert_eq!(
            intent(KeyCode::Char('Q'), true),
            QuitIntent::Confirm(ExitMode::Shell)
        );
    }

    #[test]
    fn lowercase_q_and_esc_confirm_when_enabled() {
        assert_eq!(
            intent(KeyCode::Char('q'), true),
            QuitIntent::Confirm(ExitMode::Quit)
        );
        assert_eq!(
            intent(KeyCode::Esc, true),
            QuitIntent::Confirm(ExitMode::Quit)
        );
    }

    #[test]
    fn lowercase_q_and_esc_quit_immediately_when_disabled() {
        assert_eq!(
            intent(KeyCode::Char('q'), false),
            QuitIntent::Leave(ExitMode::Quit)
        );
        assert_eq!(
            intent(KeyCode::Esc, false),
            QuitIntent::Leave(ExitMode::Quit)
        );
    }

    #[test]
    fn unrelated_keys_are_none() {
        assert_eq!(intent(KeyCode::Char('h'), true), QuitIntent::None);
        assert_eq!(intent(KeyCode::Char('j'), false), QuitIntent::None);
    }
}

mod popup_action_tests {
    use super::super::*;

    fn action(code: KeyCode, modifiers: KeyModifiers) -> PopupAction {
        let keymap = KeyMap::from_bindings(&KeyBindings::default()).expect("default keymap");
        popup_action(&keymap, event::KeyEvent::new(code, modifiers))
    }

    #[test]
    fn plain_arrows_and_vim_keys_scroll_the_body() {
        assert_eq!(
            action(KeyCode::Up, KeyModifiers::NONE),
            PopupAction::ScrollUp
        );
        assert_eq!(
            action(KeyCode::Char('k'), KeyModifiers::NONE),
            PopupAction::ScrollUp
        );
        assert_eq!(
            action(KeyCode::Down, KeyModifiers::NONE),
            PopupAction::ScrollDown
        );
        assert_eq!(
            action(KeyCode::Char('j'), KeyModifiers::NONE),
            PopupAction::ScrollDown
        );
    }

    #[test]
    fn esc_and_q_close_the_popup() {
        assert_eq!(action(KeyCode::Esc, KeyModifiers::NONE), PopupAction::Close);
        assert_eq!(
            action(KeyCode::Char('q'), KeyModifiers::NONE),
            PopupAction::Close
        );
    }

    #[test]
    fn v_closes_the_popup_for_toggle_behavior() {
        assert_eq!(
            action(KeyCode::Char('v'), KeyModifiers::NONE),
            PopupAction::Close
        );
    }

    #[test]
    fn e_enters_edit_mode_for_the_shown_item() {
        assert_eq!(
            action(KeyCode::Char('e'), KeyModifiers::NONE),
            PopupAction::Edit
        );
    }

    #[test]
    fn unrelated_keys_are_ignored() {
        assert_eq!(
            action(KeyCode::Char('x'), KeyModifiers::NONE),
            PopupAction::None
        );
        assert_eq!(
            action(KeyCode::Enter, KeyModifiers::NONE),
            PopupAction::None
        );
    }
}

mod interaction_decision_tests {
    use super::super::render::*;
    use super::super::*;
    use pinto::backlog::{BacklogItem, Status};
    use pinto::rank::Rank;
    use pinto::service::SearchFilter;

    fn keymap() -> KeyMap {
        KeyMap::from_bindings(&KeyBindings::default()).expect("default keymap")
    }

    fn view_with_item() -> BoardView {
        let item = BacklogItem::new(
            "T-1".parse().expect("item id"),
            "task".to_string(),
            Status::new("todo"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            chrono::Utc::now(),
        )
        .expect("item");
        BoardView::new(pinto::service::Board {
            columns: vec![pinto::service::BoardColumn {
                status: Status::new("todo"),
                items: vec![item],
            }],
            orphaned: Vec::new(),
        })
    }

    #[test]
    fn popup_selection_keys_are_classified_individually() {
        let keymap = keymap();
        let cases = [
            (KeyCode::Char('K'), PopupAction::SelectUp),
            (KeyCode::Char('J'), PopupAction::SelectDown),
            (KeyCode::Char('H'), PopupAction::SelectLeft),
            (KeyCode::Char('L'), PopupAction::SelectRight),
        ];
        for (code, expected) in cases {
            assert_eq!(
                popup_action(&keymap, event::KeyEvent::new(code, KeyModifiers::NONE)),
                expected
            );
        }
    }

    #[test]
    fn help_keys_cover_close_scroll_and_passthrough() {
        let keymap = keymap();
        assert_eq!(
            help_key_action(
                &keymap,
                event::KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            ),
            HelpKeyAction::Close
        );
        assert_eq!(
            help_key_action(
                &keymap,
                event::KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
            ),
            HelpKeyAction::ScrollUp
        );
        assert_eq!(
            help_key_action(
                &keymap,
                event::KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            ),
            HelpKeyAction::ScrollDown
        );
        assert_eq!(
            help_key_action(
                &keymap,
                event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            ),
            HelpKeyAction::PassThrough
        );
    }

    #[test]
    fn help_scroll_maximum_handles_default_and_filtered_views() {
        let keymap = keymap();
        let mut view = view_with_item();
        let unfiltered = help_max_scroll(&view, None, &keymap);
        view.set_search(Some(SearchFilter::new("task", false).expect("filter")));
        let filtered = help_max_scroll(&view, Some(ratatui::layout::Size::new(20, 5)), &keymap);
        assert_eq!(unfiltered, 0, "the default terminal fits the help text");
        assert!(filtered > 0, "a small terminal needs help scrolling");
    }

    #[test]
    fn help_overlay_acceptance_follows_popup_forms_search_and_board_modes() {
        let keymap = keymap();
        let key = |code| event::KeyEvent::new(code, KeyModifiers::NONE);

        let mut popup_view = view_with_item();
        popup_view.open_popup();
        assert!(should_close_help_after_key(
            &popup_view,
            &keymap,
            key(KeyCode::Char('v'))
        ));
        assert!(!should_close_help_after_key(
            &popup_view,
            &keymap,
            key(KeyCode::Char('x'))
        ));

        let mut add_view = view_with_item();
        add_view.begin_add();
        assert!(should_close_help_after_key(
            &add_view,
            &keymap,
            key(KeyCode::Char('a'))
        ));
        assert!(should_close_help_after_key(
            &add_view,
            &keymap,
            key(KeyCode::Enter)
        ));
        assert!(!should_close_help_after_key(
            &add_view,
            &keymap,
            event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ));

        let mut relation_view = view_with_item();
        assert!(relation_view.begin_dependency_add());
        assert!(should_close_help_after_key(
            &relation_view,
            &keymap,
            key(KeyCode::Char('h'))
        ));

        let mut search_view = view_with_item();
        search_view.begin_search(SearchMode::Contains);
        assert!(should_close_help_after_key(
            &search_view,
            &keymap,
            key(KeyCode::Char('q'))
        ));
        assert!(!should_close_help_after_key(
            &search_view,
            &keymap,
            key(KeyCode::F(1))
        ));

        let mut filtered_view = view_with_item();
        filtered_view.set_search(Some(SearchFilter::new("task", false).expect("filter")));
        assert!(should_close_help_after_key(
            &filtered_view,
            &keymap,
            key(KeyCode::Esc)
        ));
        assert!(should_close_help_after_key(
            &filtered_view,
            &keymap,
            key(KeyCode::Char('a'))
        ));
        assert!(!should_close_help_after_key(
            &filtered_view,
            &keymap,
            key(KeyCode::Char('x'))
        ));
    }

    #[test]
    fn text_entry_accepts_editing_keys_but_not_function_keys_or_control_chars() {
        for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Backspace] {
            assert!(text_entry_key_is_accepted(event::KeyEvent::new(
                code,
                KeyModifiers::NONE,
            )));
        }
        assert!(text_entry_key_is_accepted(event::KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )));
        assert!(!text_entry_key_is_accepted(event::KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        )));
        assert!(!text_entry_key_is_accepted(event::KeyEvent::new(
            KeyCode::F(1),
            KeyModifiers::NONE,
        )));
    }
}

mod sanitize_left_border_tests {
    use super::super::render::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn full_width_glyph_left_of_the_border_is_blanked() {
        // Border at x=3; a full-width glyph at x=2 would spill its right half onto the border.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        buf[(2, 1)].set_symbol("化");
        sanitize_left_border(&mut buf, Rect::new(3, 0, 5, 3));
        assert_eq!(buf[(2, 1)].symbol(), " ");
    }

    #[test]
    fn half_width_glyph_left_of_the_border_is_kept() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        buf[(2, 1)].set_symbol("A");
        sanitize_left_border(&mut buf, Rect::new(3, 0, 5, 3));
        assert_eq!(buf[(2, 1)].symbol(), "A");
    }

    #[test]
    fn only_the_popups_rows_are_touched() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        buf[(2, 0)].set_symbol("化"); // above the popup
        buf[(2, 1)].set_symbol("化"); // within the popup rows
        sanitize_left_border(&mut buf, Rect::new(3, 1, 5, 2));
        assert_eq!(
            buf[(2, 0)].symbol(),
            "化",
            "rows outside the popup are untouched"
        );
        assert_eq!(
            buf[(2, 1)].symbol(),
            " ",
            "rows within the popup are sanitized"
        );
    }

    #[test]
    fn popup_flush_against_the_left_edge_is_a_noop() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        buf[(0, 1)].set_symbol("化");
        sanitize_left_border(&mut buf, Rect::new(0, 0, 5, 3));
        assert_eq!(
            buf[(0, 1)].symbol(),
            "化",
            "no column exists left of a flush-left popup"
        );
    }
}

mod popup_lines_tests {
    use super::super::render::*;
    use super::super::*;
    use crate::cli::kanban::PopupContent;
    use pinto::backlog::Status;
    use pinto::rank::Rank;
    use ratatui::text::Line;

    fn content_with_body(body: &str) -> PopupContent {
        let now = chrono::Utc::now();
        PopupContent {
            id: "T-1".parse().expect("id"),
            title: "Task".to_string(),
            status: Status::new("todo"),
            acceptance_criteria: pinto::backlog::AcceptanceCriteriaProgress::from_markdown(body),
            rank: Rank::between(None, None).expect("open bounds produce a rank"),
            rank_ordinal: 1,
            points: None,
            labels: vec![],
            assignee: None,
            sprint: None,
            commits: vec![],
            body: body.to_string(),
            parent: None,
            children: vec![],
            depends_on: vec![],
            dependents: vec![],
            start_at: None,
            done_at: None,
            created: now,
            updated: now,
        }
    }

    fn joined(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect()
    }

    #[test]
    fn renders_markdown_body_when_enabled() {
        let content = content_with_body("# Heading\n\n**bold** text");
        let text = joined(&popup_lines_with_timezone(
            &content,
            60,
            true,
            DisplayTimezone::Local,
        ));
        assert!(text.contains("Heading"), "keeps heading text: {text:?}");
        assert!(
            !text.contains("# Heading"),
            "strips heading syntax: {text:?}"
        );
        assert!(
            !text.contains("**bold**"),
            "strips emphasis syntax: {text:?}"
        );
    }

    #[test]
    fn keeps_raw_body_when_markdown_disabled() {
        let content = content_with_body("# Heading\n\n**bold** text");
        let text = joined(&popup_lines_with_timezone(
            &content,
            60,
            false,
            DisplayTimezone::Local,
        ));
        assert!(text.contains("# Heading"), "keeps raw heading: {text:?}");
        assert!(text.contains("**bold**"), "keeps raw emphasis: {text:?}");
    }

    #[test]
    fn shows_placeholder_for_empty_body_regardless_of_markdown() {
        for markdown in [true, false] {
            let content = content_with_body("");
            let text = joined(&popup_lines_with_timezone(
                &content,
                60,
                markdown,
                DisplayTimezone::Local,
            ));
            assert!(
                text.contains(&current().text(Message::KanbanNoBody)),
                "empty body placeholder (markdown={markdown}): {text:?}"
            );
        }
    }

    #[test]
    fn displays_acceptance_criteria_progress() {
        let content = content_with_body("- [x] shipped\n- [ ] documented");
        let text = joined(&popup_lines_with_timezone(
            &content,
            60,
            false,
            DisplayTimezone::Local,
        ));

        assert!(
            text.contains("Acceptance Criteria"),
            "shows progress label: {text:?}"
        );
        assert!(text.contains("1/2"), "shows completed over total: {text:?}");
    }

    #[test]
    fn formats_popup_timestamps_with_configured_timezone() {
        let instant = chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).expect("timestamp");
        let mut content = content_with_body("");
        content.start_at = Some(instant);
        content.done_at = Some(instant);
        content.created = instant;
        content.updated = instant;

        let text = joined(&popup_lines_with_timezone(
            &content,
            60,
            false,
            "+09:00".parse().expect("offset"),
        ));

        assert_eq!(
            text.matches("1970-01-01T09:00:00+09:00").count(),
            4,
            "all popup timestamps use the configured offset: {text:?}"
        );
    }
}

mod popup_rect_tests {
    use super::super::render::*;
    use ratatui::layout::Rect;

    #[test]
    fn popup_rect_does_not_overflow_on_max_size_area() {
        let area = Rect::new(0, 0, u16::MAX, u16::MAX);
        let popup = popup_rect(area);
        assert!(popup.width <= area.width);
        assert!(popup.height <= area.height);
        assert!(popup.x + popup.width <= area.x + area.width);
        assert!(popup.y + popup.height <= area.y + area.height);
    }
}

mod ordering_tests {
    use super::super::actions::*;
    use super::super::*;
    use pinto::backlog::ItemId;
    use pinto::service::NewItem;
    use pinto::service::{
        BoardQuery, LabelMatch, SearchFilter, add_item_with_outcome, create_sprint, init_board,
        move_item,
    };
    use pinto::sprint::SprintId;
    use tempfile::TempDir;

    /// Kanban delegates entirely to [`board`] with the default query, so it
    /// inherits the canonical backlog order and the terminal column's
    /// `done_at`-descending exception. Pin that contract at the kanban layer.
    #[tokio::test]
    async fn load_display_board_inherits_board_default_ordering() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        let mut ids = Vec::new();
        for title in ["Alpha", "Bravo", "Charlie"] {
            let outcome = add_item_with_outcome(dir.path(), title, NewItem::default())
                .await
                .expect("add");
            ids.push(outcome.item.id);
        }
        // Complete in rank order so done_at ascending equals rank order.
        for id in &ids {
            move_item(dir.path(), id, "done").await.expect("move done");
        }

        let display_columns: Vec<String> = vec!["done".to_string()];
        let loaded = load_display_board(dir.path(), &BoardQuery::default(), &display_columns)
            .await
            .expect("load board");
        let done = loaded
            .display
            .columns
            .iter()
            .find(|c| c.status.as_str() == "done")
            .expect("done column");
        let order: Vec<&ItemId> = done.items.iter().map(|it| &it.id).collect();

        // Newest completion leads: reverse of rank order (the documented exception).
        assert_eq!(order, vec![&ids[2], &ids[1], &ids[0]]);
    }

    #[tokio::test]
    async fn load_display_board_applies_startup_scope_and_composed_search() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        create_sprint(
            dir.path(),
            &"S-1".parse::<SprintId>().expect("sprint id"),
            "Sprint One",
            None,
            None,
        )
        .await
        .expect("create sprint");
        add_item_with_outcome(
            dir.path(),
            "Keep target",
            NewItem {
                labels: vec!["ui".to_string(), "backend".to_string()],
                sprint: Some("S-1".to_string()),
                ..NewItem::default()
            },
        )
        .await
        .expect("target item");
        add_item_with_outcome(
            dir.path(),
            "Other label",
            NewItem {
                labels: vec!["ops".to_string()],
                sprint: Some("S-1".to_string()),
                ..NewItem::default()
            },
        )
        .await
        .expect("other label item");
        add_item_with_outcome(
            dir.path(),
            "Other sprint",
            NewItem {
                labels: vec!["ui".to_string()],
                ..NewItem::default()
            },
        )
        .await
        .expect("other sprint item");

        let query = BoardQuery {
            sprint: Some("S-1".to_string()),
            labels: vec!["ui".to_string(), "backend".to_string()],
            label_match: LabelMatch::All,
            search: Some(SearchFilter::new("^Keep", true).expect("regex")),
            ..BoardQuery::default()
        };
        let loaded = load_display_board(dir.path(), &query, &["todo".to_string()])
            .await
            .expect("load filtered board");

        let visible = loaded
            .display
            .columns
            .first()
            .expect("todo column")
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(visible, ["Keep target"]);
        assert_eq!(loaded.full.columns[0].items.len(), 1);
    }

    #[tokio::test]
    async fn live_search_reload_preserves_startup_scope() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        create_sprint(
            dir.path(),
            &"S-1".parse::<SprintId>().expect("sprint id"),
            "Sprint One",
            None,
            None,
        )
        .await
        .expect("create sprint");
        add_item_with_outcome(
            dir.path(),
            "Keep target",
            NewItem {
                labels: vec!["ui".to_string()],
                sprint: Some("S-1".to_string()),
                ..NewItem::default()
            },
        )
        .await
        .expect("target item");
        add_item_with_outcome(
            dir.path(),
            "Other label",
            NewItem {
                labels: vec!["ops".to_string()],
                sprint: Some("S-1".to_string()),
                ..NewItem::default()
            },
        )
        .await
        .expect("other label item");

        let query = BoardQuery {
            sprint: Some("S-1".to_string()),
            labels: vec!["ui".to_string()],
            ..BoardQuery::default()
        };
        let display_columns = vec!["todo".to_string()];
        let loaded = load_display_board(dir.path(), &query, &display_columns)
            .await
            .expect("load startup scope");
        let mut view = BoardView::new_with_scope_and_query(
            loaded.display,
            loaded.full,
            display_columns,
            query,
        );

        let reload_dir = dir.path().to_path_buf();
        let handle = Handle::current();
        let view = tokio::task::spawn_blocking(move || {
            reload_with_filter(
                &handle,
                &reload_dir,
                &mut view,
                Some(SearchFilter::new("Keep", false).expect("search")),
            )?;
            Ok::<_, anyhow::Error>(view)
        })
        .await
        .expect("reload task")
        .expect("reload with live search");

        assert_eq!(view.board_query().sprint.as_deref(), Some("S-1"));
        assert_eq!(view.board_query().labels, ["ui"]);
        let visible = view
            .columns()
            .first()
            .expect("todo column")
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(visible, ["Keep target"]);
    }
}

mod lifecycle_tests {
    use super::super::terminal::PanicHookGuard;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    fn panic_hook_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn panic_hook_guard_restores_previous_hook_after_normal_exit() {
        let _lock = panic_hook_lock().lock().expect("panic hook lock");
        let test_runner_hook = std::panic::take_hook();
        let previous_calls = Arc::new(AtomicUsize::new(0));
        let previous_calls_for_hook = Arc::clone(&previous_calls);
        std::panic::set_hook(Box::new(move |_| {
            previous_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        }));
        let previous_hook = std::panic::take_hook();
        let transient_calls = Arc::new(AtomicUsize::new(0));
        let transient_calls_for_hook = Arc::clone(&transient_calls);
        std::panic::set_hook(Box::new(move |_| {
            transient_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        }));

        {
            let _guard = PanicHookGuard::install(previous_hook);
        }
        let panic_result = catch_unwind(AssertUnwindSafe(|| panic!("after normal exit")));
        let current_hook = std::panic::take_hook();
        std::panic::set_hook(test_runner_hook);

        assert!(panic_result.is_err());
        assert_eq!(previous_calls.load(Ordering::SeqCst), 1);
        assert_eq!(transient_calls.load(Ordering::SeqCst), 0);
        drop(current_hook);
    }

    #[test]
    fn panic_hook_guard_restores_previous_hook_after_unwind() {
        let _lock = panic_hook_lock().lock().expect("panic hook lock");
        let test_runner_hook = std::panic::take_hook();
        let previous_calls = Arc::new(AtomicUsize::new(0));
        let previous_calls_for_hook = Arc::clone(&previous_calls);
        std::panic::set_hook(Box::new(move |_| {
            previous_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        }));
        let previous_hook = std::panic::take_hook();
        let transient_calls = Arc::new(AtomicUsize::new(0));
        let transient_calls_for_hook = Arc::clone(&transient_calls);
        std::panic::set_hook(Box::new(move |_| {
            transient_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        }));

        let first_panic = {
            let _guard = PanicHookGuard::install(previous_hook);
            catch_unwind(AssertUnwindSafe(|| panic!("inside terminal lifecycle")))
        };
        let second_panic = catch_unwind(AssertUnwindSafe(|| panic!("after unwind")));
        let current_hook = std::panic::take_hook();
        std::panic::set_hook(test_runner_hook);

        assert!(first_panic.is_err());
        assert!(second_panic.is_err());
        assert_eq!(transient_calls.load(Ordering::SeqCst), 1);
        assert_eq!(previous_calls.load(Ordering::SeqCst), 1);
        drop(current_hook);
    }

    #[test]
    fn repeated_terminal_lifecycles_do_not_accumulate_hooks() {
        let _lock = panic_hook_lock().lock().expect("panic hook lock");
        let test_runner_hook = std::panic::take_hook();
        let previous_calls = Arc::new(AtomicUsize::new(0));
        let previous_calls_for_hook = Arc::clone(&previous_calls);
        std::panic::set_hook(Box::new(move |_| {
            previous_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        }));

        for transient in 0..2 {
            let previous_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |_| {
                let _ = transient;
            }));
            let _guard = PanicHookGuard::install(previous_hook);
            drop(_guard);
        }

        let panic_result = catch_unwind(AssertUnwindSafe(|| panic!("after repeated exits")));
        let current_hook = std::panic::take_hook();
        std::panic::set_hook(test_runner_hook);

        assert!(panic_result.is_err());
        assert_eq!(previous_calls.load(Ordering::SeqCst), 1);
        drop(current_hook);
    }
}
