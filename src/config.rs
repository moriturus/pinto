//! Shared board configuration (`.pinto/config.toml`).
//!
//! Stores Kanban columns (the workflow), project settings, and board-wide presentation settings
//! using TOML, the same format used by item frontmatter. Personal Kanban keybindings live in the
//! user configuration loaded by [`crate::user_config`].

use crate::backlog::ItemId;
use crate::error::{Error, Result};
use crate::timezone::DisplayTimezone;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use tokio::fs;

/// Default workflow columns, from left to right.
pub const DEFAULT_COLUMNS: [&str; 4] = ["todo", "in-progress", "review", "done"];

/// Board settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Kanban columns (workflow states).
    pub columns: Vec<String>,
    /// Completion column name. Items in this column are explicitly sorted by completion time
    /// (`done_at`, newest first) when the board is displayed.
    ///
    /// The completion column is selected by name, independently of its position in `columns`.
    /// Completion-time sorting applies only to this column.
    ///
    /// This field is required in `config.toml`; `init` writes `done`, the last default column.
    /// TOML places it before the `[project]` table, so keep it in this position.
    pub done_column: String,
    /// Project information.
    pub project: Project,
    /// Configuring Interactive Kanban (TUI). `init` writes out default values.
    ///
    /// In TOML, it is written as a table after `[project]`.
    pub tui: TuiConfig,
    /// Persistence backend selection. `init` writes out the default (file).
    ///
    /// In TOML, it is written out as a table at the end.
    pub storage: StorageConfig,
    /// WIP (work in progress) limit. `init` writes out the default (enabled, no restrictions).
    ///
    /// In TOML, it is written out as a table at the end.
    pub wip: WipConfig,
    /// Display settings shared by `show` and the Kanban details popup. Missing
    /// entries use the built-in defaults; Markdown rendering is enabled by default.
    #[serde(default)]
    pub display: DisplayConfig,
    /// Optional story-point aggregation for parent PBIs.
    #[serde(default)]
    pub points: PointsConfig,
}

/// Work-in-progress (WIP) limit settings.
///
/// Set a per-column upper limit in [`WipConfig::limits`]. The default is `enabled = true` with no
/// limits, so behavior is unchanged until a limit is configured. Set `enabled = false` to disable
/// checking for the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WipConfig {
    /// Whether WIP limit checking is enabled; `false` disables it for the project.
    ///
    /// This scalar is serialized before the [`WipConfig::limits`] table.
    pub enabled: bool,
    /// Map of column names to maximum concurrent PBI counts. A column without an entry is unlimited.
    ///
    /// `BTreeMap` keeps `[wip.limits]` deterministic; omit the table when it is empty.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub limits: BTreeMap<String, u32>,
}

impl Default for WipConfig {
    fn default() -> Self {
        // Enable checking by default; with no limits configured, it produces no warnings.
        Self {
            enabled: true,
            limits: BTreeMap::new(),
        }
    }
}

/// Persistence backend configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// Type of destination backend.
    pub backend: StorageBackend,
}

/// Storage backend selected for the board.
///
/// All backends preserve the local-first, Git-friendly workflow. Serde rejects unknown values and
/// `Config::load` reports a clear parse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    /// Local files (the default), stored as Markdown under `.pinto/`.
    #[default]
    File,
    /// Git backend; commit each change operation in addition to saving the files.
    Git,
    /// SQLite backend (optional `sqlite` feature), stored in `.pinto/board.sqlite3`. Use
    /// `migrate` to move between backends; builds without the feature reject `sqlite` as unknown.
    #[cfg(feature = "sqlite")]
    Sqlite,
}

impl std::fmt::Display for StorageBackend {
    /// Match the value written to `config.toml` (`rename_all = "lowercase"` of serde).
    /// Use the same notation for message display and migration command argument interpretation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            StorageBackend::File => "file",
            StorageBackend::Git => "git",
            #[cfg(feature = "sqlite")]
            StorageBackend::Sqlite => "sqlite",
        };
        f.write_str(s)
    }
}

/// Project information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Display name.
    pub name: String,
    /// PBI ID prefix (e.g. `T`).
    pub key: String,
}

/// Configuring Interactive Kanban (TUI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TuiConfig {
    /// Whether to display a confirmation popup when exiting (`q`). `false` terminates immediately without confirmation.
    pub confirm_quit: bool,
    /// Workflow columns hidden from the default Kanban display. Explicit `kanban --column` values override this list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_columns: Vec<String>,
}

