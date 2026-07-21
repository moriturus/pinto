//! Board integrity diagnostics and conservative recovery.

use super::open_board;
#[cfg(feature = "sqlite")]
use crate::backlog::BacklogItem;
use crate::backlog::ItemId;
use crate::config::{Config, StorageBackend};
use crate::error::{Error, Result};
use crate::rank::Rank;
#[cfg(feature = "sqlite")]
use crate::sprint::Sprint;
use crate::sprint::{SprintId, SprintState};
use crate::storage::{
    Backend, atomic_write, item_from_markdown, item_issued_ids_path, item_to_markdown,
    parse_frontmatter, record_issued_id,
};
#[cfg(feature = "sqlite")]
use crate::storage::{BacklogItemRepository, SprintRepository};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::task::JoinSet;

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
    let initial = inspect_board(board_dir, backend, config).await?;
    let fixes = if fix {
        // Renumber duplicate IDs first, then re-inspect so the filename and issued-history repairs
        // see the post-renumber board (for example, a surviving copy whose filename still needs to
        // be normalized). Only pay for the extra inspection when a duplicate was actually repaired.
        let mut fixes = repair_duplicate_item_ids(board_dir, &initial).await?;
        let inspection = if fixes.is_empty() {
            initial
        } else {
            inspect_board(board_dir, backend, config).await?
        };
        fixes.extend(apply_safe_fixes(board_dir, &inspection).await?);
        fixes
    } else {
        Vec::new()
    };
    if !fixes.is_empty() {
        backend.commit("pinto: doctor --fix").await?;
    }
    let final_state = inspect_board(board_dir, backend, config).await?;
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

async fn inspect_board(
    board_dir: &Path,
    _backend: &Backend,
    config: &Config,
) -> Result<Inspection> {
    let (records, sprints) = match config.storage.backend {
        StorageBackend::File | StorageBackend::Git => inspect_file_storage(board_dir).await?,
        #[cfg(feature = "sqlite")]
        StorageBackend::Sqlite => inspect_sqlite_storage(board_dir, _backend).await?,
    };
    let issued = read_issued_history(board_dir).await?;
    let mut issues = analyze_sprints(&sprints);
    issues.extend(analyze_records(&records, &sprints, config));
    issues.extend(analyze_issued(&records, &issued));
    issues.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.location.cmp(&right.location))
            .then_with(|| left.detail.cmp(&right.detail))
    });
    Ok(Inspection {
        records,
        issues,
        issued,
    })
}

async fn inspect_file_storage(
    board_dir: &Path,
) -> Result<(Vec<RawItemRecord>, Vec<RawSprintRecord>)> {
    let tasks_dir = board_dir.join("tasks");
    let archive_dir = board_dir.join("archive");
    let sprints_dir = board_dir.join("sprints");
    let (active, archived, sprint_documents) = tokio::try_join!(
        read_documents(&tasks_dir),
        read_documents(&archive_dir),
        read_documents(&sprints_dir),
    )?;
    let mut records = active
        .into_par_iter()
        .map(|(path, text)| RawItemRecord::from_document(path, RecordArea::Tasks, text))
        .chain(
            archived
                .into_par_iter()
                .map(|(path, text)| RawItemRecord::from_document(path, RecordArea::Archive, text)),
        )
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.path.cmp(&right.path));
    let mut sprints = sprint_documents
        .into_par_iter()
        .map(|(path, text)| RawSprintRecord::from_document(path, text))
        .collect::<Vec<_>>();
    sprints.sort_by(|left, right| left.path.cmp(&right.path));
    Ok((records, sprints))
}

#[cfg(feature = "sqlite")]
async fn inspect_sqlite_storage(
    board_dir: &Path,
    backend: &Backend,
) -> Result<(Vec<RawItemRecord>, Vec<RawSprintRecord>)> {
    let items = BacklogItemRepository::list(backend).await?;
    let sprints = SprintRepository::list(backend).await?;
    let records = items
        .into_iter()
        .map(|item| RawItemRecord::from_item(board_dir, item))
        .collect();
    let sprints = sprints
        .into_iter()
        .map(RawSprintRecord::from_sprint)
        .collect();
    Ok((records, sprints))
}

async fn read_documents(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io(dir, &error)),
    };
    let mut paths = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| Error::io(dir, &error))?
    {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("md") {
            paths.push(path);
        }
    }
    paths.sort();
    let mut reads = JoinSet::new();
    for path in paths {
        reads.spawn(async move {
            fs::read_to_string(&path)
                .await
                .map(|text| (path.clone(), text))
                .map_err(|error| Error::io(&path, &error))
        });
    }
    let mut documents = Vec::new();
    while let Some(result) = reads.join_next().await {
        documents.push(result.map_err(Error::task)??);
    }
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(documents)
}

async fn read_issued_history(board_dir: &Path) -> Result<IssuedHistory> {
    let path = item_issued_ids_path(board_dir);
    let text = match fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(Error::io(&path, &error)),
    };
    let mut history = IssuedHistory {
        path,
        ..IssuedHistory::default()
    };
    for (line_number, line) in text.lines().enumerate() {
        let value = line.trim();
        if value.is_empty() {
            continue;
        }
        match value.parse::<ItemId>() {
            Ok(id) if !history.ids.insert(id.clone()) => {
                history.duplicates.push((line_number + 1, id.to_string()));
            }
            Ok(_) => {}
            Err(error) => history
                .invalid
                .push((line_number + 1, format!("{value:?}: {error}"))),
        }
    }
    Ok(history)
}

