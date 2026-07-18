//! Read-only selection of backlog items that are ready to start.

use crate::backlog::{BacklogItem, ItemId, Status};
use crate::error::{Error, Result};
use crate::storage::BacklogItemRepository;
use std::collections::HashMap;
use std::path::Path;

/// Filters for [`next_items`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextFilter {
    /// Maximum number of actionable PBIs to return.
    pub count: usize,
    /// Optional exact Sprint ID filter.
    pub sprint: Option<String>,
}

/// Return the highest-ranked PBIs that are ready to start without modifying the board.
pub async fn next_items(project_dir: &Path, filter: &NextFilter) -> Result<Vec<BacklogItem>> {
    let (board_dir, repo, config) = super::open_board(project_dir).await?;
    let config_path = board_dir.join("config.toml");
    let first_status = config
        .columns
        .first()
        .map(Status::new)
        .ok_or_else(|| Error::parse(&config_path, "board config has no columns"))?;
    let done_status = Status::new(&config.done_column);
    let mut items = repo.list().await?;
    super::apply_effective_points(&mut items, config.points.aggregate_children, &done_status);
    Ok(actionable_items(
        &items,
        &first_status,
        &done_status,
        filter.sprint.as_deref(),
        filter.count,
    ))
}

/// Select actionable items from a canonical backlog snapshot.
fn actionable_items(
    items: &[BacklogItem],
    first_status: &Status,
    done_status: &Status,
    sprint: Option<&str>,
    count: usize,
) -> Vec<BacklogItem> {
    if count == 0 {
        return Vec::new();
    }

    let status_by_id: HashMap<&ItemId, &Status> =
        items.iter().map(|item| (&item.id, &item.status)).collect();

    let selected = items
        .iter()
        .filter(|item| item.status == *first_status && item.status != *done_status)
        .filter(|item| sprint.is_none_or(|wanted| item.sprint.as_deref() == Some(wanted)))
        .filter(|item| {
            item.depends_on.iter().all(|dependency| {
                status_by_id
                    .get(dependency)
                    .is_some_and(|status| *status == done_status)
            })
        })
        .cloned()
        .collect::<Vec<_>>();

    // Apply the same parent/child priority used by list and board before limiting the result.
    // Otherwise a child with a lower raw rank could consume the count before its parent is seen.
    super::hierarchical(selected)
        .into_iter()
        .take(count)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::actionable_items;
    use crate::backlog::{BacklogItem, ItemId, Status};
    use crate::rank::Rank;
    use chrono::Utc;

    fn item(id: &str, status: &str, rank: &str) -> BacklogItem {
        BacklogItem::new(
            id.parse::<ItemId>().expect("valid item ID"),
            id,
            Status::new(status),
            Rank::parse(rank).expect("valid rank"),
            Utc::now(),
        )
        .expect("valid item")
    }

    #[test]
    fn selects_ranked_unstarted_items_with_completed_dependencies() {
        let mut blocked = item("T-2", "todo", "b");
        blocked.depends_on.push("T-3".parse().expect("valid ID"));
        let mut ready = item("T-1", "todo", "a");
        ready.depends_on.push("T-4".parse().expect("valid ID"));
        let done = item("T-4", "done", "d");
        let in_progress = item("T-5", "in-progress", "c");
        let items = vec![ready.clone(), blocked, in_progress, done];

        let selected =
            actionable_items(&items, &Status::new("todo"), &Status::new("done"), None, 10);

        assert_eq!(selected, [ready]);
    }

    #[test]
    fn excludes_missing_dependencies_and_completed_items() {
        let mut missing = item("T-1", "todo", "a");
        missing.depends_on.push("T-99".parse().expect("valid ID"));
        let done = item("T-2", "done", "b");
        let started = item("T-3", "review", "c");
        let items = vec![missing, done, started];

        let selected =
            actionable_items(&items, &Status::new("todo"), &Status::new("done"), None, 10);

        assert!(selected.is_empty());
    }

    #[test]
    fn filters_by_sprint_and_limits_in_priority_order() {
        let mut first = item("T-1", "todo", "a");
        first.sprint = Some("S-1".to_string());
        let mut second = item("T-2", "todo", "b");
        second.sprint = Some("S-1".to_string());
        let mut other = item("T-3", "todo", "c");
        other.sprint = Some("S-2".to_string());
        let items = vec![first.clone(), second, other];

        let selected = actionable_items(
            &items,
            &Status::new("todo"),
            &Status::new("done"),
            Some("S-1"),
            1,
        );

        assert_eq!(selected, [first]);
    }

    #[test]
    fn applies_hierarchical_priority_before_limiting_candidates() {
        let mut child = item("T-2", "todo", "a");
        child.parent = Some("T-1".parse().expect("valid ID"));
        let sibling = item("T-3", "todo", "c");
        let parent = item("T-1", "todo", "b");
        // Repository order is raw rank order; canonical order must put the parent before the
        // sibling and its child before applying a count limit.
        let items = vec![child, parent.clone(), sibling];

        let selected =
            actionable_items(&items, &Status::new("todo"), &Status::new("done"), None, 1);

        assert_eq!(selected[0].id, parent.id);
    }
}
