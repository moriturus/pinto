//! PBI-to-PBI link services.

use super::{open_board, open_board_locked};
use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::service::relations::validate_dependencies;
use crate::storage::BacklogItemRepository;
use chrono::Utc;
use std::path::Path;

/// Result of [`add_dependency`], including the updated PBI and any cycle warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyOutcome {
    /// PBI with the updated dependency list.
    pub item: BacklogItem,
    /// Whether the new dependency creates a transitive cycle. Cycles are recorded but reported as
    /// warnings rather than errors.
    pub cycle_warning: bool,
}

/// Add dependency `dep` to the PBI `id` and return [`DependencyOutcome`].
///
/// The operation is idempotent: an existing dependency is not duplicated. Record cycles,
/// including self-dependencies, and set `cycle_warning` to `true`. Return [`Error::NotInitialized`]
/// for an uninitialized board or [`Error::NotFound`] when either ID is absent.
pub async fn add_dependency(
    project_dir: &Path,
    id: &ItemId,
    dep: &ItemId,
) -> Result<DependencyOutcome> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let items = repo.list().await?;

    if !items.iter().any(|it| &it.id == id) {
        return Err(Error::NotFound(id.clone()));
    }
    // Validate the target and determine whether the new edge is a warning-only cycle.
    let cycle_warning = validate_dependencies(&items, id, std::slice::from_ref(dep))?;

    let mut item = repo.load(id).await?;
    if !item.depends_on.contains(dep) {
        item.depends_on.push(dep.clone());
        item.updated = Utc::now();
        repo.save(&item).await?;
        repo.commit(&format!("pinto: update {}", item.id)).await?;
    }
    Ok(DependencyOutcome {
        item,
        cycle_warning,
    })
}

/// Remove dependency `dep` from PBI `id` and return the saved [`BacklogItem`].
///
/// Return [`Error::NotFound`] when `dep` is not present or `id` does not exist, and
/// [`Error::NotInitialized`] for an uninitialized board.
pub async fn remove_dependency(
    project_dir: &Path,
    id: &ItemId,
    dep: &ItemId,
) -> Result<BacklogItem> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut item = repo.load(id).await?;
    let before = item.depends_on.len();
    item.depends_on.retain(|d| d != dep);
    if item.depends_on.len() == before {
        return Err(Error::NotFound(dep.clone()));
    }
    item.updated = Utc::now();
    repo.save(&item).await?;
    repo.commit(&format!("pinto: update {}", item.id)).await?;
    Ok(item)
}

/// Result of [`item_detail`], with bidirectional links in ascending rank order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDetail {
    /// Target PBI; `parent` and `depends_on` are its forward links.
    pub item: BacklogItem,
    /// 1-based ordinal (for display only) among this PBI's siblings in the same
    /// column, in ascending rank order.
    ///
    /// Rank is sibling-local: for a child it counts position among the parent's
    /// children; for a top-level PBI, among the column's top-level PBIs. Dense
    /// fractional index strings are unintuitive, so this gives "Nth under the
    /// parent" / "Nth at top level". It is display-only and never persisted.
    pub rank_ordinal: usize,
    /// IDs of child items parented by this PBI (in ascending rank order).
    pub children: Vec<ItemId>,
    /// IDs of items that depend on this PBI (in ascending rank order).
    pub dependents: Vec<ItemId>,
}

/// Returns the 1-based ordinal of `target` **among its siblings** in the same
/// column, in ascending rank order.
///
/// Rank is sibling-local under the hierarchical ordering: a child is ranked
/// only against the other children of its parent, and a top-level item against
/// the other top-level items in the column. Siblings share the same `parent`
/// (both `None`, or both the same id) and the same `status`. This keeps the
/// displayed "#N" meaningful even though the whole-column position no longer is
/// (a child may render above a lower-numbered top-level item).
fn rank_ordinal(items: &[BacklogItem], target: &BacklogItem) -> usize {
    items
        .iter()
        .filter(|it| {
            it.parent == target.parent && it.status == target.status && it.rank <= target.rank
        })
        .count()
}

/// Load PBI `id` with backward links (children and dependents).
///
/// In addition to the forward `parent` and `depends_on` links, scan all items to find children and
/// dependents. Return [`Error::NotInitialized`] for an uninitialized board or [`Error::NotFound`]
/// when `id` does not exist.
pub async fn item_detail(project_dir: &Path, id: &ItemId) -> Result<ItemDetail> {
    item_detail_from_store(project_dir, id, false).await
}