fn make_issue(
    kind: DoctorIssueKind,
    location: impl Into<String>,
    detail: impl Into<String>,
    repair: impl Into<String>,
) -> DoctorIssue {
    DoctorIssue {
        kind,
        location: location.into(),
        detail: detail.into(),
        repair: repair.into(),
    }
}

fn analyze_sprints(sprints: &[RawSprintRecord]) -> Vec<DoctorIssue> {
    let mut issues = Vec::new();
    let mut ids: BTreeMap<String, Vec<&RawSprintRecord>> = BTreeMap::new();

    for sprint in sprints {
        if let Some(error) = &sprint.frontmatter_error {
            issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                sprint.location(),
                format!("sprint frontmatter is invalid: {error}"),
                "restore valid TOML frontmatter with the required sprint fields",
            ));
            continue;
        }

        let Some(raw_id) = sprint.id.as_ref() else {
            issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                sprint.location(),
                "required sprint field id is missing",
                "restore the sprint ID in frontmatter and keep the filename aligned",
            ));
            continue;
        };
        let Ok(id) = raw_id.parse::<SprintId>() else {
            issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                sprint.location(),
                format!("sprint ID is invalid: {raw_id:?}"),
                "replace the sprint ID with a path-safe non-empty value",
            ));
            continue;
        };
        ids.entry(id.to_string()).or_default().push(sprint);

        match &sprint.state {
            RawField::Present(state) => {
                if state.parse::<SprintState>().is_err() {
                    issues.push(make_issue(
                        DoctorIssueKind::InvalidStatus,
                        sprint.location(),
                        format!("sprint state is invalid: {state:?}"),
                        "set state to planned, active, or closed",
                    ));
                }
            }
            RawField::Missing => issues.push(make_issue(
                DoctorIssueKind::InvalidStatus,
                sprint.location(),
                "required sprint field state is missing",
                "set state to planned, active, or closed",
            )),
            RawField::Invalid(error) => issues.push(make_issue(
                DoctorIssueKind::InvalidStatus,
                sprint.location(),
                error.clone(),
                "set state to a string: planned, active, or closed",
            )),
        }
    }

    for (id, records) in ids {
        if records.len() > 1 {
            let locations = records
                .iter()
                .map(|record| record.location())
                .collect::<Vec<_>>()
                .join(", ");
            for record in records {
                issues.push(make_issue(
                    DoctorIssueKind::DuplicateId,
                    record.location(),
                    format!("sprint ID {id} is also present at {locations}"),
                    "keep one record for the ID and rename or remove the duplicate manually",
                ));
            }
        }
    }
    issues
}

