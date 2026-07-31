//! Board integrity diagnostics and conservative recovery.
//!
//! The parent module holds the public report types, the inspect/fix/report entry points, and the
//! raw-record data model shared by both submodules. Board reading and analysis live in `inspect`;
//! the conservative, mechanical repairs live in `repair`. The raw-record structs stay here so both
//! submodules can read their private fields without any visibility widening.

use super::open_board;
#[cfg(feature = "sqlite")]
use crate::backlog::BacklogItem;
use crate::backlog::ItemId;
use crate::config::Config;
use crate::error::Result;
use crate::sprint::{Sprint, SprintId, SprintState};
use crate::storage::{Backend, parse_frontmatter, sprint_from_markdown_raw};
use inspect::inspect_board;
use repair::{apply_safe_fixes, repair_duplicate_item_ids};
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};

// Inspection helpers the test module drives directly; re-imported so `use super::*` resolves them.
#[cfg(test)]
use inspect::{
    analyze_action_sources, analyze_records, analyze_sprints, graph_cycles, read_issued_history,
};

mod inspect;
mod repair;

/// Category of an integrity issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DoctorIssueKind {
    DanglingDependency,
    DanglingParent,
    DanglingSprint,
    ParentCycle,
    DependencyCycle,
    DuplicateId,
    IssuedId,
    InvalidStatus,
    RankAnomaly,
    Collision,
    MalformedRecord,
    Filename,
}

/// One actionable board-integrity finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorIssue {
    pub kind: DoctorIssueKind,
    pub location: String,
    pub detail: String,
    pub repair: String,
}

/// One safe mechanical change applied by doctor --fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorFix {
    pub description: String,
}

/// Result of a board integrity scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub issues: Vec<DoctorIssue>,
    pub fixes: Vec<DoctorFix>,
}

/// Inspect a board and optionally apply safe, mechanical repairs.
///
/// # Errors
///
/// Returns [`crate::error::Error::NotInitialized`], persistence, parsing, or backend errors while
/// inspecting the board. With `fix = false` no durable changes are made and retrying after a
/// transient read failure is safe. With `fix = true`, earlier conservative repairs may remain
/// durable if a later repair or commit fails; inspect the report and rerun after correcting the
/// cause rather than assuming a complete repair.
pub async fn doctor(project_dir: &Path, fix: bool) -> Result<DoctorReport> {
    if fix {
        let (board_dir, backend, config, _lock) = super::open_board_locked(project_dir).await?;
        run_doctor(&board_dir, &backend, &config, true).await
    } else {
        let (board_dir, backend, config) = open_board(project_dir).await?;
        run_doctor(&board_dir, &backend, &config, false).await
    }
}

async fn run_doctor(
    board_dir: &Path,
    backend: &Backend,
    config: &Config,
    fix: bool,
) -> Result<DoctorReport> {
    run_doctor_with(board_dir, backend, fix, || {
        inspect_board(board_dir, backend, config)
    })
    .await
}

/// Drive the inspect/fix/report flow with an injected inspection step so tests
/// can observe exactly how many full inspections one doctor run performs.
async fn run_doctor_with<F, Fut>(
    board_dir: &Path,
    backend: &Backend,
    fix: bool,
    mut inspect: F,
) -> Result<DoctorReport>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Inspection>>,
{
    let initial = inspect().await?;
    if !fix {
        return Ok(DoctorReport {
            issues: initial.issues,
            fixes: Vec::new(),
        });
    }
    // Renumber duplicate IDs first, then re-inspect so the filename and issued-history repairs
    // see the post-renumber board (for example, a surviving copy whose filename still needs to
    // be normalized). Only pay for the extra inspection when a duplicate was actually repaired.
    let mut fixes = repair_duplicate_item_ids(board_dir, &initial).await?;
    let inspection = if fixes.is_empty() {
        initial
    } else {
        inspect().await?
    };
    let safe_fixes = apply_safe_fixes(board_dir, &inspection).await?;
    let board_changed_after_inspection = !safe_fixes.is_empty();
    fixes.extend(safe_fixes);
    if fixes.is_empty() {
        return Ok(DoctorReport {
            issues: inspection.issues,
            fixes,
        });
    }
    backend.commit("pinto: doctor --fix").await?;
    // The report must describe the post-fix board, so re-inspect only when the
    // safe-fix stage changed the board after the latest inspection.
    let final_state = if board_changed_after_inspection {
        inspect().await?
    } else {
        inspection
    };
    Ok(DoctorReport {
        issues: final_state.issues,
        fixes,
    })
}

#[derive(Debug)]
struct Inspection {
    records: Vec<RawItemRecord>,
    issues: Vec<DoctorIssue>,
    issued: IssuedHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordArea {
    Tasks,
    Archive,
    #[cfg(feature = "sqlite")]
    DatabaseActive,
    #[cfg(feature = "sqlite")]
    DatabaseArchive,
}

impl RecordArea {
    fn directory(self, board_dir: &Path) -> Option<PathBuf> {
        match self {
            Self::Tasks => Some(board_dir.join("tasks")),
            Self::Archive => Some(board_dir.join("archive")),
            #[cfg(feature = "sqlite")]
            Self::DatabaseActive | Self::DatabaseArchive => None,
        }
    }

