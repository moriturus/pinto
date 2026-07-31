//! SQLite backend (optional feature `sqlite`).
//!
//! Store backlog items and sprints in one SQLite file (`<root>/board.sqlite3`) using a normalized
//! schema. This optional backend is useful when a board needs fast ID lookup, searching, or
//! aggregation over a large data set.
//!
//! **Storage format**: unlike the file and Git backends, which use Markdown with TOML frontmatter,
//! SQLite stores each domain field in a **typed column** and multi-value fields such as labels and
//! dependencies in **related tables**. This enables SQLite constraints and query facilities;
//! Markdown bodies are stored as text values, not as frontmatter documents.
//!
//! **Asynchronous boundary**: `rusqlite` is synchronous, so each operation runs inside
//! [`tokio::task::spawn_blocking`] to keep blocking I/O off async workers. Open a connection per
//! operation; this is simpler for short-lived CLI commands and matches SQLite's file locking.
//! Because values are read directly from typed columns, rayon is unnecessary.
//!
//! **Schema**: `<root>/board.sqlite3` contains active and archived items in `items`, distinguished
//! by the `archived` flag (the SQLite equivalent of `tasks/` and `archive/`), with multi-value data
//! in related tables.
//!
//! ```text
//! board.sqlite3
//! ├── items ── backlog item (archived=0: active / 1: archived)
//! │     ├─ id, prefix, number, title, status, rank, points, assignee,
//! │     │  sprint, parent, start_at, done_at, body, created, updated, archived
//! │     └─ PK(id)
//! ├── item_labels ── item labels (multi-valued)
//! │ └─ (item_id → items.id, label, position) PK(item_id, label) (duplicate labels prohibited)
//! ├── item_dependencies ── item dependencies (multi-valued)
//! │     └─ (item_id → items.id, depends_on, position)  PK(item_id, depends_on)
//! ├── item_commits ── Git commits associated with items (multi-valued)
//! │ └─ (item_id → items.id, sha, position) PK(item_id, sha) (duplicate SHA prohibited)
//! ├── item_action_sources ── Retro/Review source links for action PBIs
//! │ └─ (item_id → items.id, kind, sprint_id) PK(item_id)
//! ├── sprints ── sprint records
//!       └─ id, title, goal, state, close time, schedule, capacity, spillover, timestamps  PK(id)
//! └── metadata ── extensible key/value metadata
//!       └─ schema_version = "2", format = "pinto-sqlite"
//! ```
//!
//! - **Referential integrity**: related tables reference `items.id` with `ON DELETE CASCADE`, so
//!   deleting an item removes its labels, dependencies, and commits. Primary keys prevent duplicate
//!   labels and dependencies. The `parent`, `depends_on`, and `sprint` targets remain raw values,
//!   matching the file backend; the domain layer resolves and validates them.
//! - **Numbering**: `next_id` considers all items, including archived items, plus the shared
//!   issued-ID history, so deleted IDs are never reused.
//! - **Archive**: `archive` sets `items.archived` to `1`. It returns the logical display path
//!   `"<db>#archived/<id>"` because there is no separate archive file.

