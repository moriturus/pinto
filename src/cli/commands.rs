//! Execute CLI commands.

use super::args::*;
use super::format::DEFAULT_TERM_WIDTH;
use super::format::board::{format_board, format_board_long};
use super::format::item::{
    DetailOptions, ListLongOptions, format_detail, format_list, format_list_long,
};
use super::format::report::{format_burndown, format_cycletime};
use super::format::sprint::{
    format_sprint_capacity, format_sprints_with_timezone, format_velocity,
};
use super::json::{
    board_json, burndown_json, cycletime_json, detail_json, list_json, sprint_capacity_json,
    sprints_json,
};
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use pinto::automation::{AutomationCommandResult, AutomationPlan, AutomationReport};
use pinto::backlog::ItemId;
use pinto::error::Error;
use pinto::i18n::{Localizer, Message, current};
use pinto::service::SearchFilter;
use pinto::service::{
    BoardQuery, CycleTimeFilter, EditOutcome, InitOutcome, ItemEdit, LabelMatch, ListFilter,
    MigrateOutcome, MoveOutcome, NewItem, NextFilter, RemoveOutcome, ReorderTarget, SortKey,
    SprintCloseAction, WipViolation, add_dependency, add_item_with_outcome, apply_item_edit,
    assign_sprint_by_status, assign_sprint_raw, board, burndown, check_wip, clear_common_dod,
    close_sprint, common_dod, create_sprint, cycle_time, delete_sprint, display_settings,
    edit_item, edit_sprint, init_board, item_detail, item_edit_template, link_commits, list_items,
    list_sprints, lock_board, migrate_storage, move_item_with_outcome, next_items, rebalance,
    remove_dependency, remove_item, reorder_item, set_common_dod, set_sprint_capacity,
    sprint_capacity, start_sprint, sync_commits, template_body, unassign_sprint, unlink_commits,
    velocity,
};
use std::io::{IsTerminal, Read};

use pinto::sprint::SprintId;
use pinto::storage::StorageBackend;
use pinto::template::{TemplateKind, TemplateName};
use std::path::Path;
use std::process::{ExitCode, Stdio};
use tokio::process::Command as ProcessCommand;

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
fn format_anyhow_error(error: &anyhow::Error, localizer: &Localizer) -> String {
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
        Command::Init => cmd_init().await,
        Command::Add(args) => cmd_add(args).await,
        Command::List(args) => cmd_list(args).await,
        Command::Next(args) => cmd_next(args).await,
        Command::Show(args) => cmd_show(args).await,
        Command::Move(args) => cmd_move(args).await,
        Command::Reorder(args) => cmd_reorder(args).await,
        Command::Edit(args) => cmd_edit(args).await,
        Command::Remove(args) => cmd_rm(args).await,
        Command::Dep(args) => cmd_dep(args).await,
        Command::Link(args) => cmd_link(args).await,
        Command::Dod(args) => cmd_dod(args).await,
        Command::Sprint(args) => cmd_sprint(args).await,
        Command::Board(args) => cmd_board(args).await,
        Command::CycleTime(args) => cmd_cycletime(args).await,
        Command::Rebalance(args) => cmd_rebalance(args).await,
        Command::Migrate(args) => cmd_migrate(args).await,
        Command::Doctor(args) => cmd_doctor(args).await,
        Command::Automate(args) => cmd_automate(args).await,
        Command::Shell => cmd_shell().await,
        Command::Kanban(args) => cmd_kanban(args, in_shell).await,
        Command::Completion(args) => cmd_completion(args),
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
    if args.plan == "-" || Path::new(&args.plan).is_absolute() {
        return Ok(());
    }

    let inline_json = args.plan.trim_start().starts_with('{');
    let candidate = invocation_dir.join(&args.plan);
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
        args.plan = candidate.to_string_lossy().into_owned();
    }
    Ok(())
}

