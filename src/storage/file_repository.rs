//! [`FileRepository`]: the `.pinto/` plain-text backend, split by concern into
//! item persistence (`items`) and sprint persistence (`sprints`).

use crate::backlog::{BacklogItem, ItemId};
use crate::error::Error;
use crate::error::Result;
use crate::sprint::{Sprint, SprintId};
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;

mod items;
mod sprints;

/// [`BacklogItemRepository`] and [`SprintRepository`] implementation backed by the `.pinto/` directory.
///
/// [`BacklogItemRepository`]: crate::storage::repository::BacklogItemRepository
/// [`SprintRepository`]: crate::storage::repository::SprintRepository
#[derive(Debug, Clone)]
pub struct FileRepository {
    root: PathBuf,
}

type ItemRecord = (PathBuf, BacklogItem);
type SprintRecord = (PathBuf, Sprint);

impl FileRepository {
    /// Build by specifying the board root (`.pinto/`). No file I/O is performed.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Directory to place task files (`<root>/tasks`).
    #[must_use]
    pub fn tasks_dir(&self) -> PathBuf {
        self.root.join("tasks")
    }

    /// Directory to put sprint files (`<root>/sprints`).
    #[must_use]
    pub fn sprints_dir(&self) -> PathBuf {
        self.root.join("sprints")
    }

    /// The sprint file path for the specified ID (`<root>/sprints/<id>.md`).
    pub(crate) fn sprint_path_for(&self, id: &SprintId) -> PathBuf {
        self.sprints_dir().join(format!("{id}.md"))
    }

    /// Task file path for the specified ID (`<root>/tasks/<id>.md`).
    pub(crate) fn path_for(&self, id: &ItemId) -> Result<PathBuf> {
        self.safe_item_path(&self.tasks_dir(), id)
    }

    /// Destination directory (`<root>/archive`).
    fn archive_dir(&self) -> PathBuf {
        self.root.join("archive")
    }

    /// Archive file path for the specified ID (`<root>/archive/<id>.md`).
    fn archive_path_for(&self, id: &ItemId) -> Result<PathBuf> {
        self.safe_item_path(&self.archive_dir(), id)
    }

    /// Build one item path while checking that the ID contributes exactly one filename component.
    fn safe_item_path(&self, directory: &Path, id: &ItemId) -> Result<PathBuf> {
        let path = directory.join(format!("{id}.md"));
        if path.parent() != Some(directory) || !path.starts_with(directory) {
            return Err(Error::InvalidItemId(id.to_string()));
        }
        Ok(path)
    }

    /// Collect the `.md` file paths directly under `tasks/` (directory scanning is asynchronous).
    ///
    /// If the directory does not exist, return `None` to indicate that it has no items yet.
    async fn markdown_paths(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>> {
        let mut read_dir = match fs::read_dir(dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::io(dir, &e)),
        };

        let mut paths = Vec::new();
        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| Error::io(dir, &e))?
        {
            let path = entry.path();
            if Self::is_markdown(&path) {
                paths.push(path);
            }
        }
        Ok(Some(paths))
    }

    /// Return whether `path` has the `.md` extension.
    fn is_markdown(path: &Path) -> bool {
        path.extension().and_then(|s| s.to_str()) == Some("md")
    }
}

#[cfg(test)]
mod tests;
