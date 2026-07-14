//! Board initialization and creation of the default configuration.

use crate::config::Config;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Result of [`init_board`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitOutcome {
    /// A new board was initialized; contains the generated `.pinto` path.
    Created(PathBuf),
    /// The board was already initialized; contains the existing `.pinto` path.
    AlreadyInitialized(PathBuf),
}

/// Initialize `.pinto/` (`config.toml` and `tasks/`) in `project_dir`.
///
/// If `config.toml` already exists, leave the board unchanged and return
/// [`InitOutcome::AlreadyInitialized`].
pub async fn init_board(project_dir: &Path) -> Result<InitOutcome> {
    let board_dir = project_dir.join(".pinto");
    let config_path = board_dir.join("config.toml");
    if fs::try_exists(&config_path)
        .await
        .map_err(|e| Error::io(&config_path, &e))?
    {
        return Ok(InitOutcome::AlreadyInitialized(board_dir));
    }

    let tasks_dir = board_dir.join("tasks");
    fs::create_dir_all(&tasks_dir)
        .await
        .map_err(|e| Error::io(&tasks_dir, &e))?;

    let mut config = Config::default();
    if let Some(name) = project_dir.file_name().and_then(|s| s.to_str()) {
        config.project.name = name.to_string();
    }
    config.save(&config_path).await?;

    Ok(InitOutcome::Created(board_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn init_creates_config_and_tasks_dir() {
        let dir = TempDir::new().expect("temp dir");
        let board = dir.path().join(".pinto");

        let outcome = init_board(dir.path()).await.expect("init succeeds");

        assert_eq!(outcome, InitOutcome::Created(board.clone()));
        assert!(board.join("config.toml").is_file());
        assert!(board.join("tasks").is_dir());
    }

    #[tokio::test]
    async fn init_is_idempotent_and_preserves_config() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("first init");
        let config_path = dir.path().join(".pinto").join("config.toml");
        let before = std::fs::read_to_string(&config_path).expect("read config");

        let outcome = init_board(dir.path()).await.expect("second init");

        assert!(matches!(outcome, InitOutcome::AlreadyInitialized(_)));
        let after = std::fs::read_to_string(&config_path).expect("read config");
        assert_eq!(before, after, "existing config must not be modified");
    }

    #[tokio::test]
    async fn init_sets_project_name_from_directory() {
        let dir = TempDir::new().expect("temp dir");
        let project = dir.path().join("myproject");
        std::fs::create_dir(&project).expect("mkdir");

        init_board(&project).await.expect("init succeeds");

        let config = Config::load(&project.join(".pinto").join("config.toml"))
            .await
            .expect("load");
        assert_eq!(config.project.name, "myproject");
    }
}
