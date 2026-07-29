//! Sprint management commands.

use crate::cli::args::*;
use crate::cli::format::report::format_burndown;
use crate::cli::format::sprint::{
    format_sprint_capacity, format_sprint_context, format_sprint_goal_report,
    format_sprints_with_timezone, format_velocity,
};
use crate::cli::json::{
    burndown_json, review_json, reviews_json, sprint_capacity_json, sprint_goal_report_json,
    sprints_json,
};
use pinto::automation::AutomationProducerResult;
use pinto::backlog::{ActionSource, ActionSourceKind, BacklogItem, ItemId};
use pinto::i18n::{Localizer, Message, current};
use pinto::service::{
    SprintCloseAction, SprintDeletionOptions, assign_sprint_by_status, assign_sprint_raw, burndown,
    close_sprint, create_sprint, create_sprint_retro, create_sprint_review,
    delete_sprint_with_options, display_settings, edit_sprint, edit_sprint_retro,
    edit_sprint_review, linked_action_items, list_sprint_retros, list_sprint_reviews, list_sprints,
    set_sprint_capacity, show_sprint_retro, show_sprint_review, sprint_capacity, sprint_context,
    sprint_goal_report, sprint_load_warnings, start_sprint, template_body, unassign_sprint,
    velocity,
};

use pinto::retro::SprintRetro;
use pinto::review::SprintReview;
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

/// Resolve a Retro creation body from direct text, a plain-text template, and optional editor
/// input using the same precedence as the item add command.
async fn retro_creation_body(
    dir: &Path,
    sprint_id: &SprintId,
    body: Option<String>,
    template: Option<String>,
    edit: bool,
) -> anyhow::Result<String> {
    let template_body = if let Some(template) = template {
        let template: TemplateName = template.parse()?;
        Some(template_body(dir, TemplateKind::Retro, &template).await?)
    } else {
        None
    };
    if edit {
        let initial = template_body.unwrap_or_default();
        let slug = format!("retro-{sprint_id}");
        return tokio::task::spawn_blocking(move || {
            crate::cli::editor::edit_in_editor(&initial, &slug)
        })
        .await?;
    }

    Ok(match (template_body, body) {
        (Some(template), Some(body)) => super::item::combine_template_body(template, body),
        (Some(template), None) => template,
        (None, Some(body)) => body,
        (None, None) => String::new(),
    })
}

/// Open the standard editor for an existing Retro and return the edited body.
async fn edit_retro_in_editor(retro: &SprintRetro) -> anyhow::Result<String> {
    if crate::cli::editor::resolve_editor().is_none() {
        return Err(pinto::error::Error::EditorNotSet.into());
    }
    let initial = retro.body.clone();
    let slug = format!("retro-{}", retro.id);
    tokio::task::spawn_blocking(move || crate::cli::editor::edit_in_editor(&initial, &slug)).await?
}

/// Resolve a Review creation body from direct text, a plain-text template, and optional editor
/// input using the same precedence as the item add command.
async fn review_creation_body(
    dir: &Path,
    sprint_id: &SprintId,
    body: Option<String>,
    template: Option<String>,
    edit: bool,
) -> anyhow::Result<String> {
    let template_body = if let Some(template) = template {
        let template: TemplateName = template.parse()?;
        Some(template_body(dir, TemplateKind::Review, &template).await?)
    } else {
        None
    };
    if edit {
        let initial = template_body.unwrap_or_default();
        let slug = format!("review-{sprint_id}");
        return tokio::task::spawn_blocking(move || {
            crate::cli::editor::edit_in_editor(&initial, &slug)
        })
        .await?;
    }

    Ok(match (template_body, body) {
        (Some(template), Some(body)) => super::item::combine_template_body(template, body),
        (Some(template), None) => template,
        (None, Some(body)) => body,
        (None, None) => String::new(),
    })
}

/// Open the standard editor for an existing Review and return the edited body.
async fn edit_review_in_editor(review: &SprintReview) -> anyhow::Result<String> {
    if crate::cli::editor::resolve_editor().is_none() {
        return Err(pinto::error::Error::EditorNotSet.into());
    }
    let initial = review.body.clone();
    let slug = format!("review-{}", review.id);
    tokio::task::spawn_blocking(move || crate::cli::editor::edit_in_editor(&initial, &slug)).await?
}

