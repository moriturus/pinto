//! Row layout for the Kanban view: flatten the parent/child tree into visible rows,
//! and the popup content model.

use pinto::backlog::{AcceptanceCriteriaProgress, BacklogItem, ItemId};
use pinto::service::{Board, Forest, build_forest};
use std::collections::HashSet;

/// 1 visible row in the column (result of flattening the parent-child tree according to the collapse).
///
/// `item_index` is the subscript to the column `items` (in ascending rank order). `depth` is the indent level (0=root).
/// Drawing and navigation treat this sequence as the only truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisplayRow {
    /// Subscript within column `items`.
    pub(crate) item_index: usize,
    /// Indentation depth (0=root, 1=child,...).
    pub(crate) depth: usize,
    /// Number of direct children in the same column (used as count hint when collapsing).
    pub(crate) child_count: usize,
    /// Is it currently being expanded (true if it has children and is included in the `expanded` set).
    pub(crate) expanded: bool,
    /// `item_index` of the parent in the same column (root is `None`). Used to determine siblings.
    pub(crate) parent_index: Option<usize>,
}

/// Flatten the item column (in ascending order of rank) as a parent-child tree and return the visible rows.
///
/// - Arrange the roots (no parent in the same column = no parent/parent in a different column) in rank order.
/// - Recursively insert children directly under the parent included in `expanded` in rank order.
/// - Items that cannot be traced from any route even if parent-child links are (incorrectly) circulated
///   Pick it up as a root at the end and don't miss it (the display won't collapse).
pub(crate) fn column_display_rows(
    items: &[BacklogItem],
    expanded: &HashSet<ItemId>,
) -> Vec<DisplayRow> {
    // Share the exact classification (`roots` / `children` / cycle handling) with
    // `list` and `board` via the service `Forest`, so every view agrees on order.
    // This layout only adds fold-aware visibility and indentation on top.
    let forest = build_forest(items);
    let mut out = Vec::new();
    let mut emitted = HashSet::new();
    for &r in &forest.roots {
        visit(r, items, &forest, expanded, &mut emitted, &mut out);
    }
    // Pick up items that cannot be reached from any route (such as circulating parent links).
    for &i in &forest.unreachable {
        if emitted.insert(i) {
            out.push(DisplayRow {
                item_index: i,
                depth: 0,
                child_count: forest.children[i].len(),
                expanded: false,
                parent_index: None,
            });
        }
    }
    out
}

/// DFS body of `column_display_rows`, rooted at `root` (depth 0, no parent).
///
/// Uses an explicit heap stack so a chain thousands of levels deep cannot
/// overflow the native call stack. Only expanded parents dive into their
/// children; `emitted` breaks any (invalid) cycle reached through children.
fn visit(
    root: usize,
    items: &[BacklogItem],
    forest: &Forest,
    expanded: &HashSet<ItemId>,
    emitted: &mut HashSet<usize>,
    out: &mut Vec<DisplayRow>,
) {
    let mut stack = vec![(root, 0_usize, None)];
    while let Some((i, depth, parent_index)) = stack.pop() {
        if !emitted.insert(i) {
            continue;
        }
        let kids = &forest.children[i];
        let child_count = kids.len();
        let is_expanded = child_count > 0 && expanded.contains(&items[i].id);
        out.push(DisplayRow {
            item_index: i,
            depth,
            child_count,
            expanded: is_expanded,
            parent_index,
        });
        if is_expanded {
            // Push children in reverse so they pop — and render — in input order,
            // reproducing the recursive top-to-bottom layout exactly.
            for &c in kids.iter().rev() {
                stack.push((c, depth + 1, Some(i)));
            }
        }
    }
}

/// Iterate over all PBIs in the board, including orphaned items.
pub(crate) fn board_items(board: &Board) -> impl Iterator<Item = &BacklogItem> {
    board
        .columns
        .iter()
        .flat_map(|c| c.items.iter())
        .chain(board.orphaned.iter())
}

