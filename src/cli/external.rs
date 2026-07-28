//! Git-style delegation of unknown top-level commands to separately installed `pinto-*`
//! executables discovered beside the running binary or on `PATH`.

use anyhow::Context;
use pinto::i18n::Localizer;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus, Stdio};
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;

const COMMAND_PREFIX: &str = "pinto-";
const HOST_VERSION_ENV: &str = "PINTO_HOST_VERSION";
const CONTRACT_VERSION_ENV: &str = "PINTO_PLUGIN_CONTRACT_VERSION";
const PLUGIN_CONTRACT_VERSION: &str = "1";
const HELP_TIMEOUT: Duration = Duration::from_secs(1);

/// A missing external command is a user-facing error rather than an internal failure.
#[derive(Debug, Error)]
#[error("external command `{command}` was not found; install `{executable}` or run `pinto help`")]
pub(super) struct NotFound {
    command: String,
    executable: String,
}

/// Run the longest matching external command prefix and forward the remaining arguments.
pub(super) async fn run(
    args: Vec<OsString>,
    project_dir: Option<&Path>,
) -> anyhow::Result<ExitCode> {
    let Some((path, consumed, command_name)) = find_longest_executable(&args).await else {
        let command = display_args(&args);
        let first = args
            .first()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<empty>".to_string());
        return Err(NotFound {
            command,
            executable: format!("{COMMAND_PREFIX}{first}"),
        }
        .into());
    };

    run_path(&path, &command_name, &args[consumed..], project_dir).await
}

/// Render discovered external commands for the root help screen.
///
/// The first non-empty, non-usage line of `pinto-* --help` is used as the concise summary. A
/// broken or slow plugin is omitted from the summary rather than making `pinto help` panic or
/// block indefinitely.
pub(super) async fn help_section(localizer: &Localizer) -> String {
    let mut commands = discover_executables().await;
    commands.retain(|name, _| current_command_name(name).is_none());
    if commands.is_empty() {
        return String::new();
    }

    let heading = localizer
        .lookup("external-commands-heading")
        .unwrap_or_else(|| "External commands:".to_string());
    let width = commands
        .keys()
        .map(String::len)
        .max()
        .unwrap_or_default()
        .saturating_add(2);

    let mut summary_tasks = tokio::task::JoinSet::new();
    for (name, path) in commands {
        summary_tasks.spawn(async move { (name, help_summary(&path).await) });
    }
    let mut summaries = BTreeMap::new();
    while let Some(result) = summary_tasks.join_next().await {
        if let Ok((name, summary)) = result {
            summaries.insert(name, summary);
        }
    }

    let mut output = format!("\n{heading}\n");
    for (name, summary) in summaries {
        if summary.is_empty() {
            output.push_str(&format!("  {name}\n"));
        } else {
            output.push_str(&format!("  {name:<width$}{summary}\n"));
        }
    }
    output
}

/// Detect the root help forms that are handled by clap before normal dispatch.
pub(super) fn is_root_help_request(args: &[OsString]) -> bool {
    let mut index = 1;
    while index < args.len() {
        let token = args[index].to_string_lossy();
        match token.as_ref() {
            "--dir" | "-C" => index = index.saturating_add(2),
            value
                if value.starts_with("--dir=") || (value.starts_with("-C") && value.len() > 2) =>
            {
                index = index.saturating_add(1);
            }
            "help" | "--help" | "-h" => return index + 1 == args.len(),
            _ => return false,
        }
    }
    false
}

async fn find_longest_executable(args: &[OsString]) -> Option<(PathBuf, usize, String)> {
    for consumed in (1..=args.len()).rev() {
        let Some(name) = external_name(&args[..consumed]) else {
            continue;
        };
        if let Some(path) = find_executable(&name).await {
            return Some((path, consumed, name));
        }
    }
    None
}

async fn find_executable(name: &str) -> Option<PathBuf> {
    for directory in search_directories() {
        for candidate in executable_names(name) {
            let path = directory.join(candidate);
            if is_executable(&path).await {
                return Some(path);
            }
        }
    }
    None
}

async fn run_path(
    path: &Path,
    command_name: &str,
    forwarded: &[OsString],
    project_dir: Option<&Path>,
) -> anyhow::Result<ExitCode> {
    let mut command = command_for_path(path);
    configure_plugin_environment(&mut command, project_dir);
    command.args(forwarded);
    let status = command
        .status()
        .await
        .with_context(|| format!("failed to execute external command `{command_name}`"))?;
    Ok(exit_code(status))
}

