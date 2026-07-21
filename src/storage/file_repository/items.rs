//! Backlog-item persistence for [`FileRepository`]: the [`BacklogItemRepository`]
//! implementation and the item record reading/validation helpers it relies on.

use super::{FileRepository, ItemRecord};
use crate::backlog::{BacklogItem, ItemId};
use crate::error::Error;
use crate::error::Result;
use crate::storage::atomic_write;
use crate::storage::issued_ids::{max_number, record};
use crate::storage::markdown::{from_markdown, to_markdown};
use crate::storage::repository::BacklogItemRepository;
use rayon::prelude::*;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::task::JoinSet;

impl BacklogItemRepository for FileRepository {
    async fn save(&self, item: &BacklogItem) -> Result<()> {
        let (_, archived) = self.read_all_item_records().await?;
        let dir = self.tasks_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| Error::io(&dir, &e))?;
        let path = self.path_for(&item.id)?;
        if let Some((archive_path, _)) =
            archived.iter().find(|(_, existing)| existing.id == item.id)
        {
            return Err(Error::parse(
                &path,
                format!(
                    "cannot save item `{}`: archive file {} already exists; remove the archived copy before restoring the item",
                    item.id,
                    archive_path.display()
                ),
            ));
        }
        record(&self.root, &item.id).await?;
        let text = to_markdown(item)?;
        atomic_write(&path, &text).await
    }

    async fn load(&self, id: &ItemId) -> Result<BacklogItem> {
        let (active, _) = self.read_all_item_records().await?;
        active
            .into_iter()
            .find_map(|(_, item)| (item.id == *id).then_some(item))
            .ok_or_else(|| Error::NotFound(id.clone()))
    }

    async fn list(&self) -> Result<Vec<BacklogItem>> {
        let (active, _) = self.read_all_item_records().await?;
        let mut items = active.into_iter().map(|(_, item)| item).collect::<Vec<_>>();

        // Canonical backlog order (rank asc, ID tie-break) shared with every view.
        items.sort_by(BacklogItem::backlog_cmp);
        Ok(items)
    }

    async fn list_archived(&self) -> Result<Vec<BacklogItem>> {
        let (_, archived) = self.read_all_item_records().await?;
        let mut items = archived
            .into_iter()
            .map(|(_, item)| item)
            .collect::<Vec<_>>();
        items.sort_by(BacklogItem::backlog_cmp);
        Ok(items)
    }

    async fn load_archived(&self, id: &ItemId) -> Result<BacklogItem> {
        let (_, archived) = self.read_all_item_records().await?;
        archived
            .into_iter()
            .find_map(|(_, item)| (item.id == *id).then_some(item))
            .ok_or_else(|| Error::NotFound(id.clone()))
    }

    async fn delete(&self, id: &ItemId) -> Result<()> {
        self.read_all_item_records().await?;
        let path = self.path_for(id)?;
        match fs::remove_file(&path).await {
            Ok(()) => record(&self.root, id).await,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Error::NotFound(id.clone())),
            Err(e) => Err(Error::io(&path, &e)),
        }
    }

    async fn archive(&self, id: &ItemId) -> Result<PathBuf> {
        let src = self.path_for(id)?;
        let source_exists = fs::try_exists(&src)
            .await
            .map_err(|e| Error::io(&src, &e))?;
        let dest = self.archive_path_for(id)?;
        let destination_exists = fs::try_exists(&dest)
            .await
            .map_err(|e| Error::io(&dest, &e))?;
        if source_exists && destination_exists {
            return Err(Error::parse(
                &dest,
                format!(
                    "cannot archive `{id}`: destination already exists at {}; remove the archived copy before retrying",
                    dest.display()
                ),
            ));
        }
        self.read_all_item_records().await?;
        if !source_exists {
            return Err(Error::NotFound(id.clone()));
        }
        let archive_dir = self.archive_dir();
        fs::create_dir_all(&archive_dir)
            .await
            .map_err(|e| Error::io(&archive_dir, &e))?;

        // Rename after validation; map a source that disappears between validation and the move
        // to `NotFound` without allowing an existing destination to be replaced.
        match fs::rename(&src, &dest).await {
            Ok(()) => {
                record(&self.root, id).await?;
                Ok(dest)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Error::NotFound(id.clone())),
            Err(e) => Err(Error::io(&src, &e)),
        }
    }

    async fn restore(&self, id: &ItemId) -> Result<()> {
        let src = self.archive_path_for(id)?;
        let source_exists = fs::try_exists(&src)
            .await
            .map_err(|e| Error::io(&src, &e))?;
        if !source_exists {
            return Err(Error::NotFound(id.clone()));
        }

        let dest = self.path_for(id)?;
        let destination_exists = fs::try_exists(&dest)
            .await
            .map_err(|e| Error::io(&dest, &e))?;
        if destination_exists {
            return Err(Error::parse(
                &dest,
                format!(
                    "cannot restore `{id}`: active item already exists at {}; remove or rename the active copy before retrying",
                    dest.display()
                ),
            ));
        }

        // Validate both stores before moving the archive. The destination check above deliberately
        // happens first so a collision is reported without letting duplicate IDs obscure it.
        self.read_all_item_records().await?;
        let tasks_dir = self.tasks_dir();
        fs::create_dir_all(&tasks_dir)
            .await
            .map_err(|e| Error::io(&tasks_dir, &e))?;
        match fs::rename(&src, &dest).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Error::NotFound(id.clone())),
            Err(e) => Err(Error::io(&src, &e)),
        }
    }

    async fn next_id(&self, prefix: &str) -> Result<ItemId> {
        // IDs are never reused after issuance. The history file also covers physically deleted IDs
        // and backend migrations; scanning both directories keeps older boards safe as well.
        let mut max = max_number(&self.root, prefix).await?;
        let (active, archived) = self.read_all_item_records().await?;
        let existing_max = active
            .iter()
            .chain(archived.iter())
            .map(|(_, item)| item.id.clone())
            .filter(|id| id.prefix() == prefix)
            .map(|id| id.number())
            .max()
            .unwrap_or(0);
        max = max.max(existing_max);
        let next = max
            .checked_add(1)
            .ok_or_else(|| Error::InvalidItemId(format!("{prefix}-{max}")))?;
        ItemId::try_new(prefix, next)
    }
}

