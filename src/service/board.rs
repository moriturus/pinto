//! Board display service: group PBIs by workflow column.

use super::{LabelMatch, apply_effective_points, open_board};
use crate::backlog::{BacklogItem, Status, Workflow};
use crate::error::{Error, Result};
use crate::storage::BacklogItemRepository;
use std::path::Path;

/// Sort key used within a board column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// Backlog rank order (ascending fractional index).
    Rank,
    /// Completion time (`done_at`). Incomplete items always remain at the end.
    Done,
    /// Creation time.
    Created,
}

/// Filtering, scope, and sorting options for [`board`]. The default includes all PBIs and columns.
#[derive(Debug, Default, Clone)]
pub struct BoardQuery {
    /// Include only PBIs whose persisted parent link is unset.
    pub roots_only: bool,
    /// Sprint scope. When set, include only PBIs assigned to this sprint.
    pub sprint: Option<String>,
    /// Labels to match. An empty list does not filter by label.
    pub labels: Vec<String>,
    /// Matching mode for [`Self::labels`].
    pub label_match: LabelMatch,
    /// Columns to display. An empty list includes all columns. Every supplied name must exist in
    /// `config.toml`; otherwise return [`Error::UnknownStatus`].
    pub statuses: Vec<String>,
    /// Sort key within each column. `None` uses `done_at` descending order for the completion
    /// column and rank ascending order elsewhere. `Some` applies the key to every column.
    pub sort: Option<SortKey>,
    /// Reverse the selected sort key; valid only when `sort` is `Some`.
    pub reverse: bool,
    /// Search the item's fields and assigned sprint metadata.
    pub search: Option<super::SearchFilter>,
}

/// One board column with its status and PBIs in ascending rank order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardColumn {
    /// Workflow status (the column name in `config.toml`).
    pub status: Status,
    /// PBIs belonging to this column, in ascending rank order.
    pub items: Vec<BacklogItem>,
}

/// Board data with columns ordered according to `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    /// Columns in workflow order, from left to right.
    pub columns: Vec<BoardColumn>,
    /// PBIs whose status is not present in `config.toml`, in ascending rank order.
    ///
    /// These items remain visible so users can detect and repair statuses after a column is deleted
    /// or renamed; they are reported separately instead of being dropped.
    pub orphaned: Vec<BacklogItem>,
}