use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::sprint::Sprint;
use crate::sprint_record::SprintRecord;
use crate::storage::{
    FileRepository, SprintRecordRepository, WriteFailureInjector, record_issued_ids,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::{Path, PathBuf};

mod items;
mod records;
mod sprints;
#[cfg(test)]
mod tests;

/// [`BacklogItemRepository`](super::BacklogItemRepository) and
/// [`SprintRepository`](super::SprintRepository) implementation backed by SQLite.
#[derive(Debug, Clone)]
pub struct SqliteRepository {
    /// Board root (`.pinto/`); the database file is stored directly below it.
    root: PathBuf,
    /// Optional deterministic failure counter used by recovery tests.
    failure: WriteFailureInjector,
}

/// Current SQLite schema understood by this build.
const CURRENT_SCHEMA_VERSION: u32 = 2;
const SCHEMA_VERSION_KEY: &str = "schema_version";
const FORMAT_KEY: &str = "format";
const FORMAT_VALUE: &str = "pinto-sqlite";

/// Schema definition, applied when creating a new database (`IF NOT EXISTS`).
///
/// The database is created at version 2. The constraints cover values
/// that SQLite can validate directly; the loader below validates domain relationships that require
/// parsing or date arithmetic.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS items (
  id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
  prefix TEXT NOT NULL CHECK (length(prefix) > 0 AND prefix NOT GLOB '*[^A-Za-z]*'),
  number INTEGER NOT NULL CHECK (number BETWEEN 0 AND 4294967295),
  title TEXT NOT NULL CHECK (length(trim(title)) > 0),
  status TEXT NOT NULL CHECK (length(trim(status)) > 0),
  rank TEXT NOT NULL CHECK (length(rank) > 0 AND rank NOT GLOB '*[^0-9a-z]*' AND substr(rank, -1) <> '0'),
  points INTEGER CHECK (points IS NULL OR points BETWEEN 0 AND 4294967295),
  assignee TEXT,
  sprint TEXT,
  parent TEXT,
  start_at TEXT,
  done_at TEXT,
  body TEXT NOT NULL,
  created TEXT NOT NULL CHECK (length(trim(created)) > 0),
  updated TEXT NOT NULL CHECK (length(trim(updated)) > 0),
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1))
);
CREATE TABLE IF NOT EXISTS item_labels (
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  label TEXT NOT NULL CHECK (length(trim(label)) > 0),
  position INTEGER NOT NULL CHECK (position >= 0),
  PRIMARY KEY (item_id, label)
);
CREATE TABLE IF NOT EXISTS item_dependencies (
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  depends_on TEXT NOT NULL CHECK (length(trim(depends_on)) > 0),
  position INTEGER NOT NULL CHECK (position >= 0),
  PRIMARY KEY (item_id, depends_on)
);
CREATE TABLE IF NOT EXISTS item_commits (
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  sha TEXT NOT NULL CHECK (length(trim(sha)) > 0),
  position INTEGER NOT NULL CHECK (position >= 0),
  PRIMARY KEY (item_id, sha)
);
CREATE TABLE IF NOT EXISTS sprints (
  id TEXT PRIMARY KEY CHECK (length(id) > 0 AND id NOT GLOB '*[^A-Za-z0-9_-]*'),
  title TEXT NOT NULL CHECK (length(trim(title)) > 0),
  goal TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('planned', 'active', 'closed')),
  closed_at TEXT CHECK (closed_at IS NULL OR length(trim(closed_at)) > 0),
  start_at TEXT CHECK (start_at IS NULL OR length(trim(start_at)) > 0),
  end_at TEXT CHECK (end_at IS NULL OR length(trim(end_at)) > 0),
  daily_work_hours REAL CHECK (daily_work_hours IS NULL OR daily_work_hours >= 0),
  holiday_days INTEGER CHECK (holiday_days IS NULL OR holiday_days BETWEEN 0 AND 4294967295),
  deduction_factor REAL CHECK (deduction_factor IS NULL OR (deduction_factor >= 0 AND deduction_factor <= 1)),
  spillover_points INTEGER NOT NULL DEFAULT 0 CHECK (spillover_points BETWEEN 0 AND 4294967295),
  spillover_items INTEGER NOT NULL DEFAULT 0 CHECK (spillover_items BETWEEN 0 AND 4294967295),
  unestimated_spillover_items INTEGER NOT NULL DEFAULT 0 CHECK (unestimated_spillover_items BETWEEN 0 AND 4294967295),
  created TEXT NOT NULL CHECK (length(trim(created)) > 0),
  updated TEXT NOT NULL CHECK (length(trim(updated)) > 0),
  CHECK ((start_at IS NULL) = (end_at IS NULL)),
  CHECK ((daily_work_hours IS NULL AND holiday_days IS NULL AND deduction_factor IS NULL)
      OR (daily_work_hours IS NOT NULL AND holiday_days IS NOT NULL AND deduction_factor IS NOT NULL))
);
CREATE TABLE IF NOT EXISTS metadata (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

/// Additive schema extensions that can also be created for an existing supported database.
///
/// This table is deliberately separate from `sprints`: adding an outcome must not change the
/// existing Sprint table layout or the meaning of velocity and other historical reports.
const SPRINT_GOAL_OUTCOME_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sprint_goal_outcomes (
  sprint_id TEXT PRIMARY KEY REFERENCES sprints(id) ON DELETE CASCADE,
  achieved INTEGER NOT NULL CHECK (achieved IN (0, 1))
);
"#;

/// Additive source-link storage for action PBIs. It is kept separate from the versioned item
/// table so existing SQLite boards gain the optional relationship without a schema-version rewrite.
const ITEM_ACTION_SOURCE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS item_action_sources (
  item_id TEXT PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('retro', 'review')),
  sprint_id TEXT NOT NULL CHECK (length(trim(sprint_id)) > 0)
);
"#;

