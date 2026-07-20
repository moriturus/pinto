//! Backlog item commands: add, list, next, show, transitions, edits, and removal.

use crate::cli::args::*;
use crate::cli::format::item::{
    DetailOptions, ListLongOptions, format_detail, format_list, format_list_long,
};
use crate::cli::json::{detail_json, list_json};
use pinto::backlog::ItemId;
use pinto::error::Error;
use pinto::i18n::{Message, current};
use pinto::service::{
    EditOutcome, ItemEdit, ListFilter, MoveOutcome, NewItem, NextFilter, RemoveOutcome,
    ReorderTarget, add_item_with_outcome, apply_item_edit, archived_item_detail, check_wip,
    common_dod, display_settings, edit_item, item_detail, item_edit_template, list_items,
    move_item_with_outcome, next_items, remove_item, reorder_item, restore_item, template_body,
};
use std::io::IsTerminal;

use pinto::template::{TemplateKind, TemplateName};
use std::path::Path;
use std::process::ExitCode;

use super::{
    build_search_filter, resolve_label_filter, resolve_label_match, resolve_optional_filter,
    terminal_width, warn_wip,
};

/// `pinto add` — Add a PBI to the backlog.
pub(super) async fn cmd_add(args: AddArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let parent = args
        .parent
        .as_deref()
        .map(str::parse::<ItemId>)
        .transpose()?;
    let depends_on = args
        .depends_on
        .iter()
        .map(|id| id.parse::<ItemId>())
        .collect::<Result<Vec<_>, _>>()?;
    let template_body = if let Some(template) = args.template {
        let template: TemplateName = template.parse()?;
        Some(template_body(&dir, TemplateKind::Item, &template).await?)
    } else {
        None
    };
    let body = if args.edit {
        let initial = template_body.unwrap_or_default();
        let slug = format!("add-{}", args.title);
        tokio::task::spawn_blocking(move || crate::cli::editor::edit_in_editor(&initial, &slug))
            .await??
    } else {
        match (template_body, args.body) {
            (Some(template), Some(body)) => combine_template_body(template, body),
            (Some(template), None) => template,
            (None, Some(body)) => body,
            (None, None) => String::new(),
        }
    };
    let new = NewItem {
        points: args.points,
        labels: args.labels,
        sprint: args.sprint,
        body,
        parent,
        depends_on,
    };
    let outcome = add_item_with_outcome(&dir, &args.title, new).await?;
    let item = outcome.item;
    if outcome.cycle_warning {
        eprintln!("{}", current().text(Message::DependencyCycleWarningGeneric));
    }
    let id = item.id.to_string();
    println!(
        "{}",
        current().format(
            Message::Created,
            [("id", id.as_str()), ("title", item.title.as_str())]
        )
    );
    Ok(ExitCode::SUCCESS)
}

pub(super) fn combine_template_body(template: String, body: String) -> String {
    if template.is_empty() {
        return body;
    }
    if body.is_empty() {
        return template;
    }
    let separator = if template.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{template}{separator}{body}")
}

