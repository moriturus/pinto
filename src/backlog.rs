//! Domain model.
//!
//! Pure logic representing Product Backlog Items (PBIs) and Kanban workflows.
//! It has no I/O dependency: persistence belongs to `storage` and time is injected.

mod acceptance;
mod graph;
mod item;
mod item_id;
mod status;
mod workflow;

pub use acceptance::AcceptanceCriteriaProgress;
pub use graph::{dependency_creates_cycle, parent_creates_cycle};
pub use item::BacklogItem;
pub use item_id::ItemId;
pub use status::Status;
pub use workflow::Workflow;