/// Load archived PBI `id` with bidirectional links.
pub async fn archived_item_detail(project_dir: &Path, id: &ItemId) -> Result<ItemDetail> {
    item_detail_from_store(project_dir, id, true).await
}

async fn item_detail_from_store(
    project_dir: &Path,
    id: &ItemId,
    archived: bool,
) -> Result<ItemDetail> {
    let (_board_dir, repo, config) = open_board(project_dir).await?;
    let mut items = repo.list().await?; // Ascending rank order.
    if archived {
        items.extend(repo.list_archived().await?);
        items.sort_by(BacklogItem::backlog_cmp);
    }
    super::apply_effective_points(
        &mut items,
        config.points.aggregate_children,
        &crate::backlog::Status::new(&config.done_column),
    );
    let item = items
        .iter()
        .find(|it| &it.id == id)
        .cloned()
        .ok_or_else(|| Error::NotFound(id.clone()))?;

    let children = items
        .iter()
        .filter(|it| it.parent.as_ref() == Some(id))
        .map(|it| it.id.clone())
        .collect();
    let dependents = items
        .iter()
        .filter(|it| it.depends_on.contains(id))
        .map(|it| it.id.clone())
        .collect();
    let rank_ordinal = rank_ordinal(&items, &item);

    Ok(ItemDetail {
        item,
        rank_ordinal,
        children,
        dependents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::service::test_support::{init_temp, parent_edit};
    use crate::service::{NewItem, add_item, edit_item};
    use crate::storage::{BacklogItemRepository, FileRepository};

    #[tokio::test]
    async fn add_dependency_sets_and_persists_without_warning() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();

        let outcome = add_dependency(dir.path(), &a.id, &b.id)
            .await
            .expect("add dependency succeeds");
        assert!(!outcome.cycle_warning, "unrelated dependency is acyclic");
        assert_eq!(outcome.item.depends_on, vec![b.id.clone()]);

        let repo = FileRepository::new(dir.path().join(".pinto"));
        assert_eq!(repo.load(&a.id).await.unwrap().depends_on, vec![b.id]);
    }

    #[tokio::test]
    async fn add_dependency_is_idempotent() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        add_dependency(dir.path(), &a.id, &b.id).await.unwrap();

        let outcome = add_dependency(dir.path(), &a.id, &b.id).await.unwrap();
        assert_eq!(outcome.item.depends_on, [b.id], "no duplicate dependency");
    }

    #[tokio::test]
    async fn add_dependency_warns_on_cycle_but_still_records() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        // b depends on a. Here, adding "depends on b" to a creates a cycle (warning).
        add_dependency(dir.path(), &b.id, &a.id).await.unwrap();

        let outcome = add_dependency(dir.path(), &a.id, &b.id).await.unwrap();
        assert!(outcome.cycle_warning, "back-edge should warn");
        assert_eq!(outcome.item.depends_on, [b.id], "recorded despite warning");
    }

    #[tokio::test]
    async fn add_dependency_missing_target_returns_not_found() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let err = add_dependency(dir.path(), &a.id, &ItemId::new("T", 99))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn remove_dependency_drops_edge() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        add_dependency(dir.path(), &a.id, &b.id).await.unwrap();

        let updated = remove_dependency(dir.path(), &a.id, &b.id)
            .await
            .expect("remove dependency succeeds");
        assert!(updated.depends_on.is_empty());
    }

    #[tokio::test]
    async fn remove_dependency_absent_edge_returns_not_found() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        let err = remove_dependency(dir.path(), &a.id, &b.id)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn item_detail_reports_children_and_dependents() {
        let dir = init_temp().await;
        let epic = add_item(dir.path(), "Epic", NewItem::default())
            .await
            .unwrap();
        let s1 = add_item(dir.path(), "Story 1", NewItem::default())
            .await
            .unwrap();
        let s2 = add_item(dir.path(), "Story 2", NewItem::default())
            .await
            .unwrap();
        let other = add_item(dir.path(), "Other", NewItem::default())
            .await
            .unwrap();
        edit_item(dir.path(), &s1.id, parent_edit(Some(epic.id.clone())))
            .await
            .unwrap();
        edit_item(dir.path(), &s2.id, parent_edit(Some(epic.id.clone())))
            .await
            .unwrap();
        // other depends on epic.
        add_dependency(dir.path(), &other.id, &epic.id)
            .await
            .unwrap();

        let detail = item_detail(dir.path(), &epic.id)
            .await
            .expect("detail succeeds");
        assert_eq!(detail.children, [s1.id, s2.id], "children in rank order");
        assert_eq!(detail.dependents, [other.id], "reverse dependency edge");
    }

    #[tokio::test]
    async fn archived_item_detail_reports_links_to_active_items() {
        let dir = init_temp().await;
        let parent = add_item(dir.path(), "Archived parent", NewItem::default())
            .await
            .unwrap();
        let child = add_item(
            dir.path(),
            "Active child",
            NewItem {
                parent: Some(parent.id.clone()),
                ..NewItem::default()
            },
        )
        .await
        .unwrap();
        let dependent = add_item(dir.path(), "Active dependent", NewItem::default())
            .await
            .unwrap();
        add_dependency(dir.path(), &dependent.id, &parent.id)
            .await
            .expect("dependency");
        crate::service::remove_item(dir.path(), &parent.id, false)
            .await
            .expect("archive parent");

        let detail = archived_item_detail(dir.path(), &parent.id)
            .await
            .expect("archived detail");
        assert_eq!(detail.item, parent);
        assert_eq!(detail.children, [child.id]);
        assert_eq!(detail.dependents, [dependent.id]);
    }

    #[tokio::test]
    async fn item_detail_missing_id_returns_not_found() {
        let dir = init_temp().await;
        let err = item_detail(dir.path(), &ItemId::new("T", 99))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
    }

    // --- human-readable ordinal number of rank ---

    #[test]
    fn rank_ordinal_counts_position_within_same_status() {
        use crate::backlog::Status;
        use crate::rank::Rank;

        let mk = |n: u32, status: &str, rank: Rank| {
            BacklogItem::new(
                ItemId::new("T", n),
                "x",
                Status::new(status),
                rank,
                chrono::DateTime::from_timestamp(0, 0).unwrap(),
            )
            .unwrap()
        };
        // rank In ascending order, a < b < c (todo), d is another column (done).
        let a = mk(1, "todo", Rank::parse("a").unwrap());
        let b = mk(2, "todo", Rank::parse("m").unwrap());
        let c = mk(3, "todo", Rank::parse("z").unwrap());
        let d = mk(4, "done", Rank::parse("a").unwrap());
        let items = vec![a.clone(), b.clone(), c.clone(), d.clone()];

        assert_eq!(rank_ordinal(&items, &a), 1);
        assert_eq!(rank_ordinal(&items, &b), 2);
        assert_eq!(rank_ordinal(&items, &c), 3);
        // The beginning of another column (done) is the intra-column ordinal number 1 (unaffected by the number of items in other columns).
        assert_eq!(rank_ordinal(&items, &d), 1);
    }

    #[test]
    fn rank_ordinal_is_sibling_local_for_children() {
        use crate::backlog::Status;
        use crate::rank::Rank;

        let mk = |n: u32, parent: Option<u32>, rank: Rank| {
            let mut it = BacklogItem::new(
                ItemId::new("T", n),
                "x",
                Status::new("todo"),
                rank,
                chrono::DateTime::from_timestamp(0, 0).unwrap(),
            )
            .unwrap();
            it.parent = parent.map(|p| ItemId::new("T", p));
            it
        };
        // Two top-level roots (T-1, T-4) and two children of T-1 (T-2, T-3).
        let p = mk(1, None, Rank::parse("a").unwrap());
        let r2 = mk(4, None, Rank::parse("b").unwrap());
        let c1 = mk(2, Some(1), Rank::parse("m").unwrap());
        let c2 = mk(3, Some(1), Rank::parse("z").unwrap());
        let items = vec![p.clone(), r2.clone(), c1.clone(), c2.clone()];

        // Roots are ranked among roots; children among their siblings — not the
        // whole column, so the number reflects sibling order under the parent.
        assert_eq!(rank_ordinal(&items, &p), 1, "first root");
        assert_eq!(rank_ordinal(&items, &r2), 2, "second root");
        assert_eq!(rank_ordinal(&items, &c1), 1, "first child of T-1");
        assert_eq!(rank_ordinal(&items, &c2), 2, "second child of T-1");
    }

    #[tokio::test]
    async fn item_detail_reports_within_column_rank_ordinal() {
        let dir = init_temp().await;
        let _a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();

        // All todo (addition order = rank ascending order). C is third in column.
        let detail = item_detail(dir.path(), &c.id).await.expect("detail");
        assert_eq!(detail.rank_ordinal, 3);

        // When B is moved to done, it becomes first in that column.
        crate::service::move_item(dir.path(), &b.id, "done")
            .await
            .unwrap();
        let detail = item_detail(dir.path(), &b.id).await.expect("detail");
        assert_eq!(detail.rank_ordinal, 1, "first in the done column");
    }
}
