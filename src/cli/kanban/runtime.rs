//! Drawing and the interactive event loop for the Kanban view.

use super::keymap::KeyMap;
use super::{
    BoardView, InputMode, InputSubmission, InputValidation, MIN_COLUMN_WIDTH, PopupContent,
    display_width, wrap,
};
use anyhow::Result;
use pinto::backlog::ItemId;
use pinto::i18n::{Message, current};
use pinto::kanban_keys::{KeyAction, KeyBindings};
use pinto::service::{
    Board, BoardQuery, EditOutcome, ItemEdit, MoveOutcome, NewItem, SearchFilter, SearchMode,
    add_dependency, add_item_with_outcome, apply_item_edit, board, check_wip, edit_item,
    item_edit_template, move_item_with_outcome, remove_dependency, reorder_item,
};
use pinto::timezone::DisplayTimezone;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use std::io::IsTerminal;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::runtime::Handle;

/// Polling interval while waiting for input.
const POLL: Duration = Duration::from_millis(250);

/// Background color for a selected item; apply it to the entire row, including the ID/key cell.
const SELECTION_BG: Color = Color::Cyan;

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

/// The maximum scroll position that the body of the details view popup can take.
///
/// If the terminal size cannot be obtained, use a conservative default for the calculation.
/// The result is the number of wrapped content lines minus the number of visible popup lines.
pub(super) fn popup_max_scroll(
    view: &BoardView,
    size: Option<ratatui::layout::Size>,
    keymap: &KeyMap,
) -> u16 {
    let Some(content) = view.popup_content() else {
        return 0;
    };
    let (width, height) = size.map_or((DEFAULT_POPUP_AREA.0, DEFAULT_POPUP_AREA.1), |s| {
        (s.width, s.height)
    });
    // `render` overlays the popup in the center area excluding the header and variable height footer.
    // Use the same base height as `render` so scrolling matches the drawn popup.
    let footer_height = footer_lines(view, width.saturating_sub(2), keymap).len() as u16;
    let board_area_height = height.saturating_sub(1 + footer_height);
    let area = popup_rect(Rect::new(0, 0, width, board_area_height));
    let inner_width = area.width.saturating_sub(2) as usize; // left and right frames.
    let inner_height = area.height.saturating_sub(2); // upper and lower frames.
    let total_lines = popup_lines_with_timezone(
        &content,
        inner_width,
        view.render_markdown(),
        view.display_timezone(),
    )
    .len() as u16;
    total_lines.saturating_sub(inner_height)
}

/// The maximum scroll position of the Kanban help window.
pub(super) fn help_max_scroll(
    view: &BoardView,
    size: Option<ratatui::layout::Size>,
    keymap: &KeyMap,
) -> u16 {
    let (width, height) = size.map_or((DEFAULT_POPUP_AREA.0, DEFAULT_POPUP_AREA.1), |s| {
        (s.width, s.height)
    });
    // Keep this calculation in sync with `render`: the board area excludes the one-line header
    // and the variable-height prompt/status/footer area.
    let footer_height = footer_lines(view, width.saturating_sub(2), keymap).len() as u16;
    let board_area_height = height.saturating_sub(1 + footer_height);
    let show_clear_filter = view.search_filter().is_some();
    let area = help_popup_rect(
        Rect::new(0, 0, width, board_area_height),
        show_clear_filter,
        keymap,
    );
    let inner_height = area.height.saturating_sub(2);
    help_lines(keymap, show_clear_filter)
        .len()
        .try_into()
        .unwrap_or(u16::MAX)
        .saturating_sub(inner_height)
}

/// Default (width, height) used to calculate popup dimensions when device size is unknown.
const DEFAULT_POPUP_AREA: (u16, u16) = (80, 24);

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

/// What a key press means for leaving the view when no popup is open.
///
/// Kept as a pure decision so the key-to-outcome mapping can be unit tested without a terminal.
/// Both leave keys carry the [`ExitMode`] they resolve to, so the confirmation flow can honour the
/// original intent (`q` → [`ExitMode::Quit`], `Q` → [`ExitMode::Shell`]) once confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuitIntent {
    /// Not a quit-related key.
    None,
    /// `confirm_quit` is enabled — show the confirmation popup first, then leave with this mode.
    Confirm(ExitMode),
    /// `confirm_quit` is disabled — leave immediately with this mode.
    Leave(ExitMode),
}

/// Decide what pressing `key` means for leaving the view (no popup open).
fn quit_intent(keymap: &KeyMap, key: event::KeyEvent, confirm_quit: bool) -> QuitIntent {
    let mode = if keymap.matches(KeyAction::Shell, key) {
        ExitMode::Shell
    } else if keymap.matches(KeyAction::Quit, key) {
        ExitMode::Quit
    } else {
        return QuitIntent::None;
    };
    if confirm_quit {
        QuitIntent::Confirm(mode)
    } else {
        QuitIntent::Leave(mode)
    }
}

/// What a key press means while the details popup is open.
///
/// Kept as a pure decision (like [`quit_intent`]) so the popup key mapping can be unit tested
/// without a terminal. Plain arrow / vim keys keep scrolling the body, while `H`/`J`/`K`/`L`
/// move the board selection so the popup follows it.
/// `e` opens the shown item in `$EDITOR` without leaving the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopupAction {
    /// Not a popup key.
    None,
    /// Close the popup (`Esc` / `q`).
    Close,
    /// Scroll the body up one line (`Up` / `k`).
    ScrollUp,
    /// Scroll the body down one line (`Down` / `j`).
    ScrollDown,
    /// Select the row above (`K`).
    SelectUp,
    /// Select the row below (`J`).
    SelectDown,
    /// Select the column to the left (`H`).
    SelectLeft,
    /// Select the column to the right (`L`).
    SelectRight,
    /// Edit the shown item with `$EDITOR`, keeping the popup open (`e`).
    Edit,
}

/// Decide what pressing `key` means while the details popup is open.
fn popup_action(keymap: &KeyMap, key: event::KeyEvent) -> PopupAction {
    if keymap.matches(KeyAction::PopupClose, key) || keymap.matches(KeyAction::Details, key) {
        PopupAction::Close
    } else if keymap.matches(KeyAction::PopupSelectUp, key) {
        PopupAction::SelectUp
    } else if keymap.matches(KeyAction::PopupSelectDown, key) {
        PopupAction::SelectDown
    } else if keymap.matches(KeyAction::PopupSelectLeft, key) {
        PopupAction::SelectLeft
    } else if keymap.matches(KeyAction::PopupSelectRight, key) {
        PopupAction::SelectRight
    } else if keymap.matches(KeyAction::PopupScrollUp, key) {
        PopupAction::ScrollUp
    } else if keymap.matches(KeyAction::PopupScrollDown, key) {
        PopupAction::ScrollDown
    } else if keymap.matches(KeyAction::Edit, key) {
        PopupAction::Edit
    } else {
        PopupAction::None
    }
}