/// `pinto automate` — Executes a plan generated by an external AI agent as an existing CLI.
///
/// The plan is a JSON argv array and does not start a shell. Therefore, the output of the agent
/// Does not evaluate as shellcode, performs normal `clap` validation, service layer, and plain text saving.
/// You can pass it as is. Reject unknown fields containing API keys and do not log the input value itself.
async fn cmd_automate(args: AutomateArgs) -> anyhow::Result<ExitCode> {
    let input = read_automation_plan(&args.plan).await?;
    let plan = AutomationPlan::parse(&input).map_err(|_| Error::InvalidAutomationPlan)?;
    let validated = validate_automation_commands(&plan);

    if validated.iter().any(|command| command.error.is_some()) {
        let commands = validated
            .iter()
            .map(|command| AutomationCommandResult {
                index: command.index,
                command: command.name.clone(),
                status: if command.error.is_some() {
                    "invalid".to_string()
                } else {
                    "valid".to_string()
                },
                created_ids: Vec::new(),
                updated_ids: automation_target_ids(&command.argv),
                error: command.error.clone(),
            })
            .collect();
        let report = AutomationReport {
            status: "invalid".to_string(),
            dry_run: args.dry_run,
            commands,
        };
        if args.json {
            print_automation_json(&report)?;
        } else {
            print_automation_validation(&report, false);
        }
        return Ok(ExitCode::from(1));
    }

    if args.dry_run {
        let dir = std::env::current_dir()?;
        let report = dry_run_automation(&dir, &validated).await?;
        if args.json {
            print_automation_json(&report)?;
        } else {
            print_automation_validation(&report, report.status == "dry_run");
        }
        return Ok(if report.status == "dry_run" {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }

    let dir = std::env::current_dir()?;
    let mut results = Vec::with_capacity(validated.len());
    let mut internal_failure = false;
    let mut failed_at = None;

    for (position, command) in validated.iter().enumerate() {
        let execution = run_automation_command(&dir, &command.argv).await?;
        if execution.success {
            if !args.json {
                print!("{}", execution.stdout);
            }
            results.push(automation_execution_result(
                command,
                &execution,
                "succeeded",
            ));
        } else {
            internal_failure = execution.exit_code != Some(1);
            results.push(automation_execution_result(command, &execution, "failed"));
            failed_at = Some(position);
            for skipped in validated.iter().skip(position + 1) {
                results.push(AutomationCommandResult {
                    index: skipped.index,
                    command: skipped.name.clone(),
                    status: "skipped".to_string(),
                    created_ids: Vec::new(),
                    updated_ids: automation_target_ids(&skipped.argv),
                    error: Some(current().text(Message::AutomationNotExecutedAfterFailure)),
                });
            }
            break;
        }
    }

    let failed = failed_at.is_some();
    let report = AutomationReport {
        status: if failed {
            "partial_failure".to_string()
        } else {
            "completed".to_string()
        },
        dry_run: false,
        commands: results,
    };

    if args.json {
        print_automation_json(&report)?;
    } else if let Some(failed_at) = failed_at {
        let failed_command = &report.commands[failed_at];
        let index = failed_command.index.to_string();
        let completed = report
            .commands
            .iter()
            .filter(|command| command.status == "succeeded")
            .count()
            .to_string();
        let failed_count = report
            .commands
            .iter()
            .filter(|command| command.status == "failed")
            .count()
            .to_string();
        let skipped = report
            .commands
            .iter()
            .filter(|command| command.status == "skipped")
            .count()
            .to_string();
        let error = failed_command.error.as_deref().unwrap_or("unknown error");
        eprintln!(
            "{}",
            current().format(
                Message::AutomationCommandFailed,
                [
                    ("index", index.as_str()),
                    ("command", failed_command.command.as_str()),
                    ("error", error),
                ],
            )
        );
        eprintln!(
            "{}",
            current().format(
                Message::AutomationPartialFailure,
                [
                    ("index", index.as_str()),
                    ("command", failed_command.command.as_str()),
                    ("completed", completed.as_str()),
                    ("failed", failed_count.as_str()),
                    ("skipped", skipped.as_str()),
                ],
            )
        );
        for skipped_command in report
            .commands
            .iter()
            .filter(|command| command.status == "skipped")
        {
            let skipped_index = skipped_command.index.to_string();
            eprintln!(
                "{}",
                current().format(
                    Message::AutomationCommandSkipped,
                    [
                        ("index", skipped_index.as_str()),
                        ("command", skipped_command.command.as_str()),
                    ],
                )
            );
        }
    } else {
        let total = report.commands.len().to_string();
        println!(
            "{}",
            current().format(Message::AutomationCompleted, [("total", total.as_str())])
        );
    }

    Ok(if failed {
        if internal_failure {
            ExitCode::from(2)
        } else {
            ExitCode::from(1)
        }
    } else {
        ExitCode::SUCCESS
    })
}

#[derive(Debug)]
struct ValidatedAutomationCommand {
    index: usize,
    argv: Vec<String>,
    name: String,
    error: Option<String>,
}

#[derive(Debug)]
struct AutomationExecution {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn read_automation_plan(source: &str) -> anyhow::Result<String> {
    if source == "-" {
        let input = tokio::task::spawn_blocking(|| {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            Ok::<String, std::io::Error>(input)
        })
        .await??;
        return Ok(input);
    }

    let inline_json = source.trim_start().starts_with('{');
    let path = Path::new(source);
    let exists = match tokio::fs::try_exists(path).await {
        Ok(exists) => exists,
        // JSON such as `{"commands": [...]}` is not a valid Windows path. Preserve the
        // existing-file precedence while allowing the parser to report malformed inline JSON.
        Err(_error) if inline_json => return Ok(source.to_string()),
        Err(error) => {
            return Err(Error::AutomationPlanSource {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
            .into());
        }
    };
    if exists {
        return tokio::fs::read_to_string(path).await.map_err(|error| {
            Error::AutomationPlanSource {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
            .into()
        });
    }
    if inline_json {
        return Ok(source.to_string());
    }
    Err(Error::AutomationPlanSource {
        path: path.to_path_buf(),
        message: "file does not exist".to_string(),
    }
    .into())
}

fn validate_automation_commands(plan: &AutomationPlan) -> Vec<ValidatedAutomationCommand> {
    plan.commands()
        .iter()
        .enumerate()
        .map(|(position, argv)| {
            let parsed =
                Cli::try_parse_from(std::iter::once("pinto".to_string()).chain(argv.clone()));
            let error = match parsed {
                Err(_) => Some(current().text(Message::AutomationInvalidCommandArguments)),
                Ok(cli) => validate_automation_item_ids(&cli),
            };
            ValidatedAutomationCommand {
                index: position + 1,
                argv: argv.clone(),
                name: automation_command_name(argv),
                error,
            }
        })
        .collect()
}

fn validate_automation_item_ids(cli: &Cli) -> Option<String> {
    let ids: Vec<&String> = match &cli.command {
        Command::Add(args) => args.parent.iter().chain(args.depends_on.iter()).collect(),
        Command::Show(args) => args.ids.iter().collect(),
        Command::Move(args) => args
            .destination_and_ids()
            .map_or_else(Vec::new, |(_, ids)| ids.iter().collect()),
        Command::Reorder(args) => {
            let mut ids = vec![&args.id];
            if let Some(reference) = &args.before {
                ids.push(reference);
            }
            if let Some(reference) = &args.after {
                ids.push(reference);
            }
            ids
        }
        Command::Edit(args) => {
            let mut ids = vec![&args.id];
            if let Some(parent) = &args.parent {
                ids.push(parent);
            }
            ids
        }
        Command::Remove(args) => args.ids.iter().collect(),
        Command::Dep(args) => match &args.command {
            DepCommand::Add { id, depends_on } | DepCommand::Rm { id, depends_on } => {
                vec![id, depends_on]
            }
        },
        Command::Link(args) => match &args.command {
            LinkCommand::Add { id, .. } | LinkCommand::Rm { id, .. } => vec![id],
            LinkCommand::Sync { .. } => Vec::new(),
        },
        Command::Sprint(args) => match &args.command {
            SprintCommand::Add { item_id, .. } => item_id.iter().collect(),
            SprintCommand::Unassign { item_id, .. } => vec![item_id],
            SprintCommand::New { .. }
            | SprintCommand::Edit { .. }
            | SprintCommand::Remove { .. }
            | SprintCommand::Start { .. }
            | SprintCommand::Close { .. }
            | SprintCommand::List { .. }
            | SprintCommand::Burndown { .. }
            | SprintCommand::Velocity { .. }
            | SprintCommand::Capacity { .. } => Vec::new(),
        },
        Command::Init
        | Command::List(_)
        | Command::Next(_)
        | Command::Dod(_)
        | Command::Board(_)
        | Command::CycleTime(_)
        | Command::Rebalance(_)
        | Command::Migrate(_)
        | Command::Doctor(_)
        | Command::Automate(_)
        | Command::Shell
        | Command::Kanban(_)
        | Command::Completion(_) => Vec::new(),
    };

    ids.into_iter()
        .find_map(|raw| raw.parse::<ItemId>().err().map(|error| error.to_string()))
}

async fn run_automation_command(
    dir: &Path,
    argv: &[String],
) -> anyhow::Result<AutomationExecution> {
    let executable = std::env::current_exe()?;
    let output = ProcessCommand::new(executable)
        .args(argv)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    Ok(AutomationExecution {
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

async fn dry_run_automation(
    dir: &Path,
    commands: &[ValidatedAutomationCommand],
) -> anyhow::Result<AutomationReport> {
    // Keep the source board stable while taking the snapshot. The guard also excludes normal
    // writers from changing backend/config state between the board copy and preview execution.
    let _lock = lock_board(dir).await?;
    let workspace = create_dry_run_workspace(dir).await?;
    let report = run_dry_run_commands(&workspace, commands).await;
    let cleanup = tokio::fs::remove_dir_all(&workspace).await;
    match report {
        Err(error) => {
            let _ = cleanup;
            Err(error)
        }
        Ok(report) => {
            cleanup?;
            Ok(report)
        }
    }
}

async fn run_dry_run_commands(
    workspace: &Path,
    commands: &[ValidatedAutomationCommand],
) -> anyhow::Result<AutomationReport> {
    let mut results = Vec::with_capacity(commands.len());
    let mut failed = false;

    for (position, command) in commands.iter().enumerate() {
        let execution = run_automation_command(workspace, &command.argv).await?;
        if execution.success {
            results.push(automation_execution_result(command, &execution, "valid"));
        } else {
            results.push(automation_execution_result(command, &execution, "invalid"));
            for skipped in commands.iter().skip(position + 1) {
                results.push(AutomationCommandResult {
                    index: skipped.index,
                    command: skipped.name.clone(),
                    status: "skipped".to_string(),
                    created_ids: Vec::new(),
                    updated_ids: automation_target_ids(&skipped.argv),
                    error: Some(current().text(Message::AutomationNotValidatedAfterFailure)),
                });
            }
            failed = true;
            break;
        }
    }

    Ok(AutomationReport {
        status: if failed {
            "invalid".to_string()
        } else {
            "dry_run".to_string()
        },
        dry_run: true,
        commands: results,
    })
}

async fn create_dry_run_workspace(dir: &Path) -> anyhow::Result<std::path::PathBuf> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let base = std::env::temp_dir();
    for attempt in 0..100_u32 {
        let workspace = base.join(format!(
            "pinto-dry-run-{}-{timestamp}-{attempt}",
            std::process::id()
        ));
        match tokio::fs::create_dir(&workspace).await {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Err(error) = tokio::fs::set_permissions(
                        &workspace,
                        std::fs::Permissions::from_mode(0o700),
                    )
                    .await
                    {
                        let _ = tokio::fs::remove_dir_all(&workspace).await;
                        return Err(error.into());
                    }
                }
                let source = dir.join(".pinto");
                let destination = workspace.join(".pinto");
                if let Err(error) = copy_directory(&source, &destination).await {
                    let _ = tokio::fs::remove_dir_all(&workspace).await;
                    return Err(error);
                }
                let has_git = match tokio::fs::try_exists(dir.join(".git")).await {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = tokio::fs::remove_dir_all(&workspace).await;
                        return Err(error.into());
                    }
                };
                if has_git {
                    let output = match ProcessCommand::new("git")
                        .args(["init"])
                        .current_dir(&workspace)
                        .output()
                        .await
                    {
                        Ok(output) => output,
                        Err(error) => {
                            let _ = tokio::fs::remove_dir_all(&workspace).await;
                            let message = current().format(
                                Message::AutomationDryRunGitInitFailed,
                                [("message", error.to_string().as_str())],
                            );
                            return Err(anyhow::anyhow!("{message}"));
                        }
                    };
                    if !output.status.success() {
                        let _ = tokio::fs::remove_dir_all(&workspace).await;
                        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        let message = current().format(
                            Message::AutomationDryRunGitInitFailed,
                            [("message", detail.as_str())],
                        );
                        return Err(anyhow::anyhow!("{message}"));
                    }
                }
                return Ok(workspace);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(anyhow::anyhow!(
        "{}",
        current().text(Message::AutomationDryRunWorkspaceUnavailable)
    ))
}

async fn copy_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((source, destination)) = pending.pop() {
        tokio::fs::create_dir_all(&destination).await?;
        let mut entries = tokio::fs::read_dir(&source).await?;
        while let Some(entry) = entries.next_entry().await? {
            // The source lock belongs to the original board and must never become part of the
            // ephemeral preview state (or a Git commit made inside it).
            if entry.file_name() == ".lock" {
                continue;
            }
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            if entry.file_type().await?.is_dir() {
                pending.push((source_path, destination_path));
            } else {
                tokio::fs::copy(source_path, destination_path).await?;
            }
        }
    }
    Ok(())
}

fn automation_command_name(argv: &[String]) -> String {
    match (argv.first(), argv.get(1)) {
        (Some(command), Some(subcommand))
            if matches!(command.as_str(), "dep" | "link" | "sprint") =>
        {
            format!("{command} {subcommand}")
        }
        (Some(command), _) => command.clone(),
        (None, _) => "unknown".to_string(),
    }
}

fn parsed_item_id(raw: Option<&String>) -> Option<String> {
    raw.and_then(|value| value.parse::<ItemId>().ok())
        .map(|id| id.to_string())
}

fn automation_target_ids(argv: &[String]) -> Vec<String> {
    let Some(command) = argv.first().map(String::as_str) else {
        return Vec::new();
    };
    match command {
        "move" => argv
            .iter()
            .skip(1)
            .filter_map(|value| parsed_item_id(Some(value)))
            .collect(),
        "edit" | "reorder" => parsed_item_id(argv.get(1)).into_iter().collect(),
        "remove" => argv
            .iter()
            .skip(1)
            .filter_map(|value| parsed_item_id(Some(value)))
            .collect(),
        "dep" | "link" => parsed_item_id(argv.get(2)).into_iter().collect(),
        "sprint" => parsed_item_id(argv.get(3)).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn first_item_id_in_output(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && character != '-' && character != '_'
        });
        token.parse::<ItemId>().ok().map(|id| id.to_string())
    })
}

fn automation_execution_result(
    command: &ValidatedAutomationCommand,
    execution: &AutomationExecution,
    status: &str,
) -> AutomationCommandResult {
    let created_ids = (command.argv.first().map(String::as_str) == Some("add"))
        .then(|| first_item_id_in_output(&execution.stdout))
        .flatten()
        .into_iter()
        .collect();
    AutomationCommandResult {
        index: command.index,
        command: command.name.clone(),
        status: status.to_string(),
        created_ids,
        updated_ids: automation_target_ids(&command.argv),
        error: (!execution.success).then(|| {
            let error = execution.stderr.trim();
            if error.is_empty() {
                let status = execution
                    .exit_code
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string());
                current().format(
                    Message::AutomationCommandExited,
                    [("status", status.as_str())],
                )
            } else {
                error.to_string()
            }
        }),
    }
}

fn print_automation_json(report: &AutomationReport) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(report)?);
    Ok(())
}

fn print_automation_validation(report: &AutomationReport, dry_run: bool) {
    for command in &report.commands {
        let index = command.index.to_string();
        let message = if command.status == "invalid" {
            Message::AutomationCommandInvalid
        } else {
            Message::AutomationCommandValid
        };
        eprintln!(
            "{}",
            current().format(
                message,
                [
                    ("index", index.as_str()),
                    ("command", command.command.as_str())
                ],
            )
        );
    }
    if dry_run {
        let total = report.commands.len().to_string();
        println!(
            "{}",
            current().format(
                Message::AutomationDryRunCompleted,
                [("total", total.as_str())]
            )
        );
    }
}

/// `pinto shell` — Starts an interactive shell (REPL).
///
/// Reads one command per line from standard input, interprets it with existing `clap` `Command`, and sends it to [`dispatch`]
/// Delegate. Shares the same service layer as the CLI and has no duplication of logic. Print the diagnostics to stderr,
/// Do not pollute the original output (stdout) of the command.
///
/// Line editing, history (↑↓), and completion are provided by `rustyline` (completion candidates are calculated by [`super::shell`]).
/// For pipe input (non-TTY), `rustyline` falls back to reading one line, so you can use it even if you are not an interactive terminal.
/// Just keep moving.
///
/// - The loop does not crash due to a command error (interpretation error/runtime error), but continues with a message.
/// - `exit` / `quit`, exit normally with EOF (Ctrl-D). Ctrl-C discards the current line and continues.
///
/// `readline` of `rustyline` is synchronous blocking, but it is an I/O bound process that waits for interactive input, so
/// Executes with [`tokio::task::block_in_place`] on multi-thread runtime, and `dispatch` is
/// Await asynchronously as before.
async fn cmd_shell() -> anyhow::Result<ExitCode> {
    use rustyline::error::ReadlineError;

    let localizer = current();
    let mut editor = super::shell::build_editor()?;

    loop {
        // Blocking while waiting for terminal input is released to another thread, so it does not block the runtime.
        let readline = tokio::task::block_in_place(|| editor.readline("pinto> "));
        let line = match readline {
            Ok(line) => line,
            // Ctrl-C discards the line and continues, Ctrl-D / EOF terminates successfully.
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("{} {e}", localizer.text(Message::ErrorPrefix));
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Leave it in the history so that it can be traced with ↑↓ (duplicates are ignored by rustyline settings).
        let _ = editor.add_history_entry(trimmed);
        // REPL built-in commands. Terminate here without passing it to `clap`.
        if trimmed == "exit" || trimmed == "quit" || trimmed == "q" {
            break;
        }

        let argv = match super::shell::split_args(trimmed) {
            Ok(argv) => argv,
            Err(e) => {
                eprintln!(
                    "{}",
                    localizer.format(
                        Message::ShellParseError,
                        [("error", e.to_string().as_str())]
                    )
                );
                continue;
            }
        };
        if argv.is_empty() {
            continue;
        }

        // Reuse existing argument definitions. `clap` expects the program name at the beginning, so fill it in.
        let parsed = Cli::try_parse_from(std::iter::once("pinto".to_string()).chain(argv));
        match parsed {
            // Interpretation errors and `--help` / `--version` are left to the formatted output of `clap`, and the loop continues.
            Err(e) => {
                let _ = e.print();
            }
            // Nested `shell`s are rejected. Keep REPL one, issue messages and continue.
            Ok(cli) if matches!(cli.command, Command::Shell) => {
                eprintln!("{}", current().text(Message::AlreadyInInteractiveShell));
            }
            // Even if a runtime error occurs, the loop does not stop; it just prints a message and moves on to the next line.
            // `dispatch` is a static cycle that returns to this function again with `Command::Shell`, so
            // Break asynchronous type recursion with `Box::pin`.
            Ok(cli) => {
                // Already inside the shell: the Kanban view's `Q` returns to this prompt rather
                // than starting a nested shell.
                if let Err(e) = Box::pin(dispatch(cli, true)).await {
                    eprintln!(
                        "{} {}",
                        localizer.text(Message::ErrorPrefix),
                        format_anyhow_error(&e, localizer)
                    );
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto kanban` (also known as `k`) — launch Interactive Kanban (TUI).
///
/// Actual processing is delegated to [`super::kanban`]. Shares the same service layer and persistence layer as CLI.
///
/// Pressing `Q` leaves the view for the interactive shell instead of quitting. When launched
/// directly (`in_shell == false`), that starts a new REPL; when already inside a shell
/// (`in_shell == true`), returning here simply drops back to the existing prompt.
async fn cmd_kanban(args: KanbanArgs, in_shell: bool) -> anyhow::Result<ExitCode> {
    let search = build_search_filter(args.search, args.regex)?;
    let mode = super::kanban::run(args.column.as_deref(), args.maximize, search).await?;
    if mode == super::kanban::ExitMode::Shell && !in_shell {
        return cmd_shell().await;
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto completion <shell>` — Generates a completion script for the specified shell to stdout.
///
/// It is a pure generation process that does not wait for I/O, so it is done synchronously (just generate from the `clap` definition).
/// See the README for installation instructions.
fn cmd_completion(args: CompletionArgs) -> anyhow::Result<ExitCode> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(args.shell, &mut cmd, bin_name, &mut std::io::stdout());
    Ok(ExitCode::SUCCESS)
}

/// `pinto rebalance` — Reassign oversized ranks within sibling scopes while preserving their order.
///
/// `--dry-run` leaves the board unchanged and reports planned scope changes and rank lengths.
/// User errors such as board uninitialization are assigned code 1 by `main`.
async fn cmd_rebalance(args: RebalanceArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let outcome = rebalance(&dir, args.dry_run).await?;
    if outcome.changed == 0 {
        println!(
            "{}",
            current().format(
                Message::RebalanceAlreadyBalanced,
                [
                    ("count", outcome.total.to_string().as_str()),
                    ("max_length", outcome.before.max_len.to_string().as_str()),
                ],
            )
        );
    } else {
        let message = if args.dry_run {
            Message::RebalanceDryRun
        } else {
            Message::RebalanceCompleted
        };
        println!(
            "{}",
            current().format(
                message,
                [
                    ("changed", outcome.changed.to_string().as_str()),
                    ("total", outcome.total.to_string().as_str()),
                    ("before", outcome.before.max_len.to_string().as_str()),
                    ("after", outcome.after.max_len.to_string().as_str()),
                ],
            )
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto migrate` — Migrate a storage backend.
async fn cmd_migrate(args: MigrateArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let target = match args.to {
        MigrateTarget::File => StorageBackend::File,
        MigrateTarget::Git => StorageBackend::Git,
        MigrateTarget::Sqlite => {
            #[cfg(feature = "sqlite")]
            {
                StorageBackend::Sqlite
            }
            // sqlite cannot be selected in a feature-disabled build. Since the user can fix it (rebuild),
            // Issue a guide and exit with exit code 1.
            #[cfg(not(feature = "sqlite"))]
            {
                eprintln!(
                    "{} {}",
                    current().text(Message::ErrorPrefix),
                    current().text(Message::MigrationSqliteUnavailable)
                );
                return Ok(ExitCode::from(1));
            }
        }
    };
    match migrate_storage(&dir, target).await? {
        MigrateOutcome::Migrated {
            from,
            to,
            items,
            sprints,
        } => {
            let localizer = current();
            println!(
                "{}",
                localizer.format(
                    Message::MigrationCompleted,
                    [
                        ("items", items.to_string().as_str()),
                        ("sprints", sprints.to_string().as_str()),
                        ("from", from.to_string().as_str()),
                        ("to", to.to_string().as_str()),
                    ],
                )
            );
            println!(
                "{}",
                localizer.format(
                    Message::MigrationBackendUpdated,
                    [("backend", to.to_string().as_str())],
                )
            );
        }
        MigrateOutcome::AlreadyUsing(backend) => {
            println!(
                "{}",
                current().format(
                    Message::MigrationAlreadyUsing,
                    [("backend", backend.to_string().as_str())],
                )
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto doctor` — Inspect board integrity and apply safe repairs when requested.
async fn cmd_doctor(args: DoctorArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let report = pinto::service::doctor(&dir, args.fix).await?;
    let localizer = current();
    if report.issues.is_empty() {
        println!("{}", localizer.text(Message::DoctorHealthy));
    } else {
        println!(
            "{}",
            localizer.format(
                Message::DoctorIssues,
                [("count", report.issues.len().to_string().as_str())],
            )
        );
    }
    for fix in &report.fixes {
        println!(
            "{}",
            localizer.format(
                Message::DoctorFixed,
                [("description", fix.description.as_str())]
            )
        );
    }
    for issue in &report.issues {
        let kind = doctor_issue_kind_name(issue.kind, localizer);
        println!(
            "{}",
            localizer.format(
                Message::DoctorIssue,
                [
                    ("kind", kind.as_str()),
                    ("location", issue.location.as_str()),
                    ("detail", issue.detail.as_str()),
                    ("repair", issue.repair.as_str()),
                ],
            )
        );
    }
    Ok(if report.issues.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn doctor_issue_kind_name(
    kind: pinto::service::DoctorIssueKind,
    localizer: &pinto::i18n::Localizer,
) -> String {
    match kind {
        pinto::service::DoctorIssueKind::DanglingDependency => {
            localizer.text(Message::DoctorKindDanglingDependency)
        }
        pinto::service::DoctorIssueKind::DanglingParent => {
            localizer.text(Message::DoctorKindDanglingParent)
        }
        pinto::service::DoctorIssueKind::DanglingSprint => {
            localizer.text(Message::DoctorKindDanglingSprint)
        }
        pinto::service::DoctorIssueKind::ParentCycle => {
            localizer.text(Message::DoctorKindParentCycle)
        }
        pinto::service::DoctorIssueKind::DependencyCycle => {
            localizer.text(Message::DoctorKindDependencyCycle)
        }
        pinto::service::DoctorIssueKind::DuplicateId => {
            localizer.text(Message::DoctorKindDuplicateId)
        }
        pinto::service::DoctorIssueKind::IssuedId => localizer.text(Message::DoctorKindIssuedId),
        pinto::service::DoctorIssueKind::InvalidStatus => {
            localizer.text(Message::DoctorKindInvalidStatus)
        }
        pinto::service::DoctorIssueKind::RankAnomaly => {
            localizer.text(Message::DoctorKindRankAnomaly)
        }
        pinto::service::DoctorIssueKind::Collision => localizer.text(Message::DoctorKindCollision),
        pinto::service::DoctorIssueKind::MalformedRecord => {
            localizer.text(Message::DoctorKindMalformedRecord)
        }
        pinto::service::DoctorIssueKind::Filename => localizer.text(Message::DoctorKindFilename),
    }
}

/// `pinto init` — Initialize the board in the current directory.
async fn cmd_init() -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    match init_board(&dir).await? {
        InitOutcome::Created(path) => {
            println!(
                "{}",
                current().format(
                    Message::InitializedBoardAt,
                    [("path", path.display().to_string().as_str())],
                )
            );
        }
        InitOutcome::AlreadyInitialized(path) => {
            println!(
                "{}",
                current().format(
                    Message::AlreadyInitialized,
                    [("path", path.display().to_string().as_str())],
                )
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto add` — Add a PBI to the backlog.
async fn cmd_add(args: AddArgs) -> anyhow::Result<ExitCode> {
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
        tokio::task::spawn_blocking(move || super::editor::edit_in_editor(&initial, &slug))
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

fn combine_template_body(template: String, body: String) -> String {
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
/// omit their values in long mode to show the columns without filtering. Multiple labels use OR
/// by default; `--all-labels` switches to AND. `--roots-only` omits PBIs with a persisted parent
/// link. `--json` takes precedence because it already contains all metadata.
async fn cmd_list(args: ListArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let long = args.long;
    let show_labels = args.label.is_some();
    let show_sprint = args.sprint.is_some();
    let labels = resolve_label_filter("--label", args.label, long)?;
    let label_match = resolve_label_match(args.all_labels, &labels)?;
    let sprint = resolve_optional_filter("--sprint", args.sprint, long)?;
    let filter = ListFilter {
        roots_only: args.roots_only,
        status: args.status,
        sprint,
        labels,
        label_match,
        search: build_search_filter(args.search, args.regex)?,
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
async fn cmd_next(args: NextArgs) -> anyhow::Result<ExitCode> {
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
async fn cmd_show(args: ShowArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let ids: Vec<ItemId> = args
        .ids
        .iter()
        .map(|id| id.parse())
        .collect::<Result<_, _>>()?;
    let mut details = Vec::with_capacity(ids.len());
    for id in &ids {
        details.push(item_detail(&dir, id).await?);
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

async fn cmd_move(args: MoveArgs) -> anyhow::Result<ExitCode> {
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
fn report_failures(mut failures: Vec<(String, Error)>, action: &str) -> anyhow::Result<ExitCode> {
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

/// `pinto reorder` — Reorder PBI ranks (without changing `status`).
///
/// The destination (`--before` / `--after` / `--top` / `--bottom`) is an exclusive group of clap.
/// Constrain to exactly one. User errors such as non-existent IDs and self-references are handled by `main`.
/// Assign to code 1.
async fn cmd_reorder(args: ReorderArgs) -> anyhow::Result<ExitCode> {
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
async fn cmd_edit(args: EditArgs) -> anyhow::Result<ExitCode> {
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
async fn cmd_edit_in_editor(dir: &Path, id: &ItemId) -> anyhow::Result<ExitCode> {
    // Fail before creating an edit template when no editor is configured.
    if super::editor::resolve_editor().is_none() {
        return Err(pinto::error::Error::EditorNotSet.into());
    }

    let template = item_edit_template(dir, id).await?;
    let slug = id.to_string();
    let edited =
        tokio::task::spawn_blocking(move || super::editor::edit_in_editor(&template, &slug))
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

/// `pinto dep add|rm` — Set/remove dependencies between PBIs.
///
/// Dependencies that create a cycle are not treated as errors, but are logged with a warning sent to stderr.
/// User errors such as invalid ID format or non-existent ID will be assigned code 1 by `main`.
async fn cmd_dep(args: DepArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    match args.command {
        DepCommand::Add { id, depends_on } => {
            let id: ItemId = id.parse()?;
            let dep: ItemId = depends_on.parse()?;
            let outcome = add_dependency(&dir, &id, &dep).await?;
            let localizer = current();
            println!(
                "{}",
                localizer.format(
                    Message::DependencyAdded,
                    [
                        ("id", id.to_string().as_str()),
                        ("dependency", dep.to_string().as_str()),
                    ],
                )
            );
            if outcome.cycle_warning {
                eprintln!(
                    "{}",
                    localizer.format(
                        Message::DependencyCycleWarning,
                        [
                            ("id", id.to_string().as_str()),
                            ("dependency", dep.to_string().as_str()),
                        ],
                    )
                );
            }
        }
        DepCommand::Rm { id, depends_on } => {
            let id: ItemId = id.parse()?;
            let dep: ItemId = depends_on.parse()?;
            let item = remove_dependency(&dir, &id, &dep).await?;
            println!(
                "{}",
                current().format(
                    Message::DependencyRemoved,
                    [
                        ("id", item.id.to_string().as_str()),
                        ("dependency", dep.to_string().as_str()),
                    ],
                )
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto link [add|rm|sync]` — Associate/remove Git commit (SHA) with PBI, synchronize from history.
///
/// `add` / `rm` treats SHA as a plain string, so Git is not required. `sync` reads `git log` and
/// associates matching commits.
/// User errors such as uninitialized, invalid ID, and absence of Git are assigned to the exit code by `main`.
async fn cmd_link(args: LinkArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    match args.command {
        LinkCommand::Add { id, shas } => {
            let id: ItemId = id.parse()?;
            let outcome = link_commits(&dir, &id, &shas).await?;
            let localizer = current();
            if outcome.changed.is_empty() {
                println!(
                    "{}",
                    localizer.format(
                        Message::LinkAlreadyLinked,
                        [("id", id.to_string().as_str())]
                    )
                );
            } else {
                let commits = outcome.changed.join(", ");
                println!(
                    "{}",
                    localizer.format(
                        Message::LinkAdded,
                        [
                            ("id", id.to_string().as_str()),
                            ("commits", commits.as_str())
                        ],
                    )
                );
            }
        }
        LinkCommand::Rm { id, shas } => {
            let id: ItemId = id.parse()?;
            let outcome = unlink_commits(&dir, &id, &shas).await?;
            let localizer = current();
            if outcome.changed.is_empty() {
                println!(
                    "{}",
                    localizer.format(
                        Message::LinkNoMatchingCommit,
                        [("id", id.to_string().as_str())],
                    )
                );
            } else {
                let commits = outcome.changed.join(", ");
                println!(
                    "{}",
                    localizer.format(
                        Message::LinkRemoved,
                        [
                            ("id", id.to_string().as_str()),
                            ("commits", commits.as_str())
                        ],
                    )
                );
            }
        }
        LinkCommand::Sync { since } => {
            let outcome = sync_commits(&dir, since.as_deref()).await?;
            let localizer = current();
            if outcome.links.is_empty() {
                println!("{}", localizer.text(Message::LinkNoNewCommits));
            } else {
                for (id, sha) in &outcome.links {
                    let short: String = sha.chars().take(8).collect();
                    let id = id.to_string();
                    println!(
                        "{}",
                        localizer.format(
                            Message::LinkCommitLinked,
                            [("id", id.as_str()), ("commit", short.as_str())],
                        )
                    );
                }
                println!(
                    "{}",
                    localizer.format(
                        Message::LinkSummary,
                        [("count", outcome.links.len().to_string().as_str())],
                    )
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto dod [set|clear]` — View, set, and delete DoD common to all PBIs.
///
/// If the subcommand is omitted, the current common DoD will be displayed (if not set, the information will be output to stdout).
/// User errors such as uninitialization and empty strings are assigned code 1 by `main`.
async fn cmd_dod(args: DodArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    match args.command {
        None => match common_dod(&dir).await? {
            Some(text) => println!("{text}"),
            None => println!("{}", current().text(Message::DodUnset)),
        },
        Some(DodCommand::Set { text }) => {
            set_common_dod(&dir, &text).await?;
            println!("{}", current().text(Message::DodUpdated));
        }
        Some(DodCommand::Clear) => {
            if clear_common_dod(&dir).await? {
                println!("{}", current().text(Message::DodCleared));
            } else {
                println!("{}", current().text(Message::DodNoCommonToClear));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto rm` — Archive PBIs (default) or physically delete them with `--force`.
///
/// Parse every requested ID before attempting any archive/delete operation. Valid-but-missing IDs
/// are still attempted together so the command can report all lookup failures, while malformed IDs
/// never allow a preceding valid target to be changed.
async fn cmd_rm(args: RemoveArgs) -> anyhow::Result<ExitCode> {
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

/// `pinto sprint <sub>` — Sprint creation, editing, deletion, state transition, assignment, and list.
///
/// User errors such as invalid ID format, non-existent ID, invalid state transition, etc. will be assigned code 1 by `main`.
async fn cmd_sprint(args: SprintArgs) -> anyhow::Result<ExitCode> {
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
                current().format(
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
                current().format(
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
                current().format(Message::DeletedSprint, [("id", id.to_string().as_str())])
            );
        }
        SprintCommand::Start { id } => {
            let id: SprintId = id.parse()?;
            let sprint = start_sprint(&dir, &id).await?;
            println!(
                "{}",
                current().format(
                    Message::StartedSprint,
                    [("id", sprint.id.to_string().as_str())],
                )
            );
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
                current().format(
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
                let item = assign_sprint_raw(&dir, &sprint_id, &item_id).await?;
                println!(
                    "{}",
                    current().format(
                        Message::AssignedToSprint,
                        [
                            ("id", item.id.to_string().as_str()),
                            ("sprint", sprint_id.as_str()),
                        ],
                    )
                );
            } else if let Some(status) = status {
                let sprint_id: SprintId = sprint_id.parse()?;
                for item in assign_sprint_by_status(&dir, &sprint_id, &status, limit).await? {
                    println!(
                        "{}",
                        current().format(
                            Message::AssignedToSprint,
                            [
                                ("id", item.id.to_string().as_str()),
                                ("sprint", sprint_id.to_string().as_str()),
                            ],
                        )
                    );
                }
            } else {
                return Err(anyhow::anyhow!(
                    "{}",
                    current().text(Message::SprintAddRequiresItemOrStatus)
                ));
            }
        }
        SprintCommand::Unassign { sprint_id, item_id } => {
            let sprint_id: SprintId = sprint_id.parse()?;
            let item_id: ItemId = item_id.parse()?;
            let item = unassign_sprint(&dir, &sprint_id, &item_id).await?;
            println!(
                "{}",
                current().format(
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
                println!("{}", current().text(Message::NoSprints));
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
                println!("{}", current().text(Message::NoSprints));
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
                        current().text(Message::InvalidCapacityOptions)
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

/// Return the terminal width in columns. Use [`DEFAULT_TERM_WIDTH`] when output is not a TTY or
/// the terminal width cannot be determined.
fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(terminal_size::Width(w), _)| usize::from(w))
        .filter(|&w| w > 0)
        .unwrap_or(DEFAULT_TERM_WIDTH)
}

/// `pinto board` — Display board columns and their PBIs.
///
/// When `--sprint <id>` is specified, display only PBIs assigned to that sprint.
/// `--long/-l` uses the same detail columns as `list --long` for each board column. `--label`
/// and `--sprint` add their columns; in long mode, omit their values to show the columns without
/// filtering. Multiple labels use OR by default; `--all-labels` switches to AND. `--roots-only`
/// omits PBIs with a persisted parent link.
///
/// If an item's status is missing from `config.toml` because a column was removed or renamed,
/// show it in a warning section and print repair guidance. The command still succeeds with exit
/// code 0.
///
/// When `[wip]` defines a column limit, warn on stderr when the full board exceeds it. Suppress
/// these warnings with `--no-wip-check` or `wip.enabled = false`; filtering does not change the
/// count used for the check, and `--json` emits no warning.
async fn cmd_board(args: BoardArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let long = args.long;
    let show_labels = args.label.is_some();
    let show_sprint = args.sprint.is_some();
    let labels = resolve_label_filter("--label", args.label, long)?;
    let label_match = resolve_label_match(args.all_labels, &labels)?;
    let sprint = resolve_optional_filter("--sprint", args.sprint, long)?;
    let query = BoardQuery {
        roots_only: args.roots_only,
        sprint,
        labels,
        label_match,
        statuses: args.status,
        sort: args.sort.map(|s| match s {
            SortArg::Rank => SortKey::Rank,
            SortArg::Done => SortKey::Done,
            SortArg::Created => SortKey::Created,
        }),
        reverse: args.reverse,
        search: build_search_filter(args.search, args.regex)?,
    };
    let board = board(&dir, &query).await?;
    if args.json {
        // In JSON, orphaned PBIs are represented by `orphaned` arrays, so no warning is issued.
        // (Keep machine-readable output only as JSON on stdout).
        println!("{}", board_json(&board)?);
        return Ok(ExitCode::SUCCESS);
    }
    // `--no-truncate` disables truncation (displays the full text with virtually unlimited width).
    let width = if args.no_truncate {
        usize::MAX
    } else {
        terminal_width()
    };
    if long {
        let timezone = display_settings(&dir).await?.timezone;
        print!(
            "{}",
            format_board_long(
                &board,
                width,
                ListLongOptions::new(show_labels, show_sprint)
                    .with_acceptance_criteria(args.acceptance_criteria)
                    .with_timezone(timezone),
            )
        );
    } else {
        print!("{}", format_board(&board, width));
    }
    if !board.orphaned.is_empty() {
        let ids = board
            .orphaned
            .iter()
            .map(|it| it.id.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "{}",
            current().format(
                Message::OrphanedItemsWarning,
                [
                    ("count", board.orphaned.len().to_string().as_str()),
                    ("ids", ids.as_str()),
                ],
            )
        );
    }
    // Warn about exceeding WIP limit (success, exit code 0). `--no-wip-check`
    // Nothing is output when specified or when `wip.enabled=false`.
    if !args.no_wip_check {
        for v in check_wip(&dir).await? {
            warn_wip(&v);
        }
    }
    Ok(ExitCode::SUCCESS)
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

/// `pinto cycletime` — Aggregate and display Cycle Time / Lead Time of completed PBI.
///
/// `--sprint` / `--since` / `--until` can be used to narrow down the list easily (targeting completion date and time). `start_at` is missing
/// Exclude completed PBI from Cycle Time and indicate ID as a warning (include in Lead Time).
async fn cmd_cycletime(args: CycleTimeArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let filter = CycleTimeFilter {
        sprint: args.sprint,
        since: args.since,
        until: args.until,
    };
    let report = cycle_time(&dir, &filter).await?;
    if args.json {
        println!("{}", cycletime_json(&report)?);
    } else {
        print!("{}", format_cycletime(&report));
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn automation_names_and_target_ids_cover_command_shapes() {
        assert_eq!(automation_command_name(&argv(&["dep", "add"])), "dep add");
        assert_eq!(
            automation_command_name(&argv(&["link", "sync"])),
            "link sync"
        );
        assert_eq!(
            automation_command_name(&argv(&["sprint", "new"])),
            "sprint new"
        );
        assert_eq!(automation_command_name(&argv(&["list"])), "list");
        assert_eq!(automation_command_name(&[]), "unknown");

        assert!(automation_target_ids(&[]).is_empty());
        assert_eq!(
            automation_target_ids(&argv(&["move", "T-1", "invalid", "T-2"])),
            ["T-1", "T-2"]
        );
        assert_eq!(automation_target_ids(&argv(&["edit", "T-3"])), ["T-3"]);
        assert_eq!(
            automation_target_ids(&argv(&["reorder", "T-4", "--top"])),
            ["T-4"]
        );
        assert_eq!(
            automation_target_ids(&argv(&["remove", "T-5", "T-6"])),
            ["T-5", "T-6"]
        );
        assert_eq!(
            automation_target_ids(&argv(&["dep", "add", "T-7", "T-8"])),
            ["T-7"]
        );
        assert_eq!(
            automation_target_ids(&argv(&["link", "add", "T-9", "abc"])),
            ["T-9"]
        );
        assert_eq!(
            automation_target_ids(&argv(&["sprint", "add", "S-1", "T-10"])),
            ["T-10"]
        );
        assert!(automation_target_ids(&argv(&["unknown", "T-11"])).is_empty());
    }

    #[test]
    fn automation_results_extract_created_ids_and_sanitize_errors() {
        let command = ValidatedAutomationCommand {
            index: 1,
            argv: argv(&["add", "Task"]),
            name: "add".to_string(),
            error: None,
        };
        let created = automation_execution_result(
            &command,
            &AutomationExecution {
                success: true,
                exit_code: Some(0),
                stdout: "Created T-42: Task".to_string(),
                stderr: String::new(),
            },
            "succeeded",
        );
        assert_eq!(created.created_ids, ["T-42"]);
        assert_eq!(created.error, None);

        for (exit_code, expected) in [
            (Some(1), "command exited with status 1"),
            (None, "command exited with status unknown"),
        ] {
            let failed = automation_execution_result(
                &command,
                &AutomationExecution {
                    success: false,
                    exit_code,
                    stdout: String::new(),
                    stderr: String::new(),
                },
                "failed",
            );
            assert_eq!(failed.error.as_deref(), Some(expected));
        }

        let stderr = automation_execution_result(
            &command,
            &AutomationExecution {
                success: false,
                exit_code: Some(1),
                stdout: String::new(),
                stderr: "  user-facing failure\n".to_string(),
            },
            "failed",
        );
        assert_eq!(stderr.error.as_deref(), Some("user-facing failure"));
        assert_eq!(
            first_item_id_in_output("created: [T-1], next T-2"),
            Some("T-1".to_string())
        );
        assert_eq!(parsed_item_id(Some(&"bad".to_string())), None);
    }

    #[test]
    fn template_body_and_failure_reporting_keep_edge_cases_explicit() {
        assert_eq!(
            combine_template_body(String::new(), "body".to_string()),
            "body"
        );
        assert_eq!(
            combine_template_body("template".to_string(), String::new()),
            "template"
        );
        assert_eq!(
            combine_template_body("template\n".to_string(), "body".to_string()),
            "template\n\nbody"
        );
        assert_eq!(
            combine_template_body("template".to_string(), "body".to_string()),
            "template\n\nbody"
        );

        let item_id = "T-1".parse::<ItemId>().expect("valid item id");
        let result = report_failures(
            vec![
                ("T-1".to_string(), Error::NotFound(item_id)),
                (
                    "T-2".to_string(),
                    Error::Io {
                        path: "/tmp/pinto-test".into(),
                        message: "read failed".to_string(),
                    },
                ),
            ],
            "move",
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sprint_capacity_rejects_an_incomplete_programmatic_argument_set() {
        let error = cmd_sprint(SprintArgs {
            command: SprintCommand::Capacity {
                id: "S-1".to_string(),
                daily_hours: Some(8.0),
                holidays: None,
                deduction_factor: None,
                json: false,
            },
        })
        .await
        .expect_err("partial capacity settings must be rejected");

        assert!(error.to_string().contains("must be provided together"));
    }

    #[tokio::test]
    async fn automation_plan_source_handles_inline_and_invalid_sources() {
        let inline = "{\0\"commands\":[]}";
        assert_eq!(read_automation_plan(inline).await.unwrap(), inline);

        let invalid_path = "automation\0plan.json";
        let error = read_automation_plan(invalid_path)
            .await
            .expect_err("invalid path should return a structured source error");
        assert!(matches!(
            error.downcast_ref::<Error>(),
            Some(Error::AutomationPlanSource { path, .. })
                if path == Path::new(invalid_path)
        ));

        let directory = tempfile::tempdir().expect("temporary directory");
        let directory_path = directory.path().to_str().expect("temporary path is UTF-8");
        let error = read_automation_plan(directory_path)
            .await
            .expect_err("a directory is not a readable plan file");
        assert!(matches!(
            error.downcast_ref::<Error>(),
            Some(Error::AutomationPlanSource { path, .. })
                if path == directory.path()
        ));

        let missing_path = directory.path().join("missing.json");
        let missing_path = missing_path.to_str().expect("temporary path is UTF-8");
        let error = read_automation_plan(missing_path)
            .await
            .expect_err("missing path should return a structured source error");
        assert!(matches!(
            error.downcast_ref::<Error>(),
            Some(Error::AutomationPlanSource { path, message })
                if path == Path::new(missing_path) && message == "file does not exist"
        ));
    }
}