/// `pinto list` — List backlogged PBIs.
///
/// `--long/-l` shows ID, title, status, points, assignee, and creation/update dates.
/// `--label`, `--sprint`, and `--acceptance-criteria` add columns between assignee and creation date;
/// omit their values in long mode to show the columns without filtering. `--assignee` is an exact
/// match and composes with the other filters. Multiple labels use OR by default; `--all-labels`
/// switches to AND. `--roots-only` omits PBIs with a persisted parent link. `--stale` filters by
/// the `updated` timestamp and composes with the other filters.
/// `--json` takes precedence because it already contains all metadata.
pub(super) async fn cmd_list(args: ListArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let long = args.long;
    let show_labels = args.label.is_some();
    let show_sprint = args.sprint.is_some();
    let labels = resolve_label_filter("--label", args.label, long)?;
    let label_match = resolve_label_match(args.all_labels, &labels)?;
    let sprint = resolve_optional_filter("--sprint", args.sprint, long)?;
    let stale_before = args
        .stale
        .map(|duration| {
            chrono::Utc::now()
                .checked_sub_signed(duration)
                .ok_or_else(|| {
                    Error::InvalidFilterOption(
                        "--stale duration is too large for a UTC timestamp".to_string(),
                    )
                })
        })
        .transpose()?;
    let filter = ListFilter {
        roots_only: args.roots_only,
        archived: args.archived,
        status: args.status,
        sprint,
        assignee: args.assignee,
        labels,
        label_match,
        search: build_search_filter(args.search, args.regex)?,
        stale_before,
    };
    let items = list_items(&dir, &filter).await?;
    if args.json {
        // Machine-readable output returns the array `[]` even if it is empty (to make it consistent so that the consumer can handle it without branching).
        println!("{}", list_json(&items)?);
    } else if items.is_empty() {
        println!("{}", current().text(Message::NoBacklogItems));
    } else if long {
        let timezone = display_settings(&dir).await?.timezone;
        print!(
            "{}",
            format_list_long(
                &items,
                terminal_width(),
                ListLongOptions::new(show_labels, show_sprint)
                    .with_acceptance_criteria(args.acceptance_criteria)
                    .with_timezone(timezone),
            )
        );
    } else {
        print!("{}", format_list(&items));
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto next` — Display PBIs that can be started immediately.
pub(super) async fn cmd_next(args: NextArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let items = next_items(
        &dir,
        &NextFilter {
            count: args.count,
            sprint: args.sprint,
        },
    )
    .await?;
    if args.json {
        println!("{}", list_json(&items)?);
    } else if items.is_empty() {
        println!("{}", current().text(Message::NoActionableItems));
    } else {
        print!("{}", format_list(&items));
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto show` — Display PBI details for a given ID.
///
/// User errors such as non-existent IDs or invalid ID formats are handled by `main` as exit code 1.
pub(super) async fn cmd_show(args: ShowArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let ids: Vec<ItemId> = args
        .ids
        .iter()
        .map(|id| id.parse())
        .collect::<Result<_, _>>()?;
    let mut details = Vec::with_capacity(ids.len());
    for id in &ids {
        details.push(if args.archived {
            archived_item_detail(&dir, id).await?
        } else {
            item_detail(&dir, id).await?
        });
    }
    if args.json {
        println!("{}", detail_json(&details)?);
    } else {
        // Common DoD (if any) should be listed in the individual Acceptance Criteria.
        let dod = common_dod(&dir).await?;
        // Markdown rendering is the default; the board's config (`[display] markdown`)
        // or the per-invocation `--plain` flag can opt out into raw text.
        let display = display_settings(&dir).await?;
        let markdown = display.markdown && !args.plain;
        // Emit ANSI colour only for a real terminal; a redirected `show` gets
        // colourless structural rendering so pipes and files stay clean text.
        let options = DetailOptions {
            markdown,
            width: terminal_width(),
            color: std::io::stdout().is_terminal(),
            timezone: display.timezone,
        };
        let output = details
            .iter()
            .map(|detail| format_detail(detail, dod.as_deref(), options))
            .collect::<Vec<_>>()
            .join("\n---\n");
        print!("{output}");
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto move` — Transition the state (column) of PBI.
///
/// User errors such as non-existent IDs, invalid ID formats, undefined columns, etc. will be assigned code 1 by `main`.
fn acceptance_criteria_warning(outcome: &MoveOutcome) -> Option<String> {
    if !outcome.entered_done_column || !outcome.acceptance_criteria.is_incomplete() {
        return None;
    }

    let progress = outcome.acceptance_criteria.to_string();
    Some(current().format(
        Message::AcceptanceCriteriaIncomplete,
        [
            ("id", outcome.item.id.to_string().as_str()),
            ("progress", progress.as_str()),
        ],
    ))
}

pub(super) async fn cmd_move(args: MoveArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    // `mv`-style: the last operand is the destination status, the rest are source IDs.
    // clap enforces `num_args = 2..`, so `None` is unreachable here.
    let Some((status, raw_ids)) = args.destination_and_ids() else {
        return Ok(ExitCode::SUCCESS);
    };

    let mut failures = Vec::new();
    let mut moved_any = false;

    for raw_id in raw_ids {
        let id: ItemId = match raw_id.parse() {
            Ok(id) => id,
            Err(error) => {
                failures.push((raw_id.clone(), error));
                continue;
            }
        };

        match move_item_with_outcome(&dir, &id, status).await {
            Ok(outcome) => {
                let item = &outcome.item;
                println!(
                    "{}",
                    current().format(
                        Message::Moved,
                        [
                            ("id", item.id.to_string().as_str()),
                            ("status", item.status.as_str()),
                        ],
                    )
                );
                if let Some(warning) = acceptance_criteria_warning(&outcome) {
                    eprintln!("{warning}");
                }
                moved_any = true;
            }
            Err(error) => failures.push((raw_id.clone(), error)),
        }
    }

    // Warn once if the destination column exceeds the WIP limit (moves succeed, exit code is 0).
    // Nothing is output when `--no-wip-check` is specified or `wip.enabled=false`.
    if moved_any && !args.no_wip_check {
        for v in check_wip(&dir).await?.iter().filter(|v| v.column == status) {
            warn_wip(v);
        }
    }

    report_failures(failures, "move")
}

/// Print per-item failures to stderr and derive the exit code, shared by batch commands.
///
/// A user error (invalid ID, unknown status, missing item) yields exit code 1 after all
/// failures are reported. A non-user (internal) error is propagated so `main` surfaces it.
pub(super) fn report_failures(
    mut failures: Vec<(String, Error)>,
    action: &str,
) -> anyhow::Result<ExitCode> {
    let localizer = current();
    if let Some(internal_index) = failures
        .iter()
        .position(|(_, error)| !error.is_user_error())
    {
        let (id, error) = failures.swap_remove(internal_index);
        for (failed_id, error) in failures {
            eprintln!(
                "{} {}: {}",
                localizer.text(Message::ErrorPrefix),
                failed_id,
                error.localized(localizer)
            );
        }
        let context = localizer.format(
            Message::FailedToAction,
            [("action", action), ("id", id.as_str())],
        );
        return Err(anyhow::Error::new(error).context(context));
    }

    let has_failures = !failures.is_empty();
    for (id, error) in failures {
        eprintln!(
            "{} {}: {}",
            localizer.text(Message::ErrorPrefix),
            id,
            error.localized(localizer)
        );
    }
    if has_failures {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto reorder` — Reorder PBI ranks (without changing `status`).
///
/// The destination (`--before` / `--after` / `--top` / `--bottom`) is an exclusive group of clap.
/// Constrain to exactly one. User errors such as non-existent IDs and self-references are handled by `main`.
/// Assign to code 1.
pub(super) async fn cmd_reorder(args: ReorderArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let id: ItemId = args.id.parse()?;
    let target = if let Some(reference) = args.before {
        ReorderTarget::Before(reference.parse()?)
    } else if let Some(reference) = args.after {
        ReorderTarget::After(reference.parse()?)
    } else if args.top {
        ReorderTarget::Top
    } else {
        ReorderTarget::Bottom
    };
    let item = reorder_item(&dir, &id, target).await?;
    println!(
        "{}",
        current().format(Message::Reordered, [("id", item.id.to_string().as_str())],)
    );
    Ok(ExitCode::SUCCESS)
}

/// `pinto edit` — Update an existing PBI.
///
/// If a field is specified (`--title`, etc.), only that field is updated. At least one specification
/// If not, the standard behavior is to open and edit the entire PBI (frontmatter + body) with `$EDITOR`.
/// User errors such as non-existent IDs, invalid ID formats, empty titles, invalid editing contents, etc.
/// `main` assigns code 1.
pub(super) async fn cmd_edit(args: EditArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let id: ItemId = args.id.parse()?;

    // If there is no field specified, edit the entire PBI using $EDITOR (standard operation).
    if !has_field_edits(&args) {
        return cmd_edit_in_editor(&dir, &id).await;
    }

    // Change parent: `--no-parent` cancels, `--parent <id>` sets, no change if neither is present.
    // Both are exclusive with clap (`conflicts_with`).
    let parent = if args.no_parent {
        Some(None)
    } else if let Some(p) = args.parent {
        Some(Some(p.parse::<ItemId>()?))
    } else {
        None
    };

    let edit = ItemEdit {
        title: args.title,
        points: args.points,
        // `--label` If unspecified (empty), no change is made; if specified, the set is replaced.
        labels: if args.labels.is_empty() {
            None
        } else {
            Some(args.labels)
        },
        assignee: args.assignee,
        sprint: args.sprint,
        body: args.body,
        parent,
    };
    let item = edit_item(&dir, &id, edit).await?;
    println!(
        "{}",
        current().format(
            Message::Updated,
            [
                ("id", item.id.to_string().as_str()),
                ("title", item.title.as_str()),
            ],
        )
    );
    Ok(ExitCode::SUCCESS)
}

/// Is there at least one field update specification in `edit`? (If not, branches to `$EDITOR` startup).
fn has_field_edits(args: &EditArgs) -> bool {
    args.title.is_some()
        || args.points.is_some()
        || !args.labels.is_empty()
        || args.assignee.is_some()
        || args.sprint.is_some()
        || args.body.is_some()
        || args.parent.is_some()
        || args.no_parent
}

/// Open the entire PBI in `$EDITOR` and apply the edited content (the default `edit` behavior).
///
/// Format the current values into a temporary file, launch the editor on a blocking thread, and
/// validate the result with [`apply_item_edit`] before saving. The original data remains intact on
/// validation failure. Return [`pinto::error::Error::EditorNotSet`] when no editor is configured.
pub(super) async fn cmd_edit_in_editor(dir: &Path, id: &ItemId) -> anyhow::Result<ExitCode> {
    // Fail before creating an edit template when no editor is configured.
    if crate::cli::editor::resolve_editor().is_none() {
        return Err(pinto::error::Error::EditorNotSet.into());
    }

    let template = item_edit_template(dir, id).await?;
    let slug = id.to_string();
    let edited =
        tokio::task::spawn_blocking(move || crate::cli::editor::edit_in_editor(&template, &slug))
            .await??;

    match apply_item_edit(dir, id, &edited).await? {
        EditOutcome::Updated(item) => println!(
            "{}",
            current().format(
                Message::Updated,
                [
                    ("id", item.id.to_string().as_str()),
                    ("title", item.title.as_str()),
                ],
            )
        ),
        EditOutcome::Unchanged => println!(
            "{}",
            current().format(Message::NoChangesTo, [("id", id.to_string().as_str())])
        ),
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto rm` — Archive PBIs (default) or physically delete them with `--force`.
///
/// Parse every requested ID before attempting any archive/delete operation. Valid-but-missing IDs
/// are still attempted together so the command can report all lookup failures, while malformed IDs
/// never allow a preceding valid target to be changed.
pub(super) async fn cmd_rm(args: RemoveArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let mut ids = Vec::with_capacity(args.ids.len());
    let mut failures = Vec::new();

    for raw_id in args.ids {
        let id: ItemId = match raw_id.parse() {
            Ok(id) => id,
            Err(error) => {
                failures.push((raw_id, error));
                continue;
            }
        };
        ids.push((raw_id, id));
    }

    if !failures.is_empty() {
        return report_failures(failures, "remove");
    }

    for (raw_id, id) in ids {
        match remove_item(&dir, &id, args.force).await {
            Ok(RemoveOutcome::Archived(path)) => println!(
                "{} {} ({})",
                current().text(Message::Archived),
                raw_id,
                path.display()
            ),
            Ok(RemoveOutcome::Deleted) => {
                println!("{} {}", current().text(Message::Deleted), raw_id)
            }
            Err(error) => failures.push((raw_id, error)),
        }
    }

    report_failures(failures, "remove")
}

/// `pinto restore` — Restore an archived PBI to the active backlog.
pub(super) async fn cmd_restore(args: RestoreArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let id: ItemId = args.id.parse()?;
    let item = restore_item(&dir, &id).await?;
    println!("{} {}", current().text(Message::Restored), item.id);
    Ok(ExitCode::SUCCESS)
}
