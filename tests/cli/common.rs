//! Shared helpers for CLI integration tests.

pub(crate) use assert_cmd::Command;
pub(crate) use predicates::prelude::*;
pub(crate) use std::path::Path;
pub(crate) use tempfile::TempDir;
pub(crate) use unicode_width::UnicodeWidthStr;

/// Build a `pinto` command rooted at a temporary board directory.
pub(crate) fn pinto(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("pinto").expect("binary builds");
    // Keep integration tests independent from a developer's real personal keybindings. Tests
    // that exercise the user file explicitly override this value with their own XDG directory.
    //
    // Pin a fixed English locale so assertions on English output do not break when the developer
    // runs the suite under a non-English shell. i18n tests that need another language build their
    // own command and set `LC_ALL`/`LANG` explicitly.
    cmd.current_dir(dir)
        .env("XDG_CONFIG_HOME", dir.join("test-xdg-config"))
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8");
    cmd
}

/// Create a non-interactive Unix editor shim for editor-backed commands.
#[cfg(unix)]
pub(crate) fn editor_script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write editor script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod script");
    path
}

/// Rewrite the board configuration with a WIP limit for a test.
pub(crate) fn set_wip_limit(dir: &Path, column: &str, limit: u32, enabled: bool) {
    let toml = format!(
        "columns = [\"todo\", \"in-progress\", \"review\", \"done\"]\n\
         done_column = \"done\"\n\n\
         [project]\nname = \"test\"\nkey = \"T\"\n\n\
         [tui]\nconfirm_quit = true\n\n\
         [storage]\nbackend = \"file\"\n\n\
         [wip]\nenabled = {enabled}\n\n[wip.limits]\n\"{column}\" = {limit}\n",
    );
    std::fs::write(dir.join(".pinto/config.toml"), toml).expect("rewrite config");
}

/// Initialize a board and add title-only items.
pub(crate) fn init_with_items(dir: &Path, titles: &[&str]) {
    pinto(dir).arg("init").assert().success();
    for title in titles {
        pinto(dir).args(["add", title]).assert().success();
    }
}

/// Build a pinto command with isolated Git identity settings.
pub(crate) fn pinto_isolated_git(dir: &Path) -> Command {
    let mut cmd = pinto(dir);
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dir.join("nonexistent-gitconfig"))
        .env("HOME", dir);
    cmd
}

/// Read a field from the newest commits in a temporary Git repository.
pub(crate) fn git_log_field(workdir: &Path, format: &str) -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["log", &format!("--format={format}")])
        .current_dir(workdir)
        .output()
        .expect("git log");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// Run a command and parse its successful stdout as JSON.
pub(crate) fn json_stdout(cmd: &mut Command) -> serde_json::Value {
    let output = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("stdout is valid JSON")
}

/// Extract the first detail from a single-item `show --json` response.
pub(crate) fn show_json(cmd: &mut Command) -> serde_json::Value {
    json_stdout(cmd)
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .expect("show --json contains one detail")
}

/// Return the terminal display width of a string.
pub(crate) fn display_cols(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Run a Git setup command in a test repository.
pub(crate) fn run_git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}