    /// Whether this area holds *active* PBIs. Rank uniqueness is required only among active PBIs, so
    /// the SQLite active store must join the file `tasks` store in that scope while the archive
    /// stores stay out of it, keeping the three backends' rank diagnostics identical.
    fn is_active_store(self) -> bool {
        match self {
            Self::Tasks => true,
            Self::Archive => false,
            #[cfg(feature = "sqlite")]
            Self::DatabaseActive => true,
            #[cfg(feature = "sqlite")]
            Self::DatabaseArchive => false,
        }
    }
}

#[derive(Debug, Clone)]
enum RawField<T> {
    Missing,
    Invalid(String),
    Present(T),
}

impl<T> RawField<T> {
    fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Present(value) => Some(value),
            Self::Missing | Self::Invalid(_) => None,
        }
    }
}

/// The Retro/Review source link of an action PBI, as read from raw frontmatter.
#[derive(Debug, Clone)]
struct RawSource {
    kind: String,
    sprint_id: String,
}

#[derive(Debug, Clone)]
struct RawItemRecord {
    path: PathBuf,
    area: RecordArea,
    document: Option<String>,
    filename: Option<String>,
    frontmatter_error: Option<String>,
    id: RawField<String>,
    status: RawField<String>,
    rank: RawField<String>,
    title: RawField<String>,
    sprint: RawField<String>,
    parent: RawField<String>,
    depends_on: RawField<Vec<String>>,
    source: RawField<RawSource>,
}

impl RawItemRecord {
    fn from_document(path: PathBuf, area: RecordArea, text: String) -> Self {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        let missing = || Self {
            path: path.clone(),
            area,
            document: Some(text.clone()),
            filename: filename.clone(),
            frontmatter_error: None,
            id: RawField::Missing,
            status: RawField::Missing,
            rank: RawField::Missing,
            title: RawField::Missing,
            sprint: RawField::Missing,
            parent: RawField::Missing,
            depends_on: RawField::Missing,
            source: RawField::Missing,
        };
        let Some((front, _body)) = parse_frontmatter(&text) else {
            let mut record = missing();
            record.frontmatter_error = Some("missing frontmatter delimiter".to_string());
            return record;
        };
        let value = match toml::from_str::<toml::Value>(front) {
            Ok(value) => value,
            Err(error) => {
                let mut record = missing();
                record.frontmatter_error = Some(error.to_string());
                return record;
            }
        };
        let Some(table) = value.as_table() else {
            let mut record = missing();
            record.frontmatter_error = Some("frontmatter must be a TOML table".to_string());
            return record;
        };
        Self {
            path,
            area,
            document: Some(text),
            filename,
            frontmatter_error: None,
            id: string_field(table, "id"),
            status: string_field(table, "status"),
            rank: string_field(table, "rank"),
            title: string_field(table, "title"),
            sprint: string_field(table, "sprint"),
            parent: string_field(table, "parent"),
            depends_on: string_list_field(table, "depends_on"),
            source: source_field(table),
        }
    }

    #[cfg(feature = "sqlite")]
    fn from_item(board_dir: &Path, item: BacklogItem, archived: bool) -> Self {
        let store = if archived { "archive" } else { "items" };
        Self {
            path: board_dir
                .join(format!("board.sqlite3#{store}"))
                .join(item.id.to_string()),
            area: if archived {
                RecordArea::DatabaseArchive
            } else {
                RecordArea::DatabaseActive
            },
            document: None,
            filename: None,
            frontmatter_error: None,
            id: RawField::Present(item.id.to_string()),
            status: RawField::Present(item.status.as_str().to_string()),
            rank: RawField::Present(item.rank.as_str().to_string()),
            title: RawField::Present(item.title.clone()),
            sprint: item.sprint.map_or(RawField::Missing, RawField::Present),
            parent: item
                .parent
                .map_or(RawField::Missing, |id| RawField::Present(id.to_string())),
            depends_on: RawField::Present(
                item.depends_on
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect(),
            ),
            source: item.source.map_or(RawField::Missing, |source| {
                RawField::Present(RawSource {
                    kind: source.kind.as_str().to_string(),
                    sprint_id: source.sprint_id.to_string(),
                })
            }),
        }
    }

    fn valid_id(&self) -> Option<ItemId> {
        self.id.as_ref()?.parse().ok()
    }

    fn location(&self) -> String {
        self.path.display().to_string()
    }
}

#[derive(Debug, Clone)]
struct RawSprintRecord {
    path: PathBuf,
    frontmatter_error: Option<String>,
    id: RawField<String>,
    state: RawField<String>,
    title: RawField<String>,
    parse_error: Option<String>,
    /// The strictly-parsed Sprint, present only when the document forms one. Doctor reuses the
    /// persistence parser (as it does for child records) so it can check the domain invariants —
    /// a two-sided non-inverted period and a Goal on an active Sprint — that lenient field
    /// extraction cannot see. When parsing fails, `parse_error` retains the reason so a valid set of
    /// required fields cannot be mistaken for a healthy record.
    parsed: Option<Sprint>,
}