/// Pure data for displaying one PBI in the details popup.
///
/// The fields mirror [`crate::cli::format::format_detail`] in `pinto show`. Build them from the
/// already-loaded [`Board`] so the TUI can assemble the popup synchronously without a database
/// query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PopupContent {
    pub(crate) id: ItemId,
    pub(crate) title: String,
    pub(crate) status: pinto::backlog::Status,
    /// Completion of Markdown task-list items in the PBI body.
    pub(crate) acceptance_criteria: AcceptanceCriteriaProgress,
    /// Fractional index rank string (same value stored in the frontmatter).
    pub(crate) rank: pinto::rank::Rank,
    /// 1-based ordinal within the same column in ascending rank order (display only, matches `pinto show`).
    pub(crate) rank_ordinal: usize,
    pub(crate) points: Option<u32>,
    pub(crate) labels: Vec<String>,
    pub(crate) assignee: Option<String>,
    pub(crate) sprint: Option<String>,
    /// Associated Git commits (full SHA).
    pub(crate) commits: Vec<String>,
    /// Body (raw Markdown text).
    pub(crate) body: String,
    /// Parent PBI ID, stringified for display.
    pub(crate) parent: Option<String>,
    /// Child PBIs whose parent is this item, found by scanning the full board.
    pub(crate) children: Vec<ItemId>,
    pub(crate) depends_on: Vec<ItemId>,
    pub(crate) dependents: Vec<ItemId>,
    /// Time when the work first started (`None` if not started).
    pub(crate) start_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Completion time (`None` if not completed).
    pub(crate) done_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) created: chrono::DateTime<chrono::Utc>,
    pub(crate) updated: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinto::backlog::Status;
    use pinto::rank::Rank;

    /// Create a column PBI with an explicit rank and optional parent, as the repository would
    /// provide it to the view: already in canonical backlog order.
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

    /// Visible IDs (with depth) for a canonical-order column under `expanded`.
    fn rows(items: &[BacklogItem], expanded: &[&str]) -> Vec<(String, usize)> {
        let set: HashSet<ItemId> = expanded.iter().map(|e| e.parse().unwrap()).collect();
        column_display_rows(items, &set)
            .into_iter()
            .map(|r| (items[r.item_index].id.to_string(), r.depth))
            .collect()
    }

    #[test]
    fn expanded_child_groups_under_its_parent_ahead_of_a_higher_ranked_root() {
        // A standalone PBI can outrank a child of a higher-ranked parent: here
        // T-2 (rank "m") outranks the child T-3 (rank "z"), yet expanding T-1
        // groups T-3 directly under it — the allowed parent/child layout
        // difference from list/board's flat rank order.
        let items = [
            item("T-1", "a", None),
            item("T-2", "m", None),
            item("T-3", "z", Some("T-1")),
        ];
        assert_eq!(
            rows(&items, &["T-1"]),
            [
                ("T-1".to_string(), 0),
                ("T-3".to_string(), 1),
                ("T-2".to_string(), 0),
            ],
            "child nests under parent even though a later root outranks it"
        );
        // Collapsed: children hidden, roots remain in canonical rank order.
        assert_eq!(
            rows(&items, &[]),
            [("T-1".to_string(), 0), ("T-2".to_string(), 0)]
        );
    }

    #[test]
    fn roots_and_siblings_stay_in_canonical_rank_order() {
        // Contract: tree grouping may move items between roots, but roots among
        // themselves and siblings under one parent must keep canonical order.
        let items = [
            item("T-1", "a", None),        // root
            item("T-5", "c", None),        // root (ranks between the parent's kids)
            item("T-2", "g", Some("T-1")), // child of T-1
            item("T-3", "m", Some("T-1")), // child of T-1 (lower rank than T-2)
            item("T-4", "t", None),        // root
        ];
        assert_eq!(
            rows(&items, &["T-1"]),
            [
                ("T-1".to_string(), 0),
                ("T-2".to_string(), 1), // siblings T-2, T-3 in rank order
                ("T-3".to_string(), 1),
                ("T-5".to_string(), 0), // roots T-1, T-5, T-4 in rank order
                ("T-4".to_string(), 0),
            ]
        );
    }

    #[test]
    fn deep_expanded_chain_flattens_without_a_stack_overflow() {
        // A single parent chain thousands of levels deep, fully expanded, must
        // flatten on an explicit heap stack rather than the native call stack.
        const DEEP: usize = 100_000;
        let ids: Vec<String> = (1..=DEEP).map(|n| format!("T-{n}")).collect();
        let items: Vec<_> = (1..=DEEP)
            .map(|n| item(&ids[n - 1], "a", (n > 1).then(|| ids[n - 2].as_str())))
            .collect();
        let expanded: HashSet<ItemId> = ids.iter().map(|id| id.parse().unwrap()).collect();

        let rows = column_display_rows(&items, &expanded);

        assert_eq!(
            rows.len(),
            DEEP,
            "every item becomes exactly one visible row"
        );
        assert_eq!(rows.first().map(|r| r.depth), Some(0));
        assert_eq!(rows.last().map(|r| r.depth), Some(DEEP - 1));
        assert!(
            rows.iter().enumerate().all(|(i, r)| r.depth == i),
            "each level indents one deeper than the last down the chain"
        );
    }
}
