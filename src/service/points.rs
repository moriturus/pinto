//! Effective story-point calculation for opt-in parent-child aggregation.

use crate::backlog::{BacklogItem, Status};
use std::collections::HashMap;

/// Apply the configured effective points to an in-memory view of PBIs.
pub(crate) fn apply_effective_points(
    items: &mut [BacklogItem],
    aggregate_children: bool,
    done_column: &Status,
) {
    if !aggregate_children {
        return;
    }
    let points = effective_points(items, true, done_column);
    for (item, points) in items.iter_mut().zip(points) {
        item.points = points;
    }
}

/// Calculate the points shown for each PBI without changing persisted data.
///
/// A parent with children uses the recursively calculated contribution of each direct child;
/// this counts leaf estimates once and avoids double-counting a nested parent's own stored value.
/// A completed item contributes no points, while its active descendants remain eligible. An
/// active unestimated leaf, an overflowing sum, or a parent cycle makes the affected result
/// uncomputable (`None`).
#[must_use]
pub(crate) fn effective_points(
    items: &[BacklogItem],
    aggregate_children: bool,
    done_column: &Status,
) -> Vec<Option<u32>> {
    if !aggregate_children {
        return items.iter().map(|item| item.points).collect();
    }

    let index_of: HashMap<_, _> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (&item.id, index))
        .collect();
    let mut children = vec![Vec::new(); items.len()];
    for (index, item) in items.iter().enumerate() {
        if let Some(parent) = item.parent.as_ref().and_then(|id| index_of.get(id))
            && *parent != index
        {
            children[*parent].push(index);
        }
    }

    let calculator = Calculator {
        items,
        children,
        done_column,
        cache: vec![Cache::Unknown; items.len()],
        on_path: vec![false; items.len()],
    };
    calculator.compute_all()
}

#[derive(Clone, Copy)]
enum Cache {
    Unknown,
    Computed(Option<u32>),
}

/// Enter/exit marker for the explicit-stack post-order traversal: `Enter`
/// schedules a node's children, `Exit` combines their already-computed values.
enum Phase {
    Enter,
    Exit,
}

struct Calculator<'a> {
    items: &'a [BacklogItem],
    children: Vec<Vec<usize>>,
    done_column: &'a Status,
    cache: Vec<Cache>,
    /// Nodes currently on the depth-first path, so a parent cycle is detected as
    /// a re-entry rather than recursing forever.
    on_path: Vec<bool>,
}