/// What a key press means while the help window is visible.
///
/// Scrolling is applied to the help overlay, but the event loop deliberately does not stop after
/// that operation: keys such as `j` and `k` are also normal cursor commands and must reach the
/// underlying view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpKeyAction {
    /// Close the help overlay and consume the toggle key.
    Close,
    /// Scroll the help overlay up, then continue handling the key underneath.
    ScrollUp,
    /// Scroll the help overlay down, then continue handling the key underneath.
    ScrollDown,
    /// No help-specific handling; let the underlying mode process the key.
    PassThrough,
}

/// Decide whether the help overlay has work to do for `key` without making it modal.
fn help_key_action(keymap: &KeyMap, key: event::KeyEvent) -> HelpKeyAction {
    if keymap.matches(KeyAction::Help, key) {
        HelpKeyAction::Close
    } else if keymap.matches(KeyAction::PopupScrollUp, key) {
        HelpKeyAction::ScrollUp
    } else if keymap.matches(KeyAction::PopupScrollDown, key) {
        HelpKeyAction::ScrollDown
    } else {
        HelpKeyAction::PassThrough
    }
}

/// Return whether `key` is accepted by the mode underneath the help overlay.
fn should_close_help_after_key(view: &BoardView, keymap: &KeyMap, key: event::KeyEvent) -> bool {
    if view.is_popup_open() {
        return popup_action(keymap, key) != PopupAction::None;
    }
    if view.is_input_active() {
        let selecting_target = view.is_relation_input()
            && view.input_buffer().is_empty()
            && [
                KeyAction::SelectLeft,
                KeyAction::SelectRight,
                KeyAction::SelectUp,
                KeyAction::SelectDown,
            ]
            .into_iter()
            .any(|action| keymap.matches(action, key));
        return selecting_target || text_entry_key_is_accepted(key);
    }
    if view.is_searching() {
        return text_entry_key_is_accepted(key);
    }

    if view.search_filter().is_some() && keymap.matches(KeyAction::ClearFilter, key) {
        return true;
    }
    [
        KeyAction::Shell,
        KeyAction::Quit,
        KeyAction::SelectLeft,
        KeyAction::SelectRight,
        KeyAction::SelectUp,
        KeyAction::SelectDown,
        KeyAction::MoveLeft,
        KeyAction::MoveRight,
        KeyAction::ReorderUp,
        KeyAction::ReorderDown,
        KeyAction::ToggleExpand,
        KeyAction::Add,
        KeyAction::DependencyAdd,
        KeyAction::DependencyRemove,
        KeyAction::Parent,
        KeyAction::Edit,
        KeyAction::Reload,
        KeyAction::Maximize,
        KeyAction::Search,
        KeyAction::RegexSearch,
        KeyAction::Details,
    ]
    .into_iter()
    .any(|action| keymap.matches(action, key))
}

/// Whether a key is accepted by an active text-entry prompt.
fn text_entry_key_is_accepted(key: event::KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace => true,
        KeyCode::Char(_) if !key.modifiers.contains(KeyModifiers::CONTROL) => true,
        _ => false,
    }
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

/// Restore the process-global panic hook when a terminal lifecycle ends.
///
/// `ratatui::try_init` installs its own hook. The Kanban loop adds a hook on top of it so the
/// terminal is restored before the original hook runs. Keeping the hook that was active before
/// initialization in this guard prevents each repeated Kanban invocation from leaving another
/// wrapper in the process.
type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static>;

struct PanicHookGuard {
    previous: Option<PanicHook>,
}

impl PanicHookGuard {
    /// Install the Kanban hook around the hook currently installed by ratatui.
    fn install(previous: PanicHook) -> Self {
        let terminal_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = ratatui::try_restore();
            terminal_hook(info);
        }));
        Self {
            previous: Some(previous),
        }
    }

    fn restore(&mut self) {
        if let Some(previous) = self.previous.take() {
            let _ = std::panic::take_hook();
            std::panic::set_hook(previous);
        }
    }
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Own the initialized terminal and restore it on every return path.
struct TerminalGuard {
    terminal: ratatui::DefaultTerminal,
    restored: bool,
}

impl TerminalGuard {
    fn new(terminal: ratatui::DefaultTerminal) -> Self {
        Self {
            terminal,
            restored: false,
        }
    }

    fn restore(&mut self) -> std::io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        ratatui::try_restore()
    }
}

impl Deref for TerminalGuard {
    type Target = ratatui::DefaultTerminal;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for TerminalGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore();
        }
    }
}

/// Initialize the terminal and bind the lifecycle guards to the same scope.
fn initialize_terminal() -> Result<(TerminalGuard, PanicHookGuard)> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        let error = "stdin and stdout must be connected to a TTY";
        return Err(anyhow::anyhow!(
            current().format(Message::KanbanTerminalInitFailed, [("error", error)],)
        ));
    }

    let previous_hook = std::panic::take_hook();
    let terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            // `try_init` installs ratatui's hook before enabling raw mode. Restore both the
            // terminal and the process-global hook when any initialization step fails.
            let _ = ratatui::try_restore();
            let _ = std::panic::take_hook();
            std::panic::set_hook(previous_hook);
            let error = error.to_string();
            return Err(anyhow::anyhow!(current().format(
                Message::KanbanTerminalInitFailed,
                [("error", error.as_str())]
            )));
        }
    };
    let hook = PanicHookGuard::install(previous_hook);
    Ok((TerminalGuard::new(terminal), hook))
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

