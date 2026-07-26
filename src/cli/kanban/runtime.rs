//! Drawing and the interactive event loop for the Kanban view.

use super::keymap::KeyMap;
use super::{BoardView, MIN_COLUMN_WIDTH};
use anyhow::Result;
use pinto::kanban_keys::KeyBindings;
use pinto::service::{Board, BoardQuery, board};
use pinto::timezone::DisplayTimezone;
use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
#[cfg(test)]
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Size;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::runtime::Handle;

mod input;
mod terminal;

mod actions;
mod dispatch;
mod render;
#[cfg(test)]
mod tests;

use dispatch::DispatchContext;
use dispatch::dispatch_key;
#[cfg(test)]
pub(super) use render::header;
#[cfg(test)]
pub(super) use render::render_with_localizer;
pub(super) use render::{help_max_scroll, popup_max_scroll, render};

#[cfg(test)]
use input::{
    HelpKeyAction, PopupAction, QuitIntent, help_key_action, popup_action, quit_intent,
    should_close_help_after_key, text_entry_key_is_accepted,
};
#[cfg(test)]
use pinto::i18n::{Message, current};
#[cfg(test)]
use pinto::service::SearchMode;
use terminal::{DefaultBackend, TerminalGuard, initialize_terminal};

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

/// Terminal operations needed by the event loop.
///
/// Keeping frame drawing and sizing behind this boundary lets the event loop exercise its failure
/// paths with deterministic test doubles instead of requiring a real TTY.
trait FrameDriver {
    fn size(&mut self) -> io::Result<Size>;

    fn draw(&mut self, view: &BoardView, confirming: bool, keymap: &KeyMap) -> io::Result<()>;

    fn edit_selected(&mut self, handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()>;
}

impl FrameDriver for TerminalGuard<DefaultBackend> {
    fn size(&mut self) -> io::Result<Size> {
        (**self).size()
    }

    fn draw(&mut self, view: &BoardView, confirming: bool, keymap: &KeyMap) -> io::Result<()> {
        (**self)
            .draw(|frame| render(frame, view, confirming, keymap))
            .map(|_| ())
    }

    fn edit_selected(&mut self, handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
        actions::edit_selected(self, handle, dir, view)
    }
}

/// Source of terminal events consumed by the blocking Kanban loop.
trait EventSource {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
    fn read(&mut self) -> io::Result<Event>;
}

struct CrosstermEventSource;

impl EventSource for CrosstermEventSource {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        event::poll(timeout)
    }

    fn read(&mut self) -> io::Result<Event> {
        event::read()
    }
}

/// Result of handling a key after the frame and event I/O has completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControl {
    Continue,
    Exit(ExitMode),
}

/// Run the terminal-independent part of the Kanban event loop.
///
/// The handler owns event interpretation and view-state changes. It receives the frame driver only
/// as an opaque value, so tests can verify state transitions without depending on drawing or a real
/// terminal. Resize and non-key events simply cause the next frame to be measured and drawn again.
fn run_event_loop<T, E, H>(
    terminal: &mut T,
    events: &mut E,
    mut view: BoardView,
    keymap: &KeyMap,
    mut handle_key: H,
) -> anyhow::Result<ExitMode>
where
    T: FrameDriver,
    E: EventSource,
    H: FnMut(
        &mut T,
        &mut BoardView,
        &mut Option<ExitMode>,
        KeyEvent,
    ) -> anyhow::Result<LoopControl>,
{
    let mut confirming = None;
    loop {
        let size = terminal.size()?;
        view.scroll_to_visible(effective_capacity(size.width, view.is_maximized()));
        terminal.draw(&view, confirming.is_some(), keymap)?;

        if !events.poll(POLL)? {
            continue;
        }
        let event = events.read()?;
        let Event::Key(key) = event else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match handle_key(terminal, &mut view, &mut confirming, key)? {
            LoopControl::Continue => {}
            LoopControl::Exit(mode) => return Ok(mode),
        }
    }
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
    view: BoardView,
    keymap: KeyMap,
    confirm_quit: bool,
) -> Result<ExitMode> {
    // Initialize raw mode and the alternate screen. Return non-TTY failures here instead of
    // panicking; `try_init` is used because this is an internal call.
    let (mut terminal, _panic_hook) = initialize_terminal()?;
    let mut events = CrosstermEventSource;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_event_loop(
            &mut terminal,
            &mut events,
            view,
            &keymap,
            |terminal, view, confirming, key| {
                let mut context = DispatchContext {
                    terminal,
                    handle: &handle,
                    dir: &dir,
                    view,
                    keymap: &keymap,
                    confirming,
                    confirm_quit,
                };
                dispatch_key(&mut context, key)
            },
        )
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
