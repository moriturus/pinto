//! Per-user configuration stored outside a shared board.
//!
//! Personal Kanban keybindings are read from `$XDG_CONFIG_HOME/pinto/config.toml`. When the XDG
//! variable is unset, pinto follows the platform home configuration convention (`$HOME/.config`
//! on Unix-like systems and `%APPDATA%` on Windows). An absent file is equivalent to the built-in
//! defaults; a present file is strictly validated.

use crate::error::{Error, Result};
use crate::kanban_keys::KeyBindings;
use serde::Deserialize;
use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::fs;

const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";
const HOME: &str = "HOME";
const APPDATA: &str = "APPDATA";

/// The shape of the personal configuration file.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UserConfig {
    tui: UserTuiConfig,
}

/// TUI settings that are safe to keep per user.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UserTuiConfig {
    #[serde(default)]
    key_bindings: KeyBindings,
}

/// Load personal Kanban keybindings, or return built-in defaults when no user file exists.
pub(crate) async fn load() -> Result<KeyBindings> {
    let Some(path) = config_path() else {
        return Ok(KeyBindings::default());
    };
    load_from(&path).await
}

/// Load personal Kanban keybindings from an explicit path.
///
/// This narrow entry point keeps parsing independently testable without mutating process-wide
/// environment variables. The file is optional, but malformed contents are user errors.
pub(crate) async fn load_from(path: &Path) -> Result<KeyBindings> {
    if !fs::try_exists(path)
        .await
        .map_err(|error| Error::io(path, &error))?
    {
        return Ok(KeyBindings::default());
    }

    let text = fs::read_to_string(path)
        .await
        .map_err(|error| Error::io(path, &error))?;
    let document: toml::Value =
        toml::from_str(&text).map_err(|error| Error::parse(path, error.to_string()))?;
    validate_known_fields(&document).map_err(|message| Error::parse(path, message))?;
    let config: UserConfig =
        toml::from_str(&text).map_err(|error| Error::parse(path, error.to_string()))?;
    let bindings = config.tui.key_bindings.with_defaults();
    bindings
        .validate()
        .map_err(|error| Error::parse(path, format!("[tui.key_bindings] {error}")))?;
    Ok(bindings)
}

/// Resolve the optional personal configuration path from the process environment.
fn config_path() -> Option<PathBuf> {
    let xdg = env::var_os(XDG_CONFIG_HOME).filter(|value| !value.is_empty());
    let home = env::var_os(HOME).filter(|value| !value.is_empty());
    let appdata = env::var_os(APPDATA).filter(|value| !value.is_empty());
    config_path_from(xdg.as_deref(), home.as_deref(), appdata.as_deref())
}

fn config_path_from(
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
    appdata: Option<&OsStr>,
) -> Option<PathBuf> {
    let base = xdg_config_home.map(PathBuf::from).or_else(|| {
        if cfg!(windows) {
            appdata
                .map(PathBuf::from)
                .or_else(|| home.map(|path| PathBuf::from(path).join(".config")))
        } else {
            home.map(|path| PathBuf::from(path).join(".config"))
                .or_else(|| appdata.map(PathBuf::from))
        }
    })?;
    Some(base.join("pinto").join("config.toml"))
}

fn validate_known_fields(document: &toml::Value) -> std::result::Result<(), String> {
    let Some(root) = document.as_table() else {
        return Ok(());
    };
    reject_unknown_fields(root, "config", &["tui"])?;
    let Some(tui) = root.get("tui").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    reject_unknown_fields(tui, "[tui]", &["key_bindings"])
}

fn reject_unknown_fields(
    table: &toml::map::Map<String, toml::Value>,
    path: &str,
    allowed: &[&str],
) -> std::result::Result<(), String> {
    if let Some(field) = table
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(format!(
            "unknown user configuration field {path}.{field:?}; remove it or check [tui.key_bindings]"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kanban_keys::KeyAction;
    use tempfile::TempDir;

    #[test]
    fn config_path_prefers_xdg_and_uses_home_fallback() {
        let xdg = Path::new("/tmp/xdg");
        assert_eq!(
            config_path_from(Some(xdg.as_os_str()), None, None),
            Some(PathBuf::from("/tmp/xdg/pinto/config.toml"))
        );

        let home = Path::new("/tmp/home");
        assert_eq!(
            config_path_from(None, Some(home.as_os_str()), None),
            Some(PathBuf::from("/tmp/home/.config/pinto/config.toml"))
        );
    }

    #[tokio::test]
    async fn missing_user_config_uses_all_default_bindings() {
        let dir = TempDir::new().expect("temporary directory");
        let bindings = load_from(&dir.path().join("missing.toml"))
            .await
            .expect("missing config uses defaults");

        assert_eq!(
            bindings.keys(KeyAction::Quit),
            ["q".to_string(), "Esc".to_string()]
        );
        assert_eq!(
            bindings.keys(KeyAction::SelectLeft),
            ["h".to_string(), "Left".to_string()]
        );
    }

    #[tokio::test]
    async fn partial_user_config_keeps_unconfigured_defaults() {
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[tui.key_bindings]\nquit = [\"Ctrl+a\", \"Esc\"]\n")
            .expect("write config");

        let bindings = load_from(&path).await.expect("load config");
        assert_eq!(
            bindings.keys(KeyAction::Quit),
            ["Ctrl+a".to_string(), "Esc".to_string()]
        );
        assert_eq!(
            bindings.keys(KeyAction::SelectLeft),
            ["h".to_string(), "Left".to_string()]
        );
    }

    #[tokio::test]
    async fn invalid_user_key_binding_reports_path_action_and_syntax() {
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[tui.key_bindings]\ndetails = [\"Controlled+d\"]\n")
            .expect("write config");

        let error = load_from(&path).await.expect_err("invalid key rejected");
        let message = error.to_string();
        assert!(message.contains(path.to_str().expect("path is UTF-8")));
        assert!(message.contains("details"));
        assert!(message.contains("Ctrl"));
    }

    #[tokio::test]
    async fn unknown_user_action_is_rejected() {
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[tui.key_bindings]\nunknown = [\"x\"]\n").expect("write config");

        let error = load_from(&path).await.expect_err("unknown action rejected");
        assert!(matches!(&error, Error::Parse { .. }));
        assert!(error.to_string().contains("unknown operation"));
    }
}
