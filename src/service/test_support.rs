//! Shared helper for service layer testing.
//!
//! These helpers support persistence-level tests (DESIGN §8) using isolated `tempfile` boards.
//! They live in a separate module because multiple service submodules share them.

use super::{ItemEdit, NewItem, add_item, init_board};
use crate::backlog::{BacklogItem, ItemId};
use crate::config::Config;
use std::path::Path;
use tempfile::TempDir;

/// Prepare an initialized temporary board.
pub(super) async fn init_temp() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    init_board(dir.path()).await.expect("init");
    dir
}

/// Helps save PBI by specifying state, sprint, and label for filters.
pub(super) async fn add_with(
    dir: &Path,
    title: &str,
    labels: &[&str],
    sprint: Option<&str>,
) -> BacklogItem {
    let new = NewItem {
        points: None,
        labels: labels
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        sprint: sprint.map(std::string::ToString::to_string),
        body: String::new(),
        parent: None,
        depends_on: Vec::new(),
    };
    add_item(dir, title, new).await.expect("add succeeds")
}

/// Replace the entire column of `config.toml` for testing (reproduce the situation where the user manually edited it).
pub(super) async fn set_columns(dir: &Path, columns: &[&str]) {
    let path = dir.join(".pinto").join("config.toml");
    let mut config = Config::load(&path).await.expect("load config");
    config.columns = columns
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    config.save(&path).await.expect("save config");
}

/// [`ItemEdit`] only changes the parent (other fields remain unchanged).
pub(super) fn parent_edit(parent: Option<ItemId>) -> ItemEdit {
    ItemEdit {
        parent: Some(parent),
        ..Default::default()
    }
}