/// Submit the active add/relation form through the same services used by the CLI.
fn submit_input(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    let submission = match view.submit_input() {
        Ok(submission) => submission,
        Err(InputValidation::EmptyTitle) => {
            view.set_input_error(current().text(Message::KanbanEmptyTitle));
            return Ok(());
        }
        Err(InputValidation::EmptyDependency) => {
            view.set_input_error(current().text(Message::KanbanEmptyDependency));
            return Ok(());
        }
        Err(InputValidation::InvalidItemId(error)) => {
            view.set_input_error(error);
            return Ok(());
        }
    };

    match submission {
        InputSubmission::AddTitle { .. } | InputSubmission::AddStep => Ok(()),
        InputSubmission::Add {
            title,
            body,
            parent,
            depends_on,
        } => {
            let new = NewItem {
                body,
                parent,
                depends_on,
                ..NewItem::default()
            };
            match handle.block_on(add_item_with_outcome(dir, &title, new)) {
                Ok(outcome) => {
                    let item = outcome.item;
                    rebuild(handle, dir, view, &item.id)?;
                    view.end_input();
                    let mut message = current().format(
                        Message::Created,
                        [
                            ("id", item.id.to_string().as_str()),
                            ("title", item.title.as_str()),
                        ],
                    );
                    if outcome.cycle_warning {
                        message.push_str("; ");
                        message.push_str(&current().text(Message::KanbanDependencyCycleWarning));
                    }
                    view.set_status_message(message);
                    Ok(())
                }
                Err(error) if error.is_user_error() => {
                    view.set_input_error(error.to_string());
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }
        InputSubmission::Dependency {
            source,
            dependency,
            remove,
        } => {
            let dependency = match dependency.parse::<ItemId>() {
                Ok(dependency) => dependency,
                Err(error) => {
                    view.set_input_error(error.to_string());
                    return Ok(());
                }
            };
            if remove {
                match handle.block_on(remove_dependency(dir, &source, &dependency)) {
                    Ok(_) => {
                        rebuild(handle, dir, view, &source)?;
                        view.end_input();
                        view.set_status_message(current().format(
                            Message::KanbanDependencyRemoved,
                            [
                                ("source", source.to_string().as_str()),
                                ("dependency", dependency.to_string().as_str()),
                            ],
                        ));
                        Ok(())
                    }
                    Err(error) if error.is_user_error() => {
                        view.set_input_error(error.to_string());
                        Ok(())
                    }
                    Err(error) => Err(error.into()),
                }
            } else {
                match handle.block_on(add_dependency(dir, &source, &dependency)) {
                    Ok(outcome) => {
                        rebuild(handle, dir, view, &source)?;
                        view.end_input();
                        let mut message = current().format(
                            Message::KanbanDependencyAdded,
                            [
                                ("source", source.to_string().as_str()),
                                ("dependency", dependency.to_string().as_str()),
                            ],
                        );
                        if outcome.cycle_warning {
                            message.push_str("; ");
                            message
                                .push_str(&current().text(Message::KanbanDependencyCycleWarning));
                        }
                        view.set_status_message(message);
                        Ok(())
                    }
                    Err(error) if error.is_user_error() => {
                        view.set_input_error(error.to_string());
                        Ok(())
                    }
                    Err(error) => Err(error.into()),
                }
            }
        }
        InputSubmission::Parent { source, parent } => {
            let parent = match parent.as_deref().map(str::parse::<ItemId>).transpose() {
                Ok(parent) => parent,
                Err(error) => {
                    view.set_input_error(error.to_string());
                    return Ok(());
                }
            };
            let parent_for_message = parent.clone();
            match handle.block_on(edit_item(
                dir,
                &source,
                ItemEdit {
                    parent: Some(parent),
                    ..ItemEdit::default()
                },
            )) {
                Ok(_) => {
                    rebuild(handle, dir, view, &source)?;
                    view.end_input();
                    let message = match parent_for_message {
                        Some(parent) => current().format(
                            Message::KanbanParentSet,
                            [
                                ("source", source.to_string().as_str()),
                                ("parent", parent.to_string().as_str()),
                            ],
                        ),
                        None => current().format(
                            Message::KanbanParentCleared,
                            [("source", source.to_string().as_str())],
                        ),
                    };
                    view.set_status_message(message);
                    Ok(())
                }
                Err(error) if error.is_user_error() => {
                    view.set_input_error(error.to_string());
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }
    }
}

/// Transition the selected PBI to the next column and reload it to follow the selection.
///
/// After the transition, check the destination column's WIP limit as the CLI `move` command does.
/// If it is exceeded, keep the warning in the footer so the user can continue working.
fn transition(handle: &Handle, dir: &Path, view: &mut BoardView, delta: isize) -> Result<()> {
    let Some((id, status)) = view.move_target(delta) else {
        return Ok(());
    };
    let outcome = handle.block_on(move_item_with_outcome(dir, &id, &status))?;
    rebuild(handle, dir, view, &id)?;
    let mut warnings = Vec::new();
    if let Some(warning) = acceptance_criteria_warning(&outcome) {
        warnings.push(warning);
    }
    if let Some(v) = handle
        .block_on(check_wip(dir))?
        .into_iter()
        .find(|v| v.column == status)
    {
        warnings.push(format!(
            "{} {} has {} item(s) (limit {})",
            current().text(Message::KanbanWipExceeded),
            v.column,
            v.count,
            v.limit
        ));
    }
    if !warnings.is_empty() {
        view.set_status_message(warnings.join(" | "));
    }
    Ok(())
}

fn acceptance_criteria_warning(outcome: &MoveOutcome) -> Option<String> {
    if !outcome.entered_done_column || !outcome.acceptance_criteria.is_incomplete() {
        return None;
    }

    let progress = outcome.acceptance_criteria.to_string();
    Some(current().format(
        Message::AcceptanceCriteriaIncomplete,
        [
            ("id", outcome.item.id.to_string().as_str()),
            ("progress", progress.as_str()),
        ],
    ))
}

/// Sort selected PBIs within the same column and reload to follow selection.
fn reorder(handle: &Handle, dir: &Path, view: &mut BoardView, delta: isize) -> Result<()> {
    let Some((id, target)) = view.reorder_target(delta) else {
        return Ok(());
    };
    handle.block_on(reorder_item(dir, &id, target))?;
    rebuild(handle, dir, view, &id)
}

/// Open the selected PBI with `$EDITOR`, edit it, and reload it after reflecting.
///
/// While the editor runs, suspend raw mode and the alternate screen, then restore the TUI
/// afterward. Missing editor configuration, launch failures, and invalid content are shown in the
/// footer; the loop remains active unless an internal error must be propagated.
fn edit_selected(
    terminal: &mut ratatui::DefaultTerminal,
    handle: &Handle,
    dir: &Path,
    view: &mut BoardView,
) -> Result<()> {
    use ratatui::crossterm::execute;
    use ratatui::crossterm::terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    };

    let Some(id) = view.selected_item().map(|it| it.id.clone()) else {
        return Ok(());
    };
    // If no editor is configured, keep the TUI open and skip editing.
    if crate::cli::editor::resolve_editor().is_none() {
        view.set_status_message(current().text(Message::KanbanNoEditor).to_string());
        return Ok(());
    }

    let template = handle.block_on(item_edit_template(dir, &id))?;

    // Suspend the TUI and give the terminal to the editor. Restore it regardless of the editor result.
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    let edited = crate::cli::editor::edit_in_editor(&template, &id.to_string());
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;

    let edited = match edited {
        Ok(text) => text,
        // Report launch failures in the footer and keep the loop running.
        Err(e) => {
            view.set_status_message(format!(
                "{} {e}",
                current().text(Message::KanbanEditorFailed)
            ));
            return Ok(());
        }
    };

    match handle.block_on(apply_item_edit(dir, &id, &edited)) {
        Ok(EditOutcome::Updated(_)) => rebuild(handle, dir, view, &id),
        Ok(EditOutcome::Unchanged) => {
            view.set_status_message(format!("{} {id}", current().text(Message::KanbanNoChanges)));
            Ok(())
        }
        // Keep user-correctable errors in the footer and preserve the original item.
        Err(e) if e.is_user_error() => {
            view.set_status_message(format!("{} {e}", current().text(Message::KanbanEditFailed)));
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Reread the board and keep the selected PBI as much as possible.
fn reload(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    let selected = view.selected_item().map(|it| it.id.clone());
    let query = view.board_query().clone();
    let display_columns = view.display_statuses().to_vec();
    let loaded = handle.block_on(load_display_board(dir, &query, &display_columns))?;
    view.set_boards(loaded.display, loaded.full);
    if let Some(id) = selected {
        view.select_id(&id);
    }
    Ok(())
}

/// Reread the board and reselect the `keep` PBI (common process after transition/sorting).
/// Retain the expanded state ([`BoardView::set_boards`]).
fn rebuild(handle: &Handle, dir: &Path, view: &mut BoardView, keep: &ItemId) -> Result<()> {
    let query = view.board_query().clone();
    let display_columns = view.display_statuses().to_vec();
    let loaded = handle.block_on(load_display_board(dir, &query, &display_columns))?;
    view.set_boards(loaded.display, loaded.full);
    view.select_id(keep);
    Ok(())
}

/// Reload the board through `filter`, apply it as the active filter, and keep the selection when the
/// selected PBI survives the reload.
fn reload_with_filter(
    handle: &Handle,
    dir: &Path,
    view: &mut BoardView,
    filter: Option<SearchFilter>,
) -> Result<()> {
    let selected = view.selected_item().map(|item| item.id.clone());
    let display_columns = view.display_statuses().to_vec();
    let mut query = view.board_query().clone();
    query.search = filter.clone();
    let loaded = handle.block_on(load_display_board(dir, &query, &display_columns))?;
    view.set_search(filter);
    view.set_boards(loaded.display, loaded.full);
    if let Some(selected) = selected {
        view.select_id(&selected);
    }
    Ok(())
}

/// Live-filter the board while a substring query is typed (incremental search).
///
/// Only substring (`Contains`) mode filters as you type; a partial regex is frequently invalid, so
/// regex mode defers to Enter. An empty query shows the whole board.
fn apply_incremental_filter(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    if view.search_input_mode() != Some(SearchMode::Contains) {
        return Ok(());
    }
    let query = view.search_input_buffer();
    let filter = if query.is_empty() {
        None
    } else {
        // Substring construction never fails; skip silently on the impossible error rather than panic.
        SearchFilter::new(query, false).ok()
    };
    reload_with_filter(handle, dir, view, filter)
}

/// Apply the query typed into the vim-style prompt, reloading the board through the new filter.
///
/// An empty query clears the filter. An invalid regex keeps the prompt open with an inline error so
/// the user can correct it in place. On success the prompt closes and the previously selected PBI is
/// re-selected when it survives the filter.
fn commit_search(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    let Some(mode) = view.search_input_mode() else {
        return Ok(());
    };
    let query = view.search_input_buffer();
    let filter = if query.is_empty() {
        None
    } else {
        match SearchFilter::new(query, matches!(mode, SearchMode::Regex)) {
            Ok(filter) => Some(filter),
            Err(error) => {
                // Keep editing: surface the error under the prompt rather than dropping the query.
                view.set_search_input_error(error.to_string());
                return Ok(());
            }
        }
    };
    reload_with_filter(handle, dir, view, filter)?;
    view.end_search();
    Ok(())
}

/// Cancel the prompt, rolling the board back to the filter that was active when it opened.
fn abort_search(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    let restore = view.take_search_restore();
    reload_with_filter(handle, dir, view, restore)
}

/// Clear the active search filter in one keystroke and show the whole board again.
fn clear_filter(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    reload_with_filter(handle, dir, view, None)
}

/// Draw a frame containing the header, columns, footer, and any overlay popup.
pub(super) fn render(frame: &mut Frame, view: &BoardView, confirming: bool, keymap: &KeyMap) {
    let footer_width = frame.area().width.saturating_sub(2);
    let footer_lines = footer_lines(view, footer_width, keymap);
    let footer_height = footer_lines
        .len()
        .min(frame.area().height.saturating_sub(1) as usize) as u16;
    let rows = Layout::vertical([
        Constraint::Length(1),             // header
        Constraint::Min(0),                // board
        Constraint::Length(footer_height), // Footer (key guide)
    ])
    .split(frame.area());
    let footer_area = footer_content_area(rows[2]);

    frame.render_widget(header(view, rows[0].width), rows[0]);
    render_columns(frame, view, rows[1]);
    // Footer: input/search prompts are shown directly (like Vim), any temporary status (for example, a WIP
    // warning) is highlighted, and otherwise the key guide is dimmed.
    let footer = Paragraph::new(Text::from(footer_lines)).style(
        if view.is_input_active() || view.is_searching() {
            Style::new()
        } else if view.status_message().is_some() {
            Style::new().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        },
    );
    frame.render_widget(footer, footer_area);
    // Vim-style: park the terminal cursor at the end of the query on the bottom prompt line.
    if view.is_input_active() || view.is_searching() {
        let prompt_row = footer_area.bottom().saturating_sub(1);
        let prompt_width = view
            .input_mode()
            .map(input_prompt)
            .map_or(1, |prompt| display_width(&prompt) as u16 + 1);
        let cursor_x = footer_area
            .x
            .saturating_add(if view.is_input_active() {
                prompt_width
            } else {
                1
            })
            .saturating_add(display_width(if view.is_input_active() {
                view.input_buffer()
            } else {
                view.search_input_buffer()
            }) as u16)
            .min(footer_area.right().saturating_sub(1));
        frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, prompt_row));
    }

    // While the details popup is open it always overlays the board — even on an empty column,
    // where a placeholder is shown so navigating there does not look like a return to normal mode.
    if view.is_popup_open() {
        let popup = popup_rect(rows[1]);
        match view.popup_content() {
            Some(content) => render_item_popup(
                frame,
                &content,
                view.popup_scroll(),
                view.render_markdown(),
                view.display_timezone(),
                rows[1],
            ),
            None => render_empty_popup(frame, rows[1]),
        }
        // Repair any full-width board glyph whose right half spills onto the popup's left border.
        sanitize_left_border(frame.buffer_mut(), popup);
    } else if confirming {
        render_quit_popup(frame, rows[1]);
    }

    // Help is a second-level overlay: it can be opened from either board or details mode while
    // the five primary operations remain visible in the fixed footer.
    if view.is_help_open() {
        render_help_popup(
            frame,
            view.help_scroll(),
            view.search_filter().is_some(),
            keymap,
            rows[1],
        );
    }
}

/// Inset the footer by one cell on both sides so its content aligns with the inside of column frames.
fn footer_content_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height,
    )
}

/// Overlaying the details popup with [`Clear`] cannot fix a full-width (CJK) glyph sitting in the
/// board column immediately left of the popup's left border: the glyph's right half spills onto the
/// border cell and breaks the frame (ratatui reconciles wide glyphs only within a single widget, not
/// across an overlay boundary). Blank any such glyph so the border draws cleanly. Full-width glyphs
/// only ever spill rightward, so only the left border needs this treatment.
fn sanitize_left_border(buf: &mut Buffer, popup: Rect) {
    if popup.x == 0 || popup.width == 0 {
        return;
    }
    let x = popup.x - 1;
    for y in popup.y..popup.y.saturating_add(popup.height) {
        if let Some(cell) = buf.cell_mut((x, y))
            && display_width(cell.symbol()) > 1
        {
            cell.set_symbol(" ");
        }
    }
}

/// Footer operation guide lines.
///
/// A temporary status (WIP warning, in-popup edit result, etc.) always wins the footer so the user
/// never misses it. Input prompts likewise own the footer while active. Otherwise the details
/// popup uses its own close/scroll/select/edit guide, while board mode shows the five primary
/// operations and keeps secondary operations in help.
pub(super) fn footer_lines(view: &BoardView, width: u16, keymap: &KeyMap) -> Vec<Line<'static>> {
    // The add/relation form and vim-style search prompt own the bottom line while open.
    if let Some(mode) = view.input_mode() {
        return input_prompt_lines(mode, view.input_buffer(), view.input_error());
    }
    if let Some(mode) = view.search_input_mode() {
        return search_prompt_lines(mode, view.search_input_buffer(), view.search_input_error());
    }
    if let Some(message) = view.status_message() {
        return vec![Line::from(format!(" {message} "))];
    }
    if view.is_popup_open() {
        return wrap_hint_groups(&popup_hints(keymap), width);
    }
    footer_hint_lines(keymap, width)
}

/// Build the fixed footer guide from the first configured key of the five primary operations.
fn key_hints(keymap: &KeyMap) -> String {
    let columns = key_pair(keymap, KeyAction::SelectLeft, KeyAction::SelectRight);
    let select = key_pair(keymap, KeyAction::SelectDown, KeyAction::SelectUp);
    let cursor = format!("{columns},{select}");
    current().format(
        Message::KanbanKeyHints,
        [
            ("cursor", cursor.as_str()),
            ("expand", keymap.first(KeyAction::ToggleExpand)),
            ("details", keymap.first(KeyAction::Details)),
            ("quit", keymap.first(KeyAction::Quit)),
        ],
    )
}

/// Build the footer lines with the help hint anchored to the right edge.
fn footer_hint_lines(keymap: &KeyMap, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let help = current().format(
        Message::KanbanHelpHint,
        [("help", keymap.first(KeyAction::Help))],
    );
    let help_width = display_width(&help);
    let mut lines = wrap_hint_groups(&key_hints(keymap), width as u16);
    let Some(last) = lines.last_mut() else {
        return vec![right_aligned_hint(&help, width)];
    };
    let last_text = last.to_string();
    let last_width = display_width(&last_text);
    if last_text.is_empty() {
        *last = right_aligned_hint(&help, width);
    } else if last_width.saturating_add(2).saturating_add(help_width) <= width {
        let mut combined = last_text;
        combined.push_str(&" ".repeat(width - last_width - help_width));
        combined.push_str(&help);
        *last = Line::from(combined);
    } else {
        lines.push(right_aligned_hint(&help, width));
    }
    lines
}

/// Pad one footer hint so its right edge reaches the requested display width.
fn right_aligned_hint(hint: &str, width: usize) -> Line<'static> {
    Line::from(format!(
        "{}{}",
        " ".repeat(width.saturating_sub(display_width(hint))),
        hint
    ))
}

