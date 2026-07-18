//! Persistence layer.
//!
//! Read and write backlog items as Markdown (TOML frontmatter plus body), one item per file.
//! Repository traits keep persistence separate from the domain layer and make backends replaceable
//! in tests.
//!
//! Frontmatter is TOML between `+++` delimiters and uses the same serialization format as
//! `config.toml`, keeping the data human-editable.

mod atomic;
mod backend;
mod file_repository;
mod git_repository;
mod issued_ids;
mod lock;
mod markdown;
mod repository;
#[cfg(feature = "sqlite")]
mod sqlite_repository;

pub(crate) use atomic::atomic_write;
pub(crate) use issued_ids::{path as item_issued_ids_path, record as record_issued_id};
// The Markdown representation (`+++` frontmatter plus body) is the backend-independent editing
// format used by `$EDITOR`. The facade re-exports it because the service layer uses it to build
// edit templates and parse edited content; concrete paths remain backend-internal.
pub use backend::Backend;
pub use lock::BoardLock;
pub use markdown::parse_item_markdown;
pub(crate) use markdown::split_frontmatter as parse_frontmatter;
pub(crate) use markdown::{from_markdown as item_from_markdown, to_markdown as item_to_markdown};
// Re-export the backend type here because backend selection and migration use the persistence
// facade's public API ([`Backend::open`] and [`crate::service::migrate_storage`]).
pub use crate::config::StorageBackend;
pub use file_repository::FileRepository;
pub use git_repository::GitRepository;
pub use repository::{BacklogItemRepository, SprintRepository};
#[cfg(feature = "sqlite")]
pub use sqlite_repository::SqliteRepository;