impl FileRepository {
    /// Read and validate every active and archived item before exposing the active set.
    async fn read_all_item_records(&self) -> Result<(Vec<ItemRecord>, Vec<ItemRecord>)> {
        let active = self.read_item_records(&self.tasks_dir()).await?;
        let archived = self.read_item_records(&self.archive_dir()).await?;
        Self::ensure_unique_item_ids(active.iter().chain(archived.iter()))?;
        Ok((active, archived))
    }

    /// Read item files from one directory, validate their logical IDs, and retain their paths for
    /// actionable corruption diagnostics and cross-directory collision checks.
    async fn read_item_records(&self, dir: &Path) -> Result<Vec<ItemRecord>> {
        let Some(paths) = self.markdown_paths(dir).await? else {
            return Ok(Vec::new());
        };

        // Read all files concurrently; this is I/O-bound work.
        let mut reads = JoinSet::new();
        for path in paths {
            reads.spawn(async move {
                fs::read_to_string(&path)
                    .await
                    .map_err(|e| Error::io(&path, &e))
                    .map(|text| (path, text))
            });
        }
        let mut contents = Vec::new();
        while let Some(joined) = reads.join_next().await {
            contents.push(joined.map_err(Error::task)??);
        }

        // Parse frontmatter in parallel; this is CPU-bound work.
        let records = contents
            .into_par_iter()
            .map(|(path, text)| from_markdown(&text, &path).map(|item| (path, item)))
            .collect::<Result<Vec<_>>>()?;

        // Report duplicate logical IDs before filename mismatches so the error names both records
        // when a copied file creates an ambiguous lookup key.
        Self::ensure_unique_item_ids(records.iter())?;
        for (path, item) in &records {
            Self::validate_item_filename(path, item)?;
        }
        Ok(records)
    }

    /// Reject two files that resolve to the same logical item ID.
    fn ensure_unique_item_ids<'a>(records: impl IntoIterator<Item = &'a ItemRecord>) -> Result<()> {
        let mut seen = HashMap::new();
        for (path, item) in records {
            if let Some(previous) = seen.insert(item.id.clone(), path.clone()) {
                return Err(Error::parse(
                    path,
                    format!(
                        "duplicate item ID `{}` in {} and {}; fix one frontmatter ID or rename one file",
                        item.id,
                        previous.display(),
                        path.display()
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Ensure an item's filename stem and frontmatter ID describe the same record.
    fn validate_item_filename(path: &Path, item: &BacklogItem) -> Result<()> {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                Error::parse(
                    path,
                    "item filename must be a UTF-8 `<PREFIX>-<NUMBER>.md`; rename the file",
                )
            })?;
        let filename_id = stem.parse::<ItemId>().map_err(|error| {
            Error::parse(
                path,
                format!("invalid item filename `{stem}.md`: {error}; rename the file to `<ID>.md`"),
            )
        })?;
        if filename_id != item.id {
            return Err(Error::parse(
                path,
                format!(
                    "filename ID `{filename_id}` does not match frontmatter ID `{}`; rename the file or fix its frontmatter",
                    item.id
                ),
            ));
        }
        Ok(())
    }
}
