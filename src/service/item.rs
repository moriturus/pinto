//! Backlog-item use cases, split by concern into `crud`, `reorder`, and `edit`.

mod crud;
mod edit;
mod reorder;

pub use crud::{
    AddItemOutcome, ListFilter, MoveOutcome, NewItem, RemoveOutcome, add_item,
    add_item_with_outcome, list_items, move_item, move_item_with_outcome, remove_item, show_item,
};
pub use edit::{EditOutcome, ItemEdit, apply_item_edit, edit_item, item_edit_template};
pub use reorder::{RebalanceOutcome, ReorderTarget, rebalance, reorder_item};

// Domain types referenced by the test module below via `use super::*`.
#[cfg(test)]
use crate::backlog::{ItemId, Status};

#[cfg(test)]
mod tests;
