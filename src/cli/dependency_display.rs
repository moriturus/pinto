//! Dependency display logic (common to `board` / `kanban`).
//!
//! Compute each card's dependency destinations, sources, and blocked state as pure data
//! ([`DepSummary`]). Provide compact ID formatting ([`format_ids`]) for the callers. Drawing
//! (colors, symbols, and layout) remains in `format.rs` and `kanban.rs`, while this module keeps
//! the semantics shared by `board` and `kanban`.

use pinto::backlog::{BacklogItem, ItemId};
use pinto::i18n::{Localizer, Message};
use pinto::service::Board;
use std::collections::{HashMap, HashSet};

/// Iterate over all items in the board, including items with no configured column.
fn board_items(board: &Board) -> impl Iterator<Item = &BacklogItem> {
    board
        .columns
        .iter()
        .flat_map(|c| c.items.iter())
        .chain(board.orphaned.iter())
}

/// Dependency summary for one card (pure data for drawing).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DepSummary {
    /// IDs of PBIs this card depends on, in rank order.
    pub(crate) depends_on: Vec<ItemId>,
    /// IDs of PBIs that depend on this card, in board rank order.
    pub(crate) dependents: Vec<ItemId>,
    /// Whether at least one dependency is unfinished.
    pub(crate) blocked: bool,
}

impl DepSummary {
    /// Return whether no dependency marker is needed.
    pub(crate) fn is_empty(&self) -> bool {
        self.depends_on.is_empty() && self.dependents.is_empty()
    }
}

/// Board-wide dependency index with reverse lookups and a completion set.
///
/// Build the index once for the entire board (columns and orphaned items), then use
/// [`Self::summary`] for each card.
pub(crate) struct DependencyIndex {
    /// IDs of items that depend on the indexed item.
    dependents: HashMap<ItemId, Vec<ItemId>>,
    /// IDs of completed items (`done_at` is set), used to determine blocked cards.
    completed: HashSet<ItemId>,
}

impl DependencyIndex {
    /// Build dependent indexes from the entire board (columns + orphans).
    pub(crate) fn from_board(board: &Board) -> Self {
        let mut dependents: HashMap<ItemId, Vec<ItemId>> = HashMap::new();
        let mut completed = HashSet::new();
        for it in board_items(board) {
            if it.done_at.is_some() {
                completed.insert(it.id.clone());
            }
            for dep in &it.depends_on {
                dependents
                    .entry(dep.clone())
                    .or_default()
                    .push(it.id.clone());
            }
        }
        Self {
            dependents,
            completed,
        }
    }

    /// Return the dependency summary for `item`, including whether it is blocked.
    pub(crate) fn summary(&self, item: &BacklogItem) -> DepSummary {
        DepSummary {
            depends_on: item.depends_on.clone(),
            dependents: self.dependents.get(&item.id).cloned().unwrap_or_default(),
            blocked: item.depends_on.iter().any(|d| !self.completed.contains(d)),
        }
    }

    /// Return whether any item on the board (columns or orphans) needs a dependency marker.
    ///
    /// Base this decision on dependency data rather than searching item titles for marker characters.
    pub(crate) fn any_dependencies(&self, board: &Board) -> bool {
        board_items(board).any(|it| !self.summary(it).is_empty())
    }
}

/// Return the localized dependency-marker legend used by `board` and `kanban`.
pub(crate) fn dependency_legend(localizer: &Localizer) -> String {
    localizer.text(Message::KanbanDependencyLegend)
}

/// Maximum number of IDs shown on a dependency marker; additional IDs are abbreviated as `+N`.
pub(crate) const DEP_ID_LIMIT: usize = 3;

