//! Item relation commands: `dep`, `link`, and `dod`.

use crate::cli::args::*;
use pinto::backlog::ItemId;
use pinto::i18n::{Message, current};
use pinto::service::{
    add_dependency, clear_common_dod, common_dod, link_commits, remove_dependency, set_common_dod,
    sync_commits, unlink_commits,
};

use std::process::ExitCode;

/// `pinto dep add|rm` — Set/remove dependencies between PBIs.
///
/// Dependencies that create a cycle are not treated as errors, but are logged with a warning sent to stderr.
/// User errors such as invalid ID format or non-existent ID will be assigned code 1 by `main`.
pub(super) async fn cmd_dep(args: DepArgs) -> anyhow::Result<ExitCode> {
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
pub(super) async fn cmd_link(args: LinkArgs) -> anyhow::Result<ExitCode> {
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
pub(super) async fn cmd_dod(args: DodArgs) -> anyhow::Result<ExitCode> {
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
