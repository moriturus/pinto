//! Ordering operations: reorder a single item and rebalance a bloated rank space.

use crate::backlog::{BacklogItem, ItemId, Status};
use crate::error::{Error, Result};
use crate::rank::{Rank, RankStats};
use crate::service::open_board_locked;
use crate::storage::BacklogItemRepository;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Specify the destination of [`reorder_item`] (how to change `rank`).
///
/// All variants move within the item's sibling group (same parent and status); see
/// [`reorder_item`] for the exact semantics and errors.
#[derive(Debug, Clone)]
pub enum ReorderTarget {
    /// Move to just before the given **sibling** (rank decreases to land ahead of it).
    Before(ItemId),
    /// Move to just after the given **sibling** (rank increases to land behind it).
    After(ItemId),
    /// Move to the front of the sibling group (no-op when alone in the group).
    Top,
    /// Move to the back of the sibling group (no-op when alone in the group).
    Bottom,
}

/// Sort the PBI by `id` **within its sibling group** and return the saved [`BacklogItem`].
/// Change only `rank`; keep `status` and `parent`.
///
/// Under the hierarchical display order (see [`crate::service::hierarchical_order`]),
/// `rank` orders siblings and the tree decides overall priority. Reorder therefore
/// operates only inside the sibling group — items sharing the same `parent` **and**
/// the same `status`. Moving a parent implicitly carries its whole subtree, because
/// children stay grouped under it regardless of its rank.
///
/// Adjacent numbering of the fractional index ([`Rank::between`] / [`Rank::before`] /
/// [`Rank::after`]) replaces only this one item without reassigning others.
///
/// - [`ReorderTarget::Before`] / [`ReorderTarget::After`]: move next to the reference,
///   which must be a sibling. [`Error::SelfReference`] if it is `id` itself,
///   [`Error::NotSibling`] if it is not a sibling, [`Error::NotFound`] if it is absent.
/// - [`ReorderTarget::Top`] / [`ReorderTarget::Bottom`]: move to the front/back of the
///   sibling group. A no-op (rank unchanged) when the item is alone in its group,
///   even if other same-status items exist in other groups.
///
/// [`Error::NotInitialized`] if the board is uninitialized, [`Error::NotFound`] if `id` is absent.
pub async fn reorder_item(
    project_dir: &Path,
    id: &ItemId,
    target: ReorderTarget,
) -> Result<BacklogItem> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let items = repo.list().await?; // Ascending rank order.
    let mut item = items
        .iter()
        .find(|it| &it.id == id)
        .cloned()
        .ok_or_else(|| Error::NotFound(id.clone()))?;
    // Sibling group in ascending rank order, excluding the item being moved:
    // same parent and same status (the group that shares one rank axis on the board).
    let siblings: Vec<&BacklogItem> = items
        .iter()
        .filter(|it| &it.id != id && it.parent == item.parent && it.status == item.status)
        .collect();

    let new_rank = match &target {
        ReorderTarget::Before(reference) => {
            if reference == id {
                return Err(Error::SelfReference(id.clone()));
            }
            let pos = sibling_pos(&items, &siblings, id, reference)?;
            let hi = &siblings[pos].rank;
            match pos.checked_sub(1) {
                Some(prev) => Rank::between(Some(&siblings[prev].rank), Some(hi))?,
                None => Rank::before(Some(hi)),
            }
        }
        ReorderTarget::After(reference) => {
            if reference == id {
                return Err(Error::SelfReference(id.clone()));
            }
            let pos = sibling_pos(&items, &siblings, id, reference)?;
            let lo = &siblings[pos].rank;
            match siblings.get(pos + 1) {
                Some(next) => Rank::between(Some(lo), Some(&next.rank))?,
                None => Rank::after(Some(lo)),
            }
        }
        // The first/last neighbours are taken from the sibling group (ascending rank).
        ReorderTarget::Top => match siblings.first().map(|it| &it.rank) {
            Some(first) => Rank::before(Some(first)),
            None => item.rank.clone(),
        },
        ReorderTarget::Bottom => match siblings.last().map(|it| &it.rank) {
            Some(last) => Rank::after(Some(last)),
            None => item.rank.clone(),
        },
    };

    // If the position does not change (e.g. Top/Bottom in the same state), return it as no-op.
    // Avoid changing `updated` or rewriting the file for a no-op.
    if new_rank == item.rank {
        return Ok(item);
    }
    item.rank = new_rank;
    item.updated = Utc::now();
    repo.save(&item).await?;
    repo.commit(&format!("pinto: update {}", item.id)).await?;
    Ok(item)
}

