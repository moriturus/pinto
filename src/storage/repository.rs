//! Persistence traits shared by the file, Git, and SQLite backends.

use crate::backlog::{BacklogItem, ItemId};
use crate::error::Result;
use crate::retro::SprintRetro;
use crate::review::SprintReview;
use crate::sprint::{Sprint, SprintId};
use std::future::Future;
use std::path::PathBuf;

pub trait BacklogItemRepository {
    /// Save a backlog item, replacing any existing item with the same ID.
    ///
    /// # Errors
    ///
    /// Returns validation, serialization, parsing, or I/O/backend errors. A successful save may be
    /// durable before a service-level Git commit completes; retrying the same item save is safe,
    /// but callers must inspect the board after a later commit failure.
    fn save(&self, item: &BacklogItem) -> impl Future<Output = Result<()>>;

    /// Load a backlog item by ID. Return [`crate::error::Error::NotFound`] when it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::NotFound`] or persistence, parsing, and I/O/backend errors.
    /// Loading is read-only, leaves no durable partial changes, and is safe to retry after a
    /// transient read failure.
    fn load(&self, id: &ItemId) -> impl Future<Output = Result<BacklogItem>>;

    /// Return all backlog items in lexicographic rank order, using the ID as a tie-breaker.
    ///
    /// Implementations may read files concurrently and parse them in parallel (see
    /// `docs/DESIGN.md` §3.4).
    ///
    /// # Errors
    ///
    /// Returns persistence, parsing, and I/O/backend errors if any record cannot be read or
    /// validated. Listing is read-only and safe to retry after a transient read failure.
    fn list(&self) -> impl Future<Output = Result<Vec<BacklogItem>>>;

    /// Return all archived backlog items in lexicographic rank order, using the ID as a tie-breaker.
    ///
    /// # Errors
    ///
    /// Returns persistence, parsing, and I/O/backend errors if any archived record cannot be read
    /// or validated. Listing is read-only and safe to retry after a transient read failure.
    fn list_archived(&self) -> impl Future<Output = Result<Vec<BacklogItem>>>;

    /// Load an archived backlog item by ID. Return [`crate::error::Error::NotFound`] when it does
    /// not exist in the archive.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::NotFound`] or persistence, parsing, and I/O/backend errors.
    /// Loading is read-only, leaves no durable partial changes, and is safe to retry after a
    /// transient read failure.
    fn load_archived(&self, id: &ItemId) -> impl Future<Output = Result<BacklogItem>>;

    /// Delete a backlog item by ID. Return [`crate::error::Error::NotFound`] when it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::NotFound`] or persistence, I/O, and backend errors. The
    /// record may be deleted before a service-level commit finishes, so durable partial changes may
    /// remain after a later failure; inspect the board before retrying, especially for permanent
    /// deletion.
    fn delete(&self, id: &ItemId) -> impl Future<Output = Result<()>>;

    /// Move a backlog item by ID to `archive/` and return the destination path.
    ///
    /// This is the non-destructive alternative to [`Self::delete`]. Return
    /// [`crate::error::Error::NotFound`] when the item does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::NotFound`] or persistence, I/O, and backend errors. The
    /// rename may be durable before a service-level commit finishes; retrying is not blindly safe
    /// after a failure, so inspect active and archive stores first.
    fn archive(&self, id: &ItemId) -> impl Future<Output = Result<PathBuf>>;

    /// Restore an archived backlog item to the active item store.
    ///
    /// Implementations must refuse an active item with the same ID without overwriting either
    /// copy. Return [`crate::error::Error::NotFound`] when the archived item does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::NotFound`], collision validation, or persistence/I/O/backend
    /// errors. The archive-to-active rename may remain durable after a later service commit failure;
    /// inspect both stores before retrying.
    fn restore(&self, id: &ItemId) -> impl Future<Output = Result<()>>;

    /// Return the next never-issued ID for `prefix`: one greater than the maximum issued or
    /// existing number, or `1` when no such ID exists.
    ///
    /// # Errors
    ///
    /// Returns invalid-prefix/overflow validation errors or persistence, parsing, and I/O/backend
    /// errors while reading issued history and existing records. This operation is read-only with
    /// respect to the returned ID; retrying after a transient read failure is safe.
    fn next_id(&self, prefix: &str) -> impl Future<Output = Result<ItemId>>;
}

