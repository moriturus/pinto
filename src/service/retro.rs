//! Sprint Retro record services.

use crate::error::{Error, Result};
use crate::retro::SprintRetro;
use crate::service::{open_board, open_board_locked};
use crate::sprint::SprintId;
use crate::storage::{SprintRepository, SprintRetroRepository};
use chrono::Utc;
use std::path::Path;

/// Create the one Retro belonging to an existing Sprint.
///
/// The parent may be planned, active, or closed. A second Retro for the same Sprint is rejected
/// before any write.
///
/// # Errors
///
/// Returns [`Error::SprintNotFound`], [`Error::RetroExists`], or persistence/Git errors.
pub async fn create_sprint_retro(
    project_dir: &Path,
    sprint_id: &SprintId,
    body: String,
) -> Result<SprintRetro> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    SprintRepository::load(&repo, sprint_id).await?;
    match SprintRetroRepository::load(&repo, sprint_id).await {
        Ok(_) => return Err(Error::RetroExists(sprint_id.clone())),
        Err(Error::RetroNotFound(_)) => {}
        Err(error) => return Err(error),
    }

    let retro = SprintRetro::new(sprint_id.clone(), body, Utc::now());
    SprintRetroRepository::save(&repo, &retro).await?;
    repo.commit(&format!("pinto: add retro {}", retro.id))
        .await?;
    Ok(retro)
}

/// Replace the body of an existing Sprint Retro.
///
/// # Errors
///
/// Returns [`Error::RetroNotFound`] or persistence/Git errors. A missing parent does not prevent
/// editing a previously stored record because the Retro itself is the target of this operation.
pub async fn edit_sprint_retro(
    project_dir: &Path,
    sprint_id: &SprintId,
    body: String,
) -> Result<SprintRetro> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut retro = SprintRetroRepository::load(&repo, sprint_id).await?;
    retro.update_body(body, Utc::now());
    SprintRetroRepository::save(&repo, &retro).await?;
    repo.commit(&format!("pinto: update retro {}", retro.id))
        .await?;
    Ok(retro)
}

/// Load one Sprint Retro without modifying the board.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::RetroNotFound`], or persistence errors.
pub async fn show_sprint_retro(project_dir: &Path, sprint_id: &SprintId) -> Result<SprintRetro> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    SprintRetroRepository::load(&repo, sprint_id).await
}

/// List all Sprint Retros in creation order.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`] or persistence errors.
pub async fn list_sprint_retros(project_dir: &Path) -> Result<Vec<SprintRetro>> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    SprintRetroRepository::list(&repo).await
}
