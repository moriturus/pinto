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
#[cfg(feature = "sqlite")]
use crate::sprint::Sprint;
use crate::sprint::SprintId;
use crate::storage::{Backend, parse_frontmatter};
use inspect::inspect_board;
use repair::{apply_safe_fixes, repair_duplicate_item_ids};
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};

// Inspection helpers the test module drives directly; re-imported so `use super::*` resolves them.
#[cfg(test)]
use inspect::{analyze_records, analyze_sprints, graph_cycles, read_issued_history};

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
    Database,
}

impl RecordArea {
    fn directory(self, board_dir: &Path) -> Option<PathBuf> {
        match self {
            Self::Tasks => Some(board_dir.join("tasks")),
            Self::Archive => Some(board_dir.join("archive")),
            #[cfg(feature = "sqlite")]
            Self::Database => None,
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
        }
    }

    #[cfg(feature = "sqlite")]
    fn from_item(board_dir: &Path, item: BacklogItem) -> Self {
        Self {
            path: board_dir
                .join("board.sqlite3#items")
                .join(item.id.to_string()),
            area: RecordArea::Database,
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
}

impl RawSprintRecord {
    fn from_document(path: PathBuf, text: String) -> Self {
        let missing = || Self {
            path: path.clone(),
            frontmatter_error: None,
            id: RawField::Missing,
            state: RawField::Missing,
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
        Self {
            path,
            frontmatter_error: None,
            id: string_field(table, "id"),
            state: string_field(table, "state"),
        }
    }

    #[cfg(feature = "sqlite")]
    fn from_sprint(sprint: Sprint) -> Self {
        Self {
            path: PathBuf::from(format!("board.sqlite3#sprints/{}", sprint.id)),
            frontmatter_error: None,
            id: RawField::Present(sprint.id.to_string()),
            state: RawField::Present(sprint.state.to_string()),
        }
    }

    fn valid_id(&self) -> Option<SprintId> {
        self.id.as_ref()?.parse().ok()
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