/// Position of `reference` within `siblings` for a Before/After reorder of `id`.
///
/// Distinguishes the two user errors: [`Error::NotFound`] when `reference` is not on
/// the board at all, [`Error::NotSibling`] when it exists but is in another group.
fn sibling_pos(
    items: &[BacklogItem],
    siblings: &[&BacklogItem],
    id: &ItemId,
    reference: &ItemId,
) -> Result<usize> {
    if let Some(pos) = siblings.iter().position(|it| &it.id == reference) {
        return Ok(pos);
    }
    if items.iter().any(|it| &it.id == reference) {
        Err(Error::NotSibling {
            item: id.clone(),
            reference: reference.clone(),
        })
    } else {
        Err(Error::NotFound(reference.clone()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::test_support::init_temp;
    use crate::service::{ListFilter, NewItem, add_item, list_items, move_item};
    use std::path::Path;

    /// Add a PBI, optionally parented, and return it.
    async fn add(dir: &Path, title: &str, parent: Option<&ItemId>) -> BacklogItem {
        let new = NewItem {
            parent: parent.cloned(),
            ..NewItem::default()
        };
        add_item(dir, title, new).await.expect("add succeeds")
    }

    /// IDs of the whole board in hierarchical order.
    async fn order(dir: &Path) -> Vec<String> {
        list_items(dir, &ListFilter::default())
            .await
            .expect("list")
            .into_iter()
            .map(|it| it.id.to_string())
            .collect()
    }

    #[tokio::test]
    async fn top_is_a_noop_for_an_only_child() {
        let dir = init_temp().await;
        let p = add(dir.path(), "P", None).await; // T-1
        let c = add(dir.path(), "C", Some(&p.id)).await; // T-2 (only child)
        let _p2 = add(dir.path(), "P2", None).await; // T-3 (another root)

        // The child is alone in its sibling group, so Top must not change its
        // rank. The previous behavior moved it ahead of unrelated same-status items.
        let after = reorder_item(dir.path(), &c.id, ReorderTarget::Top)
            .await
            .expect("reorder");
        assert_eq!(after.rank, c.rank, "only child: Top is a no-op");
    }

    #[tokio::test]
    async fn bottom_is_a_noop_for_an_only_child() {
        let dir = init_temp().await;
        let p = add(dir.path(), "P", None).await; // T-1
        let c = add(dir.path(), "C", Some(&p.id)).await; // T-2 (only child)
        let _p2 = add(dir.path(), "P2", None).await; // T-3

        let after = reorder_item(dir.path(), &c.id, ReorderTarget::Bottom)
            .await
            .expect("reorder");
        assert_eq!(after.rank, c.rank, "only child: Bottom is a no-op");
    }

    #[tokio::test]
    async fn before_a_non_sibling_is_rejected() {
        let dir = init_temp().await;
        let p = add(dir.path(), "P", None).await; // T-1
        let c = add(dir.path(), "C", Some(&p.id)).await; // T-2 (child of P)
        let r = add(dir.path(), "R", None).await; // T-3 (root, not a sibling of C)

        let err = reorder_item(dir.path(), &c.id, ReorderTarget::Before(r.id.clone()))
            .await
            .expect_err("non-sibling reference must be rejected");
        assert!(
            matches!(err, Error::NotSibling { .. }),
            "expected NotSibling, got {err:?}"
        );
    }

    #[tokio::test]
    async fn before_a_sibling_reorders_within_the_group() {
        let dir = init_temp().await;
        let p = add(dir.path(), "P", None).await; // T-1
        let c1 = add(dir.path(), "C1", Some(&p.id)).await; // T-2
        let _c2 = add(dir.path(), "C2", Some(&p.id)).await; // T-3
        let c3 = add(dir.path(), "C3", Some(&p.id)).await; // T-4

        reorder_item(dir.path(), &c3.id, ReorderTarget::Before(c1.id.clone()))
            .await
            .expect("reorder");
        // Subtree stays under P; C3 leads its siblings.
        assert_eq!(order(dir.path()).await, ["T-1", "T-4", "T-2", "T-3"]);
    }

    #[tokio::test]
    async fn moving_a_parent_carries_its_whole_subtree() {
        let dir = init_temp().await;
        let p1 = add(dir.path(), "P1", None).await; // T-1
        let _c1 = add(dir.path(), "C1", Some(&p1.id)).await; // T-2
        let _c2 = add(dir.path(), "C2", Some(&p1.id)).await; // T-3
        let _p2 = add(dir.path(), "P2", None).await; // T-4

        // Send P1 below its root sibling P2; the children follow their parent.
        reorder_item(dir.path(), &p1.id, ReorderTarget::Bottom)
            .await
            .expect("reorder");
        assert_eq!(order(dir.path()).await, ["T-4", "T-1", "T-2", "T-3"]);
    }

    #[tokio::test]
    async fn siblings_are_grouped_by_parent_even_across_columns() {
        // Two children of P whose parent sits in another column still reorder
        // among themselves: sibling grouping is by `parent`, matching the
        // canonical `list` hierarchy (where P is present as their parent).
        let dir = init_temp().await;
        let p = add(dir.path(), "P", None).await; // T-1
        let c1 = add(dir.path(), "C1", Some(&p.id)).await; // T-2
        let c2 = add(dir.path(), "C2", Some(&p.id)).await; // T-3
        let root = add(dir.path(), "ROOT", None).await; // T-4 (unrelated todo root)
        move_item(dir.path(), &p.id, "in-progress")
            .await
            .expect("move parent to another column");

        // C2 --top reorders only against its sibling C1, not the unrelated root.
        reorder_item(dir.path(), &c2.id, ReorderTarget::Top)
            .await
            .expect("reorder");
        let todo = list_items(
            dir.path(),
            &ListFilter {
                status: vec!["todo".to_string()],
                ..ListFilter::default()
            },
        )
        .await
        .expect("list todo");
        let ids: Vec<String> = todo.iter().map(|it| it.id.to_string()).collect();
        // C2 now precedes C1; ROOT keeps its own position (still a distinct group).
        assert_eq!(
            ids.iter().position(|i| i == &c2.id.to_string()),
            ids.iter()
                .position(|i| i == &c1.id.to_string())
                .map(|p| p - 1),
            "C2 sits immediately before its sibling C1"
        );
        assert!(ids.contains(&root.id.to_string()), "unrelated root remains");

        // Reordering a child relative to the unrelated root is rejected.
        let err = reorder_item(dir.path(), &c2.id, ReorderTarget::After(root.id.clone()))
            .await
            .expect_err("root is not a sibling of C2");
        assert!(matches!(err, Error::NotSibling { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn top_moves_a_child_ahead_of_its_siblings_only() {
        let dir = init_temp().await;
        let p = add(dir.path(), "P", None).await; // T-1
        let _c1 = add(dir.path(), "C1", Some(&p.id)).await; // T-2
        let _c2 = add(dir.path(), "C2", Some(&p.id)).await; // T-3
        let c3 = add(dir.path(), "C3", Some(&p.id)).await; // T-4
        let _r = add(dir.path(), "R", None).await; // T-5 (root; must stay put)

        reorder_item(dir.path(), &c3.id, ReorderTarget::Top)
            .await
            .expect("reorder");
        // C3 leads P's children; P and root R keep their positions.
        assert_eq!(order(dir.path()).await, ["T-1", "T-4", "T-2", "T-3", "T-5"]);
    }
}

/// Result of [`rebalance`]. Indicates the status of resolution of rank bloat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebalanceOutcome {
    /// Total number of PBIs on the board.
    pub total: usize,
    /// Number of PBIs whose rank would be updated (or was updated).
    pub changed: usize,
    /// Rank length statistics before execution.
    pub before: RankStats,
    /// Rank length statistics after execution (=after reallocation).
    pub after: RankStats,
}

/// Reassign oversized or duplicated ranks within each `(status, parent)` sibling scope.
///
/// A scope is rewritten when either its rank length has grown beyond the shortest
/// fixed width for that number of siblings, or it already contains a literal
/// duplicate rank (which a length-only check cannot see, e.g. two `"i"` ranks).
/// Each rewritten scope is reassigned independently from [`Rank::rebalance`], so
/// its ranks take the shortest fixed width for its own size; untouched scopes keep
/// both their ranks and timestamps. If `dry_run` is true, only the planned
/// statistics are returned.
///
/// Rank values are only ever compared within a scope, so two scopes may reuse the
/// same values without harm. The per-scope uniqueness invariant is upheld where a
/// collision would actually be born — [`crate::service::move_item_with_outcome`]
/// re-pegs a transitioned item to the backlog tail if its carried rank would
/// collide in the destination scope — rather than by forcing scopes to be globally
/// disjoint here. A literal duplicate therefore only arises from manual edits, and
/// this is the repair `doctor` points at.
///
/// Only PBIs whose rank changes receive a new `updated` timestamp and are
/// saved. [`Error::NotInitialized`] if the board is uninitialized.
pub async fn rebalance(project_dir: &Path, dry_run: bool) -> Result<RebalanceOutcome> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut items = repo.list().await?; // Ascending rank order.

    let before = RankStats::collect(items.iter().map(|it| &it.rank));
    let mut scopes: HashMap<(Status, Option<ItemId>), Vec<usize>> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        scopes
            .entry((item.status.clone(), item.parent.clone()))
            .or_default()
            .push(index);
    }

    let mut planned = items
        .iter()
        .map(|item| item.rank.clone())
        .collect::<Vec<_>>();

    // Each scope is rebalanced independently: two scopes reusing the same rank
    // values is harmless because ranks are only compared within a scope, and a
    // later `move` re-pegs a transitioned item to the backlog tail should its
    // carried rank collide in the destination. Rewrite a scope when it has outgrown
    // the shortest fixed width for its size, or when it already holds a literal
    // duplicate rank — a length-only check cannot see two equal ranks at the
    // canonical width. The replacement sequence is generated only when a scope is
    // actually rewritten, so untouched scopes cost nothing beyond the width check.
    for indexes in scopes.values() {
        let current_max_len = indexes
            .iter()
            .map(|&index| items[index].rank.as_str().len())
            .max()
            .unwrap_or(0);
        let mut seen = HashSet::with_capacity(indexes.len());
        let has_duplicate = indexes
            .iter()
            .any(|&index| !seen.insert(&items[index].rank));
        if current_max_len <= Rank::rebalance_width(indexes.len()) && !has_duplicate {
            continue;
        }
        for (&index, rank) in indexes.iter().zip(Rank::rebalance(indexes.len())) {
            planned[index] = rank;
        }
    }

    let after = RankStats::collect(planned.iter());

    let now = Utc::now();
    let mut changed = 0;
    for (item, rank) in items.iter_mut().zip(planned) {
        if item.rank == rank {
            continue; // Planned rank already present; avoid needless diffs and I/O.
        }
        changed += 1;
        if !dry_run {
            item.rank = rank;
            item.updated = now;
            repo.save(item).await?;
        }
    }

    if !dry_run && changed > 0 {
        repo.commit("pinto: rebalance").await?;
    }

    Ok(RebalanceOutcome {
        total: items.len(),
        changed,
        before,
        after,
    })
}
