//! Resolve the project directory used by a CLI invocation.
//!
//! Board commands normally start in the current directory, but a repository checkout often runs
//! them from a nested source directory. This module keeps that convenience at the CLI boundary so
//! service APIs can continue to receive an explicit project root.

use pinto::error::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

const BOARD_DIRECTORY: &str = ".pinto";
const CONFIG_FILE: &str = "config.toml";
const DIRECTORY_ENV: &str = "PINTO_DIR";

/// Prepare the process working directory for one invocation.
///
/// `--dir` takes precedence over [`PINTO_DIR`]. For board commands without an override, the
/// nearest ancestor containing `.pinto/config.toml` is selected. A `.git` marker is a documented
/// repository boundary: the marker's directory is checked, then its parents are not searched.
/// `init` intentionally keeps its historical current-directory behavior unless an explicit
/// override is supplied, and completion never requires a board.
pub(super) async fn prepare_working_directory(
    flag_override: Option<&Path>,
    init: bool,
    completion: bool,
    allow_missing_board: bool,
) -> Result<()> {
    if completion {
        return Ok(());
    }

    let current = std::env::current_dir().map_err(|error| Error::Io {
        path: PathBuf::from("."),
        message: error.to_string(),
    })?;
    let configured = flag_override
        .map(PathBuf::from)
        .or_else(environment_override);

    let project_dir = match configured {
        Some(path) => {
            override_project_directory(&current, &path, !init && !allow_missing_board).await?
        }
        None if init => return Ok(()),
        None => match discover_project_directory(&current).await {
            Ok(project_dir) => project_dir,
            Err(error) if allow_missing_board && matches!(error, Error::NotInitialized { .. }) => {
                // `automate` validates and reads its plan before it needs a board. Preserve that
                // source diagnostic when the caller only wants to report a missing plan file.
                return Ok(());
            }
            Err(error) => return Err(error),
        },
    };

    if project_dir != current {
        std::env::set_current_dir(&project_dir).map_err(|error| Error::Io {
            path: project_dir,
            message: error.to_string(),
        })?;
    }
    Ok(())
}

fn environment_override() -> Option<PathBuf> {
    std::env::var_os(DIRECTORY_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

async fn override_project_directory(
    current: &Path,
    configured: &Path,
    require_board: bool,
) -> Result<PathBuf> {
    let path = absolute_path(current, configured);
    let project_dir = if path.file_name().is_some_and(|name| name == BOARD_DIRECTORY) {
        path.parent().map_or(path.clone(), Path::to_path_buf)
    } else {
        path
    };

    if require_board && !has_board(&project_dir).await? {
        return Err(not_initialized(&project_dir));
    }
    Ok(project_dir)
}

async fn discover_project_directory(start: &Path) -> Result<PathBuf> {
    let mut candidate = start.to_path_buf();
    loop {
        if has_board(&candidate).await? {
            return Ok(candidate);
        }

        // Check a repository root but do not accidentally select a board belonging to an outer
        // checkout. The boundary is intentionally simple and works for both .git directories and
        // linked-worktree .git files.
        if exists(&candidate.join(".git")).await? {
            break;
        }
        let Some(parent) = candidate.parent() else {
            break;
        };
        if parent == candidate {
            break;
        }
        candidate = parent.to_path_buf();
    }
    Err(not_initialized(start))
}

async fn has_board(project_dir: &Path) -> Result<bool> {
    exists(&project_dir.join(BOARD_DIRECTORY).join(CONFIG_FILE)).await
}

async fn exists(path: &Path) -> Result<bool> {
    fs::try_exists(path).await.map_err(|error| Error::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn absolute_path(current: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        current.join(path)
    }
}

fn not_initialized(project_dir: &Path) -> Error {
    Error::NotInitialized {
        path: project_dir.join(BOARD_DIRECTORY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn discovery_prefers_the_nearest_board_and_stops_at_git_boundary() {
        let outer = TempDir::new().expect("outer temp dir");
        let inner = outer.path().join("repository");
        let nested = inner.join("src");
        fs::create_dir_all(&nested).await.expect("create nested");
        fs::create_dir(inner.join(".git"))
            .await
            .expect("git marker");

        fs::create_dir_all(outer.path().join(".pinto"))
            .await
            .expect("create outer board");
        assert!(discover_project_directory(&nested).await.is_err());
    }

    #[test]
    fn relative_paths_are_resolved_from_the_invocation_directory() {
        let current = Path::new("/tmp/worktree");
        assert_eq!(
            absolute_path(current, Path::new("../board")),
            PathBuf::from("/tmp/worktree/../board")
        );
    }
}