impl SqliteRepository {
    /// Build a repository for `.pinto/` without I/O; connections open during operations.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            failure: WriteFailureInjector::from_environment(),
        }
    }

    /// DB file path (`<root>/board.sqlite3`).
    pub(crate) fn db_path(&self) -> PathBuf {
        self.root.join("board.sqlite3")
    }

    /// Replace all active rows, archived rows, Sprint rows, and shared Markdown child records.
    ///
    /// The archive is cleared and rewritten alongside the active set so the snapshot fully mirrors
    /// the board, matching the file and Git backends. Active items are upserted (`archived = 0`) and
    /// archived items are upserted and then flagged `archived = 1` within the same transaction.
    pub(crate) async fn replace_board(
        &self,
        items: &[BacklogItem],
        archived_items: &[BacklogItem],
        sprints: &[Sprint],
        records: &[SprintRecord],
    ) -> Result<()> {
        let db = self.db_path();
        let items = items.to_vec();
        let archived_items = archived_items.to_vec();
        let sprints = sprints.to_vec();
        let records = records.to_vec();
        let issued_items = items.clone();
        let issued_archived = archived_items.clone();
        let failure = self.failure.clone();
        let old_ids = tokio::task::spawn_blocking(move || {
            let mut conn = open_conn(&db)?;
            let tx = conn.transaction().map_err(|e| sqlite_err(&db, &e))?;
            let old_ids = {
                let mut statement = tx
                    .prepare("SELECT id FROM items")
                    .map_err(|e| sqlite_err(&db, &e))?;
                statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| sqlite_err(&db, &e))?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(|e| sqlite_err(&db, &e))?
            };
            tx.execute("DELETE FROM items", [])
                .map_err(|e| sqlite_err(&db, &e))?;
            tx.execute("DELETE FROM sprints", [])
                .map_err(|e| sqlite_err(&db, &e))?;
            for item in &items {
                super::sqlite_repository::items::upsert_item(&db, &tx, item)?;
                failure.after_record_write(&db)?;
            }
            for item in &archived_items {
                super::sqlite_repository::items::upsert_item(&db, &tx, item)?;
                tx.execute(
                    "UPDATE items SET archived = 1 WHERE id = ?1",
                    [item.id.to_string()],
                )
                .map_err(|e| sqlite_err(&db, &e))?;
                failure.after_record_write(&db)?;
            }
            for sprint in &sprints {
                super::sqlite_repository::sprints::upsert_sprint(&db, &tx, sprint)?;
            }
            tx.commit().map_err(|e| sqlite_err(&db, &e))?;
            Ok::<_, Error>(old_ids)
        })
        .await
        .map_err(Error::task)??;

        let mut issued = old_ids
            .into_iter()
            .map(|id| {
                id.parse::<ItemId>().map_err(|error| {
                    Error::parse(
                        &self.db_path(),
                        format!("corrupt SQLite item ID during board replacement: {error}"),
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        issued.extend(issued_items.iter().map(|item| item.id.clone()));
        issued.extend(issued_archived.iter().map(|item| item.id.clone()));
        record_issued_ids(&self.root, &issued).await?;
        replace_child_records(&self.root, &records).await
    }
}

/// Replace child records that remain plain Markdown even when items and Sprints use SQLite.
async fn replace_child_records(root: &Path, records: &[SprintRecord]) -> Result<()> {
    let repository = FileRepository::new(root.to_path_buf());
    for kind in [
        crate::sprint_record::SprintRecordKind::Retro,
        crate::sprint_record::SprintRecordKind::Review,
    ] {
        for record in SprintRecordRepository::list(&repository, kind).await? {
            SprintRecordRepository::delete(&repository, kind, &record.id).await?;
        }
    }
    for record in records {
        SprintRecordRepository::save(&repository, record).await?;
    }
    Ok(())
}

/// Map a `rusqlite` error to [`Error::Io`] using the database path.
///
/// SQLite failures are reported as file I/O errors to avoid adding a backend-specific variant;
/// [`Error::is_user_error`] therefore treats them as internal errors.
fn sqlite_err(db_path: &Path, source: &rusqlite::Error) -> Error {
    Error::Io {
        path: db_path.to_path_buf(),
        message: source.to_string(),
    }
}

/// Construct a user-fixable parse error for a corrupt value read from a typed column.
fn corrupt(db_path: &Path, message: impl std::fmt::Display) -> Error {
    Error::parse(db_path, format!("corrupt SQLite data: {message}"))
}

/// Format a timestamp as RFC3339 for storage and consistency with the Git backend.
fn dt_to_str(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

/// Parse an RFC3339 value as UTC, returning a user-fixable error when it is corrupt.
fn dt_from_str(db: &Path, s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| corrupt(db, format!("invalid datetime {s:?}: {e}")))
}

/// Read a typed SQLite column and classify conversion failures as corrupt persisted data.
fn column<T: rusqlite::types::FromSql>(
    db: &Path,
    row: &Row<'_>,
    index: usize,
    name: &str,
) -> Result<T> {
    row.get(index)
        .map_err(|e| corrupt(db, format!("invalid {name} column: {e}")))
}

/// Read one extensible metadata value from the SQLite database.
fn read_metadata(db: &Path, conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
    .map_err(|e| corrupt(db, format!("invalid SQLite metadata {key:?}: {e}")))
}

/// Add a metadata value without overwriting a value written by another connection.
fn write_metadata_if_missing(db: &Path, conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO metadata (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map_err(|e| sqlite_err(db, &e))?;
    Ok(())
}

/// Return whether the database already contained any application table before initialization.
fn has_existing_tables(db: &Path, conn: &Connection) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%')",
        [],
        |row| row.get(0),
    )
    .map_err(|e| sqlite_err(db, &e))
}

/// Initialize metadata and reject existing databases from an unsupported schema generation.
fn ensure_metadata(db: &Path, conn: &Connection, had_existing_tables: bool) -> Result<()> {
    if !table_exists(db, conn, "metadata")? {
        return Err(Error::UnsupportedSqliteSchema {
            path: db.to_path_buf(),
            found: "missing".to_string(),
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    let Some(found) = read_metadata(db, conn, SCHEMA_VERSION_KEY)? else {
        if had_existing_tables {
            return Err(Error::UnsupportedSqliteSchema {
                path: db.to_path_buf(),
                found: "missing".to_string(),
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        write_metadata_if_missing(
            db,
            conn,
            SCHEMA_VERSION_KEY,
            &CURRENT_SCHEMA_VERSION.to_string(),
        )?;
        write_metadata_if_missing(db, conn, FORMAT_KEY, FORMAT_VALUE)?;
        return Ok(());
    };
    let version = found.parse::<u32>().ok();
    if version != Some(CURRENT_SCHEMA_VERSION) {
        return Err(Error::UnsupportedSqliteSchema {
            path: db.to_path_buf(),
            found,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }

    let Some(found_format) = read_metadata(db, conn, FORMAT_KEY)? else {
        if had_existing_tables {
            return Err(corrupt(db, "missing SQLite format metadata"));
        }
        write_metadata_if_missing(db, conn, FORMAT_KEY, FORMAT_VALUE)?;
        return Ok(());
    };
    if found_format != FORMAT_VALUE {
        return Err(corrupt(
            db,
            format!("invalid SQLite format metadata {found_format:?}"),
        ));
    }
    Ok(())
}

/// Return whether a named application table exists.
fn table_exists(db: &Path, conn: &Connection, name: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |row| row.get(0),
    )
    .map_err(|e| sqlite_err(db, &e))
}

/// Connect to the DB, enable foreign keys, and initialize a new database when needed.
///
/// If the parent directory (`.pinto/`) does not exist, create it ("works without configuration" principle. The file backend is
/// Same as creating `tasks/` when needed).
fn open_conn(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, &e))?;
    }
    let conn = Connection::open(db_path).map_err(|e| sqlite_err(db_path, &e))?;
    let had_existing_tables = has_existing_tables(db_path, &conn)?;
    let conn = conn;
    // Foreign key constraints are explicitly enabled for each connection (SQLite defaults to OFF).
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(|e| sqlite_err(db_path, &e))?;
    if !had_existing_tables {
        conn.execute_batch(SCHEMA)
            .map_err(|e| sqlite_err(db_path, &e))?;
    }
    ensure_metadata(db_path, &conn, had_existing_tables)?;
    conn.execute_batch(SPRINT_GOAL_OUTCOME_SCHEMA)
        .map_err(|e| sqlite_err(db_path, &e))?;
    conn.execute_batch(ITEM_ACTION_SOURCE_SCHEMA)
        .map_err(|e| sqlite_err(db_path, &e))?;
    Ok(conn)
}