fn analyze_records(
    records: &[RawItemRecord],
    sprints: &[RawSprintRecord],
    config: &Config,
) -> Vec<DoctorIssue> {
    let mut issues = Vec::new();
    let valid_statuses = config.columns.iter().collect::<HashSet<_>>();
    let valid_sprints = sprints
        .iter()
        .filter_map(RawSprintRecord::valid_id)
        .map(|id| id.to_string())
        .collect::<BTreeSet<_>>();
    let mut ids: BTreeMap<String, Vec<&RawItemRecord>> = BTreeMap::new();
    let mut parent_edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut dependency_edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut parent_references = Vec::new();
    let mut dependency_references = Vec::new();
    let mut sprint_references = Vec::new();
    let mut rank_scopes: BTreeMap<(String, String, String), Vec<&RawItemRecord>> = BTreeMap::new();
    let mut filenames: BTreeMap<String, Vec<&RawItemRecord>> = BTreeMap::new();

    for record in records {
        if let Some(error) = &record.frontmatter_error {
            issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                record.location(),
                format!("item frontmatter is invalid: {error}"),
                "restore valid TOML frontmatter with the required item fields",
            ));
            continue;
        }

        let id = match &record.id {
            RawField::Present(raw) => match raw.parse::<ItemId>() {
                Ok(id) => {
                    ids.entry(id.to_string()).or_default().push(record);
                    Some(id)
                }
                Err(_) => {
                    issues.push(make_issue(
                        DoctorIssueKind::IssuedId,
                        record.location(),
                        format!("item ID is invalid: {raw:?}"),
                        "replace the frontmatter ID with a valid PREFIX-NUMBER ID",
                    ));
                    None
                }
            },
            RawField::Missing => {
                issues.push(make_issue(
                    DoctorIssueKind::MalformedRecord,
                    record.location(),
                    "required item field id is missing",
                    "restore the item ID in frontmatter and keep the filename aligned",
                ));
                None
            }
            RawField::Invalid(error) => {
                issues.push(make_issue(
                    DoctorIssueKind::IssuedId,
                    record.location(),
                    error.clone(),
                    "set id to a string in PREFIX-NUMBER form",
                ));
                None
            }
        };

        match &record.title {
            RawField::Present(title) if !title.trim().is_empty() => {}
            RawField::Present(_) => issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                record.location(),
                "item title must not be empty",
                "restore a non-empty title in frontmatter",
            )),
            RawField::Missing => issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                record.location(),
                "required item field title is missing",
                "restore a non-empty title in frontmatter",
            )),
            RawField::Invalid(error) => issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                record.location(),
                error.clone(),
                "set title to a string in frontmatter",
            )),
        }

        match &record.status {
            RawField::Present(status) if valid_statuses.contains(status) => {}
            RawField::Present(status) => issues.push(make_issue(
                DoctorIssueKind::InvalidStatus,
                record.location(),
                format!("item status {status:?} is not a configured workflow column"),
                "set status to one of the columns in config.toml",
            )),
            RawField::Missing => issues.push(make_issue(
                DoctorIssueKind::InvalidStatus,
                record.location(),
                "required item field status is missing",
                "set status to one of the columns in config.toml",
            )),
            RawField::Invalid(error) => issues.push(make_issue(
                DoctorIssueKind::InvalidStatus,
                record.location(),
                error.clone(),
                "set status to a string matching a configured workflow column",
            )),
        }

        let valid_rank = match &record.rank {
            RawField::Present(rank) => match Rank::parse(rank) {
                Ok(_) => Some(rank.as_str()),
                Err(_) => {
                    issues.push(make_issue(
                        DoctorIssueKind::RankAnomaly,
                        record.location(),
                        format!("rank is not in normal form: {rank:?}"),
                        "set rank to a non-empty base-36 rank without a trailing zero",
                    ));
                    None
                }
            },
            RawField::Missing => {
                issues.push(make_issue(
                    DoctorIssueKind::RankAnomaly,
                    record.location(),
                    "required item field rank is missing",
                    "set rank with pinto reorder or pinto rebalance",
                ));
                None
            }
            RawField::Invalid(error) => {
                issues.push(make_issue(
                    DoctorIssueKind::RankAnomaly,
                    record.location(),
                    error.clone(),
                    "set rank to a string in normal base-36 form",
                ));
                None
            }
        };

        if let Some(id) = &id {
            if let Some(filename) = record.filename.as_deref()
                && filename != format!("{id}.md")
            {
                issues.push(make_issue(
                    DoctorIssueKind::Filename,
                    record.location(),
                    format!("filename does not match frontmatter ID {id}"),
                    "rename the record to the canonical ID.md filename",
                ));
            }

            if record.area == RecordArea::Tasks
                && let (RawField::Present(status), Some(rank)) = (&record.status, valid_rank)
                && valid_statuses.contains(status)
            {
                let parent = match &record.parent {
                    RawField::Present(parent) => parent.clone(),
                    _ => String::new(),
                };
                rank_scopes
                    .entry((status.clone(), parent, rank.to_string()))
                    .or_default()
                    .push(record);
            }

            match &record.parent {
                RawField::Missing => {}
                RawField::Present(parent) => match parent.parse::<ItemId>() {
                    Ok(parent) => {
                        let child = id.to_string();
                        let parent = parent.to_string();
                        parent_edges
                            .entry(child.clone())
                            .or_default()
                            .insert(parent.clone());
                        parent_references.push((record, parent, child));
                    }
                    Err(_) => issues.push(make_issue(
                        DoctorIssueKind::DanglingParent,
                        record.location(),
                        format!("parent reference is invalid: {parent:?}"),
                        "remove the parent field or set it to an existing PBI ID",
                    )),
                },
                RawField::Invalid(error) => issues.push(make_issue(
                    DoctorIssueKind::DanglingParent,
                    record.location(),
                    error.clone(),
                    "remove the parent field or set it to a string PBI ID",
                )),
            }

            match &record.depends_on {
                RawField::Missing => {}
                RawField::Present(dependencies) => {
                    for dependency in dependencies {
                        match dependency.parse::<ItemId>() {
                            Ok(dependency) => {
                                let source = id.to_string();
                                let dependency = dependency.to_string();
                                dependency_edges
                                    .entry(source.clone())
                                    .or_default()
                                    .insert(dependency.clone());
                                dependency_references.push((record, dependency, source));
                            }
                            Err(_) => issues.push(make_issue(
                                DoctorIssueKind::DanglingDependency,
                                record.location(),
                                format!("dependency reference is invalid: {dependency:?}"),
                                "remove the dependency or set it to an existing PBI ID",
                            )),
                        }
                    }
                }
                RawField::Invalid(error) => issues.push(make_issue(
                    DoctorIssueKind::DanglingDependency,
                    record.location(),
                    error.clone(),
                    "remove depends_on or set it to an array of PBI IDs",
                )),
            }

            match &record.sprint {
                RawField::Missing => {}
                RawField::Present(sprint) => match sprint.parse::<SprintId>() {
                    Ok(sprint) => sprint_references.push((record, sprint.to_string())),
                    Err(_) => issues.push(make_issue(
                        DoctorIssueKind::DanglingSprint,
                        record.location(),
                        format!("sprint reference is invalid: {sprint:?}"),
                        "remove the sprint field or set it to an existing sprint ID",
                    )),
                },
                RawField::Invalid(error) => issues.push(make_issue(
                    DoctorIssueKind::DanglingSprint,
                    record.location(),
                    error.clone(),
                    "remove the sprint field or set it to a string sprint ID",
                )),
            }
        }

        if let Some(filename) = &record.filename {
            filenames.entry(filename.clone()).or_default().push(record);
        }
    }

    for (id, records_with_id) in &ids {
        if records_with_id.len() > 1 {
            let locations = records_with_id
                .iter()
                .map(|record| record.location())
                .collect::<Vec<_>>()
                .join(", ");
            for record in records_with_id {
                issues.push(make_issue(
                    DoctorIssueKind::DuplicateId,
                    record.location(),
                    format!("item ID {id} is also present at {locations}"),
                    "run pinto doctor --fix to renumber duplicates, or resolve them manually",
                ));
            }
        }
    }

    let valid_ids = ids.keys().cloned().collect::<BTreeSet<_>>();
    for (record, target, source) in parent_references {
        if !valid_ids.contains(&target) {
            issues.push(make_issue(
                DoctorIssueKind::DanglingParent,
                record.location(),
                format!("item {source} refers to missing parent {target}"),
                "remove the parent field or set it to an existing PBI ID",
            ));
        }
    }
    for (record, target, source) in dependency_references {
        if !valid_ids.contains(&target) {
            issues.push(make_issue(
                DoctorIssueKind::DanglingDependency,
                record.location(),
                format!("item {source} depends on missing item {target}"),
                "remove the dependency or set it to an existing PBI ID",
            ));
        }
    }
    for (record, target) in sprint_references {
        if !valid_sprints.contains(&target) {
            issues.push(make_issue(
                DoctorIssueKind::DanglingSprint,
                record.location(),
                format!("item refers to missing sprint {target}"),
                "remove the sprint field or set it to an existing sprint ID",
            ));
        }
    }

    for ((status, parent, rank), records_in_scope) in rank_scopes {
        if records_in_scope.len() > 1 {
            for record in records_in_scope {
                issues.push(make_issue(
                    DoctorIssueKind::RankAnomaly,
                    record.location(),
                    format!(
                        "rank {rank:?} is duplicated in status {status:?} and parent scope {parent:?}"
                    ),
                    "run pinto rebalance for the affected workflow scope",
                ));
            }
        }
    }

    for (filename, records_with_name) in filenames {
        let has_tasks = records_with_name
            .iter()
            .any(|record| record.area == RecordArea::Tasks);
        let has_archive = records_with_name
            .iter()
            .any(|record| record.area == RecordArea::Archive);
        if has_tasks && has_archive {
            for record in records_with_name {
                issues.push(make_issue(
                    DoctorIssueKind::Collision,
                    record.location(),
                    format!("filename {filename:?} exists in both tasks and archive"),
                    "keep one copy and move or remove the other record manually",
                ));
            }
        }
    }

    for cycle in graph_cycles(&parent_edges) {
        issues.push(make_issue(
            DoctorIssueKind::ParentCycle,
            cycle.join(" -> "),
            format!("parent relationship cycle: {}", cycle.join(" -> ")),
            "remove or change one parent field in the cycle manually",
        ));
    }
    for cycle in graph_cycles(&dependency_edges) {
        issues.push(make_issue(
            DoctorIssueKind::DependencyCycle,
            cycle.join(" -> "),
            format!("dependency relationship cycle: {}", cycle.join(" -> ")),
            "remove or change one dependency in the cycle manually",
        ));
    }

    issues
}

