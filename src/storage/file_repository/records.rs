//! Kind-parameterized Sprint child-record persistence for [`FileRepository`].

use super::{ChildRecord, FileRepository};
use crate::error::{Error, Result};
use crate::sprint::SprintId;
use crate::sprint_record::{SprintRecord, SprintRecordKind};
use crate::storage::atomic_write;
use crate::storage::markdown::{record_from_markdown, record_to_markdown};
use crate::storage::repository::SprintRecordRepository;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use tokio::fs;
use tokio::task::JoinSet;

impl SprintRecordRepository for FileRepository {
    async fn save(&self, record: &SprintRecord) -> Result<()> {
        self.read_records(record.kind).await?;
        let dir = self.record_dir(record.kind);
        fs::create_dir_all(&dir)
            .await
            .map_err(|error| Error::io(&dir, &error))?;
        let path = self.record_path_for(record.kind, &record.id);
        let text = record_to_markdown(record)?;
        atomic_write(&path, &text).await
    }

    async fn load(&self, kind: SprintRecordKind, id: &SprintId) -> Result<SprintRecord> {
        self.read_records(kind)
            .await?
            .into_iter()
            .find_map(|(_, record)| (record.id == *id).then_some(record))
            .ok_or_else(|| Error::SprintRecordNotFound {
                kind,
                id: id.clone(),
            })
    }

    async fn list(&self, kind: SprintRecordKind) -> Result<Vec<SprintRecord>> {
        let mut records = self
            .read_records(kind)
            .await?
            .into_iter()
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        records.sort_by(|a, b| {
            a.created
                .cmp(&b.created)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        Ok(records)
    }

    async fn delete(&self, kind: SprintRecordKind, id: &SprintId) -> Result<()> {
        self.read_records(kind).await?;
        let path = self.record_path_for(kind, id);
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(Error::SprintRecordNotFound {
                    kind,
                    id: id.clone(),
                })
            }
            Err(error) => Err(Error::io(&path, &error)),
        }
    }
}

impl FileRepository {
    /// Read and validate every record file for one kind, retaining paths for diagnostics.
    async fn read_records(&self, kind: SprintRecordKind) -> Result<Vec<ChildRecord>> {
        let dir = self.record_dir(kind);
        let Some(paths) = self.markdown_paths(&dir).await? else {
            return Ok(Vec::new());
        };

        let mut reads = JoinSet::new();
        for path in paths {
            reads.spawn(async move {
                fs::read_to_string(&path)
                    .await
                    .map_err(|error| Error::io(&path, &error))
                    .map(|text| (path, text))
            });
        }
        let mut contents = Vec::new();
        while let Some(joined) = reads.join_next().await {
            contents.push(joined.map_err(Error::task)??);
        }

        let records = contents
            .into_iter()
            .map(|(path, text)| {
                record_from_markdown(&text, &path, kind).map(|record| (path, record))
            })
            .collect::<Result<Vec<_>>>()?;
        Self::ensure_unique_ids(kind, &records)?;
        for (path, record) in &records {
            Self::validate_filename(kind, path, record)?;
        }
        Ok(records)
    }

    fn ensure_unique_ids(kind: SprintRecordKind, records: &[ChildRecord]) -> Result<()> {
        let mut seen = HashMap::new();
        for (path, record) in records {
            if let Some(previous) = seen.insert(record.id.clone(), path.clone()) {
                return Err(Error::parse(
                    path,
                    format!(
                        "duplicate {} ID `{}` in {} and {}; fix one frontmatter ID or rename one file",
                        kind.display_name(),
                        record.id,
                        previous.display(),
                        path.display()
                    ),
                ));
            }
        }
        Ok(())
    }

    fn validate_filename(kind: SprintRecordKind, path: &Path, record: &SprintRecord) -> Result<()> {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                Error::parse(
                    path,
                    format!(
                        "{} filename must be a UTF-8 `<SPRINT-ID>.md`; rename the file",
                        kind.display_name()
                    ),
                )
            })?;
        let filename_id = stem.parse::<SprintId>().map_err(|error| {
            Error::parse(
                path,
                format!(
                    "invalid {} filename `{stem}.md`: {error}; rename the file to `<SPRINT-ID>.md`",
                    kind.display_name()
                ),
            )
        })?;
        if filename_id != record.id {
            return Err(Error::parse(
                path,
                format!(
                    "filename ID `{filename_id}` does not match {} frontmatter ID `{}`; rename the file or fix its frontmatter",
                    kind.display_name(),
                    record.id
                ),
            ));
        }
        Ok(())
    }
}