async fn discover_executables() -> BTreeMap<String, PathBuf> {
    let mut commands = BTreeMap::new();
    for directory in search_directories() {
        let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
            continue;
        };
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) | Err(_) => break,
            };
            let Some(name) = external_command_name(&entry.file_name()) else {
                continue;
            };
            let path = entry.path();
            if is_executable(&path).await {
                commands.entry(name).or_insert(path);
            }
        }
    }
    commands
}

fn search_directories() -> Vec<PathBuf> {
    let executable_parent = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf));
    search_directories_from(
        std::env::var_os("PATH").as_deref(),
        executable_parent.as_deref(),
    )
}

fn search_directories_from(path: Option<&OsStr>, executable_parent: Option<&Path>) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(executable_parent) = executable_parent {
        directories.push(executable_parent.to_path_buf());
    }
    if let Some(path) = path {
        directories.extend(
            std::env::split_paths(path).filter(|directory| !directory.as_os_str().is_empty()),
        );
    }

    let mut unique = Vec::with_capacity(directories.len());
    for directory in directories {
        if !unique.iter().any(|existing| existing == &directory) {
            unique.push(directory);
        }
    }
    unique
}

fn configure_plugin_environment(command: &mut Command, project_dir: Option<&Path>) {
    command
        .env(HOST_VERSION_ENV, pinto::VERSION)
        .env(CONTRACT_VERSION_ENV, PLUGIN_CONTRACT_VERSION);
    if let Some(project_dir) = project_dir {
        command.env("PINTO_DIR", project_dir);
    }
}

fn executable_names(name: &str) -> Vec<OsString> {
    #[cfg(windows)]
    {
        let mut names = vec![OsString::from(name)];
        for suffix in [".exe", ".cmd", ".bat"] {
            names.push(OsString::from(format!("{name}{suffix}")));
        }
        names
    }
    #[cfg(not(windows))]
    {
        vec![OsString::from(name)]
    }
}

async fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn external_name(args: &[OsString]) -> Option<String> {
    let mut segments = Vec::with_capacity(args.len());
    for argument in args {
        let segment = argument.to_str()?;
        if !valid_segment(segment) {
            return None;
        }
        segments.push(segment);
    }
    (!segments.is_empty()).then(|| format!("{COMMAND_PREFIX}{}", segments.join("-")))
}

fn external_command_name(file_name: &OsStr) -> Option<String> {
    let file_name = file_name.to_str()?;
    let base = executable_base_name(file_name);
    let suffix = base.strip_prefix(COMMAND_PREFIX)?;
    if suffix.is_empty() || suffix.split('-').any(|segment| !valid_segment(segment)) {
        return None;
    }
    Some(suffix.replace('-', " "))
}

/// Canonical top-level command names and their short aliases.
///
/// A discovered `pinto-<name>` executable that collides with a built-in command (or its alias) is
/// omitted from the external help listing so an installed executable never shadows the host's own
/// built-in commands.
fn current_command_name(name: &str) -> Option<&'static str> {
    match name {
        "init" => Some("init"),
        "add" | "a" => Some("add"),
        "list" | "ls" => Some("list"),
        "next" | "n" => Some("next"),
        "show" | "s" => Some("show"),
        "move" | "mv" => Some("move"),
        "reorder" | "ro" => Some("reorder"),
        "edit" | "e" => Some("edit"),
        "remove" | "rm" => Some("remove"),
        "restore" | "rs" => Some("restore"),
        "dep" | "d" => Some("dep"),
        "link" | "ln" => Some("link"),
        "dod" | "dd" => Some("dod"),
        "export" => Some("export"),
        "import" => Some("import"),
        "sprint" | "sp" => Some("sprint"),
        "board" | "b" => Some("board"),
        "cycletime" | "ct" => Some("cycletime"),
        "rebalance" | "reb" => Some("rebalance"),
        "migrate" | "mig" => Some("migrate"),
        "doctor" | "dr" => Some("doctor"),
        "undo" => Some("undo"),
        "automate" | "auto" => Some("automate"),
        "shell" => Some("shell"),
        "kanban" | "k" => Some("kanban"),
        "completion" => Some("completion"),
        _ => None,
    }
}

