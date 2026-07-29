//! File-backed Sprint Review persistence while the optional SQLite backend stores board entities.

use super::SqliteRepository;
use crate::error::Result;
use crate::review::SprintReview;
use crate::sprint::SprintId;
use crate::storage::file_repository::FileRepository;
use crate::storage::repository::SprintReviewRepository;

impl SqliteRepository {
    fn review_repository(&self) -> FileRepository {
        FileRepository::new(self.root.clone())
    }
}

impl SprintReviewRepository for SqliteRepository {
    async fn save(&self, review: &SprintReview) -> Result<()> {
        SprintReviewRepository::save(&self.review_repository(), review).await
    }

    async fn load(&self, id: &SprintId) -> Result<SprintReview> {
        SprintReviewRepository::load(&self.review_repository(), id).await
    }

    async fn list(&self) -> Result<Vec<SprintReview>> {
        SprintReviewRepository::list(&self.review_repository()).await
    }

    async fn delete(&self, id: &SprintId) -> Result<()> {
        SprintReviewRepository::delete(&self.review_repository(), id).await
    }
}
