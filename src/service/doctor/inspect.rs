//! Board reading and analysis: gather every raw record, then classify integrity issues.

use super::{
    DoctorIssue, DoctorIssueKind, Inspection, IssuedHistory, RawField, RawItemRecord,
    RawSprintRecord, RecordArea,
};
use crate::backlog::ItemId;
use crate::config::{Config, StorageBackend};
use crate::error::{Error, Result};
use crate::rank::Rank;
use crate::sprint::{SprintId, SprintState};
use crate::sprint_record::SprintRecordKind;
use crate::storage::{Backend, item_issued_ids_path, record_from_markdown};
#[cfg(feature = "sqlite")]
use crate::storage::{BacklogItemRepository, SprintRepository};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::task::JoinSet;

/// Keep doctor file reads below common per-process descriptor limits while retaining async I/O.
const MAX_CONCURRENT_DOCUMENT_READS: usize = 64;

pub(super) async fn inspect_board(
    board_dir: &Path,
    _backend: &Backend,
    config: &Config,
) -> Result<Inspection> {
    let (records, sprints) = match config.storage.backend {
        StorageBackend::File | StorageBackend::Git => inspect_file_storage(board_dir).await?,
        #[cfg(feature = "sqlite")]
        StorageBackend::Sqlite => inspect_sqlite_storage(board_dir, _backend).await?,
    };
    let (child_records, child_record_issues) = read_child_records(board_dir, &sprints).await?;
    let issued = read_issued_history(board_dir).await?;
    let mut issues = analyze_sprints(&sprints);
    issues.extend(analyze_records(&records, &sprints, config));
    issues.extend(analyze_action_sources(&records, &sprints, &child_records));
    issues.extend(analyze_issued(&records, &issued));
    issues.extend(child_record_issues);
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
    let archived = BacklogItemRepository::list_archived(backend).await?;
    // The normal SQLite mapper normalizes an outcome paired with a blank Goal. Doctor must use the
    // raw Sprint rows here so that corruption remains observable at the health-check boundary.
    let sprints = match backend {
        Backend::Sqlite(repository) => repository.list_sprints_raw().await?,
        _ => SprintRepository::list(backend).await?,
    };
    // Inspect the active and archived stores together, matching the file and Git backends, so an
    // archived action PBI with a dangling `source` is analyzed instead of silently skipped.
    let records = items
        .into_iter()
        .map(|item| RawItemRecord::from_item(board_dir, item, false))
        .chain(
            archived
                .into_iter()
                .map(|item| RawItemRecord::from_item(board_dir, item, true)),
        )
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
    let mut documents = Vec::with_capacity(paths.len());
    for path in paths {
        if reads.len() >= MAX_CONCURRENT_DOCUMENT_READS
            && let Some(joined) = reads.join_next().await
        {
            documents.push(joined.map_err(Error::task)??);
        }
        reads.spawn(async move {
            fs::read_to_string(&path)
                .await
                .map(|text| (path.clone(), text))
                .map_err(|error| Error::io(&path, &error))
        });
    }
    while let Some(result) = reads.join_next().await {
        documents.push(result.map_err(Error::task)??);
    }
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(documents)
}

/// Read every Retro and Review document on disk, returning the `(kind, sprint-id)` of each valid
/// record plus an issue for every malformed, misnamed, or orphaned one.
///
/// Sprint child records are stored as `<board>/retro/<ID>.md` and `<board>/review/<ID>.md` for
/// every backend (SQLite keeps them in the shared Markdown directories), so scanning those
/// directories yields the child records regardless of the configured storage backend. A record only
/// answers "does a readable record of this kind exist for this Sprint?" when its document parses
/// through the same typed reader `sprint retro show` uses, its frontmatter ID matches its filename,
/// and a parent Sprint with that ID exists. A document that fails the typed parse (a missing or
/// malformed `id`, `created`, or `updated`) is a [`DoctorIssueKind::MalformedRecord`]; a readable
/// record whose Sprint is gone is an orphan flagged as [`DoctorIssueKind::DanglingSprint`]. Both are
/// excluded from the reference set so an action PBI pointing at them is still reported, and one
/// broken document never aborts the rest of the scan.
async fn read_child_records(
    board_dir: &Path,
    sprints: &[RawSprintRecord],
) -> Result<(HashSet<(SprintRecordKind, String)>, Vec<DoctorIssue>)> {
    let valid_sprints = sprints
        .iter()
        .filter_map(RawSprintRecord::valid_id)
        .map(|id| id.to_string())
        .collect::<BTreeSet<_>>();
    let mut ids = HashSet::new();
    let mut issues = Vec::new();
    for kind in SprintRecordKind::ALL {
        for (path, text) in read_documents(&board_dir.join(kind.directory())).await? {
            match validate_child_record(kind, &path, &text) {
                Ok(id) if valid_sprints.contains(&id) => {
                    ids.insert((kind, id));
                }
                Ok(id) => issues.push(make_issue(
                    DoctorIssueKind::DanglingSprint,
                    path.display().to_string(),
                    format!("{} {id} has no parent sprint", kind.display_name()),
                    "restore the sprint or remove the orphaned record",
                )),
                Err(issue) => issues.push(issue),
            }
        }
    }
    Ok((ids, issues))
}