fn executable_base_name(file_name: &str) -> &str {
    #[cfg(windows)]
    {
        file_name
            .strip_suffix(".exe")
            .or_else(|| file_name.strip_suffix(".cmd"))
            .or_else(|| file_name.strip_suffix(".bat"))
            .unwrap_or(file_name)
    }
    #[cfg(not(windows))]
    {
        file_name
    }
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('-')
        && segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

async fn help_summary(path: &Path) -> String {
    let mut command = command_for_path(path);
    configure_plugin_environment(&mut command, None);
    command.kill_on_drop(true);
    command
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(result) = timeout(HELP_TIMEOUT, command.output()).await else {
        return String::new();
    };
    let Ok(output) = result else {
        return String::new();
    };
    first_help_line(&output.stdout)
        .or_else(|| first_help_line(&output.stderr))
        .unwrap_or_default()
}

fn first_help_line(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .map(strip_ansi)
        .find(|line| !line.is_empty() && !line.starts_with("Usage:") && !line.starts_with("usage:"))
}

fn strip_ansi(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut in_escape = false;
    for character in text.chars() {
        if in_escape {
            if character.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if character == '\u{1b}' {
            in_escape = true;
        } else {
            clean.push(character);
        }
    }
    clean
}

fn command_for_path(path: &Path) -> Command {
    #[cfg(windows)]
    if matches!(
        path.extension().and_then(OsStr::to_str),
        Some("cmd" | "bat")
    ) {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(path);
        return command;
    }

    Command::new(path)
}

fn exit_code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(2), ExitCode::from)
}

fn display_args(args: &[OsString]) -> String {
    args.iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_names_keep_nested_segments_in_order() {
        let args = [
            OsString::from("team"),
            OsString::from("report"),
            OsString::from("payload"),
        ];
        assert_eq!(
            external_name(&args[..2]),
            Some("pinto-team-report".to_string())
        );
    }

    #[test]
    fn external_names_accept_documented_segments_and_reject_unsafe_ones() {
        assert_eq!(
            external_name(&[OsString::from("team_v2"), OsString::from("report.1")]),
            Some("pinto-team_v2-report.1".to_string())
        );
        assert_eq!(external_name(&[OsString::from("team/report")]), None);
        assert_eq!(external_name(&[OsString::from("-team")]), None);
        assert_eq!(external_name(&[OsString::new()]), None);
    }

    #[test]
    fn invalid_segments_never_escape_the_executable_name() {
        assert_eq!(external_name(&[OsString::from("../escape")]), None);
        assert_eq!(external_name(&[OsString::from("--help")]), None);
    }

    #[test]
    fn root_help_detection_ignores_global_directory_options() {
        assert!(is_root_help_request(&[
            OsString::from("pinto"),
            OsString::from("--dir"),
            OsString::from("board"),
            OsString::from("help"),
        ]));
        assert!(!is_root_help_request(&[
            OsString::from("pinto"),
            OsString::from("team"),
            OsString::from("help"),
        ]));
    }

    #[test]
    fn current_command_names_include_every_top_level_command() {
        for name in [
            "init",
            "add",
            "list",
            "next",
            "show",
            "move",
            "reorder",
            "edit",
            "remove",
            "restore",
            "dep",
            "link",
            "dod",
            "export",
            "import",
            "sprint",
            "board",
            "cycletime",
            "rebalance",
            "migrate",
            "doctor",
            "undo",
            "automate",
            "shell",
            "kanban",
            "completion",
        ] {
            assert_eq!(current_command_name(name), Some(name));
        }
    }

    #[test]
    fn strips_ansi_sequences_from_external_help_summaries() {
        assert_eq!(strip_ansi("\u{1b}[32mSummary\u{1b}[0m"), "Summary");
    }

    #[test]
    fn package_directory_precedes_path_and_empty_path_entries_are_ignored() {
        let path = std::env::join_paths([Path::new("path-one"), Path::new("path-two")])
            .expect("valid PATH");
        assert_eq!(
            search_directories_from(Some(&path), Some(Path::new("package"))),
            [
                PathBuf::from("package"),
                PathBuf::from("path-one"),
                PathBuf::from("path-two")
            ]
        );

        let empty = search_directories_from(Some(OsStr::new("")), None);
        assert!(
            empty.is_empty(),
            "empty PATH must not mean current directory"
        );
    }
}
