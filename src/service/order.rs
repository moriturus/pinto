//! Canonical hierarchical (parent/child tree) ordering shared by `list`,
//! `board`, `kanban`, and JSON output.
//!
//! Priority is hierarchical: a parent's position dominates its descendants', so
//! Rank orders siblings (roots among themselves and the children of one parent)
//! while the tree structure decides the overall order. Every view flattens the
//! same forest with [`hierarchical_order`] so a child never floats above an
//! unrelated, higher-priority item just because its raw rank happens to be lower.

use crate::backlog::{BacklogItem, ItemId};
use std::collections::HashMap;

/// Parent/child forest over a set of items, classified by membership in that set.
///
/// The input is assumed to already be in canonical backlog order
/// ([`BacklogItem::backlog_cmp`]); this type preserves that order so roots and
/// siblings stay rank-ordered.
pub struct Forest {
    /// Root item indices (no parent, parent outside the set, or self-parent), in input order.
    pub roots: Vec<usize>,
    /// Direct children per item index (by input order); empty slice when none.
    pub children: Vec<Vec<usize>>,
    /// Indices unreachable from any root (parent-link cycles), in input order.
    pub unreachable: Vec<usize>,
}

/// Classify `items` into a [`Forest`] by parent membership within the same set.
///
/// An item is a root when it has no parent, its parent is not part of `items`
/// (e.g. filtered out or living in another column), or it points at itself.
/// Items reachable only through a parent-link cycle land in
/// [`Forest::unreachable`] so nothing is dropped from the view.
#[must_use]
pub fn build_forest(items: &[BacklogItem]) -> Forest {
    let index_of: HashMap<&ItemId, usize> = items
        .iter()
        .enumerate()
        .map(|(i, it)| (&it.id, i))
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        match it.parent.as_ref().and_then(|p| index_of.get(p).copied()) {
            Some(pi) if pi != i => children[pi].push(i),
            _ => roots.push(i),
        }
    }
    // Everything reachable from a root (following children regardless of fold state).
    let mut reachable = vec![false; items.len()];
    let mut stack = roots.clone();
    while let Some(i) = stack.pop() {
        if std::mem::replace(&mut reachable[i], true) {
            continue;
        }
        stack.extend(children[i].iter().copied());
    }
    let unreachable = (0..items.len()).filter(|&i| !reachable[i]).collect();
    Forest {
        roots,
        children,
        unreachable,
    }
}

/// Pre-order traversal of the fully-expanded forest: every root in input order,
/// each parent immediately followed by its subtree (children in input order).
/// Cycle-only items are appended at the end so no item is lost.
///
/// Returns indices into `items`. Callers reorder their own collection by it.
#[must_use]
pub fn hierarchical_order(items: &[BacklogItem]) -> Vec<usize> {
    let forest = build_forest(items);
    let mut out = Vec::with_capacity(items.len());
    let mut emitted = vec![false; items.len()];
    for &root in &forest.roots {
        visit(root, &forest.children, &mut emitted, &mut out);
    }
    // Cycle-only items (no root ancestor): keep them so nothing is dropped.
    for i in forest.unreachable {
        if !emitted[i] {
            emitted[i] = true;
            out.push(i);
        }
    }
    out
}

/// Reorder `items` into canonical hierarchical priority order.
///
/// Convenience wrapper over [`hierarchical_order`] for callers that hold an owned
/// `Vec` (`list`, each `board` column). `items` must already be in canonical
/// backlog order so roots and siblings come out rank-ordered.
pub fn hierarchical(items: Vec<BacklogItem>) -> Vec<BacklogItem> {
    let order = hierarchical_order(&items);
    let mut taken: Vec<Option<BacklogItem>> = items.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| taken[i].take().expect("each index visited exactly once"))
        .collect()
}

/// Depth-first pre-order emit of `i` then its subtree. `emitted` breaks any
/// (invalid) cycle reached through children.
fn visit(i: usize, children: &[Vec<usize>], emitted: &mut [bool], out: &mut Vec<usize>) {
    if std::mem::replace(&mut emitted[i], true) {
        return;
    }
    out.push(i);
    for &c in &children[i] {
        visit(c, children, emitted, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::Status;
    use crate::rank::Rank;

    /// Item with an explicit rank (and optional parent), as a repository hands
    /// it to a view: already sorted into canonical backlog order.
    fn item(id: &str, rank: &str, parent: Option<&str>) -> BacklogItem {
        let mut it = BacklogItem::new(
            id.parse::<ItemId>().unwrap(),
            id.to_string(),
            Status::new("todo"),
            Rank::parse(rank).expect("valid rank"),
            chrono::Utc::now(),
        )
        .unwrap();
        it.parent = parent.map(|p| p.parse::<ItemId>().unwrap());
        it
    }

    fn ordered_ids(items: &[BacklogItem]) -> Vec<String> {
        hierarchical_order(items)
            .into_iter()
            .map(|i| items[i].id.to_string())
            .collect()
    }

    #[test]
    fn subtree_follows_its_parent_below_a_higher_priority_root() {
        // A child whose raw rank sorts above an unrelated higher-priority root
        // must still render below it, grouped under its lower-priority parent.
        //
        // Flat rank order (input): C_a, HIGH, PARENT, C_b
        // Hierarchical order:      HIGH, PARENT, C_a, C_b
        let items = [
            item("A-1", "b", Some("A-4")), // child, but low raw rank
            item("A-2", "c", None),        // higher-priority standalone root
            item("A-4", "d", None),        // lower-priority parent
            item("A-3", "e", Some("A-4")), // child below its parent
        ];
        assert_eq!(
            ordered_ids(&items),
            ["A-2", "A-4", "A-1", "A-3"],
            "the parent's subtree groups under it, below the higher-priority root"
        );
    }

    #[test]
    fn roots_and_siblings_keep_rank_order() {
        let items = [
            item("T-1", "a", None),        // root
            item("T-5", "c", None),        // root
            item("T-2", "g", Some("T-1")), // child of T-1
            item("T-3", "m", Some("T-1")), // child of T-1 (lower rank)
            item("T-4", "t", None),        // root
        ];
        assert_eq!(
            ordered_ids(&items),
            ["T-1", "T-2", "T-3", "T-5", "T-4"],
            "roots in rank order; siblings under a parent in rank order"
        );
    }

    #[test]
    fn cycle_only_items_are_appended_not_dropped() {
        // A ↔ B form a parent cycle with no root ancestor; keep both, at the end.
        let mut a = item("T-1", "a", Some("T-2"));
        let b = item("T-2", "b", Some("T-1"));
        a.parent = Some("T-2".parse().unwrap());
        let items = [a, b];
        let ids = ordered_ids(&items);
        assert_eq!(ids.len(), 2, "no item dropped");
        assert!(ids.contains(&"T-1".to_string()) && ids.contains(&"T-2".to_string()));
    }
}
