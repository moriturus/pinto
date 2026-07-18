//! Persistence traits shared by the file, Git, and SQLite backends.

use crate::backlog::{BacklogItem, ItemId};
use crate::error::Result;
use crate::sprint::{Sprint, SprintId};
use std::future::Future;
use std::path::PathBuf;

pub trait BacklogItemRepository {
    /// Save a backlog item, replacing any existing item with the same ID.
    fn save(&self, item: &BacklogItem) -> impl Future<Output = Result<()>>;

    /// Load a backlog item by ID. Return [`crate::error::Error::NotFound`] when it does not exist.
    fn load(&self, id: &ItemId) -> impl Future<Output = Result<BacklogItem>>;

    /// Return all backlog items in lexicographic rank order, using the ID as a tie-breaker.
    ///
    /// Implementations may read files concurrently and parse them in parallel (see
    /// `docs/DESIGN.md` §3.4).
    fn list(&self) -> impl Future<Output = Result<Vec<BacklogItem>>>;

    /// Return all archived backlog items in lexicographic rank order, using the ID as a tie-breaker.
    fn list_archived(&self) -> impl Future<Output = Result<Vec<BacklogItem>>>;

    /// Load an archived backlog item by ID. Return [`crate::error::Error::NotFound`] when it does
    /// not exist in the archive.
    fn load_archived(&self, id: &ItemId) -> impl Future<Output = Result<BacklogItem>>;

    /// Delete a backlog item by ID. Return [`crate::error::Error::NotFound`] when it does not exist.
    fn delete(&self, id: &ItemId) -> impl Future<Output = Result<()>>;

    /// Move a backlog item by ID to `archive/` and return the destination path.
    ///
    /// This is the non-destructive alternative to [`Self::delete`]. Return
    /// [`crate::error::Error::NotFound`] when the item does not exist.
    fn archive(&self, id: &ItemId) -> impl Future<Output = Result<PathBuf>>;

    /// Restore an archived backlog item to the active item store.
    ///
    /// Implementations must refuse an active item with the same ID without overwriting either
    /// copy. Return [`crate::error::Error::NotFound`] when the archived item does not exist.
    fn restore(&self, id: &ItemId) -> impl Future<Output = Result<()>>;

    /// Return the next never-issued ID for `prefix`: one greater than the maximum issued or
    /// existing number, or `1` when no such ID exists.
    fn next_id(&self, prefix: &str) -> impl Future<Output = Result<ItemId>>;
}

/// Persistence operations for sprints.
///
/// Implementations store sprints separately from backlog items. The method names overlap with
/// [`BacklogItemRepository`], so callers can disambiguate them with a fully qualified call such as
/// `SprintRepository::save(&repo, &sprint)`.
pub trait SprintRepository {
    /// Save a sprint, replacing any existing sprint with the same ID.
    fn save(&self, sprint: &Sprint) -> impl Future<Output = Result<()>>;

    /// Load a sprint by ID. Return [`crate::error::Error::SprintNotFound`] when it does not exist.
    fn load(&self, id: &SprintId) -> impl Future<Output = Result<Sprint>>;

    /// Return all sprints in ascending creation-time order, using the ID as a tie-breaker.
    fn list(&self) -> impl Future<Output = Result<Vec<Sprint>>>;

    /// Delete a sprint by ID. Return [`crate::error::Error::SprintNotFound`] when it does not exist.
    ///
    /// Backend migration ([`crate::service::migrate_storage`]) uses this operation to remove
    /// destination sprints that are absent from the source.
    fn delete(&self, id: &SprintId) -> impl Future<Output = Result<()>>;
}
