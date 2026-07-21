//! Conservative, mechanical repairs applied by `doctor --fix`.

use super::{DoctorFix, Inspection, RawItemRecord, RecordArea};
use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::storage::{atomic_write, item_from_markdown, item_to_markdown, record_issued_id};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tokio::fs;

pub(super) async fn apply_safe_fixes(
    board_dir: &std::path::Path,
    inspection: &Inspection,
) -> Result<Vec<DoctorFix>> {
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
    item: BacklogItem,
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

pub(super) async fn repair_duplicate_item_ids(
    board_dir: &std::path::Path,
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