/// Create one ordinary PBI linked to a Retro or Review source record.
async fn create_action_pbi(
    dir: &Path,
    kind: ActionSourceKind,
    sprint_id: SprintId,
    title: String,
    creation: PbiCreationArgs,
) -> anyhow::Result<()> {
    let source = ActionSource::new(kind, sprint_id);
    let outcome = super::item::create_pbi_with_options(dir, &title, creation, Some(source)).await?;
    if outcome.cycle_warning {
        eprintln!(
            "{}",
            pinto::i18n::current().text(Message::DependencyCycleWarningGeneric)
        );
    }
    let id = outcome.item.id.to_string();
    println!(
        "{}",
        pinto::i18n::current().format(
            Message::Created,
            [("id", id.as_str()), ("title", outcome.item.title.as_str())],
        )
    );
    super::item::emit_automation_producer_result(AutomationProducerResult {
        created_ids: vec![id],
        updated_ids: Vec::new(),
    })?;
    Ok(())
}

/// Render the active action PBIs linked to a Retro or Review.
fn format_linked_actions(actions: &[BacklogItem], localizer: &Localizer) -> String {
    let mut out = format!("{}\n", localizer.text(Message::LinkedActionPbis));
    if actions.is_empty() {
        out.push_str(&format!(
            "  {}\n",
            localizer.text(Message::NoLinkedActionPbis)
        ));
    } else {
        for action in actions {
            out.push_str(&format!(
                "  {}  {}  {}\n",
                action.id, action.status, action.title
            ));
        }
    }
    out
}

/// Render one Review for the human-readable detail view.
fn format_review_detail(review: &SprintReview) -> String {
    if review.body.is_empty() {
        format!("{}\n", review.id)
    } else {
        format!("{}\n{}\n", review.id, review.body)
    }
}

/// Render a Review with generated parent-Sprint context followed by authored Markdown.
fn format_review_detail_with_context(
    review: &SprintReview,
    context: &pinto::service::SprintContext,
    actions: &[BacklogItem],
    timezone: pinto::timezone::DisplayTimezone,
    localizer: &Localizer,
) -> String {
    format!(
        "{}\n{}Markdown\n{}",
        format_sprint_context(context, timezone),
        format_linked_actions(actions, localizer),
        format_review_detail(review)
    )
}

/// Render the human-readable Review list.
fn format_review_list(reviews: &[SprintReview]) -> String {
    reviews
        .iter()
        .map(|review| {
            let summary = review.body.lines().next().unwrap_or("");
            if summary.is_empty() {
                format!("{}\n", review.id)
            } else {
                format!("{}  {}\n", review.id, summary)
            }
        })
        .collect()
}