/// Persistence operations for sprints.
///
/// Implementations store sprints separately from backlog items. The method names overlap with
/// [`BacklogItemRepository`], so callers can disambiguate them with a fully qualified call such as
/// `SprintRepository::save(&repo, &sprint)`.
pub trait SprintRepository {
    /// Save a sprint, replacing any existing sprint with the same ID.
    ///
    /// # Errors
    ///
    /// Returns validation, serialization, parsing, or I/O/backend errors. A successful save may be
    /// durable before a service-level Git commit completes; retrying the same sprint save is safe,
    /// but inspect the board after a later commit failure.
    fn save(&self, sprint: &Sprint) -> impl Future<Output = Result<()>>;

    /// Load a sprint by ID. Return [`crate::error::Error::SprintNotFound`] when it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::SprintNotFound`] or persistence, parsing, and I/O/backend
    /// errors. Loading is read-only and safe to retry after a transient read failure.
    fn load(&self, id: &SprintId) -> impl Future<Output = Result<Sprint>>;

    /// Return all sprints in ascending creation-time order, using the ID as a tie-breaker.
    ///
    /// # Errors
    ///
    /// Returns persistence, parsing, and I/O/backend errors if any sprint record cannot be read or
    /// validated. Listing is read-only and safe to retry after a transient read failure.
    fn list(&self) -> impl Future<Output = Result<Vec<Sprint>>>;

    /// Delete a sprint by ID. Return [`crate::error::Error::SprintNotFound`] when it does not exist.
    ///
    /// Backend migration ([`crate::service::migrate_storage`]) uses this operation to remove
    /// destination sprints that are absent from the source.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::SprintNotFound`] or persistence, I/O, and backend errors.
    /// The deletion may be durable before a service-level commit finishes; inspect the destination
    /// before retrying after a later failure.
    fn delete(&self, id: &SprintId) -> impl Future<Output = Result<()>>;
}

/// Persistence operations for Sprint retrospective records.
pub trait SprintRetroRepository {
    /// Save a Retro, replacing any existing record with the same Sprint ID.
    ///
    /// # Errors
    ///
    /// Returns serialization, parsing, I/O, or backend errors.
    fn save(&self, retro: &SprintRetro) -> impl Future<Output = Result<()>>;

    /// Load the Retro belonging to `id`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::RetroNotFound`] or persistence errors.
    fn load(&self, id: &SprintId) -> impl Future<Output = Result<SprintRetro>>;

    /// Return all Retros in ascending creation-time order, using the Sprint ID as a tie-breaker.
    ///
    /// # Errors
    ///
    /// Returns persistence, parsing, or I/O/backend errors if a record cannot be read.
    fn list(&self) -> impl Future<Output = Result<Vec<SprintRetro>>>;

    /// Delete the Retro belonging to `id`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::RetroNotFound`] or persistence errors.
    fn delete(&self, id: &SprintId) -> impl Future<Output = Result<()>>;
}

/// Persistence operations for Sprint Review records.
pub trait SprintReviewRepository {
    /// Save a Review, replacing any existing record with the same Sprint ID.
    ///
    /// # Errors
    ///
    /// Returns serialization, parsing, I/O, or backend errors.
    fn save(&self, review: &SprintReview) -> impl Future<Output = Result<()>>;

    /// Load the Review belonging to `id`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ReviewNotFound`] or persistence errors.
    fn load(&self, id: &SprintId) -> impl Future<Output = Result<SprintReview>>;

    /// Return all Reviews in ascending creation-time order, using the Sprint ID as a tie-breaker.
    ///
    /// # Errors
    ///
    /// Returns persistence, parsing, or I/O/backend errors if a record cannot be read.
    fn list(&self) -> impl Future<Output = Result<Vec<SprintReview>>>;

    /// Delete the Review belonging to `id`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ReviewNotFound`] or persistence errors.
    fn delete(&self, id: &SprintId) -> impl Future<Output = Result<()>>;
}
