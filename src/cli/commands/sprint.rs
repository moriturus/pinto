//! Sprint management commands.

use crate::cli::args::*;
use crate::cli::format::report::format_burndown;
use crate::cli::format::sprint::{
    format_sprint_capacity, format_sprints_with_timezone, format_velocity,
};
use crate::cli::json::{burndown_json, sprint_capacity_json, sprints_json};
use pinto::backlog::ItemId;
use pinto::i18n::{Localizer, Message, current};
use pinto::service::{
    SprintCloseAction, assign_sprint_by_status, assign_sprint_raw, burndown, close_sprint,
    create_sprint, delete_sprint, display_settings, edit_sprint, list_sprints, set_sprint_capacity,
    sprint_capacity, sprint_load_warnings, start_sprint, template_body, unassign_sprint, velocity,
};

use pinto::sprint::SprintId;
use pinto::template::{TemplateKind, TemplateName};
use std::path::Path;
use std::process::ExitCode;

use super::terminal_width;

/// Warn to stderr when assigned Sprint points exceed a configured planning threshold.
async fn warn_sprint_load(dir: &Path, id: &SprintId, localizer: &Localizer) -> anyhow::Result<()> {
    for warning in sprint_load_warnings(dir, id).await? {
        let points = warning.points.to_string();
        let threshold = format!("{:.1} {}", warning.threshold, warning.kind.unit());
        eprintln!(
            "{}",
            localizer.format(
                Message::SprintLoadWarning,
                [
                    ("sprint", id.as_str()),
                    ("points", points.as_str()),
                    ("kind", warning.kind.as_str()),
                    ("threshold", threshold.as_str()),
                ],
            )
        );
    }
    Ok(())
}

/// `pinto sprint <sub>` — Sprint creation, editing, deletion, state transition, assignment, and list.
///
/// User errors such as invalid ID format, non-existent ID, invalid state transition, etc. will be assigned code 1 by `main`.
pub(super) async fn cmd_sprint(args: SprintArgs) -> anyhow::Result<ExitCode> {
    cmd_sprint_with_localizer(args, current()).await
}

