//! Queries for action PBIs linked to Sprint Retro and Review records.

use crate::backlog::{ActionSource, BacklogItem};
use crate::error::Result;
use crate::service::open_board;
use crate::storage::BacklogItemRepository;
use std::path::Path;

/// Return active PBIs linked to `source` in backlog priority order.
///
/// Action progress is intentionally read from the normal PBI status field. Archived or physically
/// removed PBIs are absent because normal PBI removal controls their visibility here as well.
///
/// # Errors
///
/// Returns board initialization, persistence, and parsing errors while reading PBIs.
pub async fn linked_action_items(
    project_dir: &Path,
    source: &ActionSource,
) -> Result<Vec<BacklogItem>> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let items = BacklogItemRepository::list(&repo).await?;
    Ok(items
        .into_iter()
        .filter(|item| item.source.as_ref() == Some(source))
        .collect())
}
