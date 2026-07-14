//! Effective story-point calculation for opt-in parent-child aggregation.

use crate::backlog::{BacklogItem, Status};
use std::collections::{HashMap, HashSet};

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

    let mut calculator = Calculator {
        items,
        children,
        done_column,
        cache: vec![Cache::Unknown; items.len()],
    };
    (0..items.len())
        .map(|index| calculator.effective(index, &mut HashSet::new()))
        .collect()
}

#[derive(Clone, Copy)]
enum Cache {
    Unknown,
    Computed(Option<u32>),
}

struct Calculator<'a> {
    items: &'a [BacklogItem],
    children: Vec<Vec<usize>>,
    done_column: &'a Status,
    cache: Vec<Cache>,
}

impl Calculator<'_> {
    fn effective(&mut self, index: usize, visiting: &mut HashSet<usize>) -> Option<u32> {
        if let Cache::Computed(value) = self.cache[index] {
            return value;
        }
        if !visiting.insert(index) {
            return None;
        }

        let value = if self.children[index].is_empty() {
            self.items[index].points
        } else {
            self.sum_children(index, visiting)
        };
        visiting.remove(&index);
        self.cache[index] = Cache::Computed(value);
        value
    }

    fn contribution(&mut self, index: usize, visiting: &mut HashSet<usize>) -> Option<u32> {
        if self.items[index].status == *self.done_column {
            // A completed intermediate node is not counted itself, but active descendants are
            // still eligible because the rule is applied to each item's status independently.
            self.sum_children(index, visiting)
        } else {
            self.effective(index, visiting)
        }
    }

    fn sum_children(&mut self, index: usize, visiting: &mut HashSet<usize>) -> Option<u32> {
        self.children[index]
            .clone()
            .into_iter()
            .try_fold(0_u32, |total, child| {
                self.contribution(child, visiting)
                    .and_then(|points| total.checked_add(points))
            })
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
}
