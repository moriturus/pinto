//! Column tree rendering shared by the board and backlog views.

use super::text::{MIN_TITLE_WIDTH, display_width, truncate};
use crate::cli::dependency_display::{
    DEP_ID_LIMIT, DepSummary, DependencyIndex, format_ids as format_dep_ids,
};
use pinto::backlog::{BacklogItem, ItemId};
use std::collections::{HashMap, HashSet};

/// Format the board as human-readable text.
///
/// Preserve the column order from `config.toml`, then list each column's PBIs in ascending rank
/// order. Empty columns are shown as `(empty)`. Titles are truncated with an ellipsis to fit
/// `max_width` (or [`DEFAULT_TERM_WIDTH`] for non-TTY output), accounting for fixed borders and
/// double-width characters.
///
/// Items whose status is absent from `config.toml` appear in a `(!) undefined columns` section so
/// items are still visible after a column is removed or renamed.
///
/// Dependencies (`depends_on`) appear directly below each item as marker lines (`⊸` or `⊷`); an
/// unfinished dependency is marked `⊸!`. Parent-child links (`parent`) determine tree nesting,
/// while dependency links remain flat. If the board has no dependencies, omit the markers and
/// legend to keep the output compact.
///
/// Parent-child nesting is limited to items in the same column; a parent from another column is
/// treated as absent, so the item becomes a root. Items without an in-column parent remain flat.
/// The visited set prevents cycles or duplicate rendering, ensuring every item appears once.
pub(super) fn render_column_tree(
    items: &[BacklogItem],
    id_width: usize,
    max_width: usize,
    deps: &DependencyIndex,
) -> String {
    let in_column: HashSet<&ItemId> = items.iter().map(|it| &it.id).collect();

    // Build child lists in column order and collect roots, including items whose parent is in another column.
    let mut children: HashMap<ItemId, Vec<&BacklogItem>> = HashMap::new();
    let mut roots: Vec<&BacklogItem> = Vec::new();
    for it in items {
        match tree_parent(it, &in_column) {
            Some(parent) => children.entry(parent).or_default().push(it),
            None => roots.push(it),
        }
    }

    let mut renderer = TreeRenderer {
        children,
        visited: HashSet::new(),
        id_width,
        max_width,
        deps,
        out: String::new(),
    };
    for root in &roots {
        renderer.render(root, "", true, true);
    }
    // Render any item missed by the root traversal (for example, because of a cycle) in column order.
    for it in items {
        if !renderer.visited.contains(&it.id) {
            renderer.render(it, "", true, true);
        }
    }
    renderer.out
}

/// Return `item`'s parent when that parent is in the same column.
fn tree_parent(item: &BacklogItem, in_column: &HashSet<&ItemId>) -> Option<ItemId> {
    let parent = item.parent.as_ref()?;
    in_column.contains(parent).then(|| parent.clone())
}

/// Tree drawing state. Avoid argument bloat and bundle child lists, visited sets, and output.
struct TreeRenderer<'a> {
    children: HashMap<ItemId, Vec<&'a BacklogItem>>,
    visited: HashSet<&'a ItemId>,
    id_width: usize,
    /// Terminal width (truncate each line's title to fit within this width).
    max_width: usize,
    /// Dependency index for subtracting dependent destination, dependent source, and block status.
    deps: &'a DependencyIndex,
    out: String,
}

impl<'a> TreeRenderer<'a> {
    /// Draw `node` and its descendants to `out`, adding a dependency marker immediately below when needed.
    ///
    /// `ancestor_prefix` contains the ancestor connection glyphs. `is_last` identifies the last
    /// sibling, and `is_root` identifies a column root. The visited set prevents duplicate output
    /// even when the parent graph contains a cycle.
    fn render(
        &mut self,
        node: &'a BacklogItem,
        ancestor_prefix: &str,
        is_last: bool,
        is_root: bool,
    ) {
        if !self.visited.insert(&node.id) {
            return; // Stop cycles and duplicate output.
        }
        let connector = if is_root {
            ""
        } else if is_last {
            "└─ "
        } else {
            "├─ "
        };
        let id_width = self.id_width;
        // Measure the fixed prefix before the title so continuation lines can align with it.
        let head = format!(
            "  {ancestor_prefix}{connector}{:<id_width$}  ",
            node.id.to_string()
        );
        let avail = self
            .max_width
            .saturating_sub(display_width(&head))
            .max(MIN_TITLE_WIDTH);
        self.out
            .push_str(&format!("{head}{}\n", truncate(&node.title, avail)));

        // Build the child connection prefix. There is no extra indentation below a root, and the
        // marker row uses the same prefix so it aligns with the child title.
        let child_prefix = if is_root {
            String::new()
        } else {
            format!("{ancestor_prefix}{}", if is_last { "   " } else { "│  " })
        };
        if let Some(marker) = dependency_marker_line(
            &child_prefix,
            id_width,
            self.max_width,
            &self.deps.summary(node),
        ) {
            self.out.push_str(&marker);
            self.out.push('\n');
        }

        let Some(kids) = self.children.get(&node.id) else {
            return;
        };
        // Clone the references before recursing so `self` can be mutably borrowed.
        let kids: Vec<&BacklogItem> = kids.clone();
        let last = kids.len() - 1;
        for (i, kid) in kids.into_iter().enumerate() {
            self.render(kid, &child_prefix, i == last, false);
        }
    }
}

/// Format a dependency marker line (`⊸`, `⊸!`, or `⊷`).
///
/// `prefix` contains the ancestor connection line and is empty for a root. Reserve the fixed ID
/// prefix so the marker aligns with the title. Return `None` when there are no dependencies, and
/// truncate to `max_width` unless it is `usize::MAX` (`--no-truncate`).
pub(super) fn dependency_marker_line(
    prefix: &str,
    id_width: usize,
    max_width: usize,
    deps: &DepSummary,
) -> Option<String> {
    if deps.is_empty() {
        return None;
    }
    let indent = format!("  {prefix}{:id_width$}  ", "");
    let mut body = String::new();
    if !deps.depends_on.is_empty() {
        let mark = if deps.blocked { "⊸!" } else { "⊸" };
        body.push_str(&format!(
            "{mark} {}",
            format_dep_ids(&deps.depends_on, DEP_ID_LIMIT)
        ));
    }
    if !deps.dependents.is_empty() {
        if !body.is_empty() {
            body.push_str("  ");
        }
        body.push_str(&format!(
            "⊷ {}",
            format_dep_ids(&deps.dependents, DEP_ID_LIMIT)
        ));
    }
    let avail = max_width
        .saturating_sub(display_width(&indent))
        .max(MIN_TITLE_WIDTH);
    Some(format!("{indent}{}", truncate(&body, avail)))
}
