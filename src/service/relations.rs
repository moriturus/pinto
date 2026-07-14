//! Shared validation for PBI relationships.

use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};

/// Validate a parent assignment against the current board graph.
pub(crate) fn validate_parent(
    items: &[BacklogItem],
    child: &ItemId,
    parent: &ItemId,
) -> Result<()> {
    if crate::backlog::parent_creates_cycle(items, child, parent) {
        return Err(Error::ParentCycle {
            child: child.clone(),
            parent: parent.clone(),
        });
    }
    if !items.iter().any(|item| &item.id == parent) {
        return Err(Error::NotFound(parent.clone()));
    }
    Ok(())
}

/// Validate dependency targets and report whether any new edge would create a cycle.
pub(crate) fn validate_dependencies(
    items: &[BacklogItem],
    item: &ItemId,
    dependencies: &[ItemId],
) -> Result<bool> {
    let mut cycle_warning = false;
    for dependency in dependencies {
        // The item is not in `items` yet during creation, but a self-dependency is still a valid
        // target. It is the same warning-only cycle accepted by `dep add`.
        if dependency != item && !items.iter().any(|current| &current.id == dependency) {
            return Err(Error::NotFound(dependency.clone()));
        }
        cycle_warning |= crate::backlog::dependency_creates_cycle(items, item, dependency);
    }
    Ok(cycle_warning)
}
