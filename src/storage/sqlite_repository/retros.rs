//! File-backed Sprint Retro persistence while the optional SQLite backend stores board entities.

use super::SqliteRepository;
use crate::error::Result;
use crate::retro::SprintRetro;
use crate::sprint::SprintId;
use crate::storage::file_repository::FileRepository;
use crate::storage::repository::SprintRetroRepository;

impl SqliteRepository {
    fn retro_repository(&self) -> FileRepository {
        FileRepository::new(self.root.clone())
    }
}

impl SprintRetroRepository for SqliteRepository {
    async fn save(&self, retro: &SprintRetro) -> Result<()> {
        SprintRetroRepository::save(&self.retro_repository(), retro).await
    }

    async fn load(&self, id: &SprintId) -> Result<SprintRetro> {
        SprintRetroRepository::load(&self.retro_repository(), id).await
    }

    async fn list(&self) -> Result<Vec<SprintRetro>> {
        SprintRetroRepository::list(&self.retro_repository()).await
    }

    async fn delete(&self, id: &SprintId) -> Result<()> {
        SprintRetroRepository::delete(&self.retro_repository(), id).await
    }
}