/// Validate one child-record document and return its Sprint ID, or the issue that disqualifies it.
///
/// A record is valid only when it parses through [`record_from_markdown`] — the same typed reader
/// the persistence layer uses, which requires a Sprint-ID `id` and RFC3339 `created`/`updated`
/// fields — and its ID equals the filename stem. Reusing that reader keeps `doctor` from counting a
/// document the shared child-record readers would reject (for example one with a broken timestamp)
/// as a valid record.
fn validate_child_record(
    kind: SprintRecordKind,
    path: &Path,
    text: &str,
) -> std::result::Result<String, DoctorIssue> {
    let location = path.display().to_string();
    let record = record_from_markdown(text, path, kind).map_err(|error| {
        make_issue(
            DoctorIssueKind::MalformedRecord,
            location.clone(),
            format!("{} is not readable: {error}", kind.display_name()),
            format!(
                "restore valid TOML frontmatter with the {} ID, created, and updated fields matching the filename",
                kind.display_name()
            ),
        )
    })?;
    let id = record.id.to_string();
    match path.file_stem().and_then(|stem| stem.to_str()) {
        Some(stem) if stem == id => Ok(id),
        Some(stem) => Err(make_issue(
            DoctorIssueKind::Filename,
            location,
            format!(
                "{} filename `{stem}.md` does not match frontmatter ID {id}",
                kind.display_name()
            ),
            "rename the record to the canonical <SPRINT-ID>.md filename or fix its frontmatter",
        )),
        None => Err(make_issue(
            DoctorIssueKind::MalformedRecord,
            location,
            format!("{} filename is not valid UTF-8", kind.display_name()),
            "rename the record to a UTF-8 <SPRINT-ID>.md filename",
        )),
    }
}