/// Format IDs separated by spaces, abbreviating entries beyond `max` as `+N`.
pub(crate) fn format_ids(ids: &[ItemId], max: usize) -> String {
    if ids.len() <= max {
        ids.iter()
            .map(ItemId::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        let shown = ids[..max]
            .iter()
            .map(ItemId::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        format!("{shown} +{}", ids.len() - max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinto::backlog::{ItemId, Status};
    use pinto::rank::Rank;
    use pinto::service::BoardColumn;

    /// PBI for testing (only ID and title are meaningful).
    fn item(id: &str, title: &str) -> BacklogItem {
        BacklogItem::new(
            id.parse::<ItemId>().unwrap(),
            title.to_string(),
            Status::new("todo"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            chrono::Utc::now(),
        )
        .unwrap()
    }

    /// Test PBI with dependencies (`depends_on`) set.
    fn item_with_deps(id: &str, title: &str, deps: &[&str]) -> BacklogItem {
        let mut it = item(id, title);
        it.depends_on = deps.iter().map(|d| d.parse::<ItemId>().unwrap()).collect();
        it
    }

    /// Completed (`done_at` setting) test PBI.
    fn completed_item(id: &str, title: &str) -> BacklogItem {
        let mut it = item(id, title);
        it.done_at = Some(chrono::Utc::now());
        it
    }

    /// Create a Board from each column (state name, card group) (cards with parent/child/dependency can be passed directly).
    fn board_of(columns: &[(&str, Vec<BacklogItem>)]) -> Board {
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

    fn ids(v: &[ItemId]) -> Vec<String> {
        v.iter().map(ItemId::to_string).collect()
    }

    #[test]
    fn summary_lists_depends_on_and_dependents() {
        // T-2 depends on T-1.
        let board = board_of(&[(
            "todo",
            vec![item("T-1", "a"), item_with_deps("T-2", "b", &["T-1"])],
        )]);
        let idx = DependencyIndex::from_board(&board);
        let items = &board.columns[0].items;
        let s1 = idx.summary(&items[0]); // T-1
        assert_eq!(ids(&s1.dependents), vec!["T-2"], "T-2 depends on T-1");
        assert!(s1.depends_on.is_empty());
        let s2 = idx.summary(&items[1]); // T-2
        assert_eq!(ids(&s2.depends_on), vec!["T-1"], "T-2 depends on T-1");
        assert!(s2.dependents.is_empty());
    }

    #[test]
    fn blocked_when_a_dependency_is_incomplete() {
        let board = board_of(&[(
            "todo",
            vec![item("T-1", "a"), item_with_deps("T-2", "b", &["T-1"])],
        )]);
        let idx = DependencyIndex::from_board(&board);
        assert!(
            idx.summary(&board.columns[0].items[1]).blocked,
            "incomplete T-1 blocks T-2"
        );
    }

    #[test]
    fn not_blocked_when_all_dependencies_complete() {
        let board = board_of(&[
            ("done", vec![completed_item("T-1", "a")]),
            ("todo", vec![item_with_deps("T-2", "b", &["T-1"])]),
        ]);
        let idx = DependencyIndex::from_board(&board);
        assert!(
            !idx.summary(&board.columns[1].items[0]).blocked,
            "completed dependencies do not block the item"
        );
    }

    #[test]
    fn no_dependencies_yields_empty_summary() {
        let board = board_of(&[("todo", vec![item("T-1", "a")])]);
        let idx = DependencyIndex::from_board(&board);
        let s = idx.summary(&board.columns[0].items[0]);
        assert!(s.depends_on.is_empty() && s.dependents.is_empty() && !s.blocked);
        assert!(s.is_empty());
    }

    #[test]
    fn dependents_are_collected_across_the_whole_board() {
        // Both T-2 (todo) and T-3 (done) depend on T-1 → T-1 depends on two sources.
        let board = board_of(&[
            (
                "todo",
                vec![item("T-1", "a"), item_with_deps("T-2", "b", &["T-1"])],
            ),
            ("done", vec![item_with_deps("T-3", "c", &["T-1"])]),
        ]);
        let idx = DependencyIndex::from_board(&board);
        let s = idx.summary(&board.columns[0].items[0]);
        assert_eq!(ids(&s.dependents), vec!["T-2", "T-3"]);
    }

    #[test]
    fn dependency_index_covers_orphaned_items_too() {
        // Also picks up dependencies between orphaned PBIs.
        let board = Board {
            columns: Vec::new(),
            orphaned: vec![item("T-1", "a"), item_with_deps("T-2", "b", &["T-1"])],
        };
        let idx = DependencyIndex::from_board(&board);
        let s1 = idx.summary(&board.orphaned[0]);
        assert_eq!(ids(&s1.dependents), vec!["T-2"]);
        let s2 = idx.summary(&board.orphaned[1]);
        assert!(s2.blocked, "orphaned dependencies still block the item");
    }

    #[test]
    fn any_dependencies_is_data_driven_not_string_driven() {
        // Even if the title has the symbol `⊸`, it is false if there is no dependency.
        let without = board_of(&[("todo", vec![item("T-1", "記号 ⊸ を含むが依存なし")])]);
        let idx = DependencyIndex::from_board(&without);
        assert!(!idx.any_dependencies(&without), "no deps → false");

        let with = board_of(&[(
            "todo",
            vec![item("T-1", "a"), item_with_deps("T-2", "b", &["T-1"])],
        )]);
        let idx = DependencyIndex::from_board(&with);
        assert!(idx.any_dependencies(&with), "deps present → true");
    }

    #[test]
    fn legend_mentions_all_three_markers() {
        use pinto::i18n::localizer_from;

        // board / kanban use the same translation, and the symbols and meanings correspond even in the English fallback.
        let legend = dependency_legend(&localizer_from(Some("fr_FR.UTF-8"), None));
        assert!(legend.contains("⊸ depends on"), "{legend}");
        assert!(legend.contains("⊷ depended on by"), "{legend}");
        assert!(legend.contains("⊸! unresolved dependency"), "{legend}");
    }

    #[test]
    fn format_ids_truncates_beyond_max() {
        let all: Vec<ItemId> = ["T-1", "T-2", "T-3", "T-4", "T-5"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(format_ids(&all, 3), "T-1 T-2 T-3 +2");
        assert_eq!(format_ids(&all[..2], 3), "T-1 T-2");
        assert_eq!(format_ids(&[], 3), "");
    }
}
