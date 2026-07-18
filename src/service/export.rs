//! Read-only complete-board snapshots for machine-readable export.

use super::{apply_effective_points, hierarchical, open_board};
use crate::backlog::{BacklogItem, Status};
use crate::error::{Error, Result};
use crate::service::dod::read_common_dod;
use crate::sprint::Sprint;
use crate::storage::{BacklogItemRepository, SprintRepository};
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
    /// Sprints in the same creation order as `sprint list --json`.
    pub sprints: Vec<Sprint>,
    /// Effective validated board configuration.
    pub config: serde_json::Value,
    /// Common Definition of Done, or `None` when it is unset or empty.
    pub dod: Option<String>,
}

/// Load one read-only snapshot containing the board PBIs, Sprints, configuration, and common DoD.
///
/// Configuration and all repositories are opened once, so the export uses one validated backend
/// selection. The operation does not acquire the write lock or persist anything.
pub async fn export_snapshot(project_dir: &Path) -> Result<BoardSnapshot> {
    let (board_dir, repo, config) = open_board(project_dir).await?;
    let (mut items, sprints, dod) = tokio::try_join!(
        BacklogItemRepository::list(&repo),
        SprintRepository::list(&repo),
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
        sprints,
        config,
        dod,
    })
}