impl Calculator<'_> {
    /// Compute the effective points for every item without recursion.
    ///
    /// A heap stack replaces the native call stack so chains thousands of levels
    /// deep cannot overflow. Each node is combined only after its children, and
    /// memoized so shared subtrees and repeated start nodes are computed once.
    fn compute_all(mut self) -> Vec<Option<u32>> {
        let mut stack: Vec<(usize, Phase)> = Vec::new();
        for start in 0..self.items.len() {
            if matches!(self.cache[start], Cache::Computed(_)) {
                continue;
            }
            stack.push((start, Phase::Enter));
            while let Some((index, phase)) = stack.pop() {
                match phase {
                    Phase::Enter => {
                        if matches!(self.cache[index], Cache::Computed(_)) {
                            continue;
                        }
                        if self.on_path[index] {
                            // Already on the current path: a parent cycle. Its own
                            // `Exit` frame will record `None`; skip this re-entry.
                            continue;
                        }
                        self.on_path[index] = true;
                        stack.push((index, Phase::Exit));
                        for &child in &self.children[index] {
                            // A done leaf always contributes a fixed `Some(0)`, so it
                            // never needs its own frame. Everything else is scheduled
                            // once, unless it is already computed.
                            if !matches!(self.cache[child], Cache::Computed(_))
                                && !self.is_done_leaf(child)
                            {
                                stack.push((child, Phase::Enter));
                            }
                        }
                    }
                    Phase::Exit => {
                        let value = self.combine(index);
                        self.on_path[index] = false;
                        self.cache[index] = Cache::Computed(value);
                    }
                }
            }
        }
        self.cache
            .into_iter()
            .map(|slot| match slot {
                Cache::Computed(value) => value,
                Cache::Unknown => None,
            })
            .collect()
    }

    /// Effective points of `index`: a leaf's own estimate, otherwise the sum of
    /// its children's contributions (with `None` on any gap or overflow).
    fn combine(&self, index: usize) -> Option<u32> {
        if self.children[index].is_empty() {
            return self.items[index].points;
        }
        self.children[index]
            .iter()
            .try_fold(0_u32, |total, &child| {
                self.contribution(child)
                    .and_then(|points| total.checked_add(points))
            })
    }

    /// The points a child adds to its parent's sum.
    ///
    /// A done leaf contributes zero; any other node contributes its effective
    /// value. An uncomputed value at combine time means the child is still on the
    /// path (a parent cycle), so it contributes `None` and makes the sum
    /// uncomputable.
    fn contribution(&self, child: usize) -> Option<u32> {
        if self.is_done_leaf(child) {
            return Some(0);
        }
        match self.cache[child] {
            Cache::Computed(value) => value,
            Cache::Unknown => None,
        }
    }

    /// A completed item with no children: it is excluded from aggregation, so as
    /// a child it contributes zero rather than its own stored estimate.
    fn is_done_leaf(&self, index: usize) -> bool {
        self.children[index].is_empty() && self.items[index].status == *self.done_column
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::ItemId;
    use crate::rank::Rank;

    fn item(number: u32, status: &str, points: Option<u32>, parent: Option<u32>) -> BacklogItem {
        let mut item = BacklogItem::new(
            ItemId::new("T", number),
            format!("Item {number}"),
            Status::new(status),
            Rank::after(None),
            chrono::Utc::now(),
        )
        .expect("valid item");
        item.points = points;
        item.parent = parent.map(|number| ItemId::new("T", number));
        item
    }

    fn points(items: &[BacklogItem], enabled: bool) -> Vec<Option<u32>> {
        effective_points(items, enabled, &Status::new("done"))
    }

    #[test]
    fn aggregation_is_disabled_by_default() {
        let items = [
            item(1, "todo", Some(99), None),
            item(2, "todo", Some(3), Some(1)),
            item(3, "todo", Some(5), Some(1)),
        ];

        assert_eq!(points(&items, false), [Some(99), Some(3), Some(5)]);
    }

    #[test]
    fn aggregates_nested_children_and_excludes_done_items() {
        let items = [
            item(1, "todo", Some(99), None),
            item(2, "todo", Some(3), Some(1)),
            item(3, "todo", Some(4), Some(2)),
            item(4, "done", Some(100), Some(2)),
            item(5, "todo", Some(5), Some(1)),
            item(6, "done", Some(7), Some(1)),
        ];

        let calculated = points(&items, true);

        assert_eq!(
            calculated[0],
            Some(9),
            "4 + 5; the parent's own 99 is ignored"
        );
        assert_eq!(
            calculated[1],
            Some(4),
            "a nested parent uses its active descendant"
        );
        assert_eq!(calculated[2], Some(4));
        assert_eq!(
            calculated[3],
            Some(100),
            "done items keep their displayed own points"
        );
    }

    #[test]
    fn active_unestimated_descendant_makes_the_parent_uncomputable() {
        let items = [
            item(1, "todo", Some(99), None),
            item(2, "todo", None, Some(1)),
            item(3, "done", None, Some(1)),
            item(4, "todo", Some(8), None),
            item(5, "done", None, Some(4)),
        ];

        let calculated = points(&items, true);

        assert_eq!(calculated[0], None);
        assert_eq!(
            calculated[3],
            Some(0),
            "only done descendants contribute zero"
        );
    }

    #[test]
    fn supports_deep_nesting_without_a_depth_limit() {
        let depth = 256;
        let items: Vec<_> = (1..=depth)
            .map(|number| {
                item(
                    number,
                    "todo",
                    (number == depth).then_some(2),
                    (number > 1).then_some(number - 1),
                )
            })
            .collect();

        let calculated = points(&items, true);

        assert_eq!(calculated[0], Some(2));
        assert!(calculated.iter().all(|points| *points == Some(2)));
    }

    #[test]
    fn a_parent_cycle_is_reported_as_uncomputable_instead_of_looping() {
        let items = [
            item(1, "todo", Some(1), Some(2)),
            item(2, "todo", Some(2), Some(1)),
        ];

        assert_eq!(points(&items, true), [None, None]);
    }

    /// Depth that overflows the default test-thread stack under naive recursion,
    /// so aggregation must run on an explicit heap stack. See P-50.
    const DEEP: u32 = 100_000;

    #[test]
    fn deep_parent_chain_aggregates_without_a_stack_overflow() {
        // T-1 ← T-2 ← ... ← T-DEEP; only the deepest leaf carries an estimate.
        let items: Vec<_> = (1..=DEEP)
            .map(|n| {
                item(
                    n,
                    "todo",
                    (n == DEEP).then_some(2),
                    (n > 1).then_some(n - 1),
                )
            })
            .collect();

        let calculated = points(&items, true);

        assert_eq!(calculated.len(), items.len());
        assert!(
            calculated.iter().all(|points| *points == Some(2)),
            "the single leaf estimate propagates up the whole chain"
        );
    }

    #[test]
    fn deep_parent_chain_terminating_in_a_cycle_is_uncomputable() {
        // A deep chain whose deepest two links form a parent cycle: every node on
        // the chain is uncomputable, and the traversal must terminate.
        let items: Vec<_> = (1..=DEEP)
            .map(|n| {
                let parent = if n == 1 { DEEP } else { n - 1 };
                item(n, "todo", Some(1), Some(parent))
            })
            .collect();

        let calculated = points(&items, true);

        assert_eq!(calculated.len(), items.len());
        assert!(
            calculated.iter().all(Option::is_none),
            "a cycle anywhere on the chain makes every member uncomputable"
        );
    }
}
