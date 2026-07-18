use super::atomic_write;
use super::issued_ids::{max_number, record};
use super::markdown::{from_markdown, sprint_from_markdown, sprint_to_markdown, to_markdown};
use super::repository::{BacklogItemRepository, SprintRepository};
use crate::backlog::{BacklogItem, ItemId};
use crate::error::Error;
use crate::error::Result;
use crate::sprint::{Sprint, SprintId};
use rayon::prelude::*;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::task::JoinSet;

/// [`BacklogItemRepository`] and [`SprintRepository`] implementation backed by the `.pinto/` directory.
#[derive(Debug, Clone)]
pub struct FileRepository {
    root: PathBuf,
}

type ItemRecord = (PathBuf, BacklogItem);
type SprintRecord = (PathBuf, Sprint);

impl FileRepository {
    /// Build by specifying the board root (`.pinto/`). No file I/O is performed.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Directory to place task files (`<root>/tasks`).
    #[must_use]
    pub fn tasks_dir(&self) -> PathBuf {
        self.root.join("tasks")
    }

    /// Directory to put sprint files (`<root>/sprints`).
    #[must_use]
    pub fn sprints_dir(&self) -> PathBuf {
        self.root.join("sprints")
    }

    /// The sprint file path for the specified ID (`<root>/sprints/<id>.md`).
    pub(crate) fn sprint_path_for(&self, id: &SprintId) -> PathBuf {
        self.sprints_dir().join(format!("{id}.md"))
    }

    /// Task file path for the specified ID (`<root>/tasks/<id>.md`).
    pub(crate) fn path_for(&self, id: &ItemId) -> Result<PathBuf> {
        self.safe_item_path(&self.tasks_dir(), id)
    }

    /// Destination directory (`<root>/archive`).
    fn archive_dir(&self) -> PathBuf {
        self.root.join("archive")
    }

    /// Archive file path for the specified ID (`<root>/archive/<id>.md`).
    fn archive_path_for(&self, id: &ItemId) -> Result<PathBuf> {
        self.safe_item_path(&self.archive_dir(), id)
    }

    /// Build one item path while checking that the ID contributes exactly one filename component.
    fn safe_item_path(&self, directory: &Path, id: &ItemId) -> Result<PathBuf> {
        let path = directory.join(format!("{id}.md"));
        if path.parent() != Some(directory) || !path.starts_with(directory) {
            return Err(Error::InvalidItemId(id.to_string()));
        }
        Ok(path)
    }
}

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

    /// Collect the `.md` file paths directly under `tasks/` (directory scanning is asynchronous).
    ///
    /// If the directory does not exist, return `None` to indicate that it has no items yet.
    async fn markdown_paths(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>> {
        let mut read_dir = match fs::read_dir(dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::io(dir, &e)),
        };

        let mut paths = Vec::new();
        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| Error::io(dir, &e))?
        {
            let path = entry.path();
            if Self::is_markdown(&path) {
                paths.push(path);
            }
        }
        Ok(Some(paths))
    }

    /// Return whether `path` has the `.md` extension.
    fn is_markdown(path: &Path) -> bool {
        path.extension().and_then(|s| s.to_str()) == Some("md")
    }
}

#[cfg(test)]
mod tests {
    //! Persistence tests for `FileRepository`.
    //!
    //! Each test uses a `tempfile` directory so it cannot modify the real file system. The I/O
    //! layer is asynchronous (`tokio`), so tests use `#[tokio::test]`.

