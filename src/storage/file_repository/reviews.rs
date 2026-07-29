//! Sprint Review persistence for [`FileRepository`].

use super::{FileRepository, ReviewRecord};
use crate::error::{Error, Result};
use crate::review::SprintReview;
use crate::sprint::SprintId;
use crate::storage::atomic_write;
use crate::storage::markdown::{review_from_markdown, review_to_markdown};
use crate::storage::repository::SprintReviewRepository;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use tokio::fs;
use tokio::task::JoinSet;

impl SprintReviewRepository for FileRepository {
    async fn save(&self, review: &SprintReview) -> Result<()> {
        self.read_review_records().await?;
        let dir = self.review_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|error| Error::io(&dir, &error))?;
        let path = self.review_path_for(&review.id);
        let text = review_to_markdown(review)?;
        atomic_write(&path, &text).await
    }

    async fn load(&self, id: &SprintId) -> Result<SprintReview> {
        self.read_review_records()
            .await?
            .into_iter()
            .find_map(|(_, review)| (review.id == *id).then_some(review))
            .ok_or_else(|| Error::ReviewNotFound(id.clone()))
    }

    async fn list(&self) -> Result<Vec<SprintReview>> {
        let mut reviews = self
            .read_review_records()
            .await?
            .into_iter()
            .map(|(_, review)| review)
            .collect::<Vec<_>>();
        reviews.sort_by(|a, b| {
            a.created
                .cmp(&b.created)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        Ok(reviews)
    }

    async fn delete(&self, id: &SprintId) -> Result<()> {
        self.read_review_records().await?;
        let path = self.review_path_for(id);
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(Error::ReviewNotFound(id.clone()))
            }
            Err(error) => Err(Error::io(&path, &error)),
        }
    }
}

impl FileRepository {
    /// Read and validate every Review file, retaining paths for collision diagnostics.
    async fn read_review_records(&self) -> Result<Vec<ReviewRecord>> {
        let dir = self.review_dir();
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
            .map(|(path, text)| review_from_markdown(&text, &path).map(|review| (path, review)))
            .collect::<Result<Vec<_>>>()?;
        Self::ensure_unique_review_ids(&records)?;
        for (path, review) in &records {
            Self::validate_review_filename(path, review)?;
        }
        Ok(records)
    }

    fn ensure_unique_review_ids(records: &[ReviewRecord]) -> Result<()> {
        let mut seen = HashMap::new();
        for (path, review) in records {
            if let Some(previous) = seen.insert(review.id.clone(), path.clone()) {
                return Err(Error::parse(
                    path,
                    format!(
                        "duplicate Review ID `{}` in {} and {}; fix one frontmatter ID or rename one file",
                        review.id,
                        previous.display(),
                        path.display()
                    ),
                ));
            }
        }
        Ok(())
    }

    fn validate_review_filename(path: &Path, review: &SprintReview) -> Result<()> {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                Error::parse(
                    path,
                    "Review filename must be a UTF-8 `<SPRINT-ID>.md`; rename the file",
                )
            })?;
        let filename_id = stem.parse::<SprintId>().map_err(|error| {
            Error::parse(
                path,
                format!(
                    "invalid Review filename `{stem}.md`: {error}; rename the file to `<SPRINT-ID>.md`"
                ),
            )
        })?;
        if filename_id != review.id {
            return Err(Error::parse(
                path,
                format!(
                    "filename ID `{filename_id}` does not match Review frontmatter ID `{}`; rename the file or fix its frontmatter",
                    review.id
                ),
            ));
        }
        Ok(())
    }
}