/// Execute the sprint review namespace, including its direct creation shorthand.
async fn cmd_review(args: ReviewArgs, localizer: &Localizer) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    match args.command {
        Some(ReviewCommand::New {
            sprint_id,
            body,
            template,
            edit,
        }) => {
            let sprint_id: SprintId = sprint_id.parse()?;
            let body = review_creation_body(&dir, &sprint_id, body, template, edit).await?;
            let review = create_sprint_review(&dir, &sprint_id, body).await?;
            println!(
                "{}",
                localizer.format(
                    Message::CreatedReview,
                    [("id", review.id.to_string().as_str())],
                )
            );
        }
        Some(ReviewCommand::Edit {
            sprint_id,
            body,
            edit: _,
        }) => {
            let sprint_id: SprintId = sprint_id.parse()?;
            let body = match body {
                Some(body) => body,
                None => {
                    let review = show_sprint_review(&dir, &sprint_id).await?;
                    edit_review_in_editor(&review).await?
                }
            };
            let review = edit_sprint_review(&dir, &sprint_id, body).await?;
            println!(
                "{}",
                localizer.format(
                    Message::UpdatedReview,
                    [("id", review.id.to_string().as_str())],
                )
            );
        }
        Some(ReviewCommand::Action {
            sprint_id,
            title,
            creation,
        }) => {
            let sprint_id: SprintId = sprint_id.parse()?;
            create_action_pbi(&dir, ActionSourceKind::Review, sprint_id, title, creation).await?;
        }
        Some(ReviewCommand::Show {
            sprint_id,
            json,
            plain,
        }) => {
            let sprint_id: SprintId = sprint_id.parse()?;
            let review = show_sprint_review(&dir, &sprint_id).await?;
            if json {
                let context = sprint_context(&dir, &sprint_id).await?;
                let actions = linked_action_items(
                    &dir,
                    &ActionSource::new(ActionSourceKind::Review, sprint_id.clone()),
                )
                .await?;
                println!("{}", review_json(&review, &context, &actions)?);
            } else if plain {
                print!("{}", format_review_detail(&review));
            } else {
                let context = sprint_context(&dir, &sprint_id).await?;
                let actions = linked_action_items(
                    &dir,
                    &ActionSource::new(ActionSourceKind::Review, sprint_id.clone()),
                )
                .await?;
                let timezone = display_settings(&dir).await?.timezone;
                print!(
                    "{}",
                    format_review_detail_with_context(
                        &review, &context, &actions, timezone, localizer,
                    )
                );
            }
        }
        Some(ReviewCommand::List { json }) => {
            let reviews = list_sprint_reviews(&dir).await?;
            if json {
                println!("{}", reviews_json(&reviews)?);
            } else if reviews.is_empty() {
                println!("{}", localizer.text(Message::NoReviews));
            } else {
                print!("{}", format_review_list(&reviews));
            }
        }
        None => {
            let sprint_id = args
                .sprint_id
                .ok_or(pinto::error::Error::ReviewCommandRequired)?;
            let sprint_id: SprintId = sprint_id.parse()?;
            let body =
                review_creation_body(&dir, &sprint_id, args.body, args.template, args.edit).await?;
            let review = create_sprint_review(&dir, &sprint_id, body).await?;
            println!(
                "{}",
                localizer.format(
                    Message::CreatedReview,
                    [("id", review.id.to_string().as_str())],
                )
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Render one Retro for the human-readable detail view.
fn format_retro_detail(retro: &SprintRetro) -> String {
    if retro.body.is_empty() {
        format!("{}\n", retro.id)
    } else {
        format!("{}\n{}\n", retro.id, retro.body)
    }
}

/// Render a Retro with generated parent-Sprint context followed by authored Markdown.
fn format_retro_detail_with_context(
    retro: &SprintRetro,
    context: &pinto::service::SprintContext,
    actions: &[BacklogItem],
    timezone: pinto::timezone::DisplayTimezone,
    localizer: &Localizer,
) -> String {
    format!(
        "{}\n{}Markdown\n{}",
        format_sprint_context(context, timezone),
        format_linked_actions(actions, localizer),
        format_retro_detail(retro)
    )
}

/// Render the human-readable Retro list.
fn format_retro_list(retros: &[SprintRetro]) -> String {
    retros
        .iter()
        .map(|retro| {
            let summary = retro.body.lines().next().unwrap_or("");
            if summary.is_empty() {
                format!("{}\n", retro.id)
            } else {
                format!("{}  {}\n", retro.id, summary)
            }
        })
        .collect()
}

/// Execute the sprint retro namespace, including its direct creation shorthand.
async fn cmd_retro(args: RetroArgs, localizer: &Localizer) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    match args.command {
        Some(RetroCommand::New {
            sprint_id,
            body,
            template,
            edit,
        }) => {
            let sprint_id: SprintId = sprint_id.parse()?;
            let body = retro_creation_body(&dir, &sprint_id, body, template, edit).await?;
            let retro = create_sprint_retro(&dir, &sprint_id, body).await?;
            println!(
                "{}",
                localizer.format(
                    Message::CreatedRetro,
                    [("id", retro.id.to_string().as_str())],
                )
            );
        }
        Some(RetroCommand::Edit {
            sprint_id,
            body,
            edit: _,
        }) => {
            let sprint_id: SprintId = sprint_id.parse()?;
            let body = match body {
                Some(body) => body,
                None => {
                    let retro = show_sprint_retro(&dir, &sprint_id).await?;
                    edit_retro_in_editor(&retro).await?
                }
            };
            let retro = edit_sprint_retro(&dir, &sprint_id, body).await?;
            println!(
                "{}",
                localizer.format(
                    Message::UpdatedRetro,
                    [("id", retro.id.to_string().as_str())],
                )
            );
        }
        Some(RetroCommand::Action {
            sprint_id,
            title,
            creation,
        }) => {
            let sprint_id: SprintId = sprint_id.parse()?;
            create_action_pbi(&dir, ActionSourceKind::Retro, sprint_id, title, creation).await?;
        }
        Some(RetroCommand::Show {
            sprint_id,
            json,
            plain,
        }) => {
            let sprint_id: SprintId = sprint_id.parse()?;
            let retro = show_sprint_retro(&dir, &sprint_id).await?;
            if json {
                let context = sprint_context(&dir, &sprint_id).await?;
                let actions = linked_action_items(
                    &dir,
                    &ActionSource::new(ActionSourceKind::Retro, sprint_id.clone()),
                )
                .await?;
                println!(
                    "{}",
                    crate::cli::json::retro_json(&retro, &context, &actions)?
                );
            } else if plain {
                print!("{}", format_retro_detail(&retro));
            } else {
                let context = sprint_context(&dir, &sprint_id).await?;
                let actions = linked_action_items(
                    &dir,
                    &ActionSource::new(ActionSourceKind::Retro, sprint_id.clone()),
                )
                .await?;
                let timezone = display_settings(&dir).await?.timezone;
                print!(
                    "{}",
                    format_retro_detail_with_context(
                        &retro, &context, &actions, timezone, localizer,
                    )
                );
            }
        }
        Some(RetroCommand::List { json }) => {
            let retros = list_sprint_retros(&dir).await?;
            if json {
                println!("{}", crate::cli::json::retros_json(&retros)?);
            } else if retros.is_empty() {
                println!("{}", localizer.text(Message::NoRetros));
            } else {
                print!("{}", format_retro_list(&retros));
            }
        }
        None => {
            let sprint_id = args
                .sprint_id
                .ok_or(pinto::error::Error::RetroCommandRequired)?;
            let sprint_id: SprintId = sprint_id.parse()?;
            let body =
                retro_creation_body(&dir, &sprint_id, args.body, args.template, args.edit).await?;
            let retro = create_sprint_retro(&dir, &sprint_id, body).await?;
            println!(
                "{}",
                localizer.format(
                    Message::CreatedRetro,
                    [("id", retro.id.to_string().as_str())],
                )
            );
        }
    }
    Ok(ExitCode::SUCCESS)
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
            goal_achieved,
            clear_goal_achieved,
            start,
            end,
        } => {
            let id: SprintId = id.parse()?;
            let period = match (start, end) {
                (Some(start), Some(end)) => Some((start, end)),
                _ => None,
            };
            let sprint = edit_sprint(
                &dir,
                &id,
                title,
                goal,
                period,
                goal_achieved,
                clear_goal_achieved,
            )
            .await?;
            println!(
                "{}",
                localizer.format(
                    Message::UpdatedSprint,
                    [("id", sprint.id.to_string().as_str())],
                )
            );
        }
        SprintCommand::Remove { id, delete_records } => {
            let id: SprintId = id.parse()?;
            delete_sprint_with_options(&dir, &id, SprintDeletionOptions { delete_records }).await?;
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
        SprintCommand::Goal { recent, json } => {
            let report = sprint_goal_report(&dir, recent).await?;
            if json {
                println!("{}", sprint_goal_report_json(&report)?);
            } else {
                print!("{}", format_sprint_goal_report(&report, recent));
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
        SprintCommand::Retro(args) => return cmd_retro(args, localizer).await,
        SprintCommand::Review(args) => return cmd_review(args, localizer).await,
    }
    Ok(ExitCode::SUCCESS)
}