/// Join two related key labels while preserving the keymap's configured spelling.
fn key_pair(keymap: &KeyMap, first: KeyAction, second: KeyAction) -> String {
    let first = keymap.first(first);
    let separator = if first.ends_with('/') { "" } else { "/" };
    format!("{first}{separator}{}", keymap.first(second))
}

/// Build the details-popup key guide from the first configured key of every popup operation.
fn popup_hints(keymap: &KeyMap) -> String {
    current().format(
        Message::KanbanPopupHints,
        [
            ("close", keymap.first(KeyAction::PopupClose)),
            ("scroll_up", keymap.first(KeyAction::PopupScrollUp)),
            ("scroll_down", keymap.first(KeyAction::PopupScrollDown)),
            ("select_up", keymap.first(KeyAction::PopupSelectUp)),
            ("select_down", keymap.first(KeyAction::PopupSelectDown)),
            ("select_left", keymap.first(KeyAction::PopupSelectLeft)),
            ("select_right", keymap.first(KeyAction::PopupSelectRight)),
            ("edit", keymap.first(KeyAction::Edit)),
        ],
    )
}

/// Build a single help entry key.
fn help_key(keymap: &KeyMap, action: KeyAction) -> String {
    keymap.first(action).to_string()
}

/// Build the help window entries from every accepted operation outside the fixed footer guide.
fn help_lines(keymap: &KeyMap, show_clear_filter: bool) -> Vec<Line<'static>> {
    let shell = help_key(keymap, KeyAction::Shell);
    let move_keys = key_pair(keymap, KeyAction::MoveLeft, KeyAction::MoveRight);
    let reorder = key_pair(keymap, KeyAction::ReorderUp, KeyAction::ReorderDown);
    let add = help_key(keymap, KeyAction::Add);
    let parent = help_key(keymap, KeyAction::Parent);
    let dependency_add = help_key(keymap, KeyAction::DependencyAdd);
    let dependency_remove = help_key(keymap, KeyAction::DependencyRemove);
    let edit = help_key(keymap, KeyAction::Edit);
    let reload = help_key(keymap, KeyAction::Reload);
    let maximize = help_key(keymap, KeyAction::Maximize);
    let search = help_key(keymap, KeyAction::Search);
    let regex_search = help_key(keymap, KeyAction::RegexSearch);
    let clear_filter = show_clear_filter.then(|| help_key(keymap, KeyAction::ClearFilter));
    let keys = [
        &shell,
        &move_keys,
        &reorder,
        &add,
        &parent,
        &dependency_add,
        &dependency_remove,
        &edit,
        &reload,
        &maximize,
        &search,
        &regex_search,
    ];
    let key_width = keys
        .iter()
        .map(|key| display_width(key))
        .chain(clear_filter.iter().map(|key| display_width(key)))
        .max()
        .unwrap_or(1);
    let pad = |key: &str| -> String {
        format!(
            "{key}{}",
            " ".repeat(key_width.saturating_sub(display_width(key)))
        )
    };
    let shell = pad(&shell);
    let move_keys = pad(&move_keys);
    let reorder = pad(&reorder);
    let add = pad(&add);
    let parent = pad(&parent);
    let dependency_add = pad(&dependency_add);
    let dependency_remove = pad(&dependency_remove);
    let edit = pad(&edit);
    let reload = pad(&reload);
    let maximize = pad(&maximize);
    let search = pad(&search);
    let regex_search = pad(&regex_search);
    let clear_filter = clear_filter.map(|key| pad(&key));
    let entries = current().format(
        Message::KanbanHelpEntries,
        [
            ("shell", shell.as_str()),
            ("move", move_keys.as_str()),
            ("reorder", reorder.as_str()),
            ("add", add.as_str()),
            ("parent", parent.as_str()),
            ("dependency_add", dependency_add.as_str()),
            ("dependency_remove", dependency_remove.as_str()),
            ("edit", edit.as_str()),
            ("reload", reload.as_str()),
            ("maximize", maximize.as_str()),
            ("search", search.as_str()),
            ("regex_search", regex_search.as_str()),
        ],
    );
    let mut lines: Vec<Line<'static>> = entries
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect();
    if let Some(clear_filter) = clear_filter {
        let clear_filter = current().format(
            Message::KanbanHelpClearFilter,
            [("clear_filter", clear_filter.as_str())],
        );
        lines.push(Line::from(clear_filter));
    }
    lines
}

