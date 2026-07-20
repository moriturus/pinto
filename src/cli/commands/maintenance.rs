//! Board maintenance commands: `init`, `rebalance`, `migrate`, and `doctor`.

use crate::cli::args::*;
use pinto::i18n::{Message, current};
use pinto::service::{InitOutcome, MigrateOutcome, init_board, migrate_storage, rebalance};

use pinto::storage::StorageBackend;
use std::process::ExitCode;

/// `pinto rebalance` — Reassign oversized ranks within sibling scopes while preserving their order.
///
/// `--dry-run` leaves the board unchanged and reports planned scope changes and rank lengths.
/// User errors such as board uninitialization are assigned code 1 by `main`.
pub(super) async fn cmd_rebalance(args: RebalanceArgs) -> anyhow::Result<ExitCode> {
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
pub(super) async fn cmd_migrate(args: MigrateArgs) -> anyhow::Result<ExitCode> {
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
pub(super) async fn cmd_doctor(args: DoctorArgs) -> anyhow::Result<ExitCode> {
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
pub(super) async fn cmd_init() -> anyhow::Result<ExitCode> {
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
