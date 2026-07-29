//! File-backed Sprint child-record persistence for the optional SQLite backend.

use super::SqliteRepository;
use crate::error::Result;
use crate::sprint::SprintId;
use crate::sprint_record::{SprintRecord, SprintRecordKind};
use crate::storage::file_repository::FileRepository;
use crate::storage::repository::SprintRecordRepository;

impl SqliteRepository {
    fn record_repository(&self) -> FileRepository {
        FileRepository::new(self.root.clone())
    }
}

impl SprintRecordRepository for SqliteRepository {
    async fn save(&self, record: &SprintRecord) -> Result<()> {
        SprintRecordRepository::save(&self.record_repository(), record).await
    }

    async fn load(&self, kind: SprintRecordKind, id: &SprintId) -> Result<SprintRecord> {
        SprintRecordRepository::load(&self.record_repository(), kind, id).await
    }

    async fn list(&self, kind: SprintRecordKind) -> Result<Vec<SprintRecord>> {
        SprintRecordRepository::list(&self.record_repository(), kind).await
    }

    async fn delete(&self, kind: SprintRecordKind, id: &SprintId) -> Result<()> {
        SprintRecordRepository::delete(&self.record_repository(), kind, id).await
    }
}