impl RawSprintRecord {
    fn from_document(path: PathBuf, text: String) -> Self {
        let missing = || Self {
            path: path.clone(),
            frontmatter_error: None,
            id: RawField::Missing,
            state: RawField::Missing,
            title: RawField::Missing,
            parse_error: None,
            parsed: None,
        };
        let Some((front, _goal)) = parse_frontmatter(&text) else {
            let mut record = missing();
            record.frontmatter_error = Some("missing frontmatter delimiter".to_string());
            return record;
        };
        let value = match toml::from_str::<toml::Value>(front) {
            Ok(value) => value,
            Err(error) => {
                let mut record = missing();
                record.frontmatter_error = Some(error.to_string());
                return record;
            }
        };
        let Some(table) = value.as_table() else {
            let mut record = missing();
            record.frontmatter_error = Some("frontmatter must be a TOML table".to_string());
            return record;
        };
        let (parsed, parse_error) = match sprint_from_markdown_raw(&text, &path) {
            Ok(sprint) => (Some(sprint), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            id: string_field(table, "id"),
            state: string_field(table, "state"),
            title: string_field(table, "title"),
            // Parse without normalizing the Goal outcome so a raw `goal_achieved` paired with a
            // blank Goal is preserved and reported, matching what `import` rejects.
            parsed,
            parse_error,
            path,
            frontmatter_error: None,
        }
    }

    #[cfg(feature = "sqlite")]
    fn from_sprint(sprint: Sprint) -> Self {
        // Keep the row's raw `goal_achieved`: `doctor` reports a Goal outcome paired with a blank
        // Goal as a violation on every backend, matching what `import` rejects, rather than clearing
        // it the way the normal read paths do.
        Self {
            path: PathBuf::from(format!("board.sqlite3#sprints/{}", sprint.id)),
            frontmatter_error: None,
            id: RawField::Present(sprint.id.to_string()),
            state: RawField::Present(sprint.state.to_string()),
            title: RawField::Present(sprint.title.clone()),
            parse_error: None,
            parsed: Some(sprint),
        }
    }

    fn valid_id(&self) -> Option<SprintId> {
        self.id.as_ref()?.parse().ok()
    }

    fn required_fields_are_valid(&self) -> bool {
        matches!(&self.id, RawField::Present(id) if id.parse::<SprintId>().is_ok())
            && matches!(&self.state, RawField::Present(state) if state.parse::<SprintState>().is_ok())
            && matches!(&self.title, RawField::Present(title) if !title.trim().is_empty())
    }

    fn location(&self) -> String {
        self.path.display().to_string()
    }
}

#[derive(Debug, Default)]
struct IssuedHistory {
    path: PathBuf,
    ids: HashSet<ItemId>,
    invalid: Vec<(usize, String)>,
    duplicates: Vec<(usize, String)>,
}

/// Read the optional `[source]` action-link table from item frontmatter.
///
/// A missing table is [`RawField::Missing`]; a present table whose `kind` or `sprint_id` is absent
/// or non-string is [`RawField::Invalid`] so the analyzer can flag a malformed source without
/// panicking on partial data.
fn source_field(table: &toml::map::Map<String, toml::Value>) -> RawField<RawSource> {
    let Some(value) = table.get("source") else {
        return RawField::Missing;
    };
    let Some(source) = value.as_table() else {
        return RawField::Invalid("field `source` must be a table".to_string());
    };
    let kind = match source.get("kind").map(toml::Value::as_str) {
        Some(Some(kind)) => kind.to_string(),
        Some(None) => return RawField::Invalid("field `source.kind` must be a string".to_string()),
        None => return RawField::Invalid("field `source.kind` is missing".to_string()),
    };
    let sprint_id = match source.get("sprint_id").map(toml::Value::as_str) {
        Some(Some(sprint_id)) => sprint_id.to_string(),
        Some(None) => {
            return RawField::Invalid("field `source.sprint_id` must be a string".to_string());
        }
        None => return RawField::Invalid("field `source.sprint_id` is missing".to_string()),
    };
    RawField::Present(RawSource { kind, sprint_id })
}

fn string_field(table: &toml::map::Map<String, toml::Value>, name: &str) -> RawField<String> {
    match table.get(name) {
        None => RawField::Missing,
        Some(value) => value.as_str().map_or_else(
            || RawField::Invalid(format!("field `{name}` must be a string")),
            |value| RawField::Present(value.to_string()),
        ),
    }
}

fn string_list_field(
    table: &toml::map::Map<String, toml::Value>,
    name: &str,
) -> RawField<Vec<String>> {
    match table.get(name) {
        None => RawField::Missing,
        Some(value) => {
            let Some(values) = value.as_array() else {
                return RawField::Invalid(format!("field `{name}` must be an array of strings"));
            };
            let mut output = Vec::with_capacity(values.len());
            for value in values {
                let Some(value) = value.as_str() else {
                    return RawField::Invalid(format!("field `{name}` must contain only strings"));
                };
                output.push(value.to_string());
            }
            RawField::Present(output)
        }
    }
}

#[cfg(test)]
mod tests;
