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
use crate::storage::{Backend, item_issued_ids_path, parse_frontmatter, record_issued_id};
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
        apply_safe_fixes(board_dir, &initial).await?
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
            if record.filename.as_deref() != Some(&format!("{id}.md")) {
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
                    "keep one record for the ID and rename or remove the duplicate manually",
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
                && issue.location == path.display().to_string()
        }));
    }
}