pub(super) async fn read_issued_history(board_dir: &Path) -> Result<IssuedHistory> {
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

pub(super) fn analyze_sprints(sprints: &[RawSprintRecord]) -> Vec<DoctorIssue> {
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

        match &sprint.title {
            RawField::Present(title) if !title.trim().is_empty() => {}
            RawField::Present(_) => issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                sprint.location(),
                "sprint title must not be empty",
                "restore a non-empty title in frontmatter",
            )),
            RawField::Missing => issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                sprint.location(),
                "required sprint field title is missing",
                "restore a non-empty title in frontmatter",
            )),
            RawField::Invalid(error) => issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                sprint.location(),
                error.clone(),
                "set title to a string in frontmatter",
            )),
        }

        // Preserve strict parser failures that lenient field extraction cannot see, such as an
        // unreadable timestamp or an invalid optional numeric field. Required-field failures are
        // already reported above, so only emit this additional finding when those fields are valid.
        if sprint.required_fields_are_valid()
            && let Some(error) = &sprint.parse_error
        {
            issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                sprint.location(),
                format!("sprint is not readable: {error}"),
                "restore valid Sprint frontmatter values, including RFC3339 timestamps",
            ));
        }

        // When the document forms a Sprint, apply the shared domain invariants that lenient field
        // extraction cannot see — a two-sided non-inverted period, a Goal on an active Sprint, and
        // no Goal outcome recorded against a blank Goal — so File, Git, and SQLite report the same
        // finding for the same logical data, matching what `import` rejects. The parsed Sprint keeps
        // its raw Goal outcome (the read paths that normalize it are bypassed here), so a
        // hand-edited `goal_achieved` with a blank Goal is surfaced instead of hidden. A blank title,
        // invalid id, or invalid state is already reported from the lenient fields above.
        if let Some(parsed) = &sprint.parsed
            && let Err(error) = parsed.validate()
        {
            issues.push(make_issue(
                DoctorIssueKind::MalformedRecord,
                sprint.location(),
                error.to_string(),
                "restore a valid period and Goal, or re-import from a healthy export",
            ));
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

pub(super) fn analyze_records(
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

            if record.area.is_active_store()
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

/// Flag action PBIs whose `[source]` link points at a Sprint or child record that is not present.
///
/// After a Sprint is deleted (with its records) or a hand-edited board loses a Retro/Review, the
/// promoted action PBI keeps a `source` naming the vanished Sprint or record. Both the active and
/// the archived stores are covered because `records` already includes both areas. Findings reuse
/// [`DoctorIssueKind::DanglingSprint`]: like a dangling `sprint` assignment, the link names board
/// state that no longer exists, and doctor only reports it for conservative manual repair.
pub(super) fn analyze_action_sources(
    records: &[RawItemRecord],
    sprints: &[RawSprintRecord],
    child_records: &HashSet<(SprintRecordKind, String)>,
) -> Vec<DoctorIssue> {
    let mut issues = Vec::new();
    let valid_sprints = sprints
        .iter()
        .filter_map(RawSprintRecord::valid_id)
        .map(|id| id.to_string())
        .collect::<BTreeSet<_>>();

    for record in records {
        let source = match &record.source {
            RawField::Missing => continue,
            RawField::Present(source) => source,
            RawField::Invalid(error) => {
                issues.push(make_issue(
                    DoctorIssueKind::DanglingSprint,
                    record.location(),
                    error.clone(),
                    "remove the [source] table or restore its kind and sprint_id fields",
                ));
                continue;
            }
        };

        let kind = match source.kind.as_str() {
            "retro" => SprintRecordKind::Retro,
            "review" => SprintRecordKind::Review,
            other => {
                issues.push(make_issue(
                    DoctorIssueKind::DanglingSprint,
                    record.location(),
                    format!("action source kind is invalid: {other:?}"),
                    "set source.kind to `retro` or `review`, or remove the [source] table",
                ));
                continue;
            }
        };

        let sprint_id =
            match source.sprint_id.parse::<SprintId>() {
                Ok(id) => id.to_string(),
                Err(_) => {
                    issues.push(make_issue(
                    DoctorIssueKind::DanglingSprint,
                    record.location(),
                    format!("action source sprint reference is invalid: {:?}", source.sprint_id),
                    "set source.sprint_id to an existing sprint ID, or remove the [source] table",
                ));
                    continue;
                }
            };

        if !valid_sprints.contains(&sprint_id) {
            issues.push(make_issue(
                DoctorIssueKind::DanglingSprint,
                record.location(),
                format!(
                    "action PBI refers to missing sprint {sprint_id} through its {} source",
                    kind.display_name()
                ),
                "clear the [source] link or restore the sprint",
            ));
        } else if !child_records.contains(&(kind, sprint_id.clone())) {
            issues.push(make_issue(
                DoctorIssueKind::DanglingSprint,
                record.location(),
                format!(
                    "action PBI refers to missing {} {sprint_id}",
                    kind.display_name()
                ),
                "clear the [source] link or restore the record",
            ));
        }
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

pub(super) fn graph_cycles(edges: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    let mut state = BTreeMap::new();
    let mut seen_cycles = HashSet::new();
    let mut cycles = Vec::new();
    for node in edges.keys() {
        visit_graph(node, edges, &mut state, &mut seen_cycles, &mut cycles);
    }
    cycles.sort();
    cycles
}

/// One depth-first search rooted at `root`, run on an explicit heap stack so a
/// chain thousands of levels deep cannot overflow the native call stack.
///
/// The three-state coloring (0 = unvisited, 1 = on the current path, 2 = done)
/// and the on-path cycle extraction are preserved exactly, so the reported
/// cycles are identical to the earlier recursive traversal.
fn visit_graph<'a>(
    root: &'a str,
    edges: &'a BTreeMap<String, BTreeSet<String>>,
    state: &mut BTreeMap<String, u8>,
    seen_cycles: &mut HashSet<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    if state.get(root).copied().unwrap_or(0) == 2 {
        return;
    }

    // One frame per node on the DFS path: the node's outgoing edges and how far
    // we have walked them. `path` is the same node list the recursion kept on
    // its call stack, used to slice out a cycle at a back edge.
    struct Frame<'a> {
        node: &'a str,
        targets: Vec<&'a String>,
        cursor: usize,
    }

    let mut path: Vec<&'a str> = vec![root];
    state.insert(root.to_string(), 1);
    let mut frames: Vec<Frame<'a>> = vec![Frame {
        node: root,
        targets: edges.get(root).into_iter().flatten().collect(),
        cursor: 0,
    }];

    while let Some(frame) = frames.last_mut() {
        let next = (frame.cursor < frame.targets.len()).then(|| {
            let target = frame.targets[frame.cursor];
            frame.cursor += 1;
            target
        });

        match next {
            Some(target) => match state.get(target.as_str()).copied().unwrap_or(0) {
                0 => {
                    state.insert(target.clone(), 1);
                    path.push(target.as_str());
                    frames.push(Frame {
                        node: target.as_str(),
                        targets: edges.get(target.as_str()).into_iter().flatten().collect(),
                        cursor: 0,
                    });
                }
                1 => {
                    if let Some(start) = path.iter().position(|value| *value == target.as_str()) {
                        let mut cycle: Vec<String> = path[start..]
                            .iter()
                            .map(|value| (*value).to_string())
                            .collect();
                        cycle.sort();
                        let key = cycle.join(",");
                        if seen_cycles.insert(key) {
                            cycles.push(cycle);
                        }
                    }
                }
                _ => {}
            },
            None => {
                state.insert(frame.node.to_string(), 2);
                frames.pop();
                path.pop();
            }
        }
    }
}
