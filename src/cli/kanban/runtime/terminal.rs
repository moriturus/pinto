//! Terminal ownership and panic-hook restoration for the Kanban runtime.

use anyhow::Result;
use pinto::i18n::{Message, current};
use std::io::IsTerminal;
use std::ops::{Deref, DerefMut};

pub(super) type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static>;

/// Restore the process-global panic hook when a terminal lifecycle ends.
pub(super) struct PanicHookGuard {
    previous: Option<PanicHook>,
}

impl PanicHookGuard {
    /// Install the Kanban hook around the hook currently installed by ratatui.
    pub(super) fn install(previous: PanicHook) -> Self {
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
pub(super) struct TerminalGuard {
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

    pub(super) fn restore(&mut self) -> std::io::Result<()> {
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
pub(super) fn initialize_terminal() -> Result<(TerminalGuard, PanicHookGuard)> {
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
