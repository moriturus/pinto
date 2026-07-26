//! Kanban key interpretation and view-state transitions.

use super::super::keymap::KeyMap;
use super::actions::{
    abort_search, apply_incremental_filter, clear_filter, commit_search, reload, reorder,
    submit_input, transition,
};
use super::input::{
    HelpKeyAction, PopupAction, QuitIntent, help_key_action, popup_action, quit_intent,
    should_close_help_after_key,
};
use super::{BoardView, ExitMode, FrameDriver, LoopControl, help_max_scroll, popup_max_scroll};
use anyhow::Result;
use pinto::i18n::{Message, current};
use pinto::kanban_keys::KeyAction;
use pinto::service::SearchMode;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use tokio::runtime::Handle;

/// Interpret one pressed key and apply the resulting state transition.
pub(super) struct DispatchContext<'a, T: FrameDriver> {
    pub(super) terminal: &'a mut T,
    pub(super) handle: &'a Handle,
    pub(super) dir: &'a Path,
    pub(super) view: &'a mut BoardView,
    pub(super) keymap: &'a KeyMap,
    pub(super) confirming: &'a mut Option<ExitMode>,
    pub(super) confirm_quit: bool,
}

pub(super) fn dispatch_key<T: FrameDriver>(
    context: &mut DispatchContext<'_, T>,
    key: KeyEvent,
) -> Result<LoopControl> {
    let context = &mut *context;
    let terminal = &mut *context.terminal;
    let handle = context.handle;
    let dir = context.dir;
    let view = &mut *context.view;
    let keymap = context.keymap;
    let confirming = &mut *context.confirming;
    let confirm_quit = context.confirm_quit;

    // While the confirmation popup is displayed, interpret only the confirmation keys.
    // Esc has the same effect as yes; preserve the mode that opened the popup.
    if let Some(mode) = *confirming {
        if keymap.matches(KeyAction::ConfirmQuit, key) {
            return Ok(LoopControl::Exit(mode));
        }
        if keymap.matches(KeyAction::CancelQuit, key) {
            *confirming = None;
        }
        return Ok(LoopControl::Continue);
    }

    // The help window is a non-modal overlay. Its toggle key is handled here, while all
    // other commands continue to the underlying board/detail/form mode. Scroll keys also
    // fall through, so the default `j`/`k` keys move the cursor while help remains visible.
    if view.is_help_open() {
        let max_scroll = help_max_scroll(view, terminal.size().ok(), keymap);
        match help_key_action(keymap, key) {
            HelpKeyAction::Close => {
                view.close_help();
                return Ok(LoopControl::Continue);
            }
            HelpKeyAction::ScrollUp => view.scroll_help(-1, max_scroll),
            HelpKeyAction::ScrollDown => view.scroll_help(1, max_scroll),
            HelpKeyAction::PassThrough => {}
        }
        if should_close_help_after_key(view, keymap, key) {
            view.close_help();
        }
    }

    // While the details popup is open, arrows and `j`/`k` scroll the body; `H`/`J`/`K`/`L`
    // move the selection so the popup follows it; `e` edits the shown item in `$EDITOR`;
    // and `Esc`/`q`/`v` close it. Moving and sorting cards stay disabled.
    if view.is_popup_open() {
        let max_scroll = popup_max_scroll(view, terminal.size().ok(), keymap);
        if keymap.matches(KeyAction::Help, key) {
            view.open_help();
            return Ok(LoopControl::Continue);
        }
        view.clear_status_message();
        match popup_action(keymap, key) {
            PopupAction::Close => view.close_popup(),
            PopupAction::ScrollUp => view.scroll_popup(-1, max_scroll),
            PopupAction::ScrollDown => view.scroll_popup(1, max_scroll),
            PopupAction::SelectUp => view.select_up(),
            PopupAction::SelectDown => view.select_down(),
            PopupAction::SelectLeft => view.select_left(),
            PopupAction::SelectRight => view.select_right(),
            PopupAction::Edit => {
                terminal.edit_selected(handle, dir, view)?;
                view.reset_popup_scroll();
            }
            PopupAction::None => {}
        }
        return Ok(LoopControl::Continue);
    }

    // Add/dependency/parent forms own the keyboard until they are submitted or cancelled.
    // Relation forms additionally allow cursor navigation while their ID buffer is empty.
    if view.is_input_active() {
        let selecting_target = view.is_relation_input()
            && view.input_buffer().is_empty()
            && if keymap.matches(KeyAction::SelectLeft, key) {
                view.select_left();
                true
            } else if keymap.matches(KeyAction::SelectRight, key) {
                view.select_right();
                true
            } else if keymap.matches(KeyAction::SelectUp, key) {
                view.select_up();
                true
            } else if keymap.matches(KeyAction::SelectDown, key) {
                view.select_down();
                true
            } else {
                false
            };
        let step = if selecting_target {
            Ok(())
        } else {
            match key.code {
                KeyCode::Esc => {
                    view.end_input();
                    Ok(())
                }
                KeyCode::Enter => submit_input(handle, dir, view),
                KeyCode::Backspace => {
                    view.pop_input_char();
                    Ok(())
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.push_input_char(c);
                    Ok(())
                }
                _ => Ok(()),
            }
        };
        step?;
        return Ok(LoopControl::Continue);
    }

    // While the vim-style search prompt is open, keystrokes edit the query in place instead of
    // driving the board: printable characters are typed literally, Backspace erases, Enter
    // applies (empty clears), and Esc cancels (rolling back to the pre-search filter).
    if view.is_searching() {
        match key.code {
            KeyCode::Esc => abort_search(handle, dir, view)?,
            KeyCode::Enter => commit_search(handle, dir, view)?,
            KeyCode::Backspace => {
                view.pop_search_char();
                apply_incremental_filter(handle, dir, view)?;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.push_search_char(c);
                apply_incremental_filter(handle, dir, view)?;
            }
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    // Clear the previous temporary status (WIP warning, etc.) each time a new operation is accepted.
    // If there is a violation in the transition, the `transition` below will reset it.
    view.clear_status_message();
    if view.search_filter().is_some() && keymap.matches(KeyAction::ClearFilter, key) {
        clear_filter(handle, dir, view)?;
    } else if keymap.matches(KeyAction::Shell, key) || keymap.matches(KeyAction::Quit, key) {
        match quit_intent(keymap, key, confirm_quit) {
            QuitIntent::Leave(mode) => return Ok(LoopControl::Exit(mode)),
            QuitIntent::Confirm(mode) => *confirming = Some(mode),
            QuitIntent::None => {}
        }
    } else if keymap.matches(KeyAction::SelectLeft, key) {
        view.select_left();
    } else if keymap.matches(KeyAction::SelectRight, key) {
        view.select_right();
    } else if keymap.matches(KeyAction::SelectUp, key) {
        view.select_up();
    } else if keymap.matches(KeyAction::SelectDown, key) {
        view.select_down();
    } else if keymap.matches(KeyAction::MoveLeft, key) {
        transition(handle, dir, view, -1)?;
    } else if keymap.matches(KeyAction::MoveRight, key) {
        transition(handle, dir, view, 1)?;
    } else if keymap.matches(KeyAction::ReorderUp, key) {
        reorder(handle, dir, view, -1)?;
    } else if keymap.matches(KeyAction::ReorderDown, key) {
        reorder(handle, dir, view, 1)?;
    } else if keymap.matches(KeyAction::ToggleExpand, key) {
        view.toggle_expand();
    } else if keymap.matches(KeyAction::Add, key) {
        view.begin_add();
    } else if keymap.matches(KeyAction::DependencyAdd, key) {
        if !view.begin_dependency_add() {
            view.set_status_message(current().text(Message::KanbanNoSelection));
        }
    } else if keymap.matches(KeyAction::DependencyRemove, key) {
        if !view.begin_dependency_remove() {
            view.set_status_message(current().text(Message::KanbanNoSelection));
        }
    } else if keymap.matches(KeyAction::Parent, key) {
        if !view.begin_parent() {
            view.set_status_message(current().text(Message::KanbanNoSelection));
        }
    } else if keymap.matches(KeyAction::Split, key) {
        if !view.begin_split() {
            view.set_status_message(current().text(Message::KanbanNoSelection));
        }
    } else if keymap.matches(KeyAction::Edit, key) {
        terminal.edit_selected(handle, dir, view)?;
    } else if keymap.matches(KeyAction::Reload, key) {
        reload(handle, dir, view)?;
    } else if keymap.matches(KeyAction::Maximize, key) {
        view.toggle_maximize();
    } else if keymap.matches(KeyAction::Search, key) {
        view.begin_search(SearchMode::Contains);
        apply_incremental_filter(handle, dir, view)?;
    } else if keymap.matches(KeyAction::RegexSearch, key) {
        view.begin_search(SearchMode::Regex);
    } else if keymap.matches(KeyAction::Help, key) {
        view.open_help();
    } else if keymap.matches(KeyAction::Details, key) {
        view.open_popup();
    }
    Ok(LoopControl::Continue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinto::backlog::{BacklogItem, Status};
    use pinto::rank::Rank;
    use pinto::service::{
        Board, BoardColumn, BoardQuery, NewItem, add_item_with_outcome, board, init_board,
    };
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
    use ratatui::layout::Size;
    use std::io;
    use tempfile::TempDir;

    struct FakeFrameDriver {
        size: Size,
        edit_calls: usize,
    }

    impl FrameDriver for FakeFrameDriver {
        fn size(&mut self) -> io::Result<Size> {
            Ok(self.size)
        }

        fn draw(
            &mut self,
            _view: &BoardView,
            _confirming: bool,
            _keymap: &KeyMap,
        ) -> io::Result<()> {
            Ok(())
        }

        fn edit_selected(
            &mut self,
            _handle: &Handle,
            _dir: &Path,
            _view: &mut BoardView,
        ) -> Result<()> {
            self.edit_calls += 1;
            Ok(())
        }
    }

    fn keymap() -> KeyMap {
        KeyMap::from_bindings(&pinto::kanban_keys::KeyBindings::default()).expect("default keymap")
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[allow(clippy::too_many_arguments)]
    fn send(
        frame: &mut FakeFrameDriver,
        handle: &Handle,
        dir: &Path,
        view: &mut BoardView,
        keymap: &KeyMap,
        confirming: &mut Option<ExitMode>,
        confirm_quit: bool,
        key: KeyEvent,
    ) -> LoopControl {
        let mut context = DispatchContext {
            terminal: frame,
            handle,
            dir,
            view,
            keymap,
            confirming,
            confirm_quit,
        };
        dispatch_key(&mut context, key).expect("key dispatch")
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
        BoardView::new(Board {
            columns: vec![BoardColumn {
                status: Status::new("todo"),
                items: vec![item],
            }],
            orphaned: Vec::new(),
        })
    }

    fn empty_view() -> BoardView {
        BoardView::new(Board {
            columns: vec![BoardColumn {
                status: Status::new("todo"),
                items: Vec::new(),
            }],
            orphaned: Vec::new(),
        })
    }

    #[test]
    fn dispatches_overlay_and_form_state_without_terminal_side_effects() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let handle = runtime.handle().clone();
        let dir = Path::new(".");
        let keymap = keymap();
        let mut frame = FakeFrameDriver {
            size: Size::new(20, 5),
            edit_calls: 0,
        };
        let mut confirming = Some(ExitMode::Shell);
        let mut view = view_with_item();

        assert_eq!(
            send(
                &mut frame,
                &handle,
                dir,
                &mut view,
                &keymap,
                &mut confirming,
                true,
                key(KeyCode::Char('y')),
            ),
            LoopControl::Exit(ExitMode::Shell)
        );
        confirming = Some(ExitMode::Quit);
        assert_eq!(
            send(
                &mut frame,
                &handle,
                dir,
                &mut view,
                &keymap,
                &mut confirming,
                true,
                key(KeyCode::Char('n')),
            ),
            LoopControl::Continue
        );
        assert!(confirming.is_none());

        view.open_help();
        send(
            &mut frame,
            &handle,
            dir,
            &mut view,
            &keymap,
            &mut confirming,
            false,
            key(KeyCode::Char('k')),
        );
        assert!(!view.is_help_open());
        view.open_help();
        send(
            &mut frame,
            &handle,
            dir,
            &mut view,
            &keymap,
            &mut confirming,
            false,
            key(KeyCode::Char('x')),
        );
        assert!(view.is_help_open());
        send(
            &mut frame,
            &handle,
            dir,
            &mut view,
            &keymap,
            &mut confirming,
            false,
            key(KeyCode::Char('?')),
        );
        assert!(!view.is_help_open());

        view.open_popup();
        send(
            &mut frame,
            &handle,
            dir,
            &mut view,
            &keymap,
            &mut confirming,
            false,
            key(KeyCode::Char('e')),
        );
        assert_eq!(frame.edit_calls, 1);
        assert!(view.is_popup_open());
        send(
            &mut frame,
            &handle,
            dir,
            &mut view,
            &keymap,
            &mut confirming,
            false,
            key(KeyCode::Char('v')),
        );
        assert!(!view.is_popup_open());

        view.begin_add();
        send(
            &mut frame,
            &handle,
            dir,
            &mut view,
            &keymap,
            &mut confirming,
            false,
            key(KeyCode::Char('a')),
        );
        assert_eq!(view.input_buffer(), "a");
        send(
            &mut frame,
            &handle,
            dir,
            &mut view,
            &keymap,
            &mut confirming,
            false,
            key(KeyCode::Backspace),
        );
        send(
            &mut frame,
            &handle,
            dir,
            &mut view,
            &keymap,
            &mut confirming,
            false,
            key(KeyCode::Esc),
        );
        assert!(!view.is_input_active());

        let mut empty = empty_view();
        for code in [
            KeyCode::Char('d'),
            KeyCode::Char('D'),
            KeyCode::Char('p'),
            KeyCode::Char('s'),
        ] {
            send(
                &mut frame,
                &handle,
                dir,
                &mut empty,
                &keymap,
                &mut confirming,
                false,
                key(code),
            );
            assert!(empty.status_message().is_some());
        }
    }

    #[tokio::test]
    async fn dispatches_board_actions_and_search_without_a_tty() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        for title in ["Alpha", "Beta"] {
            add_item_with_outcome(dir.path(), title, NewItem::default())
                .await
                .expect("add item");
        }
        let display_columns = vec![
            "todo".to_string(),
            "in-progress".to_string(),
            "review".to_string(),
            "done".to_string(),
        ];
        let loaded =
            super::super::load_display_board(dir.path(), &BoardQuery::default(), &display_columns)
                .await
                .expect("load board");
        let view = BoardView::new_with_scope_and_query(
            loaded.display,
            loaded.full,
            display_columns,
            BoardQuery::default(),
        );
        let handle = Handle::current();
        let action_dir = dir.path().to_path_buf();

        tokio::task::spawn_blocking(move || {
            let keymap = keymap();
            let mut frame = FakeFrameDriver {
                size: Size::new(20, 5),
                edit_calls: 0,
            };
            let mut confirming = None;
            let mut view = view;

            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('L')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('H')),
            );
            for code in [KeyCode::Char('K'), KeyCode::Char('J')] {
                send(
                    &mut frame,
                    &handle,
                    &action_dir,
                    &mut view,
                    &keymap,
                    &mut confirming,
                    false,
                    key(code),
                );
            }
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char(' ')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('r')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('m')),
            );
            assert!(view.is_maximized());
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('m')),
            );
            assert!(!view.is_maximized());

            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('a')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Esc),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('d')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Esc),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('e')),
            );

            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::CONTROL),
            );
            assert!(view.is_searching());
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('[')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Enter),
            );
            assert!(view.is_searching());
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Esc),
            );
            assert!(!view.is_searching());

            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('/')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('A')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Backspace),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Enter),
            );
            assert!(!view.is_searching());

            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('?')),
            );
            assert!(view.is_help_open());
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('?')),
            );
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('v')),
            );
            assert!(view.is_popup_open());
            send(
                &mut frame,
                &handle,
                &action_dir,
                &mut view,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Char('v')),
            );
            assert!(!view.is_popup_open());

            let mut filtered = view;
            filtered.set_search(Some(
                pinto::service::SearchFilter::new("Alpha", false).expect("filter"),
            ));
            let control = send(
                &mut frame,
                &handle,
                &action_dir,
                &mut filtered,
                &keymap,
                &mut confirming,
                false,
                key(KeyCode::Esc),
            );
            assert_eq!(control, LoopControl::Continue);
            assert!(filtered.search_filter().is_none());

            assert_eq!(
                send(
                    &mut frame,
                    &handle,
                    &action_dir,
                    &mut filtered,
                    &keymap,
                    &mut confirming,
                    false,
                    key(KeyCode::Char('Q')),
                ),
                LoopControl::Exit(ExitMode::Shell)
            );
            Ok::<_, anyhow::Error>(())
        })
        .await
        .expect("dispatch task")
        .expect("dispatch actions");

        let persisted = board(dir.path(), &BoardQuery::default())
            .await
            .expect("board remains readable");
        assert_eq!(persisted.columns.len(), 4);
    }
}