impl Default for TuiConfig {
    fn default() -> Self {
        // To prevent accidental termination, the default setting is to confirm (opt-out possible in settings).
        Self {
            confirm_quit: true,
            hidden_columns: Vec::new(),
        }
    }
}

/// Display settings shared by `show` and the interactive Kanban details popup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    /// Render PBI bodies as Markdown (styled headings, bullets, code) instead of
    /// raw text. `true` by default; set `false` to use plain text.
    pub markdown: bool,
    /// Human-readable timestamp timezone: `local`, `UTC`, or a fixed `±HH:MM` offset.
    #[serde(default)]
    pub timezone: DisplayTimezone,
}

/// Story-point calculation settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PointsConfig {
    /// Replace a parent PBI's displayed points with the sum of its active descendants.
    pub aggregate_children: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        // Markdown rendering is the standard, readable display (opt-out available).
        Self {
            markdown: true,
            timezone: DisplayTimezone::Local,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            columns: DEFAULT_COLUMNS
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            // Specify the completion column of the default workflow (`done` at the end of `DEFAULT_COLUMNS`).
            done_column: DEFAULT_COLUMNS
                .last()
                .map(std::string::ToString::to_string)
                .unwrap_or_else(|| "done".to_string()),
            project: Project {
                name: "pinto".to_string(),
                key: "T".to_string(),
            },
            tui: TuiConfig::default(),
            storage: StorageConfig::default(),
            wip: WipConfig::default(),
            display: DisplayConfig::default(),
            points: PointsConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from TOML file (I/O is asynchronous).
    pub async fn load(path: &Path) -> Result<Config> {
        let text = fs::read_to_string(path)
            .await
            .map_err(|e| Error::io(path, &e))?;
        let document: toml::Value =
            toml::from_str(&text).map_err(|e| Error::parse(path, e.to_string()))?;
        validate_known_fields(&document).map_err(|message| Error::parse(path, message))?;
        let config: Config =
            toml::from_str(&text).map_err(|e| Error::parse(path, e.to_string()))?;
        validate_semantics(path, &config)?;
        Ok(config)
    }

    /// Export settings to TOML file (create parent directory if necessary, I/O is asynchronous).
    ///
    /// Writing is performed by replacing the temporary file → `rename` with corruption resistance ([`crate::storage::atomic_write`]).
    pub async fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| Error::parse(path, e.to_string()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::io(parent, &e))?;
        }
        crate::storage::atomic_write(path, &text).await
    }
}

/// Reject unknown fields before serde turns the document into typed settings.
///
/// `deny_unknown_fields` is also present on every fixed-shape settings table. This
/// preflight is what lets errors identify the TOML table and field rather than only
/// reporting a generic serde field name.
fn validate_known_fields(document: &toml::Value) -> std::result::Result<(), String> {
    let Some(root) = document.as_table() else {
        return Ok(());
    };

    reject_unknown_fields(
        root,
        "config",
        &[
            "columns",
            "done_column",
            "project",
            "tui",
            "storage",
            "wip",
            "display",
            "points",
        ],
    )?;
    validate_nested_fields(root, "project", "[project]", &["name", "key"])?;
    validate_nested_fields(root, "tui", "[tui]", &["confirm_quit", "hidden_columns"])?;
    validate_nested_fields(root, "storage", "[storage]", &["backend"])?;
    validate_nested_fields(root, "wip", "[wip]", &["enabled", "limits"])?;
    validate_nested_fields(root, "display", "[display]", &["markdown", "timezone"])?;
    validate_nested_fields(root, "points", "[points]", &["aggregate_children"])?;
    Ok(())
}

fn validate_nested_fields(
    root: &toml::map::Map<String, toml::Value>,
    field: &str,
    path: &str,
    allowed: &[&str],
) -> std::result::Result<(), String> {
    let Some(table) = root.get(field).and_then(toml::Value::as_table) else {
        return Ok(());
    };
    reject_unknown_fields(table, path, allowed)
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
        if path == "[tui]" && field == "key_bindings" {
            return Err(
                "personal keybindings do not belong in shared .pinto/config.toml; move [tui.key_bindings] to $XDG_CONFIG_HOME/pinto/config.toml"
                    .to_string(),
            );
        }
        return Err(format!(
            "unknown configuration field {path}.{field:?}; remove it or check the documented schema"
        ));
    }
    Ok(())
}

