//! Sprint Retro persistence for [`FileRepository`].

use super::{FileRepository, RetroRecord};
use crate::error::{Error, Result};
use crate::retro::SprintRetro;
use crate::sprint::SprintId;
use crate::storage::atomic_write;
use crate::storage::markdown::{retro_from_markdown, retro_to_markdown};
use crate::storage::repository::SprintRetroRepository;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use tokio::fs;
use tokio::task::JoinSet;

impl SprintRetroRepository for FileRepository {
    async fn save(&self, retro: &SprintRetro) -> Result<()> {
        self.read_retro_records().await?;
        let dir = self.retro_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|error| Error::io(&dir, &error))?;
        let path = self.retro_path_for(&retro.id);
        let text = retro_to_markdown(retro)?;
        atomic_write(&path, &text).await
    }

    async fn load(&self, id: &SprintId) -> Result<SprintRetro> {
        self.read_retro_records()
            .await?
            .into_iter()
            .find_map(|(_, retro)| (retro.id == *id).then_some(retro))
            .ok_or_else(|| Error::RetroNotFound(id.clone()))
    }

    async fn list(&self) -> Result<Vec<SprintRetro>> {
        let mut retros = self
            .read_retro_records()
            .await?
            .into_iter()
            .map(|(_, retro)| retro)
            .collect::<Vec<_>>();
        retros.sort_by(|a, b| {
            a.created
                .cmp(&b.created)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        Ok(retros)
    }

    async fn delete(&self, id: &SprintId) -> Result<()> {
        self.read_retro_records().await?;
        let path = self.retro_path_for(id);
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(Error::RetroNotFound(id.clone()))
            }
            Err(error) => Err(Error::io(&path, &error)),
        }
    }
}

impl FileRepository {
    /// Read and validate every Retro file, retaining paths for collision diagnostics.
    async fn read_retro_records(&self) -> Result<Vec<RetroRecord>> {
        let dir = self.retro_dir();
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
            .map(|(path, text)| retro_from_markdown(&text, &path).map(|retro| (path, retro)))
            .collect::<Result<Vec<_>>>()?;
        Self::ensure_unique_retro_ids(&records)?;
        for (path, retro) in &records {
            Self::validate_retro_filename(path, retro)?;
        }
        Ok(records)
    }

    fn ensure_unique_retro_ids(records: &[RetroRecord]) -> Result<()> {
        let mut seen = HashMap::new();
        for (path, retro) in records {
            if let Some(previous) = seen.insert(retro.id.clone(), path.clone()) {
                return Err(Error::parse(
                    path,
                    format!(
                        "duplicate Retro ID `{}` in {} and {}; fix one frontmatter ID or rename one file",
                        retro.id,
                        previous.display(),
                        path.display()
                    ),
                ));
            }
        }
        Ok(())
    }

    fn validate_retro_filename(path: &Path, retro: &SprintRetro) -> Result<()> {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                Error::parse(
                    path,
                    "Retro filename must be a UTF-8 `<SPRINT-ID>.md`; rename the file",
                )
            })?;
        let filename_id = stem.parse::<SprintId>().map_err(|error| {
            Error::parse(
                path,
                format!(
                    "invalid Retro filename `{stem}.md`: {error}; rename the file to `<SPRINT-ID>.md`"
                ),
            )
        })?;
        if filename_id != retro.id {
            return Err(Error::parse(
                path,
                format!(
                    "filename ID `{filename_id}` does not match Retro frontmatter ID `{}`; rename the file or fix its frontmatter",
                    retro.id
                ),
            ));
        }
        Ok(())
    }
}
