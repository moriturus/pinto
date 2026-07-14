//! Board text formatting (PBI grouped by status column).

use pinto::i18n::current;
use pinto::service::Board;

use super::item::{ListLongOptions, format_list_long};
use super::{
    MIN_TITLE_WIDTH, dependency_legend, dependency_marker_line, display_width, render_column_tree,
    truncate,
};
use crate::cli::dependency_display::DependencyIndex;

/// Format the board using the shared layout and dependency renderers.
pub(crate) fn format_board(board: &Board, max_width: usize) -> String {
    format_board_with_options(board, max_width, None)
}

/// Format the board with the same detailed columns as the long backlog list.
pub(crate) fn format_board_long(
    board: &Board,
    max_width: usize,
    options: ListLongOptions,
) -> String {
    format_board_with_options(board, max_width, Some(options))
}

fn format_board_with_options(
    board: &Board,
    max_width: usize,
    long_options: Option<ListLongOptions>,
) -> String {
    let id_width = board
        .columns
        .iter()
        .flat_map(|column| &column.items)
        .chain(&board.orphaned)
        .map(|item| item.id.to_string().chars().count())
        .max()
        .unwrap_or(0);
    let deps = DependencyIndex::from_board(board);

    let mut sections: Vec<String> = board
        .columns
        .iter()
        .map(|column| {
            let mut section = format!("{} ({})\n", column.status, column.items.len());
            if column.items.is_empty() {
                section.push_str("  (empty)\n");
            } else if let Some(options) = long_options {
                section.push_str(&format_list_long(&column.items, max_width, options));
            } else {
                section.push_str(&render_column_tree(
                    &column.items,
                    id_width,
                    max_width,
                    &deps,
                ));
            }
            section
        })
        .collect();

    if !board.orphaned.is_empty() {
        let mut section = format!("(!) undefined columns ({})\n", board.orphaned.len());
        if let Some(options) = long_options {
            section.push_str(&format_list_long(&board.orphaned, max_width, options));
        } else {
            for item in &board.orphaned {
                let suffix = format!("  [{}]", item.status);
                let head_width = 2 + id_width + 2;
                let available = max_width
                    .saturating_sub(head_width + display_width(&suffix))
                    .max(MIN_TITLE_WIDTH);
                section.push_str(&format!(
                    "  {:<id_width$}  {}{suffix}\n",
                    item.id.to_string(),
                    truncate(&item.title, available),
                ));
                if let Some(marker) =
                    dependency_marker_line("", id_width, max_width, &deps.summary(item))
                {
                    section.push_str(&marker);
                    section.push('\n');
                }
            }
        }
        sections.push(section);
    }

    let body = sections.join("\n");
    if long_options.is_none() && deps.any_dependencies(board) {
        format!("{}\n\n{body}", dependency_legend(current()))
    } else {
        body
    }
}
