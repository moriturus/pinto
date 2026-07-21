//! Execute CLI commands.

use super::args::*;
use super::format::DEFAULT_TERM_WIDTH;
use clap::Parser;
use pinto::backlog::ItemId;
use pinto::error::Error;
use pinto::i18n::{Localizer, Message, current};
use pinto::service::SearchFilter;
use pinto::service::{LabelMatch, WipViolation, lock_board};

use std::path::Path;
use std::process::ExitCode;

mod automation;
mod board;
mod item;
mod maintenance;
mod relations;
mod session;
mod sprint;
#[cfg(test)]
mod tests;

pub(crate) async fn entrypoint() -> ExitCode {
    // clap defaults to exit code 2 for argument errors, but pinto reserves 2 for internal failures.
    // Help/version are successful (0); other argument errors are user-fixable (1).
    let localizer = current();
    let cli = match try_parse_localized(localizer) {
        Ok(cli) => cli,
        Err(e) => {
            // Utilize clap's formatting (color/stream distribution) as is. `--help` / `--version`
            // is output to stdout, and interpretation errors are output to stderr.
            let _ = e.print();
            return if e.use_stderr() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    // The top-level invocation is not inside a shell, so `Q` in the Kanban view starts one.
    match dispatch(cli, false).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!(
                "{} {}",
                localizer.text(Message::ErrorPrefix),
                format_anyhow_error(&e, localizer)
            );
            // User-induced errors (including malformed board files) are code 1; unexpected
            // internal errors are code 2. Classification is centralized in `Error::is_user_error()`.
            match e.downcast_ref::<Error>() {
                Some(err) if err.is_user_error() => ExitCode::from(1),
                _ => ExitCode::from(2),
            }
        }
    }
}

/// Render an anyhow chain while translating pinto-owned error variants at the CLI boundary.
///
/// Context and external diagnostics remain in their original text; only the crate's structured
/// [`Error`] values are selected from the Fluent catalog.
pub(crate) fn format_anyhow_error(error: &anyhow::Error, localizer: &Localizer) -> String {
    error
        .chain()
        .map(|cause| {
            cause
                .downcast_ref::<Error>()
                .map_or_else(|| cause.to_string(), |error| error.localized(localizer))
        })
        .collect::<Vec<_>>()
        .join(": ")
}

/// Dispatch a parsed command. `in_shell` is `true` when invoked from the interactive shell (REPL),
/// which only affects the Kanban view's `Q` handoff: from a shell it returns to the existing prompt,
/// while a direct invocation starts a new shell.
async fn dispatch(mut cli: Cli, in_shell: bool) -> anyhow::Result<ExitCode> {
    let invocation_dir = std::env::current_dir()?;
    anchor_automation_plan_path(&mut cli, &invocation_dir).await?;
    let init = matches!(&cli.command, Command::Init);
    let completion = matches!(&cli.command, Command::Completion(_));
    let automation = matches!(&cli.command, Command::Automate(_));
    super::location::prepare_working_directory(cli.dir.as_deref(), init, completion, automation)
        .await?;

    let result = match cli.command {
        Command::Init => maintenance::cmd_init().await,
        Command::Add(args) => item::cmd_add(args).await,
        Command::List(args) => item::cmd_list(args).await,
        Command::Next(args) => item::cmd_next(args).await,
        Command::Show(args) => item::cmd_show(args).await,
        Command::Move(args) => item::cmd_move(args).await,
        Command::Reorder(args) => item::cmd_reorder(args).await,
        Command::Edit(args) => item::cmd_edit(args).await,
        Command::Remove(args) => item::cmd_rm(args).await,
        Command::Restore(args) => item::cmd_restore(args).await,
        Command::Dep(args) => relations::cmd_dep(args).await,
        Command::Link(args) => relations::cmd_link(args).await,
        Command::Dod(args) => relations::cmd_dod(args).await,
        Command::Export(args) => board::cmd_export(args).await,
        Command::Import(args) => maintenance::cmd_import(args).await,
        Command::Sprint(args) => sprint::cmd_sprint(args).await,
        Command::Board(args) => board::cmd_board(args).await,
        Command::CycleTime(args) => board::cmd_cycletime(args).await,
        Command::Rebalance(args) => maintenance::cmd_rebalance(args).await,
        Command::Migrate(args) => maintenance::cmd_migrate(args).await,
        Command::Doctor(args) => maintenance::cmd_doctor(args).await,
        Command::Undo => maintenance::cmd_undo().await,
        Command::Automate(args) => automation::cmd_automate(args).await,
        Command::Shell => session::cmd_shell().await,
        Command::Kanban(args) => session::cmd_kanban(args, in_shell).await,
        Command::Completion(args) => session::cmd_completion(args),
    };

    // Nested shell dispatches may select another board for one command. Restore the shell's
    // original directory before returning so a later REPL command keeps its prior context.
    if let Err(error) = std::env::set_current_dir(&invocation_dir) {
        return Err(anyhow::Error::new(error).context(format!(
            "failed to restore invocation directory {}",
            invocation_dir.display()
        )));
    }
    result
}

