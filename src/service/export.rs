//! Read-only complete-board snapshots for machine-readable export.

use super::{apply_effective_points, hierarchical, lock_board, open_board};
use crate::backlog::{BacklogItem, Status};
use crate::error::{Error, Result};
use crate::service::dod::read_common_dod;
use crate::sprint::Sprint;
use crate::sprint_record::{SprintRecord, SprintRecordKind};
use crate::storage::{BacklogItemRepository, SprintRecordRepository, SprintRepository};
use std::path::Path;

/// A complete read-only snapshot of all board data exposed by `export --json`.
///
/// The configuration is represented as JSON because the typed configuration module is an
/// implementation detail of the library. It is the effective, validated configuration loaded by
/// pinto, including defaults for omitted settings.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardSnapshot {
    /// Active PBIs in the same hierarchical priority order as `list --json`.
    pub items: Vec<BacklogItem>,
    /// Archived PBIs in the same rank order as `list --archived --json`.
    ///
    /// Including the archive makes the snapshot a lossless board copy: an active PBI may legitimately
    /// reference an archived parent, so excluding the archive would let a healthy board export to a
    /// snapshot that reimports as a dangling reference. The collection defaults to empty when it is
    /// absent from a snapshot produced by an older pinto.
    pub archived_items: Vec<BacklogItem>,
    /// Sprints in the same creation order as `sprint list --json`.
    pub sprints: Vec<Sprint>,
    /// Sprint Retros in the same creation order as `sprint retro list --json`.
    pub retros: Vec<SprintRecord>,
    /// Sprint Reviews in the same creation order as `sprint review list --json`.
    pub reviews: Vec<SprintRecord>,
    /// Effective validated board configuration.
    pub config: serde_json::Value,
    /// Common Definition of Done, or `None` when it is unset or empty.
    pub dod: Option<String>,
}

/// Load one read-only snapshot containing the board PBIs, Sprints, child records, configuration,
/// and common DoD.
///
/// Configuration and all repositories are opened once, so the export uses one validated backend
/// selection. The board write lock is acquired before configuration and storage are opened and is
/// held until the complete snapshot has been assembled. This gives automation one consistent board
/// view while ordinary read commands remain non-blocking.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], lock and persistence errors while reading board records or
/// the common DoD, or [`Error::Parse`] if the effective configuration cannot be serialized. The
/// snapshot is read-only and the lock guard prevents a concurrent writer from changing it; no
/// durable partial changes remain and retrying after a transient read failure is safe.
pub async fn export_snapshot(project_dir: &Path) -> Result<BoardSnapshot> {
    let _lock = lock_board(project_dir).await?;
    let (board_dir, repo, config) = open_board(project_dir).await?;
    let (mut items, archived_items, sprints, retros, reviews, dod) = tokio::try_join!(
        BacklogItemRepository::list(&repo),
        BacklogItemRepository::list_archived(&repo),
        SprintRepository::list(&repo),
        SprintRecordRepository::list(&repo, SprintRecordKind::Retro),
        SprintRecordRepository::list(&repo, SprintRecordKind::Review),
        read_common_dod(&board_dir),
    )?;

    apply_effective_points(
        &mut items,
        config.points.aggregate_children,
        &Status::new(&config.done_column),
    );
    let items = hierarchical(items);
    let config = serde_json::to_value(&config).map_err(|error| Error::Parse {
        path: board_dir.join("config.toml"),
        message: error.to_string(),
    })?;

    Ok(BoardSnapshot {
        items,
        archived_items,
        sprints,
        retros,
        reviews,
        config,
        dod,
    })
}
