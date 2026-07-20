//! Drawing and the interactive event loop for the Kanban view.

use super::keymap::KeyMap;
use super::{BoardView, MIN_COLUMN_WIDTH};
use anyhow::Result;
use pinto::i18n::{Message, current};
use pinto::kanban_keys::{KeyAction, KeyBindings};
use pinto::service::{Board, BoardQuery, SearchMode, board};
use pinto::timezone::DisplayTimezone;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::runtime::Handle;

mod input;
mod terminal;

mod actions;
mod render;
#[cfg(test)]
mod tests;

use actions::{
    abort_search, apply_incremental_filter, clear_filter, commit_search, edit_selected, reload,
    reorder, submit_input, transition,
};
#[cfg(test)]
pub(super) use render::header;
#[cfg(test)]
pub(super) use render::render_with_localizer;
pub(super) use render::{help_max_scroll, popup_max_scroll, render};

#[cfg(test)]
use input::text_entry_key_is_accepted;
use input::{
    HelpKeyAction, PopupAction, QuitIntent, help_key_action, popup_action, quit_intent,
    should_close_help_after_key,
};
use terminal::initialize_terminal;

/// Polling interval while waiting for input.
const POLL: Duration = Duration::from_millis(250);

/// Number of minimum-width columns that fit in the available width, clamped to at least one.
/// This is the horizontal scroll-window size.
pub(super) fn capacity_for(width: u16) -> usize {
    (width / MIN_COLUMN_WIDTH).max(1) as usize
}

/// Number of columns drawn in the horizontal viewport.
///
/// Maximized mode (`maximized`) always returns one column (selected column only) regardless of width;
/// In normal mode, the result of [`capacity_for`] is used as is.
pub(super) fn effective_capacity(width: u16, maximized: bool) -> usize {
    if maximized { 1 } else { capacity_for(width) }
}

/// How the Kanban view was left, as reported back to the caller.
///
/// `q` / `Esc` terminates the view outright, while `Q` hands control to the interactive shell
/// (REPL) instead of exiting the process. The caller ([`super::super::commands`]) decides what to
/// do with each outcome depending on whether the view was launched directly (`pinto kanban`) or
/// from within an existing shell (the `kanban` subcommand of `pinto shell`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitMode {
    /// Leave the view (`q` / `Esc`). Terminates the process when launched directly.
    Quit,
    /// Leave the view for the interactive shell (`Q`) without terminating the process.
    Shell,
}

/// Load the board and run the interaction loop. `confirm_quit` controls whether quitting requires confirmation.
///
/// Terminal control and event polling are blocking, so the loop runs on a dedicated blocking thread
/// via `spawn_blocking`. Async service calls made by the loop (`board`, `move_item`, and
/// `reorder_item`) are driven through the Tokio runtime handle.
pub(super) struct RunOptions {
    pub(super) dir: PathBuf,
    pub(super) confirm_quit: bool,
    pub(super) bindings: KeyBindings,
    pub(super) markdown: bool,
    pub(super) timezone: DisplayTimezone,
    pub(super) display_columns: Vec<String>,
    pub(super) initial_column: Option<String>,
    pub(super) maximize: bool,
    pub(super) query: BoardQuery,
}

pub(super) async fn run(options: RunOptions) -> Result<ExitMode> {
    let RunOptions {
        dir,
        confirm_quit,
        bindings,
        markdown,
        timezone,
        display_columns,
        initial_column,
        maximize,
        query,
    } = options;
    let loaded = load_display_board(&dir, &query, &display_columns).await?;
    let mut view =
        BoardView::new_with_scope_and_query(loaded.display, loaded.full, display_columns, query);
    if let Some(column) = initial_column.as_deref()
        && !view.select_column(column)
    {
        anyhow::bail!("column not found: {column}");
    }
    view.set_maximized(maximize);
    view.set_render_markdown(markdown);
    view.set_display_timezone(timezone);
    let keymap = KeyMap::from_bindings(&bindings)?;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || event_loop(handle, dir, view, keymap, confirm_quit)).await?
}

/// Board data loaded for Kanban: a display-scoped copy and the full query-scoped board.
struct LoadedBoard {
    /// Board columns rendered by Kanban.
    display: Board,
    /// Full board used for cross-column metadata such as dependencies and children.
    full: Board,
}

/// Load the full board data, then restrict only the columns rendered by Kanban.
///
/// Keeping the full board separate from the display copy preserves cross-column metadata without
/// changing which columns are rendered or persisted.
async fn load_display_board(
    project_dir: &Path,
    query: &BoardQuery,
    display_columns: &[String],
) -> Result<LoadedBoard> {
    let full = board(project_dir, query).await?;
    let display = filter_display_columns(full.clone(), display_columns);
    Ok(LoadedBoard { display, full })
}

