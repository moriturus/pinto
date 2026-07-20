//! Product backlog item text formatting.

use super::text::{MIN_TITLE_WIDTH, display_width, truncate};
use pinto::backlog::{AcceptanceCriteriaProgress, BacklogItem, ItemId};
use pinto::service::ItemDetail;
use pinto::timezone::DisplayTimezone;

/// Make the PBI list human-readable formatted text (ID/Status/Title, optionally point labels).
pub(crate) fn format_list(items: &[BacklogItem]) -> String {
    // Alignment is based on the number of characters (according to `{:<width$}` padding and `truncate`, and does not shift even in non-ASCII).
    let id_width = items
        .iter()
        .map(|it| it.id.to_string().chars().count())
        .max()
        .unwrap_or(0);
    let status_width = items
        .iter()
        .map(|it| it.status.as_str().chars().count())
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    for it in items {
        let mut line = format!(
            "{:<id_width$}  {:<status_width$}  {}",
            it.id.to_string(),
            it.status.as_str(),
            it.title,
        );
        if let Some(points) = it.points {
            line.push_str(&format!("  ({points})"));
        }
        if !it.labels.is_empty() {
            line.push_str(&format!("  [{}]", it.labels.join(", ")));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ListLongOptions {
    pub(crate) show_labels: bool,
    pub(crate) show_sprint: bool,
    pub(crate) show_acceptance_criteria: bool,
    pub(crate) timezone: DisplayTimezone,
}

impl ListLongOptions {
    pub(crate) const fn new(show_labels: bool, show_sprint: bool) -> Self {
        Self {
            show_labels,
            show_sprint,
            show_acceptance_criteria: false,
            timezone: DisplayTimezone::Local,
        }
    }

    /// Include the computed Acceptance Criteria completion column.
    pub(crate) const fn with_acceptance_criteria(mut self, show: bool) -> Self {
        self.show_acceptance_criteria = show;
        self
    }

    /// Set the timezone used by the created/updated columns.
    pub(crate) const fn with_timezone(mut self, timezone: DisplayTimezone) -> Self {
        self.timezone = timezone;
        self
    }
}

/// Format the PBI list into the detailed Scrum overview table.
///
/// The stable base order is ID, TITLE, STATUS, POINTS, ASSIGNEE, CREATED, and UPDATED.
/// LABELS, SPRINT, and ACCEPTANCE are inserted between ASSIGNEE and CREATED only when selected.
pub(crate) fn format_list_long(
    items: &[BacklogItem],
    max_width: usize,
    options: ListLongOptions,
) -> String {
    if items.is_empty() {
        return String::new();
    }

    let mut headers = vec![
        "ID".to_string(),
        "TITLE".to_string(),
        "STATUS".to_string(),
        "POINTS".to_string(),
        "ASSIGNEE".to_string(),
    ];
    let title_index = 1;
    if options.show_acceptance_criteria {
        headers.push("ACCEPTANCE".to_string());
    }
    let labels_index = options.show_labels.then(|| {
        headers.push("LABELS".to_string());
        headers.len() - 1
    });
    if options.show_sprint {
        headers.push("SPRINT".to_string());
    }
    headers.push("CREATED".to_string());
    headers.push("UPDATED".to_string());

    fn or_dash(value: Option<&str>) -> String {
        value.map(str::to_string).unwrap_or_else(|| "-".to_string())
    }

    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|it| {
            let mut cells = vec![
                it.id.to_string(),
                it.title.clone(),
                it.status.as_str().to_string(),
                it.points
                    .map(|points| points.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                or_dash(it.assignee.as_deref()),
            ];
            if options.show_acceptance_criteria {
                cells.push(AcceptanceCriteriaProgress::from_markdown(&it.body).to_string());
            }
            if options.show_labels {
                cells.push(if it.labels.is_empty() {
                    "-".to_string()
                } else {
                    it.labels.join(", ")
                });
            }
            if options.show_sprint {
                cells.push(or_dash(it.sprint.as_deref()));
            }
            cells.push(options.timezone.format_datetime(it.created, "%Y-%m-%d"));
            cells.push(options.timezone.format_datetime(it.updated, "%Y-%m-%d"));
            cells
        })
        .collect();

    let mut widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter()
                .map(|row| display_width(&row[index]))
                .max()
                .unwrap_or(0)
                .max(display_width(header))
        })
        .collect();
    let natural_title = widths[title_index];
    let natural_labels = labels_index.map_or(0, |index| widths[index]);
    let is_variable = |index: usize| index == title_index || labels_index == Some(index);
    const WIDE_SEP: usize = 2;
    let natural_width = widths.iter().sum::<usize>() + headers.len().saturating_sub(1) * WIDE_SEP;
    let separator_width = if max_width != usize::MAX && natural_width > max_width {
        1
    } else {
        WIDE_SEP
    };
    let fixed = widths
        .iter()
        .enumerate()
        .filter(|(index, _)| !is_variable(*index))
        .map(|(_, width)| width)
        .sum::<usize>()
        + headers.len().saturating_sub(1) * separator_width;
    let available = max_width.saturating_sub(fixed);

    if let Some(labels_index) = labels_index {
        let (title_width, labels_width) =
            if natural_title.saturating_add(natural_labels) <= available {
                (natural_title, natural_labels)
            } else {
                let labels_floor = display_width("LABELS");
                let mut labels_width = natural_labels.min((available / 3).max(labels_floor));
                let title_width =
                    natural_title.min(available.saturating_sub(labels_width).max(MIN_TITLE_WIDTH));
                let leftover = available.saturating_sub(title_width + labels_width);
                labels_width = natural_labels.min(labels_width + leftover);
                (title_width, labels_width)
            };
        widths[title_index] = title_width;
        widths[labels_index] = labels_width;
    } else {
        widths[title_index] = natural_title.min(available.max(MIN_TITLE_WIDTH));
    }

    let pad = |value: &str, width: usize| -> String {
        let value_width = display_width(value);
        if value_width >= width {
            value.to_string()
        } else {
            format!("{value}{}", " ".repeat(width - value_width))
        }
    };
    let render_line = |cells: &[String]| -> String {
        let mut line = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let value = if index == title_index || labels_index == Some(index) {
                    truncate(cell, widths[index])
                } else {
                    cell.clone()
                };
                pad(&value, widths[index])
            })
            .collect::<Vec<_>>()
            .join(&" ".repeat(separator_width));
        line.truncate(line.trim_end().len());
        if max_width != usize::MAX && display_width(&line) > max_width {
            line = truncate(&line, max_width);
        }
        line
    };

    let mut out = String::new();
    out.push_str(&render_line(&headers));
    out.push('\n');
    for row in &rows {
        out.push_str(&render_line(row));
        out.push('\n');
    }
    out
}