/// Load the board in `project_dir`, grouping PBIs in the column order from `config.toml`.
///
/// PBIs in each known column use ascending rank order, matching `list`. Items whose statuses do not
/// match a configured column are collected in [`Board::orphaned`] rather than discarded.
///
/// You can narrow down the display with [`BoardQuery`]:
/// - If `roots_only` is set, include only PBIs whose persisted parent link is unset. This is
///   evaluated before the other filters, so a child is not promoted when its parent is hidden.
/// - If `sprint` is set, include only PBIs assigned to that sprint. Filters apply to both sorting
///   and orphan detection.
/// - If `labels` is specified, only PBIs matching the requested labels will be targeted. The
///   default [`LabelMatch::Any`] mode is OR; [`LabelMatch::All`] is AND.
/// - If `statuses` is set, display only those known columns in their configured order. Unknown
///   names return [`Error::UnknownStatus`]. Orphaned PBIs are omitted when a subset is requested.
///
/// [`Error::NotInitialized`] if the board is uninitialized.
pub async fn board(project_dir: &Path, query: &BoardQuery) -> Result<Board> {
    let (_board_dir, repo, config) = open_board(project_dir).await?;
    let mut items = repo.list().await?; // Already in canonical rank order.
    apply_effective_points(
        &mut items,
        config.points.aggregate_children,
        &Status::new(&config.done_column),
    );
    if query.roots_only {
        items.retain(|item| item.parent.is_none());
    }
    let sprints = if query.search.is_some() {
        crate::storage::SprintRepository::list(&repo).await?
    } else {
        Vec::new()
    };

    // Only configured columns are valid filter values.
    for name in &query.statuses {
        if !config.columns.iter().any(|c| c == name) {
            return Err(Error::UnknownStatus(name.clone()));
        }
    }
    // Select columns in configured order; an empty filter keeps all columns.
    let shown: Vec<&String> = if query.statuses.is_empty() {
        config.columns.iter().collect()
    } else {
        config
            .columns
            .iter()
            .filter(|c| query.statuses.iter().any(|s| &s == c))
            .collect()
    };

    // Apply the sprint scope before the remaining filters.
    if let Some(sprint) = &query.sprint {
        items.retain(|it| it.sprint.as_deref() == Some(sprint.as_str()));
    }
    if !query.labels.is_empty() {
        items.retain(|it| query.label_match.matches(&it.labels, &query.labels));
    }
    if let Some(search) = &query.search {
        items.retain(|item| {
            let sprint = item
                .sprint
                .as_deref()
                .and_then(|id| sprints.iter().find(|sprint| sprint.id.as_str() == id));
            search.matches(item, sprint)
        });
    }

    let workflow = Workflow::new(config.columns.iter().map(Status::new));
    // By default, sort the configured completion column by `done_at` descending and leave other
    // columns in rank order. The choice is explicit in `config.done_column`, not positional.
    let terminal = Status::new(&config.done_column);

    let columns = shown
        .iter()
        .map(|name| {
            let status = Status::new(*name);
            let mut col_items: Vec<BacklogItem> = items
                .iter()
                .filter(|it| it.status == status)
                .cloned()
                .collect();
            match query.sort {
                // An explicit key applies uniformly to all columns.
                Some(key) => sort_items(&mut col_items, key, query.reverse),
                // The completion column is newest-first; other columns keep repository order.
                None if terminal == status => {
                    sort_items(&mut col_items, SortKey::Done, true);
                }
                None => {}
            }
            // Group into parent/child priority order, keeping the chosen sort as
            // the root/sibling order (same canonical order as `list`, per column).
            BoardColumn {
                status,
                items: crate::service::hierarchical(col_items),
            }
        })
        .collect();

    // Keep orphaned items only for an unfiltered board; a column subset intentionally hides them.
    let orphaned = if query.statuses.is_empty() {
        let orphans: Vec<BacklogItem> = items
            .into_iter()
            .filter(|it| !workflow.contains(&it.status))
            .collect();
        crate::service::hierarchical(orphans)
    } else {
        Vec::new()
    };

    Ok(Board { columns, orphaned })
}