/// Build the vim-style search prompt: a `/` (substring) or `?` (regex) prefix plus the typed query.
///
/// The prompt itself is always the last line so the terminal cursor can sit at the bottom edge like
/// vim; a validation error, when present, is shown on the line just above it.
fn search_prompt_lines(mode: SearchMode, buffer: &str, error: Option<&str>) -> Vec<Line<'static>> {
    let prefix = match mode {
        SearchMode::Contains => '/',
        SearchMode::Regex => '?',
    };
    let mut lines = Vec::new();
    if let Some(error) = error {
        lines.push(Line::from(format!(" {error} ")));
    }
    lines.push(Line::from(format!("{prefix}{buffer}")));
    lines
}

/// Build the add/relation prompt and optional inline validation line.
fn input_prompt_lines(mode: InputMode, buffer: &str, error: Option<&str>) -> Vec<Line<'static>> {
    let prompt = input_prompt(mode);
    let mut lines = Vec::new();
    if let Some(error) = error {
        lines.push(Line::from(format!(" {error} ")));
    }
    lines.push(Line::from(format!("{prompt} {buffer}")));
    lines
}

/// Localized label for an add/relation prompt, without the input separator.
fn input_prompt(mode: InputMode) -> String {
    match mode {
        InputMode::AddTitle => current().text(Message::KanbanAddTitlePrompt),
        InputMode::AddBody => current().text(Message::KanbanAddBodyPrompt),
        InputMode::AddParent => current().text(Message::KanbanAddParentPrompt),
        InputMode::AddDependencies => current().text(Message::KanbanAddDependenciesPrompt),
        InputMode::DependencyAdd => current().text(Message::KanbanDependencyAddPrompt),
        InputMode::DependencyRemove => current().text(Message::KanbanDependencyRemovePrompt),
        InputMode::Parent => current().text(Message::KanbanParentPrompt),
    }
}

