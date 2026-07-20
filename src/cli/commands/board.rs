//! Board views and reports: `board`, `export`, and `cycletime`.

use crate::cli::args::*;
use crate::cli::format::board::{format_board, format_board_long};
use crate::cli::format::item::ListLongOptions;
use crate::cli::format::report::format_cycletime;
use crate::cli::json::{board_json, cycletime_json, export_json};
use pinto::i18n::{Message, current};
use pinto::service::{
    BoardQuery, CycleTimeFilter, SortKey, board, check_wip, cycle_time, display_settings,
    export_snapshot,
};

use std::process::ExitCode;

use super::{
    build_search_filter, resolve_label_filter, resolve_label_match, resolve_optional_filter,
    terminal_width, warn_wip,
};

/// `pinto export --json` — Export the complete active board as one JSON document.
pub(super) async fn cmd_export(_args: ExportArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir()?;
    let snapshot = export_snapshot(&dir).await?;
    println!("{}", export_json(&snapshot)?);
    Ok(ExitCode::SUCCESS)
}

/// `pinto board` — Display board columns and their PBIs.
///
/// When `--sprint <id>` is specified, display only PBIs assigned to that sprint. `--assignee` is
/// an exact assignee-name filter and composes with the other scopes.
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
pub(super) async fn cmd_board(args: BoardArgs) -> anyhow::Result<ExitCode> {
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
        assignee: args.assignee,
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

/// `pinto cycletime` — Aggregate and display Cycle Time / Lead Time of completed PBI.
///
/// `--sprint` / `--since` / `--until` can be used to narrow down the list easily (targeting completion date and time). `start_at` is missing
/// Exclude completed PBI from Cycle Time and indicate ID as a warning (include in Lead Time).
pub(super) async fn cmd_cycletime(args: CycleTimeArgs) -> anyhow::Result<ExitCode> {
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