fn analyze_issued(records: &[RawItemRecord], issued: &IssuedHistory) -> Vec<DoctorIssue> {
    let mut issues = Vec::new();
    for (line, detail) in &issued.invalid {
        issues.push(make_issue(
            DoctorIssueKind::IssuedId,
            format!("{}:{line}", issued.path.display()),
            format!("issued ID is invalid: {detail}"),
            "remove the invalid line from issued_ids",
        ));
    }
    for (line, id) in &issued.duplicates {
        issues.push(make_issue(
            DoctorIssueKind::IssuedId,
            format!("{}:{line}", issued.path.display()),
            format!("issued ID {id} is duplicated"),
            "remove the duplicate line from issued_ids",
        ));
    }

    let mut seen = BTreeMap::new();
    for record in records {
        if let Some(id) = record.valid_id() {
            seen.entry(id.to_string()).or_insert_with(|| (id, record));
        }
    }
    for (id, (id_value, record)) in seen {
        if !issued.ids.contains(&id_value) {
            issues.push(make_issue(
                DoctorIssueKind::IssuedId,
                record.location(),
                format!("item ID {id} is missing from issued_ids"),
                "append the existing item ID to issued_ids or run pinto doctor --fix",
            ));
        }
    }
    issues
}

fn graph_cycles(edges: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    let mut state = BTreeMap::new();
    let mut stack = Vec::new();
    let mut seen_cycles = HashSet::new();
    let mut cycles = Vec::new();
    for node in edges.keys() {
        visit_graph(
            node,
            edges,
            &mut state,
            &mut stack,
            &mut seen_cycles,
            &mut cycles,
        );
    }
    cycles.sort();
    cycles
}

