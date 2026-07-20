use pinto::backlog::{BacklogItem, ItemId, Status};
use pinto::rank::Rank;
use pinto::service::{Board, BoardColumn};

/// PBI for testing (only ID and title are meaningful).
pub(super) fn item(id: &str, title: &str) -> BacklogItem {
    BacklogItem::new(
        id.parse::<ItemId>().unwrap(),
        title.to_string(),
        Status::new("todo"),
        Rank::between(None, None).expect("open bounds produce a rank"),
        chrono::Utc::now(),
    )
    .unwrap()
}

/// Test PBI with parent ID set.
pub(super) fn item_with_parent(id: &str, title: &str, parent: &str) -> BacklogItem {
    let mut it = item(id, title);
    it.parent = Some(parent.parse::<ItemId>().unwrap());
    it
}

/// Test PBI with dependencies (`depends_on`) set.
pub(super) fn item_with_deps(id: &str, title: &str, deps: &[&str]) -> BacklogItem {
    let mut it = item(id, title);
    it.depends_on = deps.iter().map(|d| d.parse::<ItemId>().unwrap()).collect();
    it
}

/// Test PBI with story points and/or an assignee set.
pub(super) fn item_with_points_assignee(
    id: &str,
    title: &str,
    points: Option<u32>,
    assignee: Option<&str>,
) -> BacklogItem {
    let mut it = item(id, title);
    it.points = points;
    it.assignee = assignee.map(str::to_string);
    it
}

/// Completed (`done_at` setting) test PBI.
pub(super) fn completed_item(id: &str, title: &str) -> BacklogItem {
    let mut it = item(id, title);
    it.done_at = Some(chrono::Utc::now());
    it
}

/// Each element of `columns` is (state name, PBI ID group of that column). The title is generated from the ID.
pub(super) fn board(columns: &[(&str, &[&str])]) -> Board {
    Board {
        columns: columns
            .iter()
            .map(|(name, ids)| BoardColumn {
                status: Status::new(*name),
                items: ids
                    .iter()
                    .map(|id| item(id, &format!("title {id}")))
                    .collect(),
            })
            .collect(),
        orphaned: Vec::new(),
    }
}

/// Create a Board from each column (state name, card group) (cards with parent/child/dependency can be passed directly).
pub(super) fn board_of(columns: &[(&str, Vec<BacklogItem>)]) -> Board {
    Board {
        columns: columns
            .iter()
            .map(|(name, items)| BoardColumn {
                status: Status::new(*name),
                items: items.clone(),
            })
            .collect(),
        orphaned: Vec::new(),
    }
}