/// Wrap a `  `-separated key-hint string into footer lines in units of operations.
///
/// Do not separate key-action pairs like `h/l: columns` and only when one pair exceeds the screen width.
/// Defer to normal word boundary wrapping.
fn wrap_hint_groups(hints: &str, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let mut lines = Vec::<String>::new();
    let mut line = String::new();
    for group in hints.split("  ").filter(|group| !group.is_empty()) {
        let group_width = display_width(group);
        let separator = usize::from(!line.is_empty()) * 2;
        if group_width <= width && display_width(&line) + separator + group_width <= width {
            if !line.is_empty() {
                line.push_str("  ");
            }
            line.push_str(group);
        } else {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if group_width <= width {
                line.push_str(group);
            } else {
                let mut wrapped = wrap(group, width);
                line = wrapped.pop().unwrap_or_default();
                lines.extend(wrapped);
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(Line::from).collect()
}

/// header row. Title, visibility range/direction indicator during horizontal scrolling, and legend for dependent markers.
pub(super) fn header(view: &BoardView, width: u16) -> Line<'static> {
    let total = view.columns().len();
    let capacity = effective_capacity(width, view.is_maximized());
    let start = view.col_offset();
    let end = (start + capacity).min(total);
    let mut label = String::from(" pinto — kanban ");
    if total > capacity {
        let left = if start > 0 { "◀" } else { " " };
        let right = if end < total { "▶" } else { " " };
        let start = (start + 1).to_string();
        let end = end.to_string();
        let total = total.to_string();
        label.push_str(&format!(
            " {} ",
            current().format(
                Message::KanbanColumnRange,
                [
                    ("left", left),
                    ("start", start.as_str()),
                    ("end", end.as_str()),
                    ("total", total.as_str()),
                    ("right", right),
                ],
            )
        ));
    }
    let mut spans = vec![Span::styled(
        label,
        Style::new().add_modifier(Modifier::BOLD),
    )];
    // Surface an active search filter so items hidden by it read as filtered, not missing. While the
    // prompt is open the bottom line already echoes the query, so the header stays uncluttered.
    if let Some(filter) = view.search_filter().filter(|_| !view.is_searching()) {
        let message = match filter.mode() {
            SearchMode::Contains => Message::KanbanActiveFilter,
            SearchMode::Regex => Message::KanbanActiveRegexFilter,
        };
        let indicator = current().format(message, [("pattern", filter.pattern())]);
        spans.push(Span::styled(
            format!(" {indicator} "),
            Style::new().fg(Color::Black).bg(Color::Yellow),
        ));
    }
    // Dependency marker legend (string shared with board). Display more modestly than the main text.
    spans.push(Span::styled(
        format!(" {} ", super::dependency_legend(current())),
        Style::new().fg(Color::DarkGray),
    ));
    Line::from(spans)
}

/// Draw columns horizontally in the board area (fixed width/horizontal scrolling).
///
/// Highlight selected columns with a frame and selected rows with a background color. Each card
/// starts with its ID in the first row; the title is wrapped to the column width.
fn render_columns(frame: &mut Frame, view: &BoardView, area: Rect) {
    let columns = view.columns();
    if columns.is_empty() {
        frame.render_widget(
            Paragraph::new(current().text(Message::KanbanEmptyColumns)),
            area,
        );
        return;
    }
    let capacity = effective_capacity(area.width, view.is_maximized());
    let start = view.col_offset();
    let end = (start + capacity).min(columns.len());
    let visible = end - start;
    // Reverse lookup of dependent sources and completion determination require the entire board, so they are constructed only once in one frame.
    let deps = view.dependency_index();

    // Divide the drawing area equally into visible columns and fill it (if there is room, it will be wider than the minimum width).
    let constraints: Vec<Constraint> = (0..visible).map(|_| Constraint::Fill(1)).collect();
    let cells = Layout::horizontal(constraints).split(area);

    for (slot, ci) in (start..end).enumerate() {
        let cell = cells[slot];
        let column = &columns[ci];
        let selected_here = ci == view.selected_col();
        let inner_width = cell.width.saturating_sub(2) as usize; // Actual inside width excluding frame.
        let id_width = column
            .items
            .iter()
            .map(|item| display_width(&item.id.to_string()))
            .max()
            .unwrap_or(0);
        let items: Vec<ListItem> = view
            .visible_rows(ci)
            .iter()
            .map(|dr| {
                let it = &column.items[dr.item_index];
                let marker = fold_marker(dr);
                ListItem::new(card_lines(
                    &it.id.to_string(),
                    &it.title,
                    dr.depth,
                    marker,
                    id_width,
                    it.points,
                    it.assignee.as_deref(),
                    &deps.summary(it),
                    inner_width,
                ))
            })
            .collect();
        let title = format!(" {} ({}) ", column.status, column.items.len());
        let border = if selected_here {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new()
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title(title);
        let mut state = ListState::default();
        if selected_here && !column.items.is_empty() {
            state.select(Some(view.selected_row()));
        }
        // For selected lines, specify the background color and apply it uniformly to the entire line. ID span's own fg is
        // It will be overwritten (patch) here, and the key will also have the same background color as the main text (if it is reversed, the background will be
        // (varies).
        let list = List::new(items).block(block).highlight_style(
            Style::new()
                .bg(SELECTION_BG)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(list, cell, &mut state);
    }
}

/// Fixed-width child indicator rendered after each card ID.
const CHILD_INDICATOR_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Marker {
    /// Has no children (the indicator slot remains reserved).
    None,
    /// Collapsed (`▸` + number of children).
    Collapsed(usize),
    /// Expanded (`▾`).
    Expanded,
}

impl Marker {
    /// Indicator display string, right-aligned in four cells (`▸99+` is the maximum).
    fn label(self) -> String {
        let indicator = match self {
            Marker::None => String::new(),
            Marker::Collapsed(n) => {
                let count = if n >= 100 {
                    "99+".to_string()
                } else {
                    n.to_string()
                };
                format!("▸{count}")
            }
            Marker::Expanded => "▾".to_string(),
        };
        let padding = CHILD_INDICATOR_WIDTH.saturating_sub(display_width(&indicator));
        format!("{}{}", " ".repeat(padding), indicator)
    }
}

/// Determine the fold marker from the displayed line.
pub(super) fn fold_marker(dr: &super::DisplayRow) -> Marker {
    if dr.child_count == 0 {
        Marker::None
    } else if dr.expanded {
        Marker::Expanded
    } else {
        Marker::Collapsed(dr.child_count)
    }
}

/// Format a single card into multiple lines. Indent to the depth and place a fixed-width ID and
/// child indicator before the title. The title is wrapped at the remaining width, and continuation
/// lines are aligned to the title column.
/// Story points (◆) and assignee (@) are appended as a muted meta line when set, followed by
/// a dependent (⊸)/dependent source (⊷) line if there is a dependency relationship.
#[allow(clippy::too_many_arguments)]
fn card_lines(
    id: &str,
    title: &str,
    depth: usize,
    marker: Marker,
    id_width: usize,
    points: Option<u32>,
    assignee: Option<&str>,
    deps: &super::DepSummary,
    inner_width: usize,
) -> Text<'static> {
    let indent = "  ".repeat(depth); // 1 row = 2 digits.
    let marker_label = marker.label();
    // Display width of "Indent + fixed-width ID + indicator + title separators". Continuation
    // lines are indented by this width to align the title column.
    let prefix_width = display_width(&indent) + id_width + display_width(&marker_label) + 2;
    let title_width = inner_width.saturating_sub(prefix_width).max(1);
    let segments = wrap(title, title_width);
    let id_style = Style::new()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);
    let marker_style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let cont_indent = " ".repeat(prefix_width);
    let mut lines: Vec<Line> = Vec::with_capacity(segments.len().max(1) + 1);
    for (i, seg) in segments.iter().enumerate() {
        if i == 0 {
            let mut spans = Vec::with_capacity(4);
            if !indent.is_empty() {
                spans.push(Span::raw(indent.clone()));
            }
            let id_padding = id_width.saturating_sub(display_width(id));
            spans.push(Span::styled(
                format!("{id}{}", " ".repeat(id_padding)),
                id_style,
            ));
            spans.push(Span::raw(" "));
            if marker == Marker::None {
                spans.push(Span::raw(marker_label.clone()));
            } else {
                spans.push(Span::styled(marker_label.clone(), marker_style));
            }
            spans.push(Span::raw(" "));
            spans.push(Span::raw(seg.clone()));
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(format!("{cont_indent}{seg}")));
        }
    }
    if let Some(line) = meta_line(points, assignee, &cont_indent) {
        lines.push(line);
    }
    if let Some(line) = dependency_line(deps, &cont_indent) {
        lines.push(line);
    }
    Text::from(lines)
}

/// Story points/assignee summary line (`◆ 5  @alice`). `None` when neither is set.
///
/// Indented to align with the title column like the dependency line, and drawn in a muted color so
/// it reads as metadata rather than the card's main text. Either field is shown independently, so an
/// unestimated but assigned card (or vice versa) still gets a meta line.
fn meta_line(
    points: Option<u32>,
    assignee: Option<&str>,
    cont_indent: &str,
) -> Option<Line<'static>> {
    if points.is_none() && assignee.is_none() {
        return None;
    }
    let style = Style::new().fg(Color::DarkGray);
    let mut spans: Vec<Span<'static>> = vec![Span::raw(cont_indent.to_string())];
    if let Some(points) = points {
        spans.push(Span::styled(format!("◆ {points}"), style));
    }
    if let Some(assignee) = assignee {
        if spans.len() > 1 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(format!("@{assignee}"), style));
    }
    Some(Line::from(spans))
}