fn visit_graph(
    node: &str,
    edges: &BTreeMap<String, BTreeSet<String>>,
    state: &mut BTreeMap<String, u8>,
    stack: &mut Vec<String>,
    seen_cycles: &mut HashSet<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    if state.get(node).copied().unwrap_or(0) == 2 {
        return;
    }
    state.insert(node.to_string(), 1);
    stack.push(node.to_string());
    if let Some(targets) = edges.get(node) {
        for target in targets {
            match state.get(target).copied().unwrap_or(0) {
                0 => visit_graph(target, edges, state, stack, seen_cycles, cycles),
                1 => {
                    if let Some(start) = stack.iter().position(|value| value == target) {
                        let mut cycle = stack[start..].to_vec();
                        cycle.sort();
                        let key = cycle.join(",");
                        if seen_cycles.insert(key) {
                            cycles.push(cycle);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    stack.pop();
    state.insert(node.to_string(), 2);
}

async fn apply_safe_fixes(board_dir: &Path, inspection: &Inspection) -> Result<Vec<DoctorFix>> {
    let mut fixes = Vec::new();
    let mut id_counts = BTreeMap::<String, usize>::new();
    for record in &inspection.records {
        if let Some(id) = record.valid_id() {
            *id_counts.entry(id.to_string()).or_default() += 1;
        }
    }

    for record in &inspection.records {
        let Some(id) = record.valid_id() else {
            continue;
        };
        if id_counts.get(&id.to_string()) != Some(&1)
            || record.filename.as_deref() == Some(&format!("{id}.md"))
        {
            continue;
        }
        let Some(directory) = record.area.directory(board_dir) else {
            continue;
        };
        let destination = directory.join(format!("{id}.md"));
        if fs::try_exists(&destination)
            .await
            .map_err(|error| Error::io(&destination, &error))?
        {
            continue;
        }
        let other_area = match record.area {
            RecordArea::Tasks => RecordArea::Archive,
            RecordArea::Archive => RecordArea::Tasks,
            #[cfg(feature = "sqlite")]
            RecordArea::Database => continue,
        };
        let other_destination = other_area
            .directory(board_dir)
            .map(|directory| directory.join(format!("{id}.md")));
        if let Some(other_destination) = other_destination
            && fs::try_exists(&other_destination)
                .await
                .map_err(|error| Error::io(&other_destination, &error))?
        {
            continue;
        }
        fs::rename(&record.path, &destination)
            .await
            .map_err(|error| Error::io(&record.path, &error))?;
        fixes.push(DoctorFix {
            description: format!(
                "renamed {} to {}",
                record.path.display(),
                destination.display()
            ),
        });
    }

    let mut recorded = BTreeSet::new();
    for record in &inspection.records {
        let Some(id) = record.valid_id() else {
            continue;
        };
        if !recorded.insert(id.to_string()) || inspection.issued.ids.contains(&id) {
            continue;
        }
        record_issued_id(board_dir, &id).await?;
        fixes.push(DoctorFix {
            description: format!("recorded {id} in {}", inspection.issued.path.display()),
        });
    }
    Ok(fixes)
}

#[derive(Debug)]
struct DuplicateRepair {
    source: PathBuf,
    area: RecordArea,
    old_id: ItemId,
    new_id: ItemId,
    occurrence: usize,
    item: crate::backlog::BacklogItem,
}

/// Deterministic plan for repairing every duplicate PBI ID in a single inspection.
///
/// `lineages` maps each shared ID to the ordered IDs assigned to its occurrences (index `0` is the
/// canonical record that keeps the original ID), so `parent`/`depends_on` references can follow the
/// same merge lineage they belonged to before renumbering.
#[derive(Debug, Default)]
struct DuplicateRepairPlan {
    repairs: Vec<DuplicateRepair>,
    lineages: BTreeMap<String, Vec<ItemId>>,
}

async fn repair_duplicate_item_ids(
    board_dir: &Path,
    inspection: &Inspection,
) -> Result<Vec<DoctorFix>> {
    let DuplicateRepairPlan { repairs, lineages } = plan_duplicate_item_repairs(inspection)?;
    let mut fixes = Vec::with_capacity(repairs.len());

    for mut repair in repairs {
        repair.item.id = repair.new_id.clone();
        if let Some(parent) = repair.item.parent.as_mut()
            && let Some(ids) = lineages.get(&parent.to_string())
            && let Some(replacement) = ids.get(repair.occurrence)
        {
            *parent = replacement.clone();
        }
        for dependency in &mut repair.item.depends_on {
            if let Some(ids) = lineages.get(&dependency.to_string())
                && let Some(replacement) = ids.get(repair.occurrence)
            {
                *dependency = replacement.clone();
            }
        }

        let Some(directory) = repair.area.directory(board_dir) else {
            continue;
        };
        let destination = directory.join(format!("{}.md", repair.new_id));
        let text = item_to_markdown(&repair.item)?;
        atomic_write(&destination, &text).await?;
        fs::remove_file(&repair.source)
            .await
            .map_err(|error| Error::io(&repair.source, &error))?;
        record_issued_id(board_dir, &repair.new_id).await?;
        fixes.push(DoctorFix {
            description: format!(
                "renumbered {} as {}: {} -> {}",
                repair.old_id,
                repair.new_id,
                repair.source.display(),
                destination.display()
            ),
        });
    }

    Ok(fixes)
}

fn plan_duplicate_item_repairs(inspection: &Inspection) -> Result<DuplicateRepairPlan> {
    let mut groups = BTreeMap::<String, Vec<&RawItemRecord>>::new();
    let mut maximum_by_prefix = BTreeMap::<String, u32>::new();

    for record in &inspection.records {
        if let Some(id) = record.valid_id() {
            maximum_by_prefix
                .entry(id.prefix().to_string())
                .and_modify(|maximum| *maximum = (*maximum).max(id.number()))
                .or_insert(id.number());
            groups.entry(id.to_string()).or_default().push(record);
        }
        if let Some(filename) = record
            .filename
            .as_deref()
            .and_then(|filename| filename.strip_suffix(".md"))
            .and_then(|stem| stem.parse::<ItemId>().ok())
        {
            maximum_by_prefix
                .entry(filename.prefix().to_string())
                .and_modify(|maximum| *maximum = (*maximum).max(filename.number()))
                .or_insert(filename.number());
        }
    }
    for id in &inspection.issued.ids {
        maximum_by_prefix
            .entry(id.prefix().to_string())
            .and_modify(|maximum| *maximum = (*maximum).max(id.number()))
            .or_insert(id.number());
    }

    let mut duplicate_groups = groups
        .into_iter()
        .filter_map(|(id, records)| {
            (records.len() > 1)
                .then(|| id.parse::<ItemId>().ok().map(|id| (id, records)))
                .flatten()
        })
        .collect::<Vec<_>>();
    duplicate_groups.sort_by(|(left, _), (right, _)| {
        (left.prefix(), left.number()).cmp(&(right.prefix(), right.number()))
    });

    let mut repairs = Vec::new();
    let mut lineages = BTreeMap::new();
    for (old_id, mut records) in duplicate_groups {
        records.sort_by(|left, right| {
            record_area_priority(left.area)
                .cmp(&record_area_priority(right.area))
                .then_with(|| left.path.cmp(&right.path))
        });
        // A duplicate with any malformed field is not safe to rewrite. Leave that group for the
        // final scan instead of turning a recoverable diagnostic into a failed repair command.
        let parsed = records
            .iter()
            .map(|record| {
                let document = record.document.as_deref()?;
                item_from_markdown(document, &record.path).ok()
            })
            .collect::<Option<Vec<_>>>();
        let Some(parsed) = parsed else {
            continue;
        };

        let mut ids = vec![old_id.clone()];
        let mut group_repairs = Vec::new();
        for (occurrence, (record, item)) in records.iter().zip(parsed).enumerate().skip(1) {
            let maximum = maximum_by_prefix
                .entry(old_id.prefix().to_string())
                .or_default();
            *maximum = maximum
                .checked_add(1)
                .ok_or_else(|| Error::InvalidItemId(old_id.to_string()))?;
            let new_id = ItemId::try_new(old_id.prefix(), *maximum)?;
            ids.push(new_id.clone());
            group_repairs.push(DuplicateRepair {
                source: record.path.clone(),
                area: record.area,
                old_id: old_id.clone(),
                new_id,
                occurrence,
                item,
            });
        }
        lineages.insert(old_id.to_string(), ids);
        repairs.extend(group_repairs);
    }

    Ok(DuplicateRepairPlan { repairs, lineages })
}

fn record_area_priority(area: RecordArea) -> u8 {
    match area {
        RecordArea::Tasks => 0,
        RecordArea::Archive => 1,
        #[cfg(feature = "sqlite")]
        RecordArea::Database => 2,
    }
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn item_record(
        path: impl Into<PathBuf>,
        area: RecordArea,
        fields: &str,
    ) -> (RawItemRecord, String) {
        let path = path.into();
        let text = format!("+++\n{fields}\n+++\n");
        (RawItemRecord::from_document(path, area, text.clone()), text)
    }

    fn sprint_record(path: impl Into<PathBuf>, fields: &str) -> RawSprintRecord {
        let path = path.into();
        RawSprintRecord::from_document(path, format!("+++\n{fields}\n+++\n"))
    }

    fn has_issue_kind(issues: &[DoctorIssue], kind: DoctorIssueKind) -> bool {
        issues.iter().any(|issue| issue.kind == kind)
    }

    #[test]
    fn doctor_classifies_malformed_documents_and_relationships() {
        let malformed = RawItemRecord::from_document(
            PathBuf::from("broken.md"),
            RecordArea::Tasks,
            "not frontmatter".to_string(),
        );
        let invalid_toml = RawItemRecord::from_document(
            PathBuf::from("invalid-toml.md"),
            RecordArea::Tasks,
            "+++\nid = [\n+++\n".to_string(),
        );
        let non_table = RawItemRecord::from_document(
            PathBuf::from("non-table.md"),
            RecordArea::Tasks,
            "+++\n[\"not a table\"]\n+++\n".to_string(),
        );
        let (missing_fields, _) = item_record("T-2.md", RecordArea::Tasks, "id = \"T-2\"");
        let (invalid_fields, _) = item_record(
            "T-3.md",
            RecordArea::Tasks,
            "id = \"T-3\"\ntitle = 3\nstatus = 4\nrank = 5\nsprint = 6\nparent = 7\ndepends_on = [8]",
        );
        let (dangling, _) = item_record(
            "T-4.md",
            RecordArea::Tasks,
            "id = \"T-4\"\ntitle = \" \"\nstatus = \"unknown\"\nrank = \"i\"\nsprint = \"S-missing\"\nparent = \"T-99\"\ndepends_on = [\"T-99\", \"not-an-id\"]",
        );
        let (invalid_id, _) = item_record(
            "invalid.md",
            RecordArea::Tasks,
            "id = \"not/an/id\"\ntitle = \"Invalid ID\"\nstatus = \"todo\"\nrank = \"j\"",
        );
        let (missing_id, _) = item_record(
            "missing-id.md",
            RecordArea::Tasks,
            "title = \"Missing ID\"\nstatus = \"todo\"\nrank = \"k\"",
        );
        let (cycle_one, _) = item_record(
            "wrong-name.md",
            RecordArea::Tasks,
            "id = \"T-6\"\ntitle = \"Cycle one\"\nstatus = \"todo\"\nrank = \"l\"\nparent = \"T-7\"\ndepends_on = [\"T-7\"]",
        );
        let (cycle_two, _) = item_record(
            "T-7.md",
            RecordArea::Tasks,
            "id = \"T-7\"\ntitle = \"Cycle two\"\nstatus = \"todo\"\nrank = \"m\"\nparent = \"T-6\"\ndepends_on = [\"T-6\"]",
        );
        let (rank_one, _) = item_record(
            "T-8.md",
            RecordArea::Tasks,
            "id = \"T-8\"\ntitle = \"Rank one\"\nstatus = \"todo\"\nrank = \"n\"",
        );
        let (rank_two, _) = item_record(
            "T-9.md",
            RecordArea::Tasks,
            "id = \"T-9\"\ntitle = \"Rank two\"\nstatus = \"todo\"\nrank = \"n\"",
        );
        let (duplicate_task, _) = item_record(
            "T-5.md",
            RecordArea::Tasks,
            "id = \"T-5\"\ntitle = \"Task copy\"\nstatus = \"todo\"\nrank = \"o\"",
        );
        let (duplicate_archive, _) = item_record(
            "T-5.md",
            RecordArea::Archive,
            "id = \"T-5\"\ntitle = \"Archive copy\"\nstatus = \"todo\"\nrank = \"p\"",
        );

        let records = vec![
            malformed,
            invalid_toml,
            non_table,
            missing_fields,
            invalid_fields,
            dangling,
            invalid_id,
            missing_id,
            cycle_one,
            cycle_two,
            rank_one,
            rank_two,
            duplicate_task,
            duplicate_archive,
        ];
        let sprints = vec![
            RawSprintRecord::from_document(
                PathBuf::from("broken-sprint.md"),
                "not frontmatter".to_string(),
            ),
            sprint_record("invalid-sprint.toml", "id = ["),
            sprint_record("non-table-sprint.md", "[\"not a table\"]"),
            sprint_record("missing-sprint-id.md", "state = \"planned\""),
            sprint_record(
                "invalid-sprint-id.md",
                "id = \"bad/id\"\nstate = \"planned\"",
            ),
            sprint_record("invalid-state.md", "id = \"S-2\"\nstate = \"broken\""),
            sprint_record("missing-state.md", "id = \"S-3\""),
            sprint_record("typed-state.md", "id = \"S-4\"\nstate = 4"),
            sprint_record("S-1.md", "id = \"S-1\"\nstate = \"planned\""),
            sprint_record("duplicate-sprint.md", "id = \"S-1\"\nstate = \"closed\""),
        ];

        let mut config = Config::default();
        let sprint_issues = analyze_sprints(&sprints);
        let item_issues = analyze_records(&records, &sprints, &config);

        for kind in [
            DoctorIssueKind::MalformedRecord,
            DoctorIssueKind::InvalidStatus,
            DoctorIssueKind::DuplicateId,
        ] {
            assert!(
                has_issue_kind(&sprint_issues, kind),
                "missing Sprint issue {kind:?}"
            );
        }
        for kind in [
            DoctorIssueKind::DanglingDependency,
            DoctorIssueKind::DanglingParent,
            DoctorIssueKind::DanglingSprint,
            DoctorIssueKind::ParentCycle,
            DoctorIssueKind::DependencyCycle,
            DoctorIssueKind::DuplicateId,
            DoctorIssueKind::IssuedId,
            DoctorIssueKind::InvalidStatus,
            DoctorIssueKind::RankAnomaly,
            DoctorIssueKind::Collision,
            DoctorIssueKind::MalformedRecord,
            DoctorIssueKind::Filename,
        ] {
            assert!(
                has_issue_kind(&item_issues, kind),
                "missing item issue {kind:?}"
            );
        }

        // Keep this explicit so the test also exercises the normal workflow lookup used by the
        // rank and status checks rather than relying only on Config's default shape.
        config.columns = vec!["todo".to_string(), "done".to_string()];
        assert!(has_issue_kind(
            &analyze_records(&records, &sprints, &config),
            DoctorIssueKind::InvalidStatus
        ));
    }

    #[tokio::test]
    async fn doctor_safe_fixes_rename_only_unambiguous_records() {
        let dir = TempDir::new().expect("temp dir");
        let board_dir = dir.path().join(".pinto");
        let tasks_dir = board_dir.join("tasks");
        let archive_dir = board_dir.join("archive");
        fs::create_dir_all(&tasks_dir)
            .await
            .expect("tasks directory");
        fs::create_dir_all(&archive_dir)
            .await
            .expect("archive directory");

        let (rename_active, rename_active_text) = item_record(
            tasks_dir.join("renamed.md"),
            RecordArea::Tasks,
            "id = \"T-1\"\ntitle = \"Rename active\"\nstatus = \"todo\"\nrank = \"i\"",
        );
        let (rename_archive, rename_archive_text) = item_record(
            archive_dir.join("archived.md"),
            RecordArea::Archive,
            "id = \"T-2\"\ntitle = \"Rename archive\"\nstatus = \"todo\"\nrank = \"j\"",
        );
        let (destination_exists, destination_exists_text) = item_record(
            tasks_dir.join("source.md"),
            RecordArea::Tasks,
            "id = \"T-3\"\ntitle = \"Destination exists\"\nstatus = \"todo\"\nrank = \"k\"",
        );
        let (other_area_exists, other_area_exists_text) = item_record(
            archive_dir.join("source.md"),
            RecordArea::Archive,
            "id = \"T-4\"\ntitle = \"Other area exists\"\nstatus = \"todo\"\nrank = \"l\"",
        );
        let (duplicate_one, duplicate_one_text) = item_record(
            tasks_dir.join("duplicate-one.md"),
            RecordArea::Tasks,
            "id = \"T-5\"\ntitle = \"Duplicate one\"\nstatus = \"todo\"\nrank = \"m\"",
        );
        let (duplicate_two, duplicate_two_text) = item_record(
            archive_dir.join("duplicate-two.md"),
            RecordArea::Archive,
            "id = \"T-5\"\ntitle = \"Duplicate two\"\nstatus = \"todo\"\nrank = \"n\"",
        );
        let (already_named, already_named_text) = item_record(
            tasks_dir.join("T-6.md"),
            RecordArea::Tasks,
            "id = \"T-6\"\ntitle = \"Already named\"\nstatus = \"todo\"\nrank = \"o\"",
        );
        let (invalid_id, invalid_id_text) = item_record(
            tasks_dir.join("invalid.md"),
            RecordArea::Tasks,
            "id = \"not-an-id\"\ntitle = \"Invalid ID\"\nstatus = \"todo\"\nrank = \"p\"",
        );
        let fixtures = [
            (rename_active, rename_active_text),
            (rename_archive, rename_archive_text),
            (destination_exists, destination_exists_text),
            (other_area_exists, other_area_exists_text),
            (duplicate_one, duplicate_one_text),
            (duplicate_two, duplicate_two_text),
            (already_named, already_named_text),
            (invalid_id, invalid_id_text),
        ];
        for (record, text) in &fixtures {
            fs::write(&record.path, text).await.expect("write fixture");
        }
        fs::write(tasks_dir.join("T-3.md"), "existing destination")
            .await
            .expect("write active destination");
        fs::write(tasks_dir.join("T-4.md"), "existing other-area destination")
            .await
            .expect("write other-area destination");

        let inspection = Inspection {
            records: fixtures.iter().map(|(record, _)| record.clone()).collect(),
            issues: Vec::new(),
            issued: IssuedHistory {
                path: board_dir.join("issued_ids"),
                ids: HashSet::from([ItemId::new("T", 6)]),
                invalid: Vec::new(),
                duplicates: Vec::new(),
            },
        };
        let fixes = apply_safe_fixes(&board_dir, &inspection)
            .await
            .expect("safe fixes succeed");

        assert!(tasks_dir.join("T-1.md").is_file());
        assert!(!tasks_dir.join("renamed.md").exists());
        assert!(archive_dir.join("T-2.md").is_file());
        assert!(!archive_dir.join("archived.md").exists());
        assert!(
            fixes
                .iter()
                .filter(|fix| fix.description.starts_with("renamed "))
                .count()
                == 2
        );
        let history = fs::read_to_string(board_dir.join("issued_ids"))
            .await
            .expect("issued history");
        for id in ["T-1", "T-2", "T-3", "T-4", "T-5"] {
            assert!(history.lines().any(|line| line == id), "missing {id}");
        }
        assert!(!history.lines().any(|line| line == "T-6"));
    }

    #[tokio::test]
    async fn doctor_reads_and_classifies_issued_id_history() {
        let dir = TempDir::new().expect("temp dir");
        let board_dir = dir.path().join(".pinto");
        fs::create_dir_all(&board_dir)
            .await
            .expect("board directory");
        fs::write(item_issued_ids_path(&board_dir), "\nT-1\nT-1\nnot-an-id\n")
            .await
            .expect("issued history");

        let history = read_issued_history(&board_dir)
            .await
            .expect("read issued history");
        assert_eq!(history.ids, HashSet::from([ItemId::new("T", 1)]));
        assert_eq!(history.duplicates, vec![(3, "T-1".to_string())]);
        assert_eq!(history.invalid.len(), 1);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn doctor_inspects_sqlite_records_through_the_backend() {
        use crate::storage::{BacklogItemRepository, SprintRepository, SqliteRepository};
        use chrono::Utc;

        let dir = TempDir::new().expect("temp dir");
        crate::service::init_board(dir.path())
            .await
            .expect("initialize board");
        let board_dir = dir.path().join(".pinto");
        let repository = SqliteRepository::new(board_dir.clone());
        let now = Utc::now();
        let item = BacklogItem::new(
            ItemId::new("T", 1),
            "SQLite item",
            crate::backlog::Status::new("todo"),
            Rank::parse("i").expect("rank"),
            now,
        )
        .expect("item");
        let sprint =
            Sprint::new(SprintId::new("S-1").expect("sprint ID"), "Sprint", now).expect("sprint");
        BacklogItemRepository::save(&repository, &item)
            .await
            .expect("save item");
        SprintRepository::save(&repository, &sprint)
            .await
            .expect("save sprint");
        record_issued_id(&board_dir, &item.id)
            .await
            .expect("record issued ID");

        let mut config = Config::default();
        config.storage.backend = StorageBackend::Sqlite;
        let backend = Backend::Sqlite(repository);
        let inspection = inspect_board(&board_dir, &backend, &config)
            .await
            .expect("inspect SQLite board");
        assert_eq!(inspection.records.len(), 1);
        assert!(
            inspection.issues.is_empty(),
            "issues: {:?}",
            inspection.issues
        );
    }

    #[test]
    fn graph_cycles_are_reported_once_with_stable_members() {
        let edges = BTreeMap::from([
            (
                "T-1".to_string(),
                BTreeSet::from(["T-2".to_string(), "T-3".to_string()]),
            ),
            ("T-2".to_string(), BTreeSet::from(["T-1".to_string()])),
            ("T-3".to_string(), BTreeSet::from(["T-1".to_string()])),
        ]);

        assert_eq!(
            graph_cycles(&edges),
            vec![
                vec!["T-1".to_string(), "T-2".to_string()],
                vec!["T-1".to_string(), "T-3".to_string()],
            ]
        );
    }

    #[tokio::test]
    async fn service_scan_keeps_diagnosing_after_malformed_record() {
        let dir = TempDir::new().expect("temp dir");
        crate::service::init_board(dir.path())
            .await
            .expect("initialize board");
        let path = dir.path().join(".pinto/tasks/broken.md");
        fs::write(&path, "this is not frontmatter")
            .await
            .expect("write malformed item");

        let report = doctor(dir.path(), false).await.expect("scan board");

        assert!(report.issues.iter().any(|issue| {
            issue.kind == DoctorIssueKind::MalformedRecord
                && std::path::Path::new(&issue.location)
                    .file_name()
                    .and_then(|name| name.to_str())
                    == path.file_name().and_then(|name| name.to_str())
        }));
    }
}
