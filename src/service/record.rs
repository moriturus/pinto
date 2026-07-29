//! Sprint child-record services shared by Retro and Review commands.

use crate::error::{Error, Result};
use crate::service::{open_board, open_board_locked};
use crate::sprint::SprintId;
use crate::sprint_record::{SprintRecord, SprintRecordKind};
use crate::storage::{SprintRecordRepository, SprintRepository};
use chrono::Utc;
use std::path::Path;

/// Create the one record of `kind` belonging to an existing Sprint.
///
/// The parent may be planned, active, or closed. A second record of the same kind for the same
/// Sprint is rejected before any write.
///
/// # Errors
///
/// Returns [`Error::SprintNotFound`], [`Error::SprintRecordExists`], or persistence/Git errors.
pub async fn create_sprint_record(
    project_dir: &Path,
    kind: SprintRecordKind,
    sprint_id: &SprintId,
    body: String,
) -> Result<SprintRecord> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    SprintRepository::load(&repo, sprint_id).await?;
    match SprintRecordRepository::load(&repo, kind, sprint_id).await {
        Ok(_) => {
            return Err(Error::SprintRecordExists {
                kind,
                id: sprint_id.clone(),
            });
        }
        Err(Error::SprintRecordNotFound { .. }) => {}
        Err(error) => return Err(error),
    }

    let record = SprintRecord::new(kind, sprint_id.clone(), body, Utc::now());
    SprintRecordRepository::save(&repo, &record).await?;
    repo.commit(&format!("pinto: add {kind} {}", record.id))
        .await?;
    Ok(record)
}

/// Replace the body of an existing Sprint child record.
///
/// A missing parent does not prevent editing a previously stored record because the record itself
/// is the target of this operation.
///
/// # Errors
///
/// Returns [`Error::SprintRecordNotFound`] or persistence/Git errors.
pub async fn edit_sprint_record(
    project_dir: &Path,
    kind: SprintRecordKind,
    sprint_id: &SprintId,
    body: String,
) -> Result<SprintRecord> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut record = SprintRecordRepository::load(&repo, kind, sprint_id).await?;
    record.update_body(body, Utc::now());
    SprintRecordRepository::save(&repo, &record).await?;
    repo.commit(&format!("pinto: update {kind} {}", record.id))
        .await?;
    Ok(record)
}

/// Load one Sprint child record without modifying the board.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintRecordNotFound`], or persistence errors.
pub async fn show_sprint_record(
    project_dir: &Path,
    kind: SprintRecordKind,
    sprint_id: &SprintId,
) -> Result<SprintRecord> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    SprintRecordRepository::load(&repo, kind, sprint_id).await
}

/// List all Sprint child records of `kind` in creation order.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`] or persistence errors.
pub async fn list_sprint_records(
    project_dir: &Path,
    kind: SprintRecordKind,
) -> Result<Vec<SprintRecord>> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    SprintRecordRepository::list(&repo, kind).await
}