/// Dependency line (`⊸ Depends on` `⊷ Depends on`). `None` if there is no dependency.
///
/// Indent and align to title column. If unfinished dependencies remain (blocked), mark them.
/// Draw it in red as `⊸!` so that it can be identified even in environments where colors cannot be used (same symbol as board).
fn dependency_line(deps: &super::DepSummary, cont_indent: &str) -> Option<Line<'static>> {
    if deps.is_empty() {
        return None;
    }
    let mut spans: Vec<Span<'static>> = vec![Span::raw(cont_indent.to_string())];
    if !deps.depends_on.is_empty() {
        // Blocked is `⊸!` + red, resolved (all dependencies are completed) is `⊸` + calm color.
        let (mark, style) = if deps.blocked {
            (
                "⊸!",
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        } else {
            ("⊸", Style::new().fg(Color::DarkGray))
        };
        spans.push(Span::styled(
            format!(
                "{mark} {}",
                super::format_ids(&deps.depends_on, super::DEP_ID_LIMIT)
            ),
            style,
        ));
    }
    if !deps.dependents.is_empty() {
        if spans.len() > 1 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!(
                "⊷ {}",
                super::format_ids(&deps.dependents, super::DEP_ID_LIMIT)
            ),
            Style::new().fg(Color::DarkGray),
        ));
    }
    Some(Line::from(spans))
}

/// Draw the completion confirmation popup overlapping the center. Make it the smallest size that fits the text.
fn render_quit_popup(frame: &mut Frame, area: Rect) {
    let body_text = current().text(Message::KanbanQuitBody);
    // Minimum width of 2 digits for frame + 2 digits for left and right padding. Match the title to the wider of the body.
    let title = format!(" {} ", current().text(Message::KanbanQuitPrompt));
    let content_width = display_width(&body_text).max(display_width(&title)) as u16;
    let popup = centered_fixed(content_width + 4, 3, area); // 1 line of text + top and bottom frames.
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Yellow))
        .title(title);
    let body = Paragraph::new(body_text)
        .block(block)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(body, popup);
}

/// Rectangle for the secondary-operation help window.
fn help_popup_rect(area: Rect, show_clear_filter: bool, keymap: &KeyMap) -> Rect {
    let lines = help_lines(keymap, show_clear_filter);
    let content_width = lines
        .iter()
        .map(|line| display_width(&line.to_string()))
        .max()
        .unwrap_or(1);
    let width = u16::try_from(content_width.saturating_add(4)).unwrap_or(u16::MAX);
    let height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(u16::MAX);
    let width = width.max(1).min(area.width);
    let height = height.max(1).min(area.height);
    Rect {
        x: area.right().saturating_sub(width),
        y: area.bottom().saturating_sub(height),
        width,
        height,
    }
}