/// Keep an automation plan path relative to the directory where the user invoked pinto.
///
/// Board discovery changes the process working directory before command execution. Plan files are
/// ordinary caller-owned paths, so anchor relative paths before that change while preserving
/// inline JSON and the existing precedence for a file whose name starts with `{`.
async fn anchor_automation_plan_path(cli: &mut Cli, invocation_dir: &Path) -> anyhow::Result<()> {
    let Command::Automate(args) = &mut cli.command else {
        return Ok(());
    };
    let Some(plan) = args.plan.as_mut() else {
        return Ok(());
    };
    if plan == "-" || Path::new(plan).is_absolute() {
        return Ok(());
    }

    let inline_json = plan.trim_start().starts_with('{');
    let candidate = invocation_dir.join(&*plan);
    let exists = match tokio::fs::try_exists(&candidate).await {
        Ok(exists) => exists,
        Err(_error) if inline_json => false,
        Err(error) => {
            return Err(Error::Io {
                path: candidate,
                message: error.to_string(),
            }
            .into());
        }
    };
    if exists || !inline_json {
        *plan = candidate.to_string_lossy().into_owned();
    }
    Ok(())
}

/// Warn to stderr that the WIP limit has been exceeded, with instructions on how to fix it.
fn warn_wip(v: &WipViolation) {
    eprintln!(
        "{}",
        current().format(
            Message::WipLimitExceeded,
            [
                ("column", v.column.as_str()),
                ("count", v.count.to_string().as_str()),
                ("limit", v.limit.to_string().as_str()),
            ],
        )
    );
}

/// Return the terminal width in columns. Use [`DEFAULT_TERM_WIDTH`] when output is not a TTY or
/// the terminal width cannot be determined.
fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(terminal_size::Width(w), _)| usize::from(w))
        .filter(|&w| w > 0)
        .unwrap_or(DEFAULT_TERM_WIDTH)
}

/// Validate and build a search filter once at the CLI boundary.
fn build_search_filter(
    pattern: Option<String>,
    regex: bool,
) -> anyhow::Result<Option<SearchFilter>> {
    pattern
        .map(|pattern| SearchFilter::new(pattern, regex))
        .transpose()
        .map_err(Into::into)
}

/// Resolve a filter option whose value is optional only in long display mode.
fn resolve_optional_filter(
    option: &str,
    value: Option<Option<String>>,
    long: bool,
) -> anyhow::Result<Option<String>> {
    match value {
        Some(Some(value)) => Ok(Some(value)),
        Some(None) if long => Ok(None),
        Some(None) => Err(Error::InvalidFilterOption(format!(
            "{option} requires a value unless --long is specified"
        ))
        .into()),
        None => Ok(None),
    }
}

/// Resolve `--label`, which accepts multiple values and can be bare only in long display mode.
fn resolve_label_filter(
    option: &str,
    value: Option<Vec<String>>,
    long: bool,
) -> anyhow::Result<Vec<String>> {
    match value {
        Some(values) if !values.is_empty() => Ok(values),
        Some(_) if long => Ok(Vec::new()),
        Some(_) => Err(Error::InvalidFilterOption(format!(
            "{option} requires a value unless --long is specified"
        ))
        .into()),
        None => Ok(Vec::new()),
    }
}

/// Select the label matching mode and reject an AND request without a label value.
fn resolve_label_match(all_labels: bool, labels: &[String]) -> anyhow::Result<LabelMatch> {
    if all_labels && labels.is_empty() {
        return Err(Error::InvalidFilterOption(
            "--all-labels requires at least one --label value".to_string(),
        )
        .into());
    }
    Ok(if all_labels {
        LabelMatch::All
    } else {
        LabelMatch::Any
    })
}