/// Format IDs separated by `, `, or return `-` when the list is empty.
pub(super) fn format_ids(ids: &[ItemId]) -> String {
    if ids.is_empty() {
        "-".to_string()
    } else {
        ids.iter()
            .map(ItemId::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Human-readable rank cell: sibling-local ordinal plus the internal fractional
/// index. Children name their parent (`#2 under <parent-id>`) so the number
/// is clearly the order among that parent's children, not a whole-column
/// position.
fn rank_label(detail: &ItemDetail) -> String {
    match &detail.item.parent {
        Some(parent) => format!(
            "#{} under {} ({})",
            detail.rank_ordinal, parent, detail.item.rank
        ),
        None => format!("#{} ({})", detail.rank_ordinal, detail.item.rank),
    }
}

/// Format all item fields, bidirectional links, and the body as human-readable text.
///
/// If `common_dod` is `Some`, append the common Definition of Done after the item's Acceptance
/// Criteria under a separate heading. If it is `None`, display only the item's content.
pub(crate) fn format_detail(
    detail: &ItemDetail,
    common_dod: Option<&str>,
    options: DetailOptions,
) -> String {
    let item = &detail.item;

    /// Labels used in the detail view to align value columns.
    const LABELS: [&str; 15] = [
        "Status",
        "Rank",
        "Points",
        "Labels",
        "Assignee",
        "Sprint",
        "Parent",
        "Children",
        "Depends on",
        "Depended by",
        "Started",
        "Completed",
        "Commits",
        "Created",
        "Updated",
    ];

    /// Return the width needed to align values after the longest ASCII label.
    const fn detail_label_width(labels: &[&str]) -> usize {
        let mut max = 0;
        let mut i = 0;
        while i < labels.len() {
            let len = labels[i].len() + 1; // The trailing colon.
            if len > max {
                max = len;
            }
            i += 1;
        }
        max + 1 // Ensure at least 1 space before the value.
    }

    const WIDTH: usize = detail_label_width(&LABELS);

    /// Format one label/value row with values aligned to the longest label.
    fn row(label: &str, value: impl AsRef<str>) -> String {
        let label = format!("{label}:");
        if label.len() >= WIDTH {
            // Long labels cannot use the shared alignment width, but still need a separator.
            format!("{label} {}\n", value.as_ref())
        } else {
            format!("{label:<WIDTH$}{}\n", value.as_ref())
        }
    }

    /// Display `-` for an unset optional field.
    fn or_dash(value: Option<&str>) -> String {
        value.map(str::to_string).unwrap_or_else(|| "-".to_string())
    }

    let labels = if item.labels.is_empty() {
        "-".to_string()
    } else {
        item.labels.join(", ")
    };

    let mut out = format!("{}  {}\n\n", item.id, item.title);
    out.push_str(&row("Status", item.status.to_string()));
    // Rank is a human-readable, sibling-local ordinal (`#3`), with the internal
    // fractional index in parentheses. For a child, name the parent so it is
    // clear the number is the order among that parent's children, not the column.
    out.push_str(&row("Rank", rank_label(detail)));
    out.push_str(&row(
        "Points",
        item.points
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string()),
    ));
    out.push_str(&row("Labels", labels));
    out.push_str(&row("Assignee", or_dash(item.assignee.as_deref())));
    out.push_str(&row("Sprint", or_dash(item.sprint.as_deref())));
    out.push_str(&row(
        "Acceptance Criteria",
        AcceptanceCriteriaProgress::from_markdown(&item.body).to_string(),
    ));
    out.push_str(&row(
        "Parent",
        item.parent
            .as_ref()
            .map(ItemId::to_string)
            .unwrap_or_else(|| "-".to_string()),
    ));
    out.push_str(&row("Children", format_ids(&detail.children)));
    out.push_str(&row("Depends on", format_ids(&item.depends_on)));
    out.push_str(&row("Depended by", format_ids(&detail.dependents)));
    out.push_str(&row(
        "Started",
        item.start_at
            .map(|d| options.timezone.format_datetime(d, "%Y-%m-%dT%H:%M:%S%:z"))
            .unwrap_or_else(|| "-".to_string()),
    ));
    out.push_str(&row(
        "Completed",
        item.done_at
            .map(|d| options.timezone.format_datetime(d, "%Y-%m-%dT%H:%M:%S%:z"))
            .unwrap_or_else(|| "-".to_string()),
    ));
    // Show abbreviated SHAs here; `--json` exposes the complete values.
    let commits = if item.commits.is_empty() {
        "-".to_string()
    } else {
        item.commits
            .iter()
            .map(|sha| sha.chars().take(8).collect::<String>())
            .collect::<Vec<_>>()
            .join(", ")
    };
    out.push_str(&row("Commits", commits));
    out.push_str(&row(
        "Created",
        options
            .timezone
            .format_datetime(item.created, "%Y-%m-%dT%H:%M:%S%:z"),
    ));
    out.push_str(&row(
        "Updated",
        options
            .timezone
            .format_datetime(item.updated, "%Y-%m-%dT%H:%M:%S%:z"),
    ));

    // Build the content region from the item body and common DoD, then render it or append it
    // verbatim according to the selected option.
    let mut content = String::new();
    if !item.body.is_empty() {
        content.push_str(&item.body);
        content.push('\n');
    }
    if let Some(dod) = common_dod {
        content.push_str("\n## Definition of Done (common)\n\n");
        content.push_str(dod);
        content.push('\n');
    }
    if !content.is_empty() {
        out.push('\n');
        if options.markdown {
            out.push_str(&crate::cli::markdown::render_body(
                &content,
                options.width,
                options.color,
            ));
        } else {
            out.push_str(&content);
        }
    }
    out
}

/// Options controlling [`format_detail`] output.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DetailOptions {
    /// Render the content region (body + common DoD) as Markdown.
    pub(crate) markdown: bool,
    /// Wrap width for Markdown rendering (terminal width for the CLI).
    pub(crate) width: usize,
    /// Emit ANSI colour for a real terminal; disable it for redirected output so pipes stay clean.
    pub(crate) color: bool,
    /// Timezone used for human-readable timestamp fields.
    pub(crate) timezone: DisplayTimezone,
}