    use super::{BacklogItemRepository, FileRepository, SprintRepository};
    use crate::backlog::{BacklogItem, ItemId, Status};
    use crate::error::Error;
    use crate::rank::Rank;
    use crate::sprint::{Sprint, SprintId};
    use chrono::{DateTime, TimeZone, Utc};
    use tempfile::TempDir;
    use tokio::fs;
    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0)
            .single()
            .expect("valid timestamp")
    }

    /// Create a temporary directory and a repository rooted at `.pinto` inside it.
    fn repo() -> (TempDir, FileRepository) {
        let dir = TempDir::new().expect("create temp dir");
        let repo = FileRepository::new(dir.path().join(".pinto"));
        (dir, repo)
    }

    /// Generate `count` monotonically increasing ranks in the order of insertion.
    fn ranks(count: usize) -> Vec<Rank> {
        let mut out = Vec::with_capacity(count);
        let mut prev: Option<Rank> = None;
        for _ in 0..count {
            let next = Rank::after(prev.as_ref());
            prev = Some(next.clone());
            out.push(next);
        }
        out
    }

    /// Create a minimal item with the specified ID and rank.
    fn item(n: u32, rank: Rank) -> BacklogItem {
        BacklogItem::new(
            ItemId::new("T", n),
            format!("Item {n}"),
            Status::new("todo"),
            rank,
            ts(1_000),
        )
        .expect("valid item")
    }

    fn sample_item() -> BacklogItem {
        let mut item = BacklogItem::new(
            ItemId::new("T", 1),
            "Implement storage layer",
            Status::new("todo"),
            Rank::after(None),
            ts(1_000),
        )
        .expect("valid item");
        item.points = Some(5);
        item.labels = vec!["storage".to_string(), "cli".to_string()];
        item.assignee = Some("alice".to_string());
        item.sprint = Some("S-1".to_string());
        item.parent = Some(ItemId::new("T", 0));
        item.depends_on = vec![ItemId::new("T", 2), ItemId::new("T", 3)];
        item.updated = ts(2_000);
        item.body = "## 説明\n本文の Markdown。\n\n- [ ] 受け入れ条件".to_string();
        item
    }

    #[tokio::test]
    async fn save_then_load_roundtrips_all_fields() {
        let (_dir, repo) = repo();
        let item = sample_item();

        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save succeeds");
        let loaded = BacklogItemRepository::load(&repo, &item.id)
            .await
            .expect("load succeeds");

        assert_eq!(loaded, item);
    }

    #[tokio::test]
    async fn save_writes_toml_frontmatter_file() {
        let (_dir, repo) = repo();
        let item = sample_item();

        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save succeeds");

        let path = repo.tasks_dir().join("T-1.md");
        let text = fs::read_to_string(&path).await.expect("file exists");
        assert!(text.starts_with("+++\n"), "should open with TOML delimiter");
        assert!(text.contains("id = \"T-1\""), "frontmatter carries id");
        assert!(
            text.contains("status = \"todo\""),
            "frontmatter carries status"
        );
        assert!(text.contains("rank = "), "frontmatter carries rank");
        assert!(
            text.contains("depends_on = [\"T-2\", \"T-3\"]"),
            "frontmatter carries dependencies"
        );
        assert!(
            text.contains("## 説明"),
            "body is preserved after frontmatter"
        );
    }

    #[tokio::test]
    async fn load_missing_item_returns_not_found() {
        let (_dir, repo) = repo();

        let err = BacklogItemRepository::load(&repo, &ItemId::new("T", 99))
            .await
            .expect_err("should be missing");
        assert_eq!(err, Error::NotFound(ItemId::new("T", 99)));
    }

    #[tokio::test]
    async fn list_returns_all_items_sorted_by_rank() {
        let (_dir, repo) = repo();
        // Give items ranks that differ from their ID order and verify that rank determines the result.
        let order = [3u32, 1, 10, 2];
        let rs = ranks(order.len());
        for (n, rank) in order.iter().zip(rs) {
            BacklogItemRepository::save(&repo, &item(*n, rank))
                .await
                .expect("save succeeds");
        }

        let items = BacklogItemRepository::list(&repo)
            .await
            .expect("list succeeds");
        let ids: Vec<u32> = items.iter().map(|i| i.id.number()).collect();
        assert_eq!(ids, order.to_vec(), "rank 昇順（= 割当順）で返る");
    }

    #[tokio::test]
    async fn list_rejects_filename_frontmatter_id_mismatch() {
        let (_dir, repo) = repo();
        BacklogItemRepository::save(&repo, &item(2, Rank::after(None)))
            .await
            .expect("save fixture");
        fs::rename(
            repo.tasks_dir().join("T-2.md"),
            repo.tasks_dir().join("T-1.md"),
        )
        .await
        .expect("rename corrupt fixture");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("filename/frontmatter mismatch must fail fast");
        let message = err.to_string();
        assert!(message.contains("filename"), "got {message}");
        assert!(
            message.contains("T-1") && message.contains("T-2"),
            "got {message}"
        );
    }

    #[tokio::test]
    async fn list_rejects_duplicate_logical_ids() {
        let (_dir, repo) = repo();
        let ranks = ranks(2);
        BacklogItemRepository::save(&repo, &item(1, ranks[0].clone()))
            .await
            .expect("save first fixture");
        BacklogItemRepository::save(&repo, &item(2, ranks[1].clone()))
            .await
            .expect("save second fixture");
        fs::copy(
            repo.tasks_dir().join("T-1.md"),
            repo.tasks_dir().join("T-2.md"),
        )
        .await
        .expect("copy duplicate fixture");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("duplicate logical IDs must fail fast");
        let message = err.to_string();
        assert!(
            message.contains("duplicate") && message.contains("T-1"),
            "got {message}"
        );
    }

    #[tokio::test]
    async fn list_parallelizes_many_items_and_keeps_order() {
        // Verify that concurrent reads and rayon parsing preserve the results for many files.
        let (_dir, repo) = repo();
        let n = 200usize;
        let rs = ranks(n);
        for (i, rank) in (1..=n).zip(rs) {
            BacklogItemRepository::save(&repo, &item(i as u32, rank))
                .await
                .expect("save succeeds");
        }

        let items = BacklogItemRepository::list(&repo)
            .await
            .expect("list succeeds");
        let ids: Vec<u32> = items.iter().map(|i| i.id.number()).collect();
        let expected: Vec<u32> = (1..=n as u32).collect();
        assert_eq!(ids, expected, "並列読込・パースでも rank 昇順を保つ");
    }

    #[tokio::test]
    async fn list_on_uninitialized_dir_is_empty_not_error() {
        let (_dir, repo) = repo();
        let items = BacklogItemRepository::list(&repo)
            .await
            .expect("list must not error on missing dir");
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn delete_removes_file_and_missing_delete_errors() {
        let (_dir, repo) = repo();
        let item = sample_item();
        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save succeeds");

        BacklogItemRepository::delete(&repo, &item.id)
            .await
            .expect("delete succeeds");
        assert_eq!(
            BacklogItemRepository::load(&repo, &item.id)
                .await
                .expect_err("gone"),
            Error::NotFound(item.id.clone())
        );
        assert_eq!(
            BacklogItemRepository::delete(&repo, &item.id)
                .await
                .expect_err("already gone"),
            Error::NotFound(item.id)
        );
    }

    #[tokio::test]
    async fn next_id_does_not_reuse_a_physically_deleted_id() {
        let (_dir, repo) = repo();
        let item = sample_item();
        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save succeeds");
        BacklogItemRepository::delete(&repo, &item.id)
            .await
            .expect("delete succeeds");

        assert_eq!(
            repo.next_id("T").await.expect("next id"),
            ItemId::new("T", 2),
            "a physically deleted ID must remain reserved"
        );
    }

    #[tokio::test]
    async fn archive_moves_file_out_of_tasks_and_missing_archive_errors() {
        let (_dir, repo) = repo();
        let item = sample_item();
        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save succeeds");

        let dest = repo.archive(&item.id).await.expect("archive succeeds");

        // The item is no longer present in `tasks/`.
        assert_eq!(
            BacklogItemRepository::load(&repo, &item.id)
                .await
                .expect_err("gone from tasks"),
            Error::NotFound(item.id.clone())
        );
        // The archived file exists at the destination.
        assert!(dest.is_file(), "archived file exists at {dest:?}");
        assert!(
            dest.ends_with("archive/T-1.md"),
            "archived under archive dir: {dest:?}"
        );
        // Archiving the same item again returns `NotFound`.
        assert_eq!(
            BacklogItemRepository::archive(&repo, &item.id)
                .await
                .expect_err("already archived"),
            Error::NotFound(item.id)
        );
    }

    #[tokio::test]
    async fn archived_items_can_be_listed_loaded_and_restored_without_changes() {
        let (_dir, repo) = repo();
        let item = sample_item();
        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save succeeds");
        BacklogItemRepository::archive(&repo, &item.id)
            .await
            .expect("archive succeeds");

        assert_eq!(
            BacklogItemRepository::list_archived(&repo)
                .await
                .expect("list archived succeeds"),
            vec![item.clone()]
        );
        assert_eq!(
            BacklogItemRepository::load_archived(&repo, &item.id)
                .await
                .expect("load archived succeeds"),
            item
        );

        BacklogItemRepository::restore(&repo, &item.id)
            .await
            .expect("restore succeeds");
        assert_eq!(
            BacklogItemRepository::load(&repo, &item.id)
                .await
                .expect("restored item loads"),
            item
        );
        assert!(
            BacklogItemRepository::list_archived(&repo)
                .await
                .expect("list archived after restore")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn restore_refuses_an_active_destination_without_overwriting_either_copy() {
        let (_dir, repo) = repo();
        let archived = sample_item();
        BacklogItemRepository::save(&repo, &archived)
            .await
            .expect("save archived fixture");
        BacklogItemRepository::archive(&repo, &archived.id)
            .await
            .expect("archive fixture");
        let archive_path = repo.archive_dir().join("T-1.md");
        let archived_contents = fs::read(&archive_path)
            .await
            .expect("read archived fixture before collision");

        let mut active = archived.clone();
        active.title = "Active collision".to_string();
        let active_path = repo.tasks_dir().join("T-1.md");
        fs::create_dir_all(repo.tasks_dir())
            .await
            .expect("create tasks directory");
        fs::write(
            &active_path,
            crate::storage::markdown::to_markdown(&active).expect("serialize active collision"),
        )
        .await
        .expect("write active collision");

        let err = BacklogItemRepository::restore(&repo, &archived.id)
            .await
            .expect_err("restore collision must fail");
        assert!(err.to_string().contains("already exists"), "got {err}");
        assert_eq!(
            fs::read_to_string(&active_path)
                .await
                .expect("active copy remains"),
            crate::storage::markdown::to_markdown(&active).expect("serialize active collision")
        );
        assert_eq!(
            fs::read(&archive_path)
                .await
                .expect("archived copy remains"),
            archived_contents
        );
    }

    #[tokio::test]
    async fn next_id_increments_from_max_and_defaults_to_one() {
        let (_dir, repo) = repo();

        assert_eq!(repo.next_id("T").await.expect("empty"), ItemId::new("T", 1));

        for (n, rank) in [1u32, 2, 7].into_iter().zip(ranks(3)) {
            BacklogItemRepository::save(&repo, &item(n, rank))
                .await
                .expect("save succeeds");
        }
        assert_eq!(repo.next_id("T").await.expect("next"), ItemId::new("T", 8));
        // Different prefixes are numbered independently.
        assert_eq!(
            repo.next_id("BUG").await.expect("next"),
            ItemId::new("BUG", 1)
        );
    }

    #[tokio::test]
    async fn next_id_does_not_reuse_archived_ids() {
        // Archived IDs must not be reused, so `next_id` scans both `tasks/` and `archive/`.
        let (_dir, repo) = repo();

        // Create T-1, then archive it; it leaves `tasks/`.
        BacklogItemRepository::save(&repo, &item(1, ranks(1).remove(0)))
            .await
            .expect("save succeeds");
        repo.archive(&ItemId::new("T", 1))
            .await
            .expect("archive succeeds");

        // The next ID is T-2 because `archive/T-1.md` remains reserved.
        assert_eq!(
            repo.next_id("T").await.expect("next"),
            ItemId::new("T", 2),
            "archived id must not be reused"
        );
    }

    #[tokio::test]
    async fn archive_rejects_an_existing_destination_without_overwriting_it() {
        let (_dir, repo) = repo();
        let item = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save fixture");
        fs::create_dir_all(repo.archive_dir())
            .await
            .expect("create archive dir");
        fs::copy(
            repo.tasks_dir().join("T-1.md"),
            repo.archive_dir().join("T-1.md"),
        )
        .await
        .expect("create archive collision");

        let err = BacklogItemRepository::archive(&repo, &item.id)
            .await
            .expect_err("archive collision must fail fast");
        assert!(err.to_string().contains("already exists"), "got {err}");
        assert!(repo.tasks_dir().join("T-1.md").is_file());
        assert!(repo.archive_dir().join("T-1.md").is_file());
    }

    #[tokio::test]
    async fn save_rejects_an_archived_duplicate_before_creating_an_active_file() {
        let (_dir, repo) = repo();
        let item = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save fixture");
        BacklogItemRepository::archive(&repo, &item.id)
            .await
            .expect("archive fixture");

        let err = BacklogItemRepository::save(&repo, &item)
            .await
            .expect_err("saving an archived duplicate must fail fast");
        assert!(err.to_string().contains("already exists"), "got {err}");
        assert!(!repo.tasks_dir().join("T-1.md").exists());
        assert!(repo.archive_dir().join("T-1.md").is_file());
    }

    #[tokio::test]
    async fn next_id_rejects_filename_frontmatter_id_mismatch() {
        let (_dir, repo) = repo();
        BacklogItemRepository::save(&repo, &item(2, Rank::after(None)))
            .await
            .expect("save fixture");
        fs::rename(
            repo.tasks_dir().join("T-2.md"),
            repo.tasks_dir().join("T-1.md"),
        )
        .await
        .expect("rename corrupt fixture");

        let err = repo
            .next_id("T")
            .await
            .expect_err("next_id must validate existing records before allocating");
        assert!(err.to_string().contains("filename"), "got {err}");
    }

    #[tokio::test]
    async fn next_id_rejects_number_overflow_in_existing_files() {
        let (_dir, repo) = repo();
        let tasks = repo.tasks_dir();
        fs::create_dir_all(&tasks).await.expect("create tasks dir");
        let maximum = item(u32::MAX, Rank::after(None));
        let text = crate::storage::markdown::to_markdown(&maximum).expect("serialize maximum ID");
        fs::write(tasks.join("T-4294967295.md"), text)
            .await
            .expect("write maximum id fixture");

        let err = repo
            .next_id("T")
            .await
            .expect_err("the next id must not wrap around");
        assert!(err.to_string().contains("T-4294967295"));
    }

    #[tokio::test]
    async fn load_tolerates_crlf_delimiters() {
        let (_dir, repo) = repo();
        let dir = repo.tasks_dir();
        fs::create_dir_all(&dir).await.expect("mkdir");
        // Files edited on Windows with CRLF line endings remain readable.
        fs::write(
            dir.join("T-1.md"),
            "+++\r\nid = \"T-1\"\r\ntitle = \"CRLF\"\r\nstatus = \"todo\"\r\nrank = \"i\"\r\ncreated = \"1970-01-01T00:00:00Z\"\r\nupdated = \"1970-01-01T00:00:00Z\"\r\n+++\r\n",
        )
        .await
        .expect("write");

        let item = BacklogItemRepository::load(&repo, &ItemId::new("T", 1))
            .await
            .expect("load succeeds");
        assert_eq!(item.title, "CRLF");
    }

    #[tokio::test]
    async fn corrupt_frontmatter_returns_parse_error_without_panic() {
        let (_dir, repo) = repo();
        let dir = repo.tasks_dir();
        fs::create_dir_all(&dir).await.expect("mkdir");
        // The closing delimiter is present, but the frontmatter is invalid TOML.
        fs::write(dir.join("T-1.md"), "+++\nid = \nbroken\n+++\n\nbody")
            .await
            .expect("write");

        let err = BacklogItemRepository::load(&repo, &ItemId::new("T", 1))
            .await
            .expect_err("should fail to parse");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn unsafe_frontmatter_id_is_rejected_before_file_backend_uses_it() {
        let (_dir, repo) = repo();
        let dir = repo.tasks_dir();
        fs::create_dir_all(&dir).await.expect("mkdir");
        fs::write(
            dir.join("T-1.md"),
            r#"+++
id = "../outside-1"
title = "Unsafe"
status = "todo"
rank = "i"
created = "1970-01-01T00:00:00Z"
updated = "1970-01-01T00:00:00Z"
+++
"#,
        )
        .await
        .expect("write unsafe frontmatter");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("unsafe frontmatter ID must be rejected");
        assert!(err.to_string().contains("invalid item id"), "got {err:?}");
    }

    #[tokio::test]
    async fn missing_frontmatter_delimiter_returns_error_without_panic() {
        let (_dir, repo) = repo();
        let dir = repo.tasks_dir();
        fs::create_dir_all(&dir).await.expect("mkdir");
        fs::write(dir.join("T-1.md"), "no frontmatter here\njust text")
            .await
            .expect("write");

        let err = BacklogItemRepository::load(&repo, &ItemId::new("T", 1))
            .await
            .expect_err("should fail");
        assert!(
            matches!(err, Error::MissingFrontmatter { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn empty_title_on_load_returns_parse_error() {
        let (_dir, repo) = repo();
        let dir = repo.tasks_dir();
        fs::create_dir_all(&dir).await.expect("mkdir");
        // All required fields are present, but an empty title violates the model invariant.
        fs::write(
            dir.join("T-1.md"),
            "+++\nid = \"T-1\"\ntitle = \"\"\nstatus = \"todo\"\nrank = \"i\"\ncreated = \"1970-01-01T00:00:00Z\"\nupdated = \"1970-01-01T00:00:00Z\"\n+++\n",
        )
        .await
        .expect("write");

        let err = BacklogItemRepository::load(&repo, &ItemId::new("T", 1))
            .await
            .expect_err("should fail");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn missing_required_field_returns_parse_error() {
        let (_dir, repo) = repo();
        let dir = repo.tasks_dir();
        fs::create_dir_all(&dir).await.expect("mkdir");
        // Required fields such as `title` are missing.
        fs::write(dir.join("T-1.md"), "+++\nid = \"T-1\"\n+++\n\nbody")
            .await
            .expect("write");

        let err = BacklogItemRepository::load(&repo, &ItemId::new("T", 1))
            .await
            .expect_err("should fail");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }

    // --- Sprint persistence ---

    /// A sample sprint with goals, dates, and status.
    fn sample_sprint() -> Sprint {
        let mut s = Sprint::new(SprintId::new("S-1").unwrap(), "Sprint 1", ts(1_000)).unwrap();
        s.goal = "## ゴール\n\nログイン機能を完成させる".to_string();
        s.start = Some(ts(2_000));
        s.end = Some(ts(9_000));
        s.start(ts(2_000)).expect("planned -> active");
        s
    }

    #[tokio::test]
    async fn save_then_load_roundtrips_sprint() {
        let (_dir, repo) = repo();
        let sprint = sample_sprint();

        SprintRepository::save(&repo, &sprint)
            .await
            .expect("save succeeds");
        let loaded = SprintRepository::load(&repo, &sprint.id)
            .await
            .expect("load succeeds");

        assert_eq!(loaded, sprint);
    }

    #[tokio::test]
    async fn save_then_load_roundtrips_default_sprint() {
        // The default sprint (no schedule or goal, still planned) also round-trips without data loss.
        let (_dir, repo) = repo();
        let sprint = Sprint::new(SprintId::new("S-1").unwrap(), "Sprint 1", ts(1_000)).unwrap();

        SprintRepository::save(&repo, &sprint)
            .await
            .expect("save succeeds");
        let loaded = SprintRepository::load(&repo, &sprint.id)
            .await
            .expect("load succeeds");

        assert_eq!(loaded, sprint);
        assert_eq!(loaded.start, None);
        assert_eq!(loaded.end, None);
        assert_eq!(loaded.goal, "");
    }

    #[tokio::test]
    async fn sprint_frontmatter_carries_fields_and_goal_body() {
        let (_dir, repo) = repo();
        let sprint = sample_sprint();
        SprintRepository::save(&repo, &sprint)
            .await
            .expect("save succeeds");

        let text = fs::read_to_string(repo.sprints_dir().join("S-1.md"))
            .await
            .expect("read file");
        assert!(text.contains("id = \"S-1\""), "frontmatter carries id");
        assert!(
            text.contains("title = \"Sprint 1\""),
            "frontmatter carries title"
        );
        assert!(
            text.contains("state = \"active\""),
            "frontmatter carries state"
        );
        let frontmatter = text.split("+++\n").nth(1).expect("frontmatter exists");
        assert!(
            !frontmatter.contains("sprint_goal"),
            "goal is not a frontmatter field"
        );
        assert!(text.contains("## ゴール"), "goal is stored as body");
    }

    #[tokio::test]
    async fn load_missing_sprint_returns_not_found() {
        let (_dir, repo) = repo();
        let id = SprintId::new("S-99").unwrap();

        let err = SprintRepository::load(&repo, &id)
            .await
            .expect_err("should be missing");
        assert_eq!(err, Error::SprintNotFound(id));
    }

    #[tokio::test]
    async fn delete_sprint_removes_and_missing_is_not_found() {
        let (_dir, repo) = repo();
        let s = Sprint::new(SprintId::new("S-1").unwrap(), "S1", ts(1_000)).unwrap();
        SprintRepository::save(&repo, &s).await.expect("save");
        SprintRepository::delete(&repo, &s.id)
            .await
            .expect("delete");
        assert!(matches!(
            SprintRepository::load(&repo, &s.id).await,
            Err(Error::SprintNotFound(_))
        ));
        assert!(matches!(
            SprintRepository::delete(&repo, &s.id).await,
            Err(Error::SprintNotFound(_))
        ));
    }

    #[tokio::test]
    async fn list_sprints_returns_all_in_creation_order() {
        let (_dir, repo) = repo();
        // The list is oldest-first even when files are saved in a different order.
        for (id, secs) in [("S-3", 3_000i64), ("S-1", 1_000), ("S-2", 2_000)] {
            let s = Sprint::new(SprintId::new(id).unwrap(), id, ts(secs)).unwrap();
            SprintRepository::save(&repo, &s)
                .await
                .expect("save succeeds");
        }

        let ids: Vec<String> = SprintRepository::list(&repo)
            .await
            .expect("list succeeds")
            .into_iter()
            .map(|s| s.id.as_str().to_string())
            .collect();
        assert_eq!(ids, ["S-1", "S-2", "S-3"]);
    }

    #[tokio::test]
    async fn list_sprints_rejects_filename_frontmatter_id_mismatch() {
        let (_dir, repo) = repo();
        let sprint =
            Sprint::new(SprintId::new("S-1").unwrap(), "Sprint 1", ts(1_000)).expect("sprint");
        SprintRepository::save(&repo, &sprint)
            .await
            .expect("save fixture");
        fs::rename(
            repo.sprints_dir().join("S-1.md"),
            repo.sprints_dir().join("S-2.md"),
        )
        .await
        .expect("rename corrupt fixture");

        let err = SprintRepository::list(&repo)
            .await
            .expect_err("sprint filename/frontmatter mismatch must fail fast");
        let message = err.to_string();
        assert!(message.contains("filename"), "got {message}");
        assert!(
            message.contains("S-1") && message.contains("S-2"),
            "got {message}"
        );
    }

    #[tokio::test]
    async fn list_sprints_rejects_duplicate_logical_ids() {
        let (_dir, repo) = repo();
        for (id, title) in [("S-1", "First"), ("S-2", "Second")] {
            let sprint = Sprint::new(SprintId::new(id).unwrap(), title, ts(1_000)).expect("sprint");
            SprintRepository::save(&repo, &sprint)
                .await
                .expect("save fixture");
        }
        fs::copy(
            repo.sprints_dir().join("S-1.md"),
            repo.sprints_dir().join("S-2.md"),
        )
        .await
        .expect("copy duplicate fixture");

        let err = SprintRepository::list(&repo)
            .await
            .expect_err("duplicate sprint IDs must fail fast");
        let message = err.to_string();
        assert!(
            message.contains("duplicate") && message.contains("S-1"),
            "got {message}"
        );
    }

    #[tokio::test]
    async fn list_sprints_on_empty_board_returns_empty() {
        let (_dir, repo) = repo();
        assert!(
            BacklogItemRepository::list(&repo)
                .await
                .expect("list succeeds")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn corrupt_sprint_state_returns_parse_error() {
        let (_dir, repo) = repo();
        let dir = repo.sprints_dir();
        fs::create_dir_all(&dir).await.expect("mkdir");
        // An unknown state is reported as a parse error without panicking.
        fs::write(
            dir.join("S-1.md"),
            "+++\nid = \"S-1\"\ntitle = \"Sprint 1\"\nstate = \"archived\"\ncreated = \"1970-01-01T00:00:00Z\"\nupdated = \"1970-01-01T00:00:00Z\"\n+++\n",
        )
        .await
        .expect("write");

        let err = SprintRepository::load(&repo, &SprintId::new("S-1").unwrap())
            .await
            .expect_err("should fail");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    }
}
