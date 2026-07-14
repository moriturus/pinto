//! Read settings used by the TUI and human-readable CLI output.
//!
//! `Config` is crate-private, so [`TuiSettings`] exposes only the values needed by the binary-side
//! TUI. Both CLI and TUI read the same `config.toml` through the shared persistence layer.

use super::open_board;
use crate::error::Result;
use crate::timezone::DisplayTimezone;
use std::path::Path;

/// Settings required to launch the interactive Kanban view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiSettings {
    /// Whether to display a confirmation popup upon exit.
    pub confirm_quit: bool,
    /// Workflow columns in their configured order.
    pub workflow: Vec<String>,
    /// Workflow columns hidden from the default Kanban display.
    pub hidden_columns: Vec<String>,
    /// Configured Kanban key assignments.
    pub key_bindings: crate::kanban_keys::KeyBindings,
    /// Render PBI bodies as Markdown in the details popup.
    pub markdown: bool,
    /// Timezone for human-readable timestamps in the details popup.
    pub timezone: DisplayTimezone,
}

/// Read TUI settings from the board. Return [`crate::error::Error::NotInitialized`] when it is
/// uninitialized.
pub async fn tui_settings(project_dir: &Path) -> Result<TuiSettings> {
    let (_board_dir, _repo, config) = open_board(project_dir).await?;
    Ok(TuiSettings {
        confirm_quit: config.tui.confirm_quit,
        workflow: config.columns,
        hidden_columns: config.tui.hidden_columns,
        key_bindings: config.tui.key_bindings,
        markdown: config.display.markdown,
        timezone: config.display.timezone,
    })
}

/// Display settings for the `show` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySettings {
    /// Render the PBI body (and common DoD) as Markdown instead of raw text.
    pub markdown: bool,
    /// Timezone for human-readable timestamps.
    pub timezone: DisplayTimezone,
}

/// Read display settings from the board. Return [`crate::error::Error::NotInitialized`] when it is
/// uninitialized.
pub async fn display_settings(project_dir: &Path) -> Result<DisplaySettings> {
    let (_board_dir, _repo, config) = open_board(project_dir).await?;
    Ok(DisplaySettings {
        markdown: config.display.markdown,
        timezone: config.display.timezone,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::init_board;
    use tempfile::TempDir;

    #[tokio::test]
    async fn tui_settings_defaults_to_confirm_quit() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        let settings = tui_settings(dir.path()).await.expect("settings");
        assert!(settings.confirm_quit);
        assert_eq!(
            settings
                .key_bindings
                .keys(crate::kanban_keys::KeyAction::Quit),
            ["q".to_string(), "Esc".to_string()]
        );
        assert_eq!(settings.workflow, ["todo", "in-progress", "review", "done"]);
        assert!(settings.hidden_columns.is_empty());
    }

    #[tokio::test]
    async fn tui_settings_on_uninitialized_dir_errors() {
        let dir = TempDir::new().expect("temp dir");
        assert!(tui_settings(dir.path()).await.is_err());
    }

    #[tokio::test]
    async fn tui_settings_defaults_to_markdown_on() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        let settings = tui_settings(dir.path()).await.expect("settings");
        assert!(settings.markdown);
    }

    #[tokio::test]
    async fn display_settings_defaults_to_markdown_on() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        let settings = display_settings(dir.path()).await.expect("settings");
        assert!(settings.markdown);
    }

    #[tokio::test]
    async fn display_settings_on_uninitialized_dir_errors() {
        let dir = TempDir::new().expect("temp dir");
        assert!(display_settings(dir.path()).await.is_err());
    }
}