/// Draw the secondary-operation help window in the style of the details popup.
fn render_help_popup(
    frame: &mut Frame,
    scroll: u16,
    show_clear_filter: bool,
    keymap: &KeyMap,
    area: Rect,
) {
    let popup = help_popup_rect(area, show_clear_filter, keymap);
    frame.render_widget(Clear, popup);
    let title = format!(" {} ", current().text(Message::KanbanHelpTitle));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan))
        .title(title);
    let body = Paragraph::new(Text::from(help_lines(keymap, show_clear_filter)))
        .block(block)
        .scroll((scroll, 0));
    frame.render_widget(body, popup);
    sanitize_left_border(frame.buffer_mut(), popup);
}

/// Place a rectangle of the specified size (not exceeding the area) in the center of the `area`.
fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Return the details-popup rectangle, targeting 80% of `area`'s width and 90% of its height.
/// Clamp both dimensions so the rectangle never exceeds `area`, even on a very small terminal.
fn popup_rect(area: Rect) -> Rect {
    let width = ((u32::from(area.width) * 4 / 5) as u16).max(1);
    let height = ((u32::from(area.height) * 9 / 10) as u16).max(1);
    centered_fixed(width, height, area)
}

/// Build the details-popup lines from the header, body, and relationship information.
///
/// `width` is the inner display width excluding the frame. The same width is used to calculate
/// scrolling, so this remains a pure function independent of drawing.
fn popup_lines_with_timezone(
    content: &PopupContent,
    width: usize,
    markdown: bool,
    timezone: DisplayTimezone,
) -> Vec<Line<'static>> {
    fn field(label: &str, value: String) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("{label}: "),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::raw(value),
        ])
    }
    fn or_dash(value: Option<&str>) -> String {
        value.map(str::to_string).unwrap_or_else(|| "-".to_string())
    }
    // Match `pinto show`: use `-` for an empty list and `+N` only when many IDs are present.
    fn ids_or_dash(ids: &[ItemId]) -> String {
        if ids.is_empty() {
            "-".to_string()
        } else {
            super::format_ids(ids, 8)
        }
    }

    // Render an RFC3339 timestamp, or `-` when unset (matches `pinto show`).
    fn time_or_dash(
        value: Option<chrono::DateTime<chrono::Utc>>,
        timezone: DisplayTimezone,
    ) -> String {
        value
            .map(|d| timezone.format_datetime(d, "%Y-%m-%dT%H:%M:%S%:z"))
            .unwrap_or_else(|| "-".to_string())
    }

    let mut lines = vec![
        field("ID", content.id.to_string()),
        field("Title", content.title.clone()),
        field("Status", content.status.to_string()),
        field(
            "Acceptance Criteria",
            content.acceptance_criteria.to_string(),
        ),
    ];
    // Rank shows the sibling-local ordinal with the internal fractional index
    // (as in `pinto show`); a child names its parent so the number is clearly
    // the order among that parent's children, not the whole column.
    let rank_value = match &content.parent {
        Some(parent) => format!(
            "#{} under {} ({})",
            content.rank_ordinal, parent, content.rank
        ),
        None => format!("#{} ({})", content.rank_ordinal, content.rank),
    };
    lines.push(field("Rank", rank_value));
    lines.push(field(
        "Points",
        content
            .points
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string()),
    ));
    lines.push(field(
        "Labels",
        if content.labels.is_empty() {
            "-".to_string()
        } else {
            content.labels.join(", ")
        },
    ));
    lines.push(field("Assignee", or_dash(content.assignee.as_deref())));
    lines.push(field("Sprint", or_dash(content.sprint.as_deref())));
    lines.push(field("Parent", or_dash(content.parent.as_deref())));
    lines.push(field("Children", ids_or_dash(&content.children)));
    lines.push(field("Depends on", ids_or_dash(&content.depends_on)));
    lines.push(field("Depended by", ids_or_dash(&content.dependents)));
    lines.push(field("Started", time_or_dash(content.start_at, timezone)));
    lines.push(field("Completed", time_or_dash(content.done_at, timezone)));
    lines.push(field(
        "Commits",
        if content.commits.is_empty() {
            "-".to_string()
        } else {
            content.commits.join(", ")
        },
    ));
    lines.push(field(
        "Created",
        timezone.format_datetime(content.created, "%Y-%m-%dT%H:%M:%S%:z"),
    ));
    lines.push(field(
        "Updated",
        timezone.format_datetime(content.updated, "%Y-%m-%dT%H:%M:%S%:z"),
    ));
    lines.push(Line::default());

    if content.body.is_empty() {
        lines.push(Line::from(Span::styled(
            current().text(Message::KanbanNoBody),
            Style::new().fg(Color::DarkGray),
        )));
    } else if markdown {
        // Render the body as Markdown, sharing `pinto show`'s rendering path.
        lines.extend(super::super::markdown::render_lines(&content.body, width));
    } else {
        // Opt-out: wrap the raw Markdown text line by line (previous behaviour).
        for src_line in content.body.lines() {
            if src_line.is_empty() {
                lines.push(Line::default());
            } else {
                for wrapped in wrap(src_line, width) {
                    lines.push(Line::from(wrapped));
                }
            }
        }
    }
    lines
}

/// Draws the details viewing popup centered. `scroll` is the vertical scroll position of the text.
///
/// Since the background is erased with [`Clear`] before drawing, the characters will not be garbled even if they overlap with cards, etc.
/// If the terminal is small, [`popup_rect`] will be clamped to a width and height that does not exceed the area.
fn render_item_popup(
    frame: &mut Frame,
    content: &PopupContent,
    scroll: u16,
    markdown: bool,
    timezone: DisplayTimezone,
    area: Rect,
) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let title = format!(
        " {} — {} ",
        content.id,
        current().text(Message::KanbanDetailsTitle),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan))
        .title(title);
    let inner_width = popup.width.saturating_sub(2) as usize;
    let lines = popup_lines_with_timezone(content, inner_width, markdown, timezone);
    let body = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(body, popup);
}

/// Draws the details popup with a "no item selected" placeholder, used when the popup is open but
/// the selection is empty (e.g. after navigating to a column with no cards). Keeps the popup frame
/// on screen so the detail mode stays visible, and the title keeps advertising the close key.
fn render_empty_popup(frame: &mut Frame, area: Rect) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let title = format!(" {} ", current().text(Message::KanbanDetailsTitle));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan))
        .title(title);
    let body = Paragraph::new(current().text(Message::KanbanNoSelection))
        .block(block)
        .style(Style::new().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(body, popup);
}

#[cfg(test)]
mod quit_intent_tests {
    use super::*;

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

#[cfg(test)]
mod popup_action_tests {
    use super::*;

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

#[cfg(test)]
mod interaction_decision_tests {
    use super::*;
    use pinto::backlog::{BacklogItem, Status};
    use pinto::rank::Rank;

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

#[cfg(test)]
mod sanitize_left_border_tests {
    use super::*;

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

#[cfg(test)]
mod popup_lines_tests {
    use super::*;
    use pinto::backlog::Status;
    use pinto::rank::Rank;

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

#[cfg(test)]
mod popup_rect_tests {
    use super::*;

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

#[cfg(test)]
mod ordering_tests {
    use super::*;
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
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