/// Keep configured workflow order while selecting only columns meant for display.
fn filter_display_columns(mut board: Board, display_columns: &[String]) -> Board {
    board.columns.retain(|column| {
        display_columns
            .iter()
            .any(|status| status == column.status.as_str())
    });
    board
}

/// An interactive loop that initializes the terminal and updates the view in response to keystrokes.
fn event_loop(
    handle: Handle,
    dir: PathBuf,
    mut view: BoardView,
    keymap: KeyMap,
    confirm_quit: bool,
) -> Result<ExitMode> {
    // Initialize raw mode and the alternate screen. Return non-TTY failures here instead of
    // panicking; `try_init` is used because this is an internal call.
    let (mut terminal, _panic_hook) = initialize_terminal()?;
    // Pending exit mode while the confirmation popup is displayed; `None` when it is hidden.
    let mut confirming: Option<ExitMode> = None;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        loop {
            // Derive the horizontal viewport from the terminal width and keep the selected column visible.
            // Maximized mode always shows one column: the selected column.
            match terminal.size() {
                Ok(size) => {
                    view.scroll_to_visible(effective_capacity(size.width, view.is_maximized()));
                }
                Err(e) => break Err(e.into()),
            }
            if let Err(e) =
                terminal.draw(|frame| render(frame, &view, confirming.is_some(), &keymap))
            {
                break Err(e.into());
            }
            match event::poll(POLL) {
                Ok(false) => continue,
                Err(e) => break Err(e.into()),
                Ok(true) => {}
            }
            let key = match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => key,
                Ok(_) => continue,
                Err(e) => break Err(e.into()),
            };

            // While the confirmation popup is displayed, interpret only the confirmation keys.
            // Esc has the same effect as yes; preserve the mode that opened the popup.
            if let Some(mode) = confirming {
                if keymap.matches(KeyAction::ConfirmQuit, key) {
                    break Ok(mode);
                }
                if keymap.matches(KeyAction::CancelQuit, key) {
                    confirming = None;
                }
                continue;
            }

            // The help window is a non-modal overlay. Its toggle key is handled here, while all
            // other commands continue to the underlying board/detail/form mode. Scroll keys also
            // fall through, so the default `j`/`k` keys move the cursor while help remains visible.
            if view.is_help_open() {
                let max_scroll = help_max_scroll(&view, terminal.size().ok(), &keymap);
                match help_key_action(&keymap, key) {
                    HelpKeyAction::Close => {
                        view.close_help();
                        continue;
                    }
                    HelpKeyAction::ScrollUp => view.scroll_help(-1, max_scroll),
                    HelpKeyAction::ScrollDown => view.scroll_help(1, max_scroll),
                    HelpKeyAction::PassThrough => {}
                }
                if should_close_help_after_key(&view, &keymap, key) {
                    view.close_help();
                }
            }

            // While the details popup is open, arrows and `j`/`k` scroll the body; `H`/`J`/`K`/`L`
            // move the selection so the popup follows it; `e` edits the shown item in `$EDITOR`;
            // and `Esc`/`q`/`v` close it. Moving and sorting cards stay disabled.
            if view.is_popup_open() {
                let max_scroll = popup_max_scroll(&view, terminal.size().ok(), &keymap);
                if keymap.matches(KeyAction::Help, key) {
                    view.open_help();
                    continue;
                }
                // Clear any transient status (e.g. a prior edit result) as each popup key is accepted,
                // mirroring the board-mode loop; the edit action below sets its own status when relevant.
                view.clear_status_message();
                match popup_action(&keymap, key) {
                    PopupAction::Close => view.close_popup(),
                    PopupAction::ScrollUp => view.scroll_popup(-1, max_scroll),
                    PopupAction::ScrollDown => view.scroll_popup(1, max_scroll),
                    PopupAction::SelectUp => view.select_up(),
                    PopupAction::SelectDown => view.select_down(),
                    PopupAction::SelectLeft => view.select_left(),
                    PopupAction::SelectRight => view.select_right(),
                    // Edit the shown item; the popup stays open and follows the (possibly updated) item,
                    // restarting its body at the top since the content may have changed under the offset.
                    PopupAction::Edit => {
                        if let Err(e) = edit_selected(&mut terminal, &handle, &dir, &mut view) {
                            break Err(e);
                        }
                        view.reset_popup_scroll();
                    }
                    PopupAction::None => {}
                }
                continue;
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
                        KeyCode::Enter => submit_input(&handle, &dir, &mut view),
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
                if let Err(e) = step {
                    break Err(e);
                }
                continue;
            }

            // While the vim-style search prompt is open, keystrokes edit the query in place instead of
            // driving the board: printable characters are typed literally, Backspace erases, Enter
            // applies (empty clears), and Esc cancels (rolling back to the pre-search filter). Substring
            // search filters incrementally as you type; the kanban display stays on screen throughout.
            if view.is_searching() {
                let outcome = match key.code {
                    KeyCode::Esc => abort_search(&handle, &dir, &mut view),
                    KeyCode::Enter => commit_search(&handle, &dir, &mut view),
                    KeyCode::Backspace => {
                        view.pop_search_char();
                        apply_incremental_filter(&handle, &dir, &mut view)
                    }
                    // Type printable characters literally; ignore control chords (Ctrl+C etc.).
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        view.push_search_char(c);
                        apply_incremental_filter(&handle, &dir, &mut view)
                    }
                    _ => Ok(()),
                };
                if let Err(e) = outcome {
                    break Err(e);
                }
                continue;
            }

            // Clears the previous temporary status (WIP warning, etc.) each time a new operation is accepted.
            // If there is a violation in the transition, the `transition` below will reset it.
            view.clear_status_message();
            let step = if view.search_filter().is_some()
                && keymap.matches(KeyAction::ClearFilter, key)
            {
                // Clear an active search filter before interpreting the same key as a quit key.
                clear_filter(&handle, &dir, &mut view)
            } else if keymap.matches(KeyAction::Shell, key) || keymap.matches(KeyAction::Quit, key)
            {
                match quit_intent(&keymap, key, confirm_quit) {
                    QuitIntent::Leave(mode) => break Ok(mode),
                    QuitIntent::Confirm(mode) => {
                        confirming = Some(mode);
                        Ok(())
                    }
                    QuitIntent::None => Ok(()),
                }
            } else if keymap.matches(KeyAction::SelectLeft, key) {
                view.select_left();
                Ok(())
            } else if keymap.matches(KeyAction::SelectRight, key) {
                view.select_right();
                Ok(())
            } else if keymap.matches(KeyAction::SelectUp, key) {
                view.select_up();
                Ok(())
            } else if keymap.matches(KeyAction::SelectDown, key) {
                view.select_down();
                Ok(())
            } else if keymap.matches(KeyAction::MoveLeft, key) {
                transition(&handle, &dir, &mut view, -1)
            } else if keymap.matches(KeyAction::MoveRight, key) {
                transition(&handle, &dir, &mut view, 1)
            } else if keymap.matches(KeyAction::ReorderUp, key) {
                reorder(&handle, &dir, &mut view, -1)
            } else if keymap.matches(KeyAction::ReorderDown, key) {
                reorder(&handle, &dir, &mut view, 1)
            } else if keymap.matches(KeyAction::ToggleExpand, key) {
                view.toggle_expand();
                Ok(())
            } else if keymap.matches(KeyAction::Add, key) {
                view.begin_add();
                Ok(())
            } else if keymap.matches(KeyAction::DependencyAdd, key) {
                if !view.begin_dependency_add() {
                    view.set_status_message(current().text(Message::KanbanNoSelection));
                }
                Ok(())
            } else if keymap.matches(KeyAction::DependencyRemove, key) {
                if !view.begin_dependency_remove() {
                    view.set_status_message(current().text(Message::KanbanNoSelection));
                }
                Ok(())
            } else if keymap.matches(KeyAction::Parent, key) {
                if !view.begin_parent() {
                    view.set_status_message(current().text(Message::KanbanNoSelection));
                }
                Ok(())
            } else if keymap.matches(KeyAction::Edit, key) {
                edit_selected(&mut terminal, &handle, &dir, &mut view)
            } else if keymap.matches(KeyAction::Reload, key) {
                reload(&handle, &dir, &mut view)
            } else if keymap.matches(KeyAction::Maximize, key) {
                view.toggle_maximize();
                Ok(())
            } else if keymap.matches(KeyAction::Search, key) {
                view.begin_search(SearchMode::Contains);
                apply_incremental_filter(&handle, &dir, &mut view)
            } else if keymap.matches(KeyAction::RegexSearch, key) {
                view.begin_search(SearchMode::Regex);
                Ok(())
            } else if keymap.matches(KeyAction::Help, key) {
                view.open_help();
                Ok(())
            } else if keymap.matches(KeyAction::Details, key) {
                view.open_popup();
                Ok(())
            } else {
                Ok(())
            };
            if let Err(e) = step {
                break Err(e);
            }
        }
    }))
    .unwrap_or_else(|payload| {
        // The panic hook has already restored the terminal. Catch the panic while it is no
        // longer active so the hook guard can safely restore the process-global hook before the
        // original payload continues unwinding through the caller.
        let _ = terminal.restore();
        drop(_panic_hook);
        std::panic::resume_unwind(payload);
    });
    // Don't ignore the failure of returning the device, but surface the loop outcome first so a
    // successful exit mode is not overwritten by a later restore error only when the loop succeeded.
    let mode = outcome?;
    terminal.restore()?;
    Ok(mode)
}
