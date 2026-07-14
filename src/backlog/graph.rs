//! Graph inspection of inter-PBI links.

use super::{BacklogItem, ItemId};
use std::collections::{HashMap, HashSet};

/// Return whether assigning `parent` as `child`'s parent would create a cycle in `items`.
///
/// Parent-child links must remain acyclic. Trace the ancestors of `parent`; reaching `child` means
/// the new link would close a cycle. Track visited IDs so malformed existing data cannot cause an
/// infinite loop.
#[must_use]
pub fn parent_creates_cycle(items: &[BacklogItem], child: &ItemId, parent: &ItemId) -> bool {
    if child == parent {
        return true;
    }
    let parents: HashMap<&ItemId, &ItemId> = items
        .iter()
        .filter_map(|it| it.parent.as_ref().map(|p| (&it.id, p)))
        .collect();
    let mut current = Some(parent);
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if id == child {
            return true;
        }
        if !seen.insert(id) {
            break;
        }
        current = parents.get(id).copied();
    }
    false
}

/// Return whether adding dependency `dep` to `item` would create a cycle.
///
/// Follow `dep`'s transitive dependencies until `item` is reached. Dependency cycles are warnings
/// rather than errors; the visited set ensures traversal terminates even when the existing graph
/// already contains a cycle.
#[must_use]
pub fn dependency_creates_cycle(items: &[BacklogItem], item: &ItemId, dep: &ItemId) -> bool {
    if item == dep {
        return true;
    }
    let deps: HashMap<&ItemId, &[ItemId]> = items
        .iter()
        .map(|it| (&it.id, it.depends_on.as_slice()))
        .collect();
    let mut stack = vec![dep];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if id == item {
            return true;
        }
        if !seen.insert(id) {
            continue;
        }
        if let Some(edges) = deps.get(id) {
            stack.extend(edges.iter());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::Status;
    use crate::rank::Rank;
    use chrono::DateTime;

    /// Create a minimal PBI with number `n` and no parent or dependencies.
    fn item(n: u32) -> BacklogItem {
        BacklogItem::new(
            ItemId::new("T", n),
            format!("Item {n}"),
            Status::new("todo"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            DateTime::from_timestamp(0, 0).expect("valid epoch"),
        )
        .expect("valid item")
    }

    /// Create a PBI with parent `parent`.
    fn item_with_parent(n: u32, parent: u32) -> BacklogItem {
        let mut it = item(n);
        it.parent = Some(ItemId::new("T", parent));
        it
    }

    /// Create a PBI with dependencies `deps`.
    fn item_with_deps(n: u32, deps: &[u32]) -> BacklogItem {
        let mut it = item(n);
        it.depends_on = deps.iter().map(|&d| ItemId::new("T", d)).collect();
        it
    }

    // --- parent_creates_cycle ---

    #[test]
    fn parent_self_reference_is_a_cycle() {
        let items = [item(1)];
        assert!(parent_creates_cycle(
            &items,
            &ItemId::new("T", 1),
            &ItemId::new("T", 1)
        ));
    }

    #[test]
    fn parent_to_unrelated_item_is_acyclic() {
        // 1 and 2 are irrelevant. Even if you change the parent of 1 to 2, it will not cycle.
        let items = [item(1), item(2)];
        assert!(!parent_creates_cycle(
            &items,
            &ItemId::new("T", 1),
            &ItemId::new("T", 2)
        ));
    }

    #[test]
    fn parent_to_descendant_is_a_cycle() {
        // The parent of 2 is 1 (parent and child of 1 → 2). If the parent of 1 is set to 2, it cycles as 1↔2.
        let items = [item(1), item_with_parent(2, 1)];
        assert!(parent_creates_cycle(
            &items,
            &ItemId::new("T", 1),
            &ItemId::new("T", 2)
        ));
    }

    #[test]
    fn parent_to_transitive_descendant_is_a_cycle() {
        // 1 ← 2 ← 3 (parent 2 of 3, parent 1 of 2). If you change the parent of 1 to 3, it becomes a cycle.
        let items = [item(1), item_with_parent(2, 1), item_with_parent(3, 2)];
        assert!(parent_creates_cycle(
            &items,
            &ItemId::new("T", 1),
            &ItemId::new("T", 3)
        ));
    }

    #[test]
    fn parent_to_sibling_subtree_is_acyclic() {
        // 1 ← 2, 1 ← 3. Even if you change the parent of 3 to 2 (2 is not a descendant of 3), it will not cycle.
        let items = [item(1), item_with_parent(2, 1), item_with_parent(3, 1)];
        assert!(!parent_creates_cycle(
            &items,
            &ItemId::new("T", 3),
            &ItemId::new("T", 2)
        ));
    }

    // --- dependency_creates_cycle ---

    #[test]
    fn dependency_self_reference_is_a_cycle() {
        let items = [item(1)];
        assert!(dependency_creates_cycle(
            &items,
            &ItemId::new("T", 1),
            &ItemId::new("T", 1)
        ));
    }

    #[test]
    fn dependency_on_independent_item_is_acyclic() {
        let items = [item(1), item(2)];
        assert!(!dependency_creates_cycle(
            &items,
            &ItemId::new("T", 1),
            &ItemId::new("T", 2)
        ));
    }

    #[test]
    fn dependency_back_edge_is_a_cycle() {
        // 2 depends on 1. If you add "depends on 2" to 1, it will cycle as 1↔2.
        let items = [item(1), item_with_deps(2, &[1])];
        assert!(dependency_creates_cycle(
            &items,
            &ItemId::new("T", 1),
            &ItemId::new("T", 2)
        ));
    }

    #[test]
    fn dependency_transitive_back_edge_is_a_cycle() {
        // 3→2→1 dependent chain. Adding “dependence on 3” to 1 creates a cycle.
        let items = [item(1), item_with_deps(2, &[1]), item_with_deps(3, &[2])];
        assert!(dependency_creates_cycle(
            &items,
            &ItemId::new("T", 1),
            &ItemId::new("T", 3)
        ));
    }

    #[test]
    fn dependency_shared_diamond_is_acyclic() {
        // 2→1, 3→1, 4 depends on 2 and 3 (diamond). Adding 4→1 does not cycle.
        let items = [
            item(1),
            item_with_deps(2, &[1]),
            item_with_deps(3, &[1]),
            item_with_deps(4, &[2, 3]),
        ];
        assert!(!dependency_creates_cycle(
            &items,
            &ItemId::new("T", 4),
            &ItemId::new("T", 1)
        ));
    }
}
