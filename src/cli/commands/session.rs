//! Interactive session commands: `shell`, `kanban`, and `completion`.

use crate::cli::args::*;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use pinto::i18n::{Message, current};
use pinto::service::BoardQuery;

use std::process::ExitCode;

use super::{
    build_search_filter, dispatch, format_anyhow_error, resolve_label_filter, resolve_label_match,
};

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
pub(super) async fn cmd_shell() -> anyhow::Result<ExitCode> {
    use rustyline::error::ReadlineError;

    let localizer = current();
    let mut editor = crate::cli::shell::build_editor()?;

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

        let argv = match crate::cli::shell::split_args(trimmed) {
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
pub(super) async fn cmd_kanban(args: KanbanArgs, in_shell: bool) -> anyhow::Result<ExitCode> {
    let labels = resolve_label_filter("--label", args.label, false)?;
    let label_match = resolve_label_match(args.all_labels, &labels)?;
    let query = BoardQuery {
        sprint: args.sprint,
        labels,
        label_match,
        search: build_search_filter(args.search, args.regex)?,
        ..BoardQuery::default()
    };
    let mode = crate::cli::kanban::run(args.column.as_deref(), args.maximize, query).await?;
    if mode == crate::cli::kanban::ExitMode::Shell && !in_shell {
        return cmd_shell().await;
    }
    Ok(ExitCode::SUCCESS)
}

/// `pinto completion <shell>` — Generates a completion script for the specified shell to stdout.
///
/// It is a pure generation process that does not wait for I/O, so it is done synchronously (just generate from the `clap` definition).
/// See the README for installation instructions.
pub(super) fn cmd_completion(args: CompletionArgs) -> anyhow::Result<ExitCode> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(args.shell, &mut cmd, bin_name, &mut std::io::stdout());
    Ok(ExitCode::SUCCESS)
}
