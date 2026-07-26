//! Terminal ownership and panic-hook restoration for the Kanban runtime.

use anyhow::Result;
use pinto::i18n::{Message, current};
use ratatui::backend::{Backend, CrosstermBackend};
use std::io::IsTerminal;
use std::io::{self, Stdout};
use std::ops::{Deref, DerefMut};

pub(super) type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static>;
pub(super) type DefaultBackend = CrosstermBackend<Stdout>;

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
pub(super) struct TerminalGuard<B: Backend = DefaultBackend> {
    terminal: ratatui::Terminal<B>,
    restored: bool,
    restore_action: Box<dyn FnMut() -> io::Result<()>>,
}

impl<B: Backend> TerminalGuard<B> {
    fn with_restore_action(
        terminal: ratatui::Terminal<B>,
        restore_action: impl FnMut() -> io::Result<()> + 'static,
    ) -> Self {
        Self {
            terminal,
            restored: false,
            restore_action: Box::new(restore_action),
        }
    }

    #[cfg(test)]
    pub(super) fn with_restore(
        terminal: ratatui::Terminal<B>,
        restore_action: impl FnMut() -> io::Result<()> + 'static,
    ) -> Self {
        Self::with_restore_action(terminal, restore_action)
    }

    pub(super) fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        let result = (self.restore_action)();
        if result.is_ok() {
            self.restored = true;
        }
        result
    }
}

impl TerminalGuard<DefaultBackend> {
    fn new(terminal: ratatui::DefaultTerminal) -> Self {
        Self::with_restore_action(terminal, ratatui::try_restore)
    }
}

impl<B: Backend> Deref for TerminalGuard<B> {
    type Target = ratatui::Terminal<B>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl<B: Backend> DerefMut for TerminalGuard<B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl<B: Backend> Drop for TerminalGuard<B> {
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
