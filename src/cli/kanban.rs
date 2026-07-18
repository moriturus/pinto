//! Interactive Kanban (`kanban` subcommand).
//!
//! The display state ([`BoardView`], in [`view`]) is a pure view model with no dependency on the
//! terminal or drawing, which keeps navigation, in-column ordering, and horizontal scrolling unit
//! testable. It shares the domain and persistence layers with the rest of the CLI. Row layout lives
//! in [`layout`], text wrapping in [`text`], and drawing plus the event loop in [`runtime`].

// `runtime` reaches the drawing helpers and dependency formatting through `super::…`, and the
// `tests` module reaches them through `super::super::…`; the imports and re-exports below expose
// exactly those names at this module's root.
use super::dependency_display::{DEP_ID_LIMIT, DepSummary, dependency_legend, format_ids};

mod keymap;
mod layout;
/// Drawing and the interactive event loop.
mod runtime;
mod text;
mod view;

pub(crate) use layout::{DisplayRow, PopupContent};
pub(crate) use runtime::ExitMode;
pub(crate) use text::{display_width, wrap};
pub(crate) use view::{BoardView, InputMode, InputSubmission, InputValidation, MIN_COLUMN_WIDTH};

// Names used only by the `tests` module, reached via `use super::super::*`.
#[cfg(test)]
use layout::column_display_rows;
#[cfg(test)]
use pinto::backlog::ItemId;
#[cfg(test)]
use pinto::service::ReorderTarget;
#[cfg(test)]
use std::collections::HashSet;

/// Run the `kanban` subcommand on the board in the current directory. Return how the view was left
/// ([`ExitMode`]) so the caller can terminate or hand off to the interactive shell.
pub(crate) async fn run(
    columns: Option<&[String]>,
    maximize: bool,
    query: pinto::service::BoardQuery,
) -> anyhow::Result<ExitMode> {
    let dir = std::env::current_dir()?;
    // Whether confirmation of exit is required depends on the setting (`[tui] confirm_quit`).
    let settings = pinto::service::tui_settings(&dir).await?;
    let workflow = settings.workflow;
    let display_columns = resolve_display_columns(&workflow, &settings.hidden_columns, columns)?;
    let initial_column = columns
        .and_then(|columns| columns.first())
        .map(String::as_str);
    runtime::run(runtime::RunOptions {
        dir,
        confirm_quit: settings.confirm_quit,
        bindings: settings.key_bindings,
        markdown: settings.markdown,
        timezone: settings.timezone,
        display_columns,
        initial_column: initial_column.map(str::to_owned),
        maximize,
        query,
    })
    .await
}

/// Resolve the columns rendered by Kanban while preserving the workflow order.
///
/// Explicit CLI values override the configured hidden columns. Unknown CLI values are rejected
/// before the terminal is initialized so the error explains which column needs fixing.
fn resolve_display_columns(
    workflow: &[String],
    hidden_columns: &[String],
    requested: Option<&[String]>,
) -> anyhow::Result<Vec<String>> {
    if let Some(requested) = requested {
        if let Some(unknown) = requested
            .iter()
            .find(|column| !workflow.iter().any(|known| known == *column))
        {
            anyhow::bail!("column not found: {unknown}");
        }
        return Ok(workflow
            .iter()
            .filter(|column| requested.iter().any(|wanted| wanted == *column))
            .cloned()
            .collect());
    }

    Ok(workflow
        .iter()
        .filter(|column| !hidden_columns.iter().any(|hidden| hidden == *column))
        .cloned()
        .collect())
}

#[cfg(test)]
mod tests;