/// Execute a Sprint command with an explicit localizer.
///
/// Production dispatch uses [`cmd_sprint`], which selects the locale from the process
/// environment. Tests can call this seam with a deterministic localizer without mutating
/// process-global environment variables.
pub(super) async fn cmd_sprint_with_localizer(
    args: SprintArgs,
    localizer: &Localizer,
) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    match args.command {
        SprintCommand::New {
            id,
            title,
            goal,
            template,
            start,
            end,
        } => {
            let id: SprintId = id.parse()?;
            // `--start` / `--end` are guaranteed to be matched by clap's `requires` and are interpreted as UTC.
            let period = match (start, end) {
                (Some(s), Some(e)) => Some((s, e)),
                _ => None,
            };
            let goal = match template {
                Some(template) => {
                    let template: TemplateName = template.parse()?;
                    Some(template_body(&dir, TemplateKind::Sprint, &template).await?)
                }
                None => goal,
            };
            let sprint = create_sprint(&dir, &id, &title, goal, period).await?;
            println!(
                "{}",
                localizer.format(
                    Message::CreatedSprint,
                    [
                        ("id", sprint.id.to_string().as_str()),
                        ("title", sprint.title.as_str()),
                    ],
                )
            );
        }
        SprintCommand::Edit {
            id,
            title,
            goal,
            start,
            end,
        } => {
            let id: SprintId = id.parse()?;
            let period = match (start, end) {
                (Some(start), Some(end)) => Some((start, end)),
                _ => None,
            };
            let sprint = edit_sprint(&dir, &id, title, goal, period).await?;
            println!(
                "{}",
                localizer.format(
                    Message::UpdatedSprint,
                    [("id", sprint.id.to_string().as_str())],
                )
            );
        }
        SprintCommand::Remove { id } => {
            let id: SprintId = id.parse()?;
            delete_sprint(&dir, &id).await?;
            println!(
                "{}",
                localizer.format(Message::DeletedSprint, [("id", id.to_string().as_str())])
            );
        }
        SprintCommand::Start { id } => {
            let id: SprintId = id.parse()?;
            let sprint = start_sprint(&dir, &id).await?;
            println!(
                "{}",
                localizer.format(
                    Message::StartedSprint,
                    [("id", sprint.id.to_string().as_str())],
                )
            );
            warn_sprint_load(&dir, &sprint.id, localizer).await?;
        }
        SprintCommand::Close {
            id,
            rollover,
            release,
        } => {
            let id: SprintId = id.parse()?;
            let action = if let Some(target) = rollover {
                SprintCloseAction::Rollover(target.parse()?)
            } else if release {
                SprintCloseAction::Release
            } else {
                SprintCloseAction::Retain
            };
            let sprint = close_sprint(&dir, &id, action).await?;
            println!(
                "{}",
                localizer.format(
                    Message::ClosedSprint,
                    [("id", sprint.id.to_string().as_str())],
                )
            );
        }
        SprintCommand::Add {
            sprint_id,
            item_id,
            status,
            limit,
        } => {
            if let Some(item_id) = item_id {
                let item_id: ItemId = item_id.parse()?;
                let sprint_id: SprintId = sprint_id.parse()?;
                let item = assign_sprint_raw(&dir, sprint_id.as_str(), &item_id).await?;
                println!(
                    "{}",
                    localizer.format(
                        Message::AssignedToSprint,
                        [
                            ("id", item.id.to_string().as_str()),
                            ("sprint", sprint_id.as_str()),
                        ],
                    )
                );
                warn_sprint_load(&dir, &sprint_id, localizer).await?;
            } else if let Some(status) = status {
                let sprint_id: SprintId = sprint_id.parse()?;
                let assigned = assign_sprint_by_status(&dir, &sprint_id, &status, limit).await?;
                for item in &assigned {
                    println!(
                        "{}",
                        localizer.format(
                            Message::AssignedToSprint,
                            [
                                ("id", item.id.to_string().as_str()),
                                ("sprint", sprint_id.to_string().as_str()),
                            ],
                        )
                    );
                }
                if !assigned.is_empty() {
                    warn_sprint_load(&dir, &sprint_id, localizer).await?;
                }
            } else {
                return Err(anyhow::anyhow!(
                    "{}",
                    localizer.text(Message::SprintAddRequiresItemOrStatus)
                ));
            }
        }
        SprintCommand::Unassign { sprint_id, item_id } => {
            let sprint_id: SprintId = sprint_id.parse()?;
            let item_id: ItemId = item_id.parse()?;
            let item = unassign_sprint(&dir, &sprint_id, &item_id).await?;
            println!(
                "{}",
                localizer.format(
                    Message::UnassignedFromSprint,
                    [
                        ("id", item.id.to_string().as_str()),
                        ("sprint", sprint_id.to_string().as_str()),
                    ],
                )
            );
        }
        SprintCommand::List { json } => {
            let sprints = list_sprints(&dir).await?;
            if json {
                println!("{}", sprints_json(&sprints)?);
            } else if sprints.is_empty() {
                println!("{}", localizer.text(Message::NoSprints));
            } else {
                let timezone = display_settings(&dir).await?.timezone;
                print!("{}", format_sprints_with_timezone(&sprints, timezone));
            }
        }
        SprintCommand::Burndown { id, json } => {
            let id: SprintId = id.parse()?;
            let chart = burndown(&dir, &id).await?;
            if json {
                println!("{}", burndown_json(&chart)?);
            } else {
                print!("{}", format_burndown(&chart, terminal_width()));
            }
        }
        SprintCommand::Velocity { recent } => {
            let report = velocity(&dir, recent).await?;
            if report.sprints.is_empty() {
                println!("{}", localizer.text(Message::NoSprints));
            } else {
                print!("{}", format_velocity(&report, recent));
            }
        }
        SprintCommand::Capacity {
            id,
            daily_hours,
            holidays,
            deduction_factor,
            json,
        } => {
            let id: SprintId = id.parse()?;
            let capacity = match (daily_hours, holidays, deduction_factor) {
                (Some(hours), Some(holidays), Some(factor)) => {
                    set_sprint_capacity(&dir, &id, hours, holidays, factor).await?
                }
                (None, None, None) => sprint_capacity(&dir, &id).await?,
                _ => {
                    return Err(anyhow::anyhow!(
                        "{}",
                        localizer.text(Message::InvalidCapacityOptions)
                    ));
                }
            };
            if json {
                println!("{}", sprint_capacity_json(&capacity)?);
            } else {
                print!("{}", format_sprint_capacity(&capacity));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