fn validate_semantics(path: &Path, config: &Config) -> Result<()> {
    if config.columns.is_empty() {
        return Err(Error::parse(
            path,
            "columns must contain at least one non-blank column",
        ));
    }

    let mut columns = BTreeSet::new();
    for (index, column) in config.columns.iter().enumerate() {
        if column.trim().is_empty() {
            return Err(Error::parse(
                path,
                format!("[columns][{index}] must not be blank"),
            ));
        }
        if !columns.insert(column) {
            return Err(Error::parse(
                path,
                format!("[columns][{index}] is a duplicate of column {column:?}"),
            ));
        }
    }

    if !config
        .columns
        .iter()
        .any(|column| column == &config.done_column)
    {
        return Err(Error::parse(
            path,
            format!(
                "done_column {:?} is not included in columns",
                config.done_column
            ),
        ));
    }
    if let Some(hidden) = config
        .tui
        .hidden_columns
        .iter()
        .find(|hidden| !config.columns.iter().any(|column| column == *hidden))
    {
        return Err(Error::parse(
            path,
            format!(
                "[tui] hidden_columns contains unknown status {hidden:?}; remove it or add it to columns"
            ),
        ));
    }
    if let Some((column, _)) = config.wip.limits.iter().find(|(column, _)| {
        !config
            .columns
            .iter()
            .any(|configured| configured == *column)
    }) {
        return Err(Error::parse(
            path,
            format!(
                "[wip.limits] column {column:?} is not included in columns; remove it or add it to columns"
            ),
        ));
    }
    if config.project.name.trim().is_empty() {
        return Err(Error::parse(
            path,
            "[project].name must not be empty or whitespace",
        ));
    }
    if ItemId::try_new(&config.project.key, 1).is_err() {
        return Err(Error::parse(
            path,
            format!(
                "[project].key {:?} is not a safe item-id prefix; use ASCII letters only",
                config.project.key
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A complete `config.toml` of the current spec. All sections are required, so use them as the basis for the test.
    /// You can add individual sections (e.g. `[wip.limits]`) by concatenating `extra` to the end.
    fn complete_config(extra: &str) -> String {
        format!(
            "columns = [\"todo\", \"in-progress\", \"review\", \"done\"]\n\
             done_column = \"done\"\n\n\
             [project]\nname = \"x\"\nkey = \"T\"\n\n\
             [tui]\nconfirm_quit = true\n\n\
             [storage]\nbackend = \"file\"\n\n\
             [wip]\nenabled = true\n{extra}"
        )
    }

    #[test]
    fn default_has_standard_columns_and_key() {
        let c = Config::default();
        assert_eq!(c.columns, ["todo", "in-progress", "review", "done"]);
        assert_eq!(c.project.key, "T");
    }

    #[test]
    fn default_done_column_is_last_column() {
        let c = Config::default();
        assert_eq!(c.done_column, "done");
    }

    #[test]
    fn default_tui_confirms_quit() {
        assert!(Config::default().tui.confirm_quit);
    }

    #[test]
    fn default_tui_shows_all_columns() {
        assert!(Config::default().tui.hidden_columns.is_empty());
    }

    #[test]
    fn default_display_renders_markdown() {
        assert!(Config::default().display.markdown);
        assert_eq!(
            Config::default().display.timezone,
            crate::timezone::DisplayTimezone::Local
        );
    }

    #[test]
    fn child_point_aggregation_is_disabled_by_default() {
        assert!(!Config::default().points.aggregate_children);
    }

    #[tokio::test]
    async fn load_reads_explicit_child_point_aggregation_opt_in() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!(
                "{}\n[points]\naggregate_children = true\n",
                complete_config("")
            ),
        )
        .expect("write");

        let loaded = Config::load(&path).await.expect("load succeeds");

        assert!(loaded.points.aggregate_children);
    }

    #[tokio::test]
    async fn load_rejects_unknown_point_setting_with_a_configuration_path() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!(
                "{}\n[points]\naggregate_chidren = true\n",
                complete_config("")
            ),
        )
        .expect("write");

        let error = Config::load(&path)
            .await
            .expect_err("unknown points setting rejected");
        let message = error.to_string();
        assert!(message.contains("[points]"), "field path: {message}");
        assert!(
            message.contains("aggregate_chidren"),
            "field name: {message}"
        );
    }

    #[tokio::test]
    async fn load_defaults_display_markdown_when_section_absent() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, complete_config("")).expect("write");
        let loaded = Config::load(&path).await.expect("load succeeds");
        assert!(loaded.display.markdown, "absent [display] defaults to on");
        assert_eq!(
            loaded.display.timezone,
            crate::timezone::DisplayTimezone::Local,
            "absent [display] uses the local timezone"
        );
    }

    #[tokio::test]
    async fn load_respects_display_markdown_opt_out() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!(
                "{}
[display]
markdown = false
",
                complete_config("")
            ),
        )
        .expect("write");
        let loaded = Config::load(&path).await.expect("load succeeds");
        assert!(
            !loaded.display.markdown,
            "[display] markdown=false opts out"
        );
    }

    #[tokio::test]
    async fn load_rejects_an_invalid_display_timezone_with_guidance() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!(
                "{}\n[display]\ntimezone = \"Mars/Base\"\n",
                complete_config("")
            ),
        )
        .expect("write");

        let error = Config::load(&path).await.expect_err("timezone rejected");
        let message = error.to_string();
        assert!(message.contains("local"), "mentions local: {message}");
        assert!(message.contains("UTC"), "mentions UTC: {message}");
        assert!(
            message.contains("HH:MM"),
            "mentions offset format: {message}"
        );
    }

    #[tokio::test]
    async fn load_rejects_unknown_nested_fields_with_a_configuration_path() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!("{}\n[display]\ntimezome = \"UTC\"\n", complete_config("")),
        )
        .expect("write");

        let error = Config::load(&path)
            .await
            .expect_err("unknown display field rejected");
        let message = error.to_string();
        assert!(message.contains("display"), "field path: {message}");
        assert!(message.contains("timezome"), "field name: {message}");
    }

    #[tokio::test]
    async fn load_rejects_unknown_top_level_fields() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, format!("unknown = true\n{}", complete_config(""))).expect("write");

        let error = Config::load(&path)
            .await
            .expect_err("unknown top-level field rejected");
        let message = error.to_string();
        assert!(message.contains("unknown"), "field name: {message}");
    }

    #[tokio::test]
    async fn load_rejects_blank_and_duplicate_columns() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");

        for (columns, expected) in [
            ("[]", "at least one"),
            ("[\"todo\", \" \"]", "blank"),
            ("[\"todo\", \"todo\"]", "duplicate"),
        ] {
            let config = complete_config("")
                .replace(
                    "columns = [\"todo\", \"in-progress\", \"review\", \"done\"]",
                    &format!("columns = {columns}"),
                )
                .replace("done_column = \"done\"", "done_column = \"todo\"");
            std::fs::write(&path, config).expect("write");

            let error = Config::load(&path)
                .await
                .expect_err("invalid columns rejected");
            assert!(
                error.to_string().contains(expected),
                "{expected} columns: {error}"
            );
        }
    }

    #[tokio::test]
    async fn load_rejects_wip_limits_for_unknown_columns() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!("{}\n[wip.limits]\nmissing = 1\n", complete_config("")),
        )
        .expect("write");

        let error = Config::load(&path)
            .await
            .expect_err("unknown WIP column rejected");
        let message = error.to_string();
        assert!(message.contains("wip.limits"), "field path: {message}");
        assert!(message.contains("missing"), "column name: {message}");
    }

    #[tokio::test]
    async fn load_rejects_blank_project_name() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            complete_config("").replace("name = \"x\"", "name = \"  \""),
        )
        .expect("write");

        let error = Config::load(&path)
            .await
            .expect_err("blank project name rejected");
        let message = error.to_string();
        assert!(message.contains("[project].name"), "field path: {message}");
        assert!(message.contains("empty"), "guidance: {message}");
    }

    #[tokio::test]
    async fn load_reads_tui_hidden_columns() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        let text = complete_config("").replace(
            "[tui]\nconfirm_quit = true",
            "[tui]\nconfirm_quit = true\nhidden_columns = [\"in-progress\", \"review\"]",
        );
        std::fs::write(&path, text).expect("write");

        let loaded = Config::load(&path).await.expect("load succeeds");

        assert_eq!(loaded.tui.hidden_columns, ["in-progress", "review"]);
    }

    #[tokio::test]
    async fn load_rejects_unknown_tui_hidden_column_with_guidance() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        let text = complete_config("").replace(
            "[tui]\nconfirm_quit = true",
            "[tui]\nconfirm_quit = true\nhidden_columns = [\"missing\"]",
        );
        std::fs::write(&path, text).expect("write");

        let error = Config::load(&path)
            .await
            .expect_err("unknown column rejected");
        let message = error.to_string();

        assert!(
            message.contains("hidden_columns"),
            "mentions the setting: {message}"
        );
        assert!(
            message.contains("missing"),
            "mentions the invalid value: {message}"
        );
        assert!(
            message.contains("columns"),
            "explains how to fix it: {message}"
        );
    }

    #[test]
    fn default_shared_config_omits_personal_key_bindings() {
        let text = toml::to_string(&Config::default()).expect("default config serializes");
        assert!(!text.contains("key_bindings"));
    }

    #[tokio::test]
    async fn load_rejects_key_bindings_from_shared_board_config() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!(
                "{}\n[tui.key_bindings]\nquit = [\"Ctrl+a\", \"Esc\"]\n",
                complete_config("")
            ),
        )
        .expect("write");

        let error = Config::load(&path)
            .await
            .expect_err("shared config must reject personal bindings");
        let message = error.to_string();
        assert!(message.contains("key_bindings"));
        assert!(message.contains("XDG_CONFIG_HOME/pinto/config.toml"));
    }

    #[tokio::test]
    async fn shared_config_key_binding_error_is_not_a_key_syntax_error() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!(
                "{}\n[tui.key_bindings]\nedit = [\"Controlled+a\"]\n",
                complete_config("")
            ),
        )
        .expect("write");

        let error = Config::load(&path).await.expect_err("invalid key rejected");
        let message = error.to_string();
        assert!(message.contains("XDG_CONFIG_HOME/pinto/config.toml"));
        assert!(!message.contains("Controlled+a"));
    }

    #[tokio::test]
    async fn load_without_tui_table_is_error() {
        // `[tui]` is required in the current specification. Do not ignore the omissions and treat them as parse errors.
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[storage]\nbackend = \"file\"\n\n[wip]\nenabled = true\n",
        )
        .expect("write");

        let err = Config::load(&path)
            .await
            .expect_err("missing [tui] rejected");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn load_tui_confirm_quit_false_overrides_default() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"in-progress\", \"review\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = false\n\n[storage]\nbackend = \"file\"\n\n[wip]\nenabled = true\n",
        )
        .expect("write");

        let loaded = Config::load(&path).await.expect("load succeeds");
        assert!(!loaded.tui.confirm_quit);
    }

    #[test]
    fn default_storage_backend_is_file() {
        assert_eq!(Config::default().storage.backend, StorageBackend::File);
    }

    #[test]
    fn known_field_preflight_accepts_every_serialized_config_field() {
        // Keep the diagnostic preflight in sync with serde's schema whenever a serializable
        // configuration field is added. Without this check, a field could be accepted by serde
        // but rejected by the more specific preflight error path.
        let text = toml::to_string(&Config::default()).expect("default config serializes");
        let document = toml::from_str(&text).expect("serialized config is valid TOML");

        validate_known_fields(&document).expect("serialized config fields are all allowlisted");
    }

    #[tokio::test]
    async fn load_without_storage_table_is_error() {
        // `[storage]` is required in the current specifications. Any omissions will result in a parsing error.
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[wip]\nenabled = true\n",
        )
        .expect("write");

        let err = Config::load(&path)
            .await
            .expect_err("missing [storage] rejected");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn load_explicit_file_backend() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, complete_config("")).expect("write");

        let loaded = Config::load(&path).await.expect("load succeeds");
        assert_eq!(loaded.storage.backend, StorageBackend::File);
    }

    #[tokio::test]
    async fn load_rejects_an_unsafe_project_key_before_any_backend_opens() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        for key in [
            "123",
            "p9",
            "bug_fix",
            "PROJ-1",
            "../outside",
            "/tmp/outside",
            "T\\outside",
            "T space",
        ] {
            let toml_key = key.replace('\\', "\\\\");
            let config =
                complete_config("").replace("key = \"T\"", &format!("key = \"{toml_key}\""));
            std::fs::write(&path, config).expect("write");

            let err = Config::load(&path)
                .await
                .expect_err("unsafe project key rejected");
            assert!(
                err.to_string().contains("safe item-id prefix"),
                "key {key:?}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn load_unknown_backend_is_a_clear_error() {
        // Don't ignore unknown backend values and make them clear errors. `sqlite` is enabled when the function is enabled.
        // Always verify with an unknown fictitious value to ensure it is a legitimate value.
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"postgres\"\n\n[wip]\nenabled = true\n",
        )
        .expect("write");

        let err = Config::load(&path)
            .await
            .expect_err("unknown backend rejected");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    /// In builds with the `sqlite` feature disabled, `backend = "sqlite"` is rejected as an unknown value.
    #[cfg(not(feature = "sqlite"))]
    #[tokio::test]
    async fn load_sqlite_backend_rejected_without_feature() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"sqlite\"\n\n[wip]\nenabled = true\n",
        )
        .expect("write");

        let err = Config::load(&path)
            .await
            .expect_err("sqlite rejected without feature");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    /// In a build with the `sqlite` feature enabled, `backend = "sqlite"` can be read as a legal value.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn load_sqlite_backend_accepted_with_feature() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"sqlite\"\n\n[wip]\nenabled = true\n",
        )
        .expect("write");

        let loaded = Config::load(&path).await.expect("load succeeds");
        assert_eq!(loaded.storage.backend, StorageBackend::Sqlite);
    }

    #[test]
    fn default_wip_is_enabled_with_no_limits() {
        let c = Config::default();
        assert!(c.wip.enabled);
        assert!(c.wip.limits.is_empty());
    }

    #[tokio::test]
    async fn load_without_wip_table_is_error() {
        // `[wip]` is required in the current specification. Any omissions will result in a parsing error.
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"file\"\n",
        )
        .expect("write");

        let err = Config::load(&path)
            .await
            .expect_err("missing [wip] rejected");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn load_wip_limits_and_enabled_flag() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"in-progress\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"file\"\n\n[wip]\nenabled = false\n\n[wip.limits]\nin-progress = 3\n",
        )
        .expect("write");

        let loaded = Config::load(&path).await.expect("load succeeds");
        assert!(!loaded.wip.enabled);
        assert_eq!(loaded.wip.limits.get("in-progress"), Some(&3));
    }

    #[tokio::test]
    async fn save_then_load_roundtrips_wip_limits() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.wip.limits.insert("in-progress".to_string(), 2);
        config.wip.limits.insert("review".to_string(), 1);

        config.save(&path).await.expect("save succeeds");
        let loaded = Config::load(&path).await.expect("load succeeds");

        assert_eq!(loaded, config);
    }

    #[tokio::test]
    async fn load_without_done_column_is_error() {
        // done_column is required in the current specification. Any omissions will result in a parsing error.
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "columns = [\"todo\", \"done\"]\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"file\"\n\n[wip]\nenabled = true\n",
        )
        .expect("write");

        let err = Config::load(&path)
            .await
            .expect_err("missing done_column rejected");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn load_rejects_done_column_outside_workflow() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        let config = complete_config("")
            .replace(
                "columns = [\"todo\", \"in-progress\", \"review\", \"done\"]",
                "columns = [\"todo\"]",
            )
            .replace("done_column = \"done\"", "done_column = \"missing\"");
        std::fs::write(&path, config).expect("write");

        let error = Config::load(&path).await.expect_err("invalid done column");
        assert!(error.to_string().contains("done_column"));
    }

    #[tokio::test]
    async fn save_then_load_roundtrips() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        let config = Config::default();

        config.save(&path).await.expect("save succeeds");
        let loaded = Config::load(&path).await.expect("load succeeds");

        assert_eq!(loaded, config);
    }

    #[tokio::test]
    async fn save_creates_missing_parent_dirs() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("nested").join("config.toml");

        Config::default()
            .save(&path)
            .await
            .expect("save creates parents");
        assert!(path.is_file());
    }

    #[tokio::test]
    async fn load_invalid_toml_errors_without_panic() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "columns = \nthis is not valid toml").expect("write");

        assert!(Config::load(&path).await.is_err());
    }
}