/// Stable sorting of items in a column by `key` (optionally `reverse` in descending order).
///
/// The stable sort preserves input order (canonical rank order) for ties. For `Done`, items without
/// `done_at` always remain at the end because they have no completion time to compare.
fn sort_items(items: &mut [BacklogItem], key: SortKey, reverse: bool) {
    use std::cmp::Ordering;
    let flip = |o: Ordering| if reverse { o.reverse() } else { o };
    match key {
        // Reuse the canonical backlog order so an explicit `--sort rank` matches `list` exactly (ID tie-break included).
        SortKey::Rank => items.sort_by(|a, b| flip(a.backlog_cmp(b))),
        SortKey::Created => items.sort_by(|a, b| flip(a.created.cmp(&b.created))),
        SortKey::Done => items.sort_by(|a, b| match (a.done_at, b.done_at) {
            (Some(x), Some(y)) => flip(x.cmp(&y)), // Ascending by default; descending with reverse.
            (Some(_), None) => Ordering::Less, // Completed items precede unset ones in either direction.
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal, // Stable sort preserves ascending rank order.
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::service::test_support::{init_temp, set_columns};
    use crate::service::{NewItem, add_item, move_item};
    use tempfile::TempDir;

    #[tokio::test]
    async fn board_groups_items_by_column_in_config_order() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        // Move B to in-progress (A remains todo).
        move_item(dir.path(), &b.id, "in-progress").await.unwrap();

        let board = board(dir.path(), &BoardQuery::default())
            .await
            .expect("board succeeds");

        // Columns are in config.toml order (default: todo → in-progress → review → done).
        let names: Vec<_> = board
            .columns
            .iter()
            .map(|c| c.status.as_str().to_string())
            .collect();
        assert_eq!(names, ["todo", "in-progress", "review", "done"]);

        // The PBI to which it belongs is assigned to each column.
        assert_eq!(
            board.columns[0]
                .items
                .iter()
                .map(|it| &it.id)
                .collect::<Vec<_>>(),
            [&a.id]
        );
        assert_eq!(
            board.columns[1]
                .items
                .iter()
                .map(|it| &it.id)
                .collect::<Vec<_>>(),
            [&b.id]
        );
        assert!(board.columns[2].items.is_empty());
        assert!(board.columns[3].items.is_empty());
    }

    #[tokio::test]
    async fn board_items_within_column_are_rank_ordered() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();

        let board = board(dir.path(), &BoardQuery::default()).await.unwrap();
        let ids: Vec<_> = board.columns[0]
            .items
            .iter()
            .map(|it| it.id.clone())
            .collect();
        assert_eq!(ids, [a.id, b.id, c.id]);
    }

    #[tokio::test]
    async fn board_on_uninitialized_dir_prompts_init() {
        let dir = TempDir::new().expect("temp dir");
        let err = board(dir.path(), &BoardQuery::default()).await.unwrap_err();
        assert!(
            matches!(err, Error::NotInitialized { .. }),
            "expected NotInitialized, got {err:?}"
        );
    }

    #[tokio::test]
    async fn board_reflects_custom_column_order_and_additions() {
        let dir = init_temp().await;
        // Users define their own workflows (sort + add).
        set_columns(dir.path(), &["backlog", "doing", "done", "blocked"]).await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        // You can transition to the added column `blocked` (reflected in move).
        move_item(dir.path(), &a.id, "blocked").await.unwrap();

        let board = board(dir.path(), &BoardQuery::default())
            .await
            .expect("board succeeds");

        let names: Vec<_> = board
            .columns
            .iter()
            .map(|c| c.status.as_str().to_string())
            .collect();
        assert_eq!(names, ["backlog", "doing", "done", "blocked"]);
        // The default state is the first row (backlog), and after a move it belongs to blocked.
        assert_eq!(board.columns[0].items, []);
        assert_eq!(
            board.columns[3]
                .items
                .iter()
                .map(|it| &it.id)
                .collect::<Vec<_>>(),
            [&a.id]
        );
        assert!(board.orphaned.is_empty(), "no undefined-column items");
    }

    #[tokio::test]
    async fn move_to_newly_added_column_succeeds() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        set_columns(dir.path(), &["todo", "in-progress", "review", "done", "qa"]).await;

        let moved = move_item(dir.path(), &a.id, "qa")
            .await
            .expect("move to added column succeeds");
        assert_eq!(moved.status, Status::new("qa"));
    }

    #[tokio::test]
    async fn board_collects_items_in_undefined_columns_as_orphaned() {
        let dir = init_temp().await;
        let stay = add_item(dir.path(), "Stay", NewItem::default())
            .await
            .unwrap();
        let orphan = add_item(dir.path(), "Orphan", NewItem::default())
            .await
            .unwrap();
        move_item(dir.path(), &orphan.id, "review").await.unwrap();

        // User edits config and deletes `review` column → orphan refers to undefined column.
        set_columns(dir.path(), &["todo", "in-progress", "done"]).await;

        let board = board(dir.path(), &BoardQuery::default())
            .await
            .expect("board succeeds");

        // Only PBIs remaining in the known column (orphans do not appear in the column).
        let in_columns: Vec<_> = board
            .columns
            .iter()
            .flat_map(|c| &c.items)
            .map(|it| it.id.clone())
            .collect();
        assert_eq!(in_columns, [stay.id]);

        // Orphans are detected as orphaned (retaining their original undefined state).
        assert_eq!(
            board.orphaned.iter().map(|it| &it.id).collect::<Vec<_>>(),
            [&orphan.id]
        );
        assert_eq!(board.orphaned[0].status, Status::new("review"));
    }

    #[tokio::test]
    async fn board_scoped_to_sprint_shows_only_assigned_items() {
        use crate::service::{assign_sprint, create_sprint};
        use crate::sprint::SprintId;

        let dir = init_temp().await;
        let sprint = SprintId::new("S-1").unwrap();
        create_sprint(dir.path(), &sprint, "Sprint 1", None, None)
            .await
            .unwrap();
        let in_sprint = add_item(dir.path(), "In sprint", NewItem::default())
            .await
            .unwrap();
        let _out = add_item(dir.path(), "Not in sprint", NewItem::default())
            .await
            .unwrap();
        assign_sprint(dir.path(), &sprint, &in_sprint.id)
            .await
            .unwrap();

        let board = board(
            dir.path(),
            &BoardQuery {
                sprint: Some("S-1".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("scoped board succeeds");

        // Only PBIs belonging to Sprint (unassigned will not appear).
        let ids: Vec<_> = board
            .columns
            .iter()
            .flat_map(|c| &c.items)
            .map(|it| it.id.clone())
            .collect();
        assert_eq!(ids, [in_sprint.id]);
    }

    #[tokio::test]
    async fn board_filters_by_label() {
        let dir = init_temp().await;
        let backend = NewItem {
            labels: vec!["backend".to_string()],
            ..Default::default()
        };
        let matching = add_item(dir.path(), "Backend item", backend).await.unwrap();
        let frontend = NewItem {
            labels: vec!["frontend".to_string()],
            ..Default::default()
        };
        add_item(dir.path(), "Frontend item", frontend)
            .await
            .unwrap();

        let board = board(
            dir.path(),
            &BoardQuery {
                labels: vec!["backend".to_string()],
                ..Default::default()
            },
        )
        .await
        .expect("label-scoped board succeeds");

        let ids: Vec<_> = board
            .columns
            .iter()
            .flat_map(|column| &column.items)
            .map(|item| item.id.clone())
            .collect();
        assert_eq!(ids, [matching.id]);
    }

    #[tokio::test]
    async fn board_filters_by_multiple_labels_with_any_or_all_matching() {
        let dir = init_temp().await;
        let backend = add_item(
            dir.path(),
            "Backend item",
            NewItem {
                labels: vec!["backend".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let frontend = add_item(
            dir.path(),
            "Frontend item",
            NewItem {
                labels: vec!["frontend".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let both = add_item(
            dir.path(),
            "Both labels",
            NewItem {
                labels: vec!["backend".to_string(), "frontend".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let labels = vec!["backend".to_string(), "frontend".to_string()];

        let any = board(
            dir.path(),
            &BoardQuery {
                labels: labels.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let any_ids: Vec<_> = any
            .columns
            .iter()
            .flat_map(|column| &column.items)
            .map(|item| item.id.clone())
            .collect();
        assert_eq!(
            any_ids,
            [backend.id.clone(), frontend.id.clone(), both.id.clone()]
        );

        let all = board(
            dir.path(),
            &BoardQuery {
                labels,
                label_match: LabelMatch::All,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let all_ids: Vec<_> = all
            .columns
            .iter()
            .flat_map(|column| &column.items)
            .map(|item| item.id.clone())
            .collect();
        assert_eq!(all_ids, [both.id]);
    }

    // --- Column filter ---

    #[tokio::test]
    async fn board_status_filter_shows_only_requested_columns_in_config_order() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        move_item(dir.path(), &b.id, "done").await.unwrap();

        // Even if the specified order is reversed, they are arranged in config order (todo → done).
        let query = BoardQuery {
            statuses: vec!["done".to_string(), "todo".to_string()],
            ..Default::default()
        };
        let board = board(dir.path(), &query).await.expect("board succeeds");

        let names: Vec<_> = board
            .columns
            .iter()
            .map(|c| c.status.as_str().to_string())
            .collect();
        assert_eq!(
            names,
            ["todo", "done"],
            "only requested columns, config order"
        );
        assert_eq!(
            board.columns[0]
                .items
                .iter()
                .map(|i| &i.id)
                .collect::<Vec<_>>(),
            [&a.id]
        );
        assert_eq!(
            board.columns[1]
                .items
                .iter()
                .map(|i| &i.id)
                .collect::<Vec<_>>(),
            [&b.id]
        );
    }

    #[tokio::test]
    async fn board_status_filter_rejects_unknown_column() {
        let dir = init_temp().await;
        let query = BoardQuery {
            statuses: vec!["nonexistent".to_string()],
            ..Default::default()
        };
        let err = board(dir.path(), &query).await.unwrap_err();
        assert!(
            matches!(err, Error::UnknownStatus(ref s) if s == "nonexistent"),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn board_status_filter_combines_with_sprint_scope() {
        use crate::service::{assign_sprint, create_sprint};
        use crate::sprint::SprintId;

        let dir = init_temp().await;
        let sprint = SprintId::new("S-1").unwrap();
        create_sprint(dir.path(), &sprint, "Sprint", None, None)
            .await
            .unwrap();
        let a = add_item(dir.path(), "In sprint todo", NewItem::default())
            .await
            .unwrap();
        let b = add_item(dir.path(), "In sprint done", NewItem::default())
            .await
            .unwrap();
        let c = add_item(dir.path(), "Off sprint todo", NewItem::default())
            .await
            .unwrap();
        assign_sprint(dir.path(), &sprint, &a.id).await.unwrap();
        assign_sprint(dir.path(), &sprint, &b.id).await.unwrap();
        move_item(dir.path(), &b.id, "done").await.unwrap();

        // sprint scope + todo column only → a only (b is done, c is outside sprint).
        let query = BoardQuery {
            sprint: Some("S-1".to_string()),
            statuses: vec!["todo".to_string()],
            ..Default::default()
        };
        let board = board(dir.path(), &query).await.expect("board succeeds");

        assert_eq!(board.columns.len(), 1);
        assert_eq!(board.columns[0].status.as_str(), "todo");
        let ids: Vec<_> = board.columns[0]
            .items
            .iter()
            .map(|i| i.id.clone())
            .collect();
        assert_eq!(ids, [a.id]);
        let _ = c;
    }

    #[tokio::test]
    async fn board_status_filter_suppresses_orphaned() {
        let dir = init_temp().await;
        let orphan = add_item(dir.path(), "Orphan", NewItem::default())
            .await
            .unwrap();
        move_item(dir.path(), &orphan.id, "review").await.unwrap();
        // Delete review column → orphan refers to undefined column.
        set_columns(dir.path(), &["todo", "in-progress", "done"]).await;

        let query = BoardQuery {
            statuses: vec!["todo".to_string()],
            ..Default::default()
        };
        let board = board(dir.path(), &query).await.expect("board succeeds");

        assert!(
            board.orphaned.is_empty(),
            "orphaned suppressed when filtering columns"
        );
    }

    // --- Completion order display ---

    /// Overwrite `done_at` with any value (deterministic setup for completion order testing).
    async fn set_done_at(dir: &Path, id: &crate::backlog::ItemId, done_at: Option<i64>) {
        use crate::storage::{BacklogItemRepository, FileRepository};
        let repo = FileRepository::new(dir.join(".pinto"));
        let mut item = repo.load(id).await.expect("load");
        item.done_at = done_at.map(|s| chrono::DateTime::from_timestamp(s, 0).expect("ts"));
        repo.save(&item).await.expect("save");
    }

    #[tokio::test]
    async fn board_done_column_orders_by_completion_desc_and_undated_last() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();
        let d = add_item(dir.path(), "D", NewItem::default()).await.unwrap();
        let e = add_item(dir.path(), "E", NewItem::default()).await.unwrap();
        for it in [&a, &b, &c, &d, &e] {
            move_item(dir.path(), &it.id, "done").await.unwrap();
        }
        // Give a definitive completion time (d and e are not set = last, addition order = maintain rank order).
        set_done_at(dir.path(), &a.id, Some(100)).await;
        set_done_at(dir.path(), &b.id, Some(300)).await;
        set_done_at(dir.path(), &c.id, Some(200)).await;
        set_done_at(dir.path(), &d.id, None).await;
        set_done_at(dir.path(), &e.id, None).await;

        let board = board(dir.path(), &BoardQuery::default())
            .await
            .expect("board succeeds");
        let done = board
            .columns
            .iter()
            .find(|c| c.status.as_str() == "done")
            .expect("done column");
        let ids: Vec<_> = done.items.iter().map(|i| i.id.clone()).collect();

        // done_at Descending order (b=300, c=200, a=100) → If not set, go to the end rank Stable arrangement in ascending order (d, e).
        assert_eq!(ids, [b.id, c.id, a.id, d.id, e.id]);
    }

    #[tokio::test]
    async fn board_non_done_columns_remain_rank_ordered() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();
        for it in [&a, &b, &c] {
            move_item(dir.path(), &it.id, "in-progress").await.unwrap();
        }

        let board = board(dir.path(), &BoardQuery::default())
            .await
            .expect("board succeeds");
        let wip = board
            .columns
            .iter()
            .find(|c| c.status.as_str() == "in-progress")
            .expect("in-progress column");
        let ids: Vec<_> = wip.items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(
            ids,
            [a.id, b.id, c.id],
            "non-terminal columns keep rank order"
        );
    }

    #[tokio::test]
    async fn board_done_column_setting_is_position_independent() {
        let dir = init_temp().await;
        // Move the completion column `done` to a location other than the end. done_column (default "done") follows.
        set_columns(dir.path(), &["todo", "done", "archived"]).await;

        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();
        for it in [&a, &b, &c] {
            move_item(dir.path(), &it.id, "done").await.unwrap();
        }
        set_done_at(dir.path(), &a.id, Some(100)).await;
        set_done_at(dir.path(), &b.id, Some(300)).await;
        set_done_at(dir.path(), &c.id, Some(200)).await;

        let board = board(dir.path(), &BoardQuery::default())
            .await
            .expect("board succeeds");
        let done = board
            .columns
            .iter()
            .find(|c| c.status.as_str() == "done")
            .expect("done column");
        let ids: Vec<_> = done.items.iter().map(|i| i.id.clone()).collect();
        // Even if it is not at the end, it is sorted in descending order by done_at (determined by done_column, not position).
        assert_eq!(ids, [b.id, c.id, a.id]);
    }

    // --- Sort selection ---

    #[tokio::test]
    async fn board_sort_created_applies_to_all_columns() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();
        // Definitively override created (a=300, b=100, c=200). Everyone remains in todo.
        set_created(dir.path(), &a.id, 300).await;
        set_created(dir.path(), &b.id, 100).await;
        set_created(dir.path(), &c.id, 200).await;

        // Ascending (default): b(100), c(200), a(300).
        let asc = BoardQuery {
            sort: Some(SortKey::Created),
            ..Default::default()
        };
        let asc_board = board(dir.path(), &asc).await.unwrap();
        assert_eq!(
            asc_board.columns[0]
                .items
                .iter()
                .map(|i| i.id.clone())
                .collect::<Vec<_>>(),
            [b.id.clone(), c.id.clone(), a.id.clone()]
        );

        // Descending (--reverse): a(300), c(200), b(100).
        let desc = BoardQuery {
            sort: Some(SortKey::Created),
            reverse: true,
            ..Default::default()
        };
        let desc_board = board(dir.path(), &desc).await.unwrap();
        assert_eq!(
            desc_board.columns[0]
                .items
                .iter()
                .map(|i| i.id.clone())
                .collect::<Vec<_>>(),
            [a.id, c.id, b.id]
        );
    }

    #[tokio::test]
    async fn board_sort_rank_overrides_default_done_ordering() {
        let dir = init_temp().await;
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();
        for it in [&a, &b, &c] {
            move_item(dir.path(), &it.id, "done").await.unwrap();
        }
        // The completion time is a<b<c, but --sort rank forces ascending rank order (a,b,c).
        set_done_at(dir.path(), &a.id, Some(100)).await;
        set_done_at(dir.path(), &b.id, Some(200)).await;
        set_done_at(dir.path(), &c.id, Some(300)).await;

        let query = BoardQuery {
            sort: Some(SortKey::Rank),
            ..Default::default()
        };
        let board = board(dir.path(), &query).await.unwrap();
        let done = board
            .columns
            .iter()
            .find(|c| c.status.as_str() == "done")
            .unwrap();
        assert_eq!(
            done.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
            [a.id, b.id, c.id],
            "explicit --sort rank overrides the default done ordering"
        );
    }

    /// Overwrite `created` with any value (definitive setup for sorting tests).
    async fn set_created(dir: &Path, id: &crate::backlog::ItemId, created: i64) {
        use crate::storage::{BacklogItemRepository, FileRepository};
        let repo = FileRepository::new(dir.join(".pinto"));
        let mut item = repo.load(id).await.expect("load");
        item.created = chrono::DateTime::from_timestamp(created, 0).expect("ts");
        repo.save(&item).await.expect("save");
    }
}
