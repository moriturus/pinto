//! Sprint persistence for [`FileRepository`]: the [`SprintRepository`]
//! implementation and the sprint record reading/validation helpers it relies on.

use super::{FileRepository, SprintRecord};
use crate::error::Error;
use crate::error::Result;
use crate::sprint::{Sprint, SprintId};
use crate::storage::atomic_write;
use crate::storage::markdown::{sprint_from_markdown, sprint_to_markdown};
use crate::storage::repository::SprintRepository;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use tokio::fs;
use tokio::task::JoinSet;

impl SprintRepository for FileRepository {
    async fn save(&self, sprint: &Sprint) -> Result<()> {
        self.read_sprint_records().await?;
        let dir = self.sprints_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| Error::io(&dir, &e))?;
        let path = self.sprint_path_for(&sprint.id);
        let text = sprint_to_markdown(sprint)?;
        atomic_write(&path, &text).await
    }

    async fn load(&self, id: &SprintId) -> Result<Sprint> {
        self.read_sprint_records()
            .await?
            .into_iter()
            .find_map(|(_, sprint)| (sprint.id == *id).then_some(sprint))
            .ok_or_else(|| Error::SprintNotFound(id.clone()))
    }

    async fn list(&self) -> Result<Vec<Sprint>> {
        let mut sprints = self
            .read_sprint_records()
            .await?
            .into_iter()
            .map(|(_, sprint)| sprint)
            .collect::<Vec<_>>();

        // Sort by creation time, using the ID as a deterministic tie-breaker.
        sprints.sort_by(|a, b| {
            a.created
                .cmp(&b.created)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        Ok(sprints)
    }

    async fn delete(&self, id: &SprintId) -> Result<()> {
        self.read_sprint_records().await?;
        let path = self.sprint_path_for(id);
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Error::SprintNotFound(id.clone())),
            Err(e) => Err(Error::io(&path, &e)),
        }
    }
}

impl FileRepository {
    /// Read and validate every sprint file, retaining paths for collision diagnostics.
    async fn read_sprint_records(&self) -> Result<Vec<SprintRecord>> {
        let dir = self.sprints_dir();
        let Some(paths) = self.markdown_paths(&dir).await? else {
            return Ok(Vec::new());
        };

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

        let records = contents
            .into_iter()
            .map(|(path, text)| sprint_from_markdown(&text, &path).map(|sprint| (path, sprint)))
            .collect::<Result<Vec<_>>>()?;
        Self::ensure_unique_sprint_ids(&records)?;
        for (path, sprint) in &records {
            Self::validate_sprint_filename(path, sprint)?;
        }
        Ok(records)
    }

    /// Reject two sprint files that resolve to the same logical sprint ID.
    fn ensure_unique_sprint_ids(records: &[SprintRecord]) -> Result<()> {
        let mut seen = HashMap::new();
        for (path, sprint) in records {
            if let Some(previous) = seen.insert(sprint.id.clone(), path.clone()) {
                return Err(Error::parse(
                    path,
                    format!(
                        "duplicate sprint ID `{}` in {} and {}; fix one frontmatter ID or rename one file",
                        sprint.id,
                        previous.display(),
                        path.display()
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Ensure a sprint filename stem and frontmatter ID describe the same record.
    fn validate_sprint_filename(path: &Path, sprint: &Sprint) -> Result<()> {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                Error::parse(
                    path,
                    "sprint filename must be a UTF-8 `<ID>.md`; rename the file",
                )
            })?;
        let filename_id = stem.parse::<SprintId>().map_err(|error| {
            Error::parse(
                path,
                format!(
                    "invalid sprint filename `{stem}.md`: {error}; rename the file to `<ID>.md`"
                ),
            )
        })?;
        if filename_id != sprint.id {
            return Err(Error::parse(
                path,
                format!(
                    "filename ID `{filename_id}` does not match frontmatter ID `{}`; rename the file or fix its frontmatter",
                    sprint.id
                ),
            ));
        }
        Ok(())
    }
}
