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

use super::issued_ids::{max_number, record};
use super::repository::{BacklogItemRepository, SprintRepository};
use crate::backlog::{BacklogItem, ItemId, Status};
use crate::error::{Error, Result};
use crate::rank::Rank;
use crate::sprint::{Sprint, SprintId, SprintSpillover, SprintState};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// [`BacklogItemRepository`] and [`SprintRepository`] implementation backed by SQLite.
#[derive(Debug, Clone)]
pub struct SqliteRepository {
    /// Board root (`.pinto/`); the database file is stored directly below it.
    root: PathBuf,
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

impl SqliteRepository {
    /// Build a repository for `.pinto/` without I/O; connections open during operations.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// DB file path (`<root>/board.sqlite3`).
    pub(crate) fn db_path(&self) -> PathBuf {
        self.root.join("board.sqlite3")
    }

    async fn load_item(&self, id: &ItemId, archived: bool) -> Result<BacklogItem> {
        let want = id.clone();
        let key = id.to_string();
        let db = self.db_path();
        let archived = if archived { 1 } else { 0 };
        tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            let sql =
                format!("SELECT {ITEM_COLUMNS} FROM items WHERE id = ?1 AND archived = {archived}");
            let scalar = conn
                .query_row(&sql, [&key], |row| Ok(item_row_from(&db, row)))
                .optional()
                .map_err(|e| sqlite_err(&db, &e))?;
            let Some(scalar) = scalar else {
                return Err(Error::NotFound(want));
            };
            let scalar = scalar?;
            let (labels, depends_on, commits) = load_relations(&db, &conn, &key)?;
            Ok(assemble_item(scalar, labels, depends_on, commits))
        })
        .await
        .map_err(Error::task)?
    }

    async fn list_items(&self, archived: bool) -> Result<Vec<BacklogItem>> {
        let db = self.db_path();
        let archived = if archived { 1 } else { 0 };
        let mut items = tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            let sql = format!("SELECT {ITEM_COLUMNS} FROM items WHERE archived = {archived}");
            let mut stmt = conn.prepare(&sql).map_err(|e| sqlite_err(&db, &e))?;
            let scalars = stmt
                .query_map([], |row| Ok(item_row_from(&db, row)))
                .map_err(|e| sqlite_err(&db, &e))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| sqlite_err(&db, &e))?
                .into_iter()
                .collect::<Result<Vec<_>>>()?;

            let labels = collect_relation(
                &db,
                &conn,
                &format!(
                    "SELECT item_id, label, position FROM item_labels WHERE item_id IN (SELECT id FROM items WHERE archived = {archived}) ORDER BY item_id, position"
                ),
                "label",
            )?;
            let mut deps_raw = collect_relation(
                &db,
                &conn,
                &format!(
                    "SELECT item_id, depends_on, position FROM item_dependencies WHERE item_id IN (SELECT id FROM items WHERE archived = {archived}) ORDER BY item_id, position"
                ),
                "depends_on",
            )?;
            let mut commits = collect_relation(
                &db,
                &conn,
                &format!(
                    "SELECT item_id, sha, position FROM item_commits WHERE item_id IN (SELECT id FROM items WHERE archived = {archived}) ORDER BY item_id, position"
                ),
                "commit sha",
            )?;

            let mut items = Vec::with_capacity(scalars.len());
            for scalar in scalars {
                let key = scalar.id.to_string();
                let item_labels = labels.get(&key).cloned().unwrap_or_default();
                let depends_on = deps_raw
                    .remove(&key)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|d| d.parse::<ItemId>())
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| corrupt(&db, format!("invalid depends_on id: {e}")))?;
                let item_commits = commits.remove(&key).unwrap_or_default();
                items.push(assemble_item(scalar, item_labels, depends_on, item_commits));
            }
            Ok::<_, Error>(items)
        })
        .await
        .map_err(Error::task)??;

        items.sort_by(BacklogItem::backlog_cmp);
        Ok(items)
    }
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
    Ok(conn)
}

/// Scalar column part of the `items` row (labels and dependencies are supplemented separately from related tables).
struct ItemRow {
    id: ItemId,
    title: String,
    status: Status,
    rank: Rank,
    points: Option<u32>,
    assignee: Option<String>,
    sprint: Option<String>,
    parent: Option<ItemId>,
    start_at: Option<DateTime<Utc>>,
    done_at: Option<DateTime<Utc>>,
    body: String,
    created: DateTime<Utc>,
    updated: DateTime<Utc>,
}

/// Copy one row of `items` to [`ItemRow`] (not including labels and dependencies).
///
/// Column order should match the `SELECT` statement ([`ITEM_COLUMNS`]).
fn item_row_from(db: &Path, row: &Row<'_>) -> Result<ItemRow> {
    let stored_id: String = column(db, row, 0, "item id")?;
    let prefix: String = column(db, row, 1, "item prefix")?;
    let number: i64 = column(db, row, 2, "item number")?;
    let title: String = column(db, row, 3, "item title")?;
    if title.trim().is_empty() {
        return Err(corrupt(db, "empty item title"));
    }
    let status: String = column(db, row, 4, "item status")?;
    if status.trim().is_empty() {
        return Err(corrupt(db, "empty item status"));
    }
    let rank: String = column(db, row, 5, "item rank")?;
    let points: Option<i64> = column(db, row, 6, "item points")?;
    let parent: Option<String> = column(db, row, 9, "item parent")?;
    let start_at: Option<String> = column(db, row, 10, "item start_at")?;
    let done_at: Option<String> = column(db, row, 11, "item done_at")?;
    let body: String = column(db, row, 12, "item body")?;
    let created: String = column(db, row, 13, "item created")?;
    let updated: String = column(db, row, 14, "item updated")?;
    let parse_dt_opt = |s: Option<String>| s.map(|v| dt_from_str(db, &v)).transpose();
    let number =
        u32::try_from(number).map_err(|_| corrupt(db, format!("invalid item number {number}")))?;
    let id = ItemId::try_new(&prefix, number)
        .map_err(|e| corrupt(db, format!("invalid item id prefix {prefix:?}: {e}")))?;
    if stored_id != id.to_string() {
        return Err(corrupt(
            db,
            format!("item id {stored_id:?} does not match prefix/number {id}"),
        ));
    }
    Ok(ItemRow {
        id,
        title,
        status: Status::new(status),
        rank: Rank::parse(&rank).map_err(|e| corrupt(db, format!("invalid rank {rank:?}: {e}")))?,
        points: points
            .map(|p| u32::try_from(p).map_err(|_| corrupt(db, format!("invalid points {p}"))))
            .transpose()?,
        assignee: column(db, row, 7, "item assignee")?,
        sprint: column(db, row, 8, "item sprint")?,
        parent: parent
            .map(|p| p.parse::<ItemId>())
            .transpose()
            .map_err(|e| corrupt(db, format!("invalid parent id: {e}")))?,
        start_at: parse_dt_opt(start_at)?,
        done_at: parse_dt_opt(done_at)?,
        body,
        created: dt_from_str(db, &created)?,
        updated: dt_from_str(db, &updated)?,
    })
}

/// A `SELECT` list containing the columns read by [`item_row_from`] in that order.
const ITEM_COLUMNS: &str = "id, prefix, number, title, status, rank, points, assignee, \
                            sprint, parent, start_at, done_at, body, created, updated";

/// Assemble [`BacklogItem`] from scalar lines + labels + dependencies + related commits.
fn assemble_item(
    scalar: ItemRow,
    labels: Vec<String>,
    depends_on: Vec<ItemId>,
    commits: Vec<String>,
) -> BacklogItem {
    BacklogItem {
        id: scalar.id,
        title: scalar.title,
        status: scalar.status,
        rank: scalar.rank,
        points: scalar.points,
        labels,
        assignee: scalar.assignee,
        sprint: scalar.sprint,
        parent: scalar.parent,
        depends_on,
        start_at: scalar.start_at,
        done_at: scalar.done_at,
        body: scalar.body,
        created: scalar.created,
        updated: scalar.updated,
        commits,
    }
}

/// For one PBI, read labels, dependencies, and related commits from related tables in order.
fn load_relations(
    db: &Path,
    conn: &Connection,
    id: &str,
) -> Result<(Vec<String>, Vec<ItemId>, Vec<String>)> {
    let labels = relation_values(
        db,
        conn,
        "SELECT label, position FROM item_labels WHERE item_id = ?1 ORDER BY position",
        id,
        "label",
    )?;
    let deps = relation_values(
        db,
        conn,
        "SELECT depends_on, position FROM item_dependencies WHERE item_id = ?1 ORDER BY position",
        id,
        "depends_on",
    )?;
    let depends_on = deps
        .into_iter()
        .map(|d| d.parse::<ItemId>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| corrupt(db, format!("invalid depends_on id: {e}")))?;
    let commits = relation_values(
        db,
        conn,
        "SELECT sha, position FROM item_commits WHERE item_id = ?1 ORDER BY position",
        id,
        "commit sha",
    )?;
    Ok((labels, depends_on, commits))
}

/// Read and validate the ordered values of one relation table for an item.
fn relation_values(
    db: &Path,
    conn: &Connection,
    sql: &str,
    id: &str,
    value_name: &str,
) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql).map_err(|e| sqlite_err(db, &e))?;
    let mut rows = stmt.query([id]).map_err(|e| sqlite_err(db, &e))?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().map_err(|e| sqlite_err(db, &e))? {
        let value: String = column(db, row, 0, value_name)?;
        let position: i64 = column(db, row, 1, &format!("{value_name} position"))?;
        if position < 0 {
            return Err(corrupt(
                db,
                format!("invalid {value_name} position {position}"),
            ));
        }
        if value.trim().is_empty() {
            return Err(corrupt(db, format!("empty {value_name}")));
        }
        values.push(value);
    }
    Ok(values)
}

/// Write the PBI body (`items`) and related tables (upsert) within the same transaction.
///
/// Since the target to be saved is an active PBI by definition (saved items do not appear in load/list and do not pass to save),
/// Upsert always returns `archived` to 0. As a result, the old save flag remains when file→sqlite is remigrated,
/// Prevent inconsistencies where items that should be mirrored continue to be hidden. Saving is performed only by [`SqliteRepository::archive`].
fn upsert_item(db: &Path, tx: &rusqlite::Transaction<'_>, item: &BacklogItem) -> Result<()> {
    let id = item.id.to_string();
    let number = i64::from(item.id.number());
    let points = item.points.map(i64::from);
    let map = |e: rusqlite::Error| sqlite_err(db, &e);
    tx.execute(
        "INSERT INTO items \
         (id, prefix, number, title, status, rank, points, assignee, sprint, parent, \
          start_at, done_at, body, created, updated) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) \
         ON CONFLICT(id) DO UPDATE SET \
          prefix = excluded.prefix, number = excluded.number, title = excluded.title, \
          status = excluded.status, rank = excluded.rank, points = excluded.points, \
          assignee = excluded.assignee, sprint = excluded.sprint, parent = excluded.parent, \
          start_at = excluded.start_at, done_at = excluded.done_at, body = excluded.body, \
          created = excluded.created, updated = excluded.updated, archived = 0",
        params![
            id,
            item.id.prefix(),
            number,
            item.title,
            item.status.as_str(),
            item.rank.as_str(),
            points,
            item.assignee,
            item.sprint,
            item.parent.as_ref().map(std::string::ToString::to_string),
            item.start_at.map(dt_to_str),
            item.done_at.map(dt_to_str),
            item.body,
            dt_to_str(item.created),
            dt_to_str(item.updated),
        ],
    )
    .map_err(map)?;

    // Multi-value relationships follow updates by "all deletion → reinsertion" (order is preserved by position).
    tx.execute("DELETE FROM item_labels WHERE item_id = ?1", [&id])
        .map_err(map)?;
    for (pos, label) in item.labels.iter().enumerate() {
        let position = position_i64(db, pos)?;
        tx.execute(
            "INSERT INTO item_labels (item_id, label, position) VALUES (?1, ?2, ?3)",
            params![id, label, position],
        )
        .map_err(map)?;
    }
    tx.execute("DELETE FROM item_dependencies WHERE item_id = ?1", [&id])
        .map_err(map)?;
    for (pos, dep) in item.depends_on.iter().enumerate() {
        let position = position_i64(db, pos)?;
        tx.execute(
            "INSERT INTO item_dependencies (item_id, depends_on, position) VALUES (?1, ?2, ?3)",
            params![id, dep.to_string(), position],
        )
        .map_err(map)?;
    }
    tx.execute("DELETE FROM item_commits WHERE item_id = ?1", [&id])
        .map_err(map)?;
    for (pos, sha) in item.commits.iter().enumerate() {
        let position = position_i64(db, pos)?;
        tx.execute(
            "INSERT INTO item_commits (item_id, sha, position) VALUES (?1, ?2, ?3)",
            params![id, sha, position],
        )
        .map_err(map)?;
    }
    Ok(())
}

/// Convert an in-memory relation index to SQLite's integer representation without wrapping.
fn position_i64(db: &Path, position: usize) -> Result<i64> {
    i64::try_from(position).map_err(|_| {
        corrupt(
            db,
            format!("relation position {position} overflows SQLite INTEGER"),
        )
    })
}

impl BacklogItemRepository for SqliteRepository {
    async fn save(&self, item: &BacklogItem) -> Result<()> {
        record(&self.root, &item.id).await?;
        let item = item.clone();
        let db = self.db_path();
        tokio::task::spawn_blocking(move || {
            let mut conn = open_conn(&db)?;
            let tx = conn.transaction().map_err(|e| sqlite_err(&db, &e))?;
            upsert_item(&db, &tx, &item)?;
            tx.commit().map_err(|e| sqlite_err(&db, &e))?;
            Ok(())
        })
        .await
        .map_err(Error::task)?
    }

    async fn load(&self, id: &ItemId) -> Result<BacklogItem> {
        self.load_item(id, false).await
    }

    async fn load_archived(&self, id: &ItemId) -> Result<BacklogItem> {
        self.load_item(id, true).await
    }

    async fn list(&self) -> Result<Vec<BacklogItem>> {
        self.list_items(false).await
    }

    async fn list_archived(&self) -> Result<Vec<BacklogItem>> {
        self.list_items(true).await
    }

    async fn delete(&self, id: &ItemId) -> Result<()> {
        let want = id.clone();
        let key = id.to_string();
        let db = self.db_path();
        tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            // Delete only active PBIs; `ON DELETE CASCADE` removes their related rows.
            let affected = conn
                .execute("DELETE FROM items WHERE id = ?1 AND archived = 0", [&key])
                .map_err(|e| sqlite_err(&db, &e))?;
            if affected == 0 {
                return Err(Error::NotFound(want));
            }
            Ok(())
        })
        .await
        .map_err(Error::task)??;
        record(&self.root, id).await
    }

    async fn archive(&self, id: &ItemId) -> Result<PathBuf> {
        let want = id.clone();
        let key = id.to_string();
        let db = self.db_path();
        let destination = tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            // Save active PBI = set `archived` flag (lines and relationships remain intact).
            let affected = conn
                .execute(
                    "UPDATE items SET archived = 1 WHERE id = ?1 AND archived = 0",
                    [&key],
                )
                .map_err(|e| sqlite_err(&db, &e))?;
            if affected == 0 {
                return Err(Error::NotFound(want));
            }
            // SQLite archives by setting `items.archived`, so return a logical display path rather
            // than a physical archive file.
            Ok(PathBuf::from(format!("{}#archived/{key}", db.display())))
        })
        .await
        .map_err(Error::task)??;
        record(&self.root, id).await?;
        Ok(destination)
    }

    async fn restore(&self, id: &ItemId) -> Result<()> {
        let want = id.clone();
        let key = id.to_string();
        let db = self.db_path();
        tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            let state = conn
                .query_row(
                    "SELECT archived FROM items WHERE id = ?1",
                    [&key],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|e| sqlite_err(&db, &e))?;
            match state {
                None => Err(Error::NotFound(want)),
                Some(0) => Err(Error::parse(
                    &db,
                    format!(
                        "cannot restore `{key}`: active item already exists; restore the archived copy only after resolving the ID collision"
                    ),
                )),
                Some(1) => {
                    conn.execute(
                        "UPDATE items SET archived = 0 WHERE id = ?1 AND archived = 1",
                        [&key],
                    )
                    .map_err(|e| sqlite_err(&db, &e))?;
                    Ok(())
                }
                Some(other) => Err(corrupt(
                    &db,
                    format!("invalid archived flag {other} for item {key}"),
                )),
            }
        })
        .await
        .map_err(Error::task)??;
        Ok(())
    }

    async fn next_id(&self, prefix: &str) -> Result<ItemId> {
        let issued_max = max_number(&self.root, prefix).await?;
        let prefix = prefix.to_string();
        let db = self.db_path();
        tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            // Since the ID is an invariant condition that "Once used, it will never be used again", even if it has been archived (archived=1).
            // Subtract the maximum number (reuse of saved ID = prevent collision).
            let max: Option<i64> = conn
                .query_row(
                    "SELECT MAX(number) FROM items WHERE prefix = ?1",
                    [&prefix],
                    |row| row.get(0),
                )
                .map_err(|e| sqlite_err(&db, &e))?;
            let max = max.unwrap_or(0).max(i64::from(issued_max));
            let next = max
                .checked_add(1)
                .ok_or_else(|| corrupt(&db, "item number overflow"))?;
            ItemId::try_new(
                &prefix,
                u32::try_from(next).map_err(|_| corrupt(&db, "item number overflow"))?,
            )
        })
        .await
        .map_err(Error::task)?
    }
}

/// Execute a query that returns `(item_id, value)` and bundle the values in order for each `item_id`.
fn collect_relation(
    db: &Path,
    conn: &Connection,
    sql: &str,
    value_name: &str,
) -> Result<HashMap<String, Vec<String>>> {
    let mut stmt = conn.prepare(sql).map_err(|e| sqlite_err(db, &e))?;
    let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
    let mut rows = stmt.query([]).map_err(|e| sqlite_err(db, &e))?;
    while let Some(row) = rows.next().map_err(|e| sqlite_err(db, &e))? {
        let item_id: String = column(db, row, 0, "relation item id")?;
        let value: String = column(db, row, 1, value_name)?;
        let position: i64 = column(db, row, 2, &format!("{value_name} position"))?;
        if position < 0 {
            return Err(corrupt(
                db,
                format!("invalid {value_name} position {position}"),
            ));
        }
        if value.trim().is_empty() {
            return Err(corrupt(db, format!("empty {value_name}")));
        }
        grouped.entry(item_id).or_default().push(value);
    }
    Ok(grouped)
}

/// Copy one line of `sprints` to [`Sprint`]. Column order should match the `SELECT` statement.
fn sprint_from(db: &Path, row: &Row<'_>) -> Result<Sprint> {
    let id: String = column(db, row, 0, "sprint id")?;
    let title: String = column(db, row, 1, "sprint title")?;
    if title.trim().is_empty() {
        return Err(corrupt(db, "empty sprint title"));
    }
    let goal: String = column(db, row, 2, "sprint goal")?;
    let state: String = column(db, row, 3, "sprint state")?;
    let closed_at: Option<String> = column(db, row, 4, "sprint closed_at")?;
    let start: Option<String> = column(db, row, 5, "sprint start_at")?;
    let end: Option<String> = column(db, row, 6, "sprint end_at")?;
    let daily_work_hours: Option<f64> = column(db, row, 7, "daily_work_hours")?;
    let holiday_raw: Option<i64> = column(db, row, 8, "holiday_days")?;
    let deduction_factor: Option<f64> = column(db, row, 9, "deduction_factor")?;
    let spillover_points_raw: i64 = column(db, row, 10, "spillover_points")?;
    let spillover_items_raw: i64 = column(db, row, 11, "spillover_items")?;
    let unestimated_spillover_items_raw: i64 = column(db, row, 12, "unestimated_spillover_items")?;
    let created: String = column(db, row, 13, "sprint created")?;
    let updated: String = column(db, row, 14, "sprint updated")?;
    let parse_dt_opt = |s: Option<String>| s.map(|v| dt_from_str(db, &v)).transpose();
    let closed_at = parse_dt_opt(closed_at)?;
    let start = parse_dt_opt(start)?;
    let end = parse_dt_opt(end)?;
    match (start, end) {
        (Some(start), Some(end)) if start > end => {
            return Err(corrupt(
                db,
                format!("invalid sprint period: start {start} is after end {end}"),
            ));
        }
        (Some(_), Some(_)) | (None, None) => {}
        _ => {
            return Err(corrupt(
                db,
                "sprint period must contain both start_at and end_at",
            ));
        }
    }
    let holiday_days = holiday_raw
        .map(|days| {
            u32::try_from(days).map_err(|_| corrupt(db, format!("invalid holiday days {days}")))
        })
        .transpose()?;
    let spillover = SprintSpillover {
        points: u32::try_from(spillover_points_raw).map_err(|_| {
            corrupt(
                db,
                format!("invalid spillover points {spillover_points_raw}"),
            )
        })?,
        items: u32::try_from(spillover_items_raw)
            .map_err(|_| corrupt(db, format!("invalid spillover items {spillover_items_raw}")))?,
        unestimated_items: u32::try_from(unestimated_spillover_items_raw).map_err(|_| {
            corrupt(
                db,
                format!("invalid unestimated spillover items {unestimated_spillover_items_raw}"),
            )
        })?,
    };
    if let Some(hours) = daily_work_hours
        && (!hours.is_finite() || hours < 0.0)
    {
        return Err(corrupt(db, format!("invalid daily work hours {hours}")));
    }
    if let Some(factor) = deduction_factor
        && (!factor.is_finite() || !(0.0..=1.0).contains(&factor))
    {
        return Err(corrupt(db, format!("invalid deduction factor {factor}")));
    }
    let has_daily = daily_work_hours.is_some();
    let has_holidays = holiday_days.is_some();
    let has_factor = deduction_factor.is_some();
    if has_daily || has_holidays || has_factor {
        if !(has_daily && has_holidays && has_factor) {
            return Err(corrupt(db, "sprint capacity fields must be set together"));
        }
        let (Some(start), Some(end), Some(holiday_days)) = (start, end, holiday_days) else {
            return Err(corrupt(
                db,
                "sprint capacity requires a complete start/end period",
            ));
        };
        let calendar_days = (end.date_naive() - start.date_naive())
            .num_days()
            .checked_add(1)
            .and_then(|days| u32::try_from(days).ok())
            .ok_or_else(|| corrupt(db, "sprint period is outside the supported date range"))?;
        if holiday_days > calendar_days {
            return Err(corrupt(
                db,
                format!(
                    "invalid holiday days {holiday_days}: period has {calendar_days} calendar days"
                ),
            ));
        }
    }
    Ok(Sprint {
        id: SprintId::new(id).map_err(|e| corrupt(db, format!("invalid sprint id: {e}")))?,
        title,
        goal,
        start,
        end,
        daily_work_hours,
        holiday_days,
        deduction_factor,
        spillover,
        state: state
            .parse::<SprintState>()
            .map_err(|e| corrupt(db, format!("invalid sprint state {state:?}: {e}")))?,
        closed_at,
        created: dt_from_str(db, &created)?,
        updated: dt_from_str(db, &updated)?,
    })
}

/// A `SELECT` list containing the columns read by [`sprint_from`] in that order.
const SPRINT_COLUMNS: &str = "id, title, goal, state, closed_at, start_at, end_at, daily_work_hours, holiday_days, deduction_factor, spillover_points, spillover_items, unestimated_spillover_items, created, updated";

impl SprintRepository for SqliteRepository {
    async fn save(&self, sprint: &Sprint) -> Result<()> {
        let sprint = sprint.clone();
        let db = self.db_path();
        tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            conn.execute(
                "INSERT INTO sprints (id, title, goal, state, closed_at, start_at, end_at, daily_work_hours, holiday_days, deduction_factor, spillover_points, spillover_items, unestimated_spillover_items, created, updated) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) \
                 ON CONFLICT(id) DO UPDATE SET \
                  title = excluded.title, goal = excluded.goal, state = excluded.state, closed_at = excluded.closed_at, \
                  start_at = excluded.start_at, end_at = excluded.end_at, \
                  daily_work_hours = excluded.daily_work_hours, holiday_days = excluded.holiday_days, deduction_factor = excluded.deduction_factor, \
                  spillover_points = excluded.spillover_points, spillover_items = excluded.spillover_items, \
                  unestimated_spillover_items = excluded.unestimated_spillover_items, \
                  created = excluded.created, updated = excluded.updated",
                params![
                    sprint.id.as_str(),
                    sprint.title,
                    sprint.goal,
                    sprint.state.as_str(),
                    sprint.closed_at.map(dt_to_str),
                    sprint.start.map(dt_to_str),
                    sprint.end.map(dt_to_str),
                    sprint.daily_work_hours,
                    sprint.holiday_days.map(i64::from),
                    sprint.deduction_factor,
                    i64::from(sprint.spillover.points),
                    i64::from(sprint.spillover.items),
                    i64::from(sprint.spillover.unestimated_items),
                    dt_to_str(sprint.created),
                    dt_to_str(sprint.updated),
                ],
            )
            .map_err(|e| sqlite_err(&db, &e))?;
            Ok(())
        })
        .await
        .map_err(Error::task)?
    }

    async fn load(&self, id: &SprintId) -> Result<Sprint> {
        let want = id.clone();
        let key = id.as_str().to_string();
        let db = self.db_path();
        tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            let sql = format!("SELECT {SPRINT_COLUMNS} FROM sprints WHERE id = ?1");
            let sprint = conn
                .query_row(&sql, [&key], |row| Ok(sprint_from(&db, row)))
                .optional()
                .map_err(|e| sqlite_err(&db, &e))?;
            match sprint {
                Some(s) => s,
                None => Err(Error::SprintNotFound(want)),
            }
        })
        .await
        .map_err(Error::task)?
    }

    async fn list(&self) -> Result<Vec<Sprint>> {
        let db = self.db_path();
        let mut sprints = tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            let sql = format!("SELECT {SPRINT_COLUMNS} FROM sprints");
            let mut stmt = conn.prepare(&sql).map_err(|e| sqlite_err(&db, &e))?;
            let sprints = stmt
                .query_map([], |row| Ok(sprint_from(&db, row)))
                .map_err(|e| sqlite_err(&db, &e))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| sqlite_err(&db, &e))?
                .into_iter()
                .collect::<Result<Vec<_>>>()?;
            Ok::<_, Error>(sprints)
        })
        .await
        .map_err(Error::task)??;

        // Match the file backend: sort by creation time, then use the ID as a tie-breaker.
        sprints.sort_by(|a, b| {
            a.created
                .cmp(&b.created)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        Ok(sprints)
    }

    async fn delete(&self, id: &SprintId) -> Result<()> {
        let want = id.clone();
        let key = id.as_str().to_string();
        let db = self.db_path();
        tokio::task::spawn_blocking(move || {
            let conn = open_conn(&db)?;
            let affected = conn
                .execute("DELETE FROM sprints WHERE id = ?1", [&key])
                .map_err(|e| sqlite_err(&db, &e))?;
            if affected == 0 {
                return Err(Error::SprintNotFound(want));
            }
            Ok(())
        })
        .await
        .map_err(Error::task)?
    }
}

#[cfg(test)]
mod tests {
    //! Persistence test for `SqliteRepository`. Same behavior as the file backend equivalent test
    //! Confirm that SQLite satisfies the same requirements as the file backend, including rank
    //! order, NotFound errors, and ID preservation when archiving.

    use super::*;
    use crate::backlog::Status;
    use crate::rank::Rank;
    use crate::sprint::Sprint;
    use chrono::{DateTime, TimeZone, Utc};
    use tempfile::TempDir;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0)
            .single()
            .expect("valid timestamp")
    }

    /// Create a SQLite Repository targeting `.pinto` in the temporary directory.
    fn repo() -> (TempDir, SqliteRepository) {
        let dir = TempDir::new().expect("create temp dir");
        let repo = SqliteRepository::new(dir.path().join(".pinto"));
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

    /// PBI with all fields filled in (including multi-value, optional fields, and body text).
    fn full_item() -> BacklogItem {
        let mut it = item(7, Rank::after(None));
        it.title = "Full item".to_string();
        it.status = Status::new("in-progress");
        it.points = Some(5);
        it.labels = vec!["backend".to_string(), "urgent".to_string()];
        it.assignee = Some("alice".to_string());
        it.sprint = Some("S-1".to_string());
        it.parent = Some(ItemId::new("T", 1));
        it.depends_on = vec![ItemId::new("T", 2), ItemId::new("T", 3)];
        it.start_at = Some(ts(2_000));
        it.done_at = Some(ts(3_000));
        it.commits = vec!["abc1234".to_string(), "def5678".to_string()];
        it.body = "Acceptance\n- one\n- two".to_string();
        it.updated = ts(4_000);
        it
    }

    /// Open DB synchronously for new schema validation (raw access for testing only).
    fn raw(repo: &SqliteRepository) -> Connection {
        Connection::open(repo.db_path()).expect("open db")
    }

    #[tokio::test]
    async fn new_database_records_schema_metadata() {
        let (_dir, repo) = repo();
        let items = BacklogItemRepository::list(&repo)
            .await
            .expect("initialize and list");
        assert!(items.is_empty());

        let conn = raw(&repo);
        let version: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .expect("schema version metadata");
        let format: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'format'",
                [],
                |row| row.get(0),
            )
            .expect("format metadata");
        assert_eq!(version, "2");
        assert_eq!(format, "pinto-sqlite");
    }

    #[tokio::test]
    async fn existing_database_with_missing_format_metadata_is_rejected() {
        let (_dir, repo) = repo();
        BacklogItemRepository::list(&repo)
            .await
            .expect("initialize");
        raw(&repo)
            .execute("DELETE FROM metadata WHERE key = 'format'", [])
            .expect("remove format metadata");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("existing database without format metadata must be rejected");
        assert!(err.to_string().contains("missing SQLite format metadata"));
    }

    #[tokio::test]
    async fn malformed_schema_metadata_is_reported_as_corruption() {
        let (_dir, repo) = repo();
        BacklogItemRepository::list(&repo)
            .await
            .expect("initialize");
        raw(&repo)
            .execute(
                "UPDATE metadata SET value = X'01' WHERE key = 'schema_version'",
                [],
            )
            .expect("corrupt schema metadata");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("malformed schema metadata must be rejected");
        assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
        assert!(err.to_string().contains("invalid SQLite metadata"));
    }

    #[tokio::test]
    async fn existing_database_without_schema_metadata_is_rejected() {
        let (_dir, repo) = repo();
        let item = full_item();
        BacklogItemRepository::save(&repo, &item)
            .await
            .expect("save");

        // Simulate a database created before metadata was introduced.
        let conn = raw(&repo);
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL); DROP TABLE metadata;",
        )
        .expect("simulate legacy schema");
        drop(conn);

        let err = BacklogItemRepository::load(&repo, &item.id)
            .await
            .expect_err("schema metadata is required for an existing database");
        assert!(err.to_string().contains("found version \"missing\""));
        assert!(err.to_string().contains("version 2"));
        let conn = raw(&repo);
        let metadata_exists: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'metadata')",
                [],
                |row| row.get(0),
            )
            .expect("inspect metadata table");
        assert!(
            !metadata_exists,
            "rejected databases must not be initialized"
        );
    }

    #[tokio::test]
    async fn metadata_keeps_future_key_value_extensions() {
        let (_dir, repo) = repo();
        BacklogItemRepository::list(&repo)
            .await
            .expect("initialize");
        let conn = raw(&repo);
        conn.execute(
            "INSERT INTO metadata (key, value) VALUES ('created_by', 'pinto-test')",
            [],
        )
        .expect("write extension metadata");
        drop(conn);

        BacklogItemRepository::list(&repo)
            .await
            .expect("read metadata");
        let conn = raw(&repo);
        let value: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'created_by'",
                [],
                |row| row.get(0),
            )
            .expect("extension metadata remains");
        assert_eq!(value, "pinto-test");
    }

    #[tokio::test]
    async fn unsupported_schema_version_returns_actionable_error() {
        let (_dir, repo) = repo();
        BacklogItemRepository::list(&repo)
            .await
            .expect("initialize");
        for found in ["1", "99", "not-a-version"] {
            let conn = raw(&repo);
            conn.execute(
                "UPDATE metadata SET value = ?1 WHERE key = 'schema_version'",
                [found],
            )
            .expect("set unsupported version");
            drop(conn);

            let error = BacklogItemRepository::list(&repo)
                .await
                .expect_err("unsupported schema must not be opened");
            let message = error.to_string();
            assert!(
                message.contains(found),
                "reports stored version {found:?}: {message}"
            );
            assert!(
                message.contains("version 2"),
                "reports supported version: {message}"
            );
            assert!(
                message.contains("upgrade") || message.contains("recreate"),
                "explains how to recover: {message}"
            );
            assert!(
                message.contains("migration"),
                "states that automatic migration is unavailable: {message}"
            );
        }
    }

    // --- Normalization Schema---

    #[tokio::test]
    async fn full_item_roundtrips_all_fields() {
        let (_dir, repo) = repo();
        let it = full_item();
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        let loaded = BacklogItemRepository::load(&repo, &it.id)
            .await
            .expect("load");
        assert_eq!(loaded, it);
    }

    #[tokio::test]
    async fn items_table_has_typed_columns_not_markdown() {
        let (_dir, repo) = repo();
        BacklogItemRepository::save(&repo, &full_item())
            .await
            .expect("save");
        let conn = raw(&repo);
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('items')")
            .expect("prepare")
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("collect");
        assert!(
            !cols.iter().any(|c| c == "markdown"),
            "markdown blob 列は廃止されている: {cols:?}"
        );
        for expected in [
            "id", "title", "status", "rank", "points", "assignee", "archived",
        ] {
            assert!(
                cols.iter().any(|c| c == expected),
                "型付き列 {expected} が存在する: {cols:?}"
            );
        }
    }

    #[tokio::test]
    async fn labels_stored_in_relation_table_in_order() {
        let (_dir, repo) = repo();
        let it = full_item();
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        let conn = raw(&repo);
        let labels: Vec<String> = conn
            .prepare("SELECT label FROM item_labels WHERE item_id = ?1 ORDER BY position")
            .expect("prepare")
            .query_map(["T-7"], |r| r.get::<_, String>(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("collect");
        assert_eq!(labels, it.labels);
    }

    #[tokio::test]
    async fn dependencies_stored_in_relation_table_in_order() {
        let (_dir, repo) = repo();
        let it = full_item();
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        let conn = raw(&repo);
        let deps: Vec<String> = conn
            .prepare(
                "SELECT depends_on FROM item_dependencies WHERE item_id = ?1 ORDER BY position",
            )
            .expect("prepare")
            .query_map(["T-7"], |r| r.get::<_, String>(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("collect");
        assert_eq!(deps, vec!["T-2".to_string(), "T-3".to_string()]);
    }

    #[tokio::test]
    async fn duplicate_label_rejected_by_constraint() {
        let (_dir, repo) = repo();
        BacklogItemRepository::save(&repo, &full_item())
            .await
            .expect("save");
        let conn = raw(&repo);
        // Duplicate labels are rejected by the unique constraint (item_id, label).
        let err = conn.execute(
            "INSERT INTO item_labels (item_id, label, position) VALUES ('T-7', 'backend', 9)",
            [],
        );
        assert!(err.is_err(), "重複ラベルは一意制約で拒否される");
    }

    #[tokio::test]
    async fn deleting_item_cascades_relations() {
        let (_dir, repo) = repo();
        let it = full_item();
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        BacklogItemRepository::delete(&repo, &it.id)
            .await
            .expect("delete");
        let conn = raw(&repo);
        let label_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM item_labels WHERE item_id = 'T-7'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        let dep_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM item_dependencies WHERE item_id = 'T-7'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(label_count, 0, "削除で関連ラベルもカスケード削除される");
        assert_eq!(dep_count, 0, "削除で関連依存もカスケード削除される");
    }

    #[tokio::test]
    async fn archive_sets_flag_and_keeps_row() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        BacklogItemRepository::archive(&repo, &it.id)
            .await
            .expect("archive");
        let conn = raw(&repo);
        let archived: i64 = conn
            .query_row("SELECT archived FROM items WHERE id = 'T-1'", [], |r| {
                r.get(0)
            })
            .expect("row still present");
        assert_eq!(archived, 1, "アーカイブ状態は archived 列で表現される");
    }

    #[tokio::test]
    async fn archived_items_can_be_listed_loaded_and_restored_without_changes() {
        let (_dir, repo) = repo();
        let it = full_item();
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        BacklogItemRepository::archive(&repo, &it.id)
            .await
            .expect("archive");

        assert_eq!(
            BacklogItemRepository::list_archived(&repo)
                .await
                .expect("list archived"),
            vec![it.clone()]
        );
        assert_eq!(
            BacklogItemRepository::load_archived(&repo, &it.id)
                .await
                .expect("load archived"),
            it
        );
        BacklogItemRepository::restore(&repo, &it.id)
            .await
            .expect("restore");
        assert_eq!(
            BacklogItemRepository::load(&repo, &it.id)
                .await
                .expect("load restored"),
            it
        );
        assert!(
            BacklogItemRepository::list_archived(&repo)
                .await
                .expect("list archived after restore")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn saving_over_an_archived_id_reactivates_it() {
        // The save target is always active. When you resave the saved ID, archived returns to 0,
        // It reappears in the list (preventing ghost hiding when re-migrating from file to sqlite).
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        BacklogItemRepository::archive(&repo, &it.id)
            .await
            .expect("archive");
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("re-save");
        let listed = BacklogItemRepository::list(&repo).await.expect("list");
        assert_eq!(listed.len(), 1, "再保存でアクティブへ戻る");
        assert_eq!(listed[0].id, it.id);
    }

    #[tokio::test]
    async fn save_replaces_labels_on_update() {
        let (_dir, repo) = repo();
        let mut it = full_item();
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        it.labels = vec!["frontend".to_string()];
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        let loaded = BacklogItemRepository::load(&repo, &it.id)
            .await
            .expect("load");
        assert_eq!(loaded.labels, vec!["frontend".to_string()]);
    }

    #[tokio::test]
    async fn sprint_roundtrips_all_fields() {
        let (_dir, repo) = repo();
        let mut s = Sprint::new(SprintId::new("S-1").unwrap(), "Sprint One", ts(1_000)).unwrap();
        s.goal = "Ship the MVP\nwith tests".to_string();
        s.state = crate::sprint::SprintState::Closed;
        s.closed_at = Some(ts(4_000));
        s.start = Some(ts(1_000));
        s.end = Some(ts(9_000));
        s.spillover = crate::sprint::SprintSpillover {
            points: 8,
            items: 2,
            unestimated_items: 1,
        };
        s.updated = ts(5_000);
        SprintRepository::save(&repo, &s).await.expect("save");
        let loaded = SprintRepository::load(&repo, &s.id).await.expect("load");
        assert_eq!(loaded, s);
        let conn = raw(&repo);
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('sprints')")
            .expect("prepare")
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("collect");
        assert!(
            !cols.iter().any(|c| c == "markdown"),
            "sprints も markdown blob を廃止: {cols:?}"
        );
        assert!(
            cols.iter().any(|c| c == "state"),
            "state 列を持つ: {cols:?}"
        );
        assert!(
            cols.iter().any(|column| column == "closed_at"),
            "closed_at column exists: {cols:?}"
        );
        for expected in [
            "spillover_points",
            "spillover_items",
            "unestimated_spillover_items",
        ] {
            assert!(
                cols.iter().any(|column| column == expected),
                "spillover column {expected} exists: {cols:?}"
            );
        }
    }

    #[tokio::test]
    async fn save_then_load_roundtrips() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("save succeeds");
        let loaded = BacklogItemRepository::load(&repo, &it.id)
            .await
            .expect("load succeeds");
        assert_eq!(loaded, it);
    }

    #[tokio::test]
    async fn schema_rejects_non_alphabetic_item_prefix() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("save succeeds");
        let conn = raw(&repo);
        for prefix in ["123", "p9", "bug_fix", "PROJ-1"] {
            let err = conn.execute("UPDATE items SET prefix = ?1 WHERE id = 'T-1'", [prefix]);
            assert!(err.is_err(), "prefix {prefix:?} must be rejected");
        }
    }

    #[tokio::test]
    async fn corrupted_prefix_is_rejected_instead_of_becoming_a_path_component() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("save succeeds");
        let conn = raw(&repo);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute(
            "UPDATE items SET id = ?1, prefix = ?2 WHERE id = 'T-1'",
            rusqlite::params!["../outside-1", "../outside"],
        )
        .expect("corrupt row");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("corrupt prefix must be rejected");
        assert!(err.to_string().contains("invalid item id prefix"));
    }

    #[tokio::test]
    async fn corrupted_stored_id_is_rejected_when_it_disagrees_with_components() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("save succeeds");
        raw(&repo)
            .execute("UPDATE items SET id = 'T-99' WHERE id = 'T-1'", [])
            .expect("corrupt stored id");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("mismatched stored id must be rejected");
        assert!(err.to_string().contains("does not match prefix/number"));
    }

    #[tokio::test]
    async fn corrupted_negative_item_number_is_rejected() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("save succeeds");
        let conn = raw(&repo);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute("UPDATE items SET number = -1 WHERE id = 'T-1'", [])
            .expect("corrupt item number");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("negative item number must be rejected");
        assert!(err.to_string().contains("invalid item number"));
    }

    #[tokio::test]
    async fn corrupted_negative_points_are_rejected() {
        let (_dir, repo) = repo();
        let mut it = item(1, Rank::after(None));
        it.points = Some(3);
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("save succeeds");
        let conn = raw(&repo);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute("UPDATE items SET points = -1 WHERE id = 'T-1'", [])
            .expect("corrupt points");

        let err = BacklogItemRepository::list(&repo)
            .await
            .expect_err("negative points must be rejected");
        assert!(err.to_string().contains("invalid points"));
    }

    #[tokio::test]
    async fn corrupted_empty_item_title_is_rejected_by_the_loader() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("save succeeds");
        let conn = raw(&repo);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute("UPDATE items SET title = '   ' WHERE id = 'T-1'", [])
            .expect("corrupt item title");

        let err = BacklogItemRepository::load(&repo, &it.id)
            .await
            .expect_err("a blank item title must be rejected");
        assert!(err.to_string().contains("empty item title"));
    }

    #[tokio::test]
    async fn corrupted_sprint_period_and_capacity_values_are_rejected() {
        let (_dir, repo) = repo();
        let mut sprint =
            Sprint::new(SprintId::new("S-1").unwrap(), "Sprint", ts(1_000)).expect("valid sprint");
        sprint.start = Some(ts(1_000));
        sprint.end = Some(ts(9_000));
        sprint.daily_work_hours = Some(8.0);
        sprint.holiday_days = Some(1);
        sprint.deduction_factor = Some(0.8);
        SprintRepository::save(&repo, &sprint).await.expect("save");

        let conn = raw(&repo);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute(
            "UPDATE sprints SET start_at = '1970-01-01T02:30:00Z', end_at = '1970-01-01T00:16:40Z' WHERE id = 'S-1'",
            [],
        )
        .expect("corrupt sprint period");
        drop(conn);

        let err = SprintRepository::load(&repo, &sprint.id)
            .await
            .expect_err("inverted sprint period must be rejected");
        assert!(err.to_string().contains("invalid sprint period"));

        let conn = raw(&repo);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute(
            "UPDATE sprints SET start_at = '1970-01-01T00:16:40Z', end_at = '1970-01-01T02:30:00Z', holiday_days = 2 WHERE id = 'S-1'",
            [],
        )
        .expect("corrupt sprint capacity");
        drop(conn);

        let err = SprintRepository::load(&repo, &sprint.id)
            .await
            .expect_err("excessive holidays must be rejected");
        assert!(err.to_string().contains("invalid holiday days"));
    }

    #[tokio::test]
    async fn corrupted_empty_item_title_is_rejected() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it)
            .await
            .expect("save succeeds");
        let err = raw(&repo)
            .execute("UPDATE items SET title = '   ' WHERE id = 'T-1'", [])
            .expect_err("schema must reject a blank item title");
        assert!(err.to_string().contains("CHECK"));
    }

    #[tokio::test]
    async fn corrupted_negative_sprint_holidays_are_rejected_as_corruption() {
        let (_dir, repo) = repo();
        let mut sprint =
            Sprint::new(SprintId::new("S-1").unwrap(), "Sprint", ts(1_000)).expect("valid sprint");
        sprint.start = Some(ts(1_000));
        sprint.end = Some(ts(9_000));
        sprint.daily_work_hours = Some(8.0);
        sprint.holiday_days = Some(1);
        sprint.deduction_factor = Some(0.8);
        SprintRepository::save(&repo, &sprint).await.expect("save");

        let conn = raw(&repo);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute("UPDATE sprints SET holiday_days = -1 WHERE id = 'S-1'", [])
            .expect("corrupt holiday count");

        let err = SprintRepository::load(&repo, &sprint.id)
            .await
            .expect_err("negative holiday count must be rejected");
        assert!(err.to_string().contains("invalid holiday days"));
    }

    #[tokio::test]
    async fn corrupted_sprint_title_and_capacity_relation_are_rejected() {
        let (_dir, repo) = repo();
        let mut sprint =
            Sprint::new(SprintId::new("S-1").unwrap(), "Sprint", ts(1_000)).expect("valid sprint");
        sprint.start = Some(ts(1_000));
        sprint.end = Some(ts(9_000));
        sprint.daily_work_hours = Some(8.0);
        sprint.holiday_days = Some(1);
        sprint.deduction_factor = Some(0.8);
        SprintRepository::save(&repo, &sprint).await.expect("save");

        let conn = raw(&repo);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute(
            "UPDATE sprints SET title = ' ', daily_work_hours = NULL WHERE id = 'S-1'",
            [],
        )
        .expect("corrupt sprint row");

        let err = SprintRepository::list(&repo)
            .await
            .expect_err("invalid sprint row must be rejected");
        assert!(err.to_string().contains("empty sprint title"));
    }

    #[tokio::test]
    async fn save_overwrites_existing_id() {
        let (_dir, repo) = repo();
        let mut it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        it.title = "Renamed".to_string();
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        let loaded = BacklogItemRepository::load(&repo, &it.id)
            .await
            .expect("load");
        assert_eq!(loaded.title, "Renamed");
        // Since it is overwritten, the number remains 1.
        assert_eq!(
            BacklogItemRepository::list(&repo)
                .await
                .expect("list")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn load_missing_returns_not_found() {
        let (_dir, repo) = repo();
        let err = BacklogItemRepository::load(&repo, &ItemId::new("T", 99))
            .await
            .expect_err("missing");
        assert_eq!(err, Error::NotFound(ItemId::new("T", 99)));
    }

    #[tokio::test]
    async fn list_on_uninitialized_is_empty_not_error() {
        let (_dir, repo) = repo();
        let items = BacklogItemRepository::list(&repo).await.expect("list");
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn list_returns_all_items_sorted_by_rank() {
        let (_dir, repo) = repo();
        let order = [3u32, 1, 10, 2];
        let rs = ranks(order.len());
        for (n, rank) in order.iter().zip(rs) {
            BacklogItemRepository::save(&repo, &item(*n, rank))
                .await
                .expect("save");
        }
        let items = BacklogItemRepository::list(&repo).await.expect("list");
        let ids: Vec<u32> = items.iter().map(|i| i.id.number()).collect();
        assert_eq!(ids, order.to_vec(), "rank 昇順（= 割当順）で返る");
    }

    #[tokio::test]
    async fn delete_removes_and_missing_is_not_found() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        BacklogItemRepository::delete(&repo, &it.id)
            .await
            .expect("delete");
        assert!(matches!(
            BacklogItemRepository::load(&repo, &it.id).await,
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            BacklogItemRepository::delete(&repo, &it.id).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn next_id_does_not_reuse_a_physically_deleted_id() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        BacklogItemRepository::delete(&repo, &it.id)
            .await
            .expect("delete");

        assert_eq!(
            BacklogItemRepository::next_id(&repo, "T")
                .await
                .expect("next id"),
            ItemId::new("T", 2),
            "a physically deleted ID must remain reserved"
        );
    }

    #[tokio::test]
    async fn archive_moves_item_out_of_active_list() {
        let (_dir, repo) = repo();
        let it = item(1, Rank::after(None));
        BacklogItemRepository::save(&repo, &it).await.expect("save");
        BacklogItemRepository::archive(&repo, &it.id)
            .await
            .expect("archive");
        // After archiving, the item is absent from the active list and load operations.
        assert!(
            BacklogItemRepository::list(&repo)
                .await
                .expect("list")
                .is_empty()
        );
        assert!(matches!(
            BacklogItemRepository::load(&repo, &it.id).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn archive_missing_is_not_found() {
        let (_dir, repo) = repo();
        assert!(matches!(
            BacklogItemRepository::archive(&repo, &ItemId::new("T", 5)).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn next_id_increments_and_skips_archived() {
        let (_dir, repo) = repo();
        assert_eq!(
            BacklogItemRepository::next_id(&repo, "T")
                .await
                .expect("next"),
            ItemId::new("T", 1)
        );
        for n in 1..=3 {
            BacklogItemRepository::save(&repo, &item(n, Rank::after(None)))
                .await
                .expect("save");
        }
        assert_eq!(
            BacklogItemRepository::next_id(&repo, "T")
                .await
                .expect("next"),
            ItemId::new("T", 4)
        );
        // Archiving T-3 does not release its ID; the next ID remains T-4.
        BacklogItemRepository::archive(&repo, &ItemId::new("T", 3))
            .await
            .expect("archive");
        assert_eq!(
            BacklogItemRepository::next_id(&repo, "T")
                .await
                .expect("next"),
            ItemId::new("T", 4)
        );
    }

    #[tokio::test]
    async fn sprint_save_load_roundtrips() {
        let (_dir, repo) = repo();
        let s = Sprint::new(SprintId::new("S-1").unwrap(), "Sprint 1", ts(1_000)).unwrap();
        SprintRepository::save(&repo, &s).await.expect("save");
        let loaded = SprintRepository::load(&repo, &s.id).await.expect("load");
        assert_eq!(loaded, s);
    }

    #[tokio::test]
    async fn load_missing_sprint_returns_not_found() {
        let (_dir, repo) = repo();
        let id = SprintId::new("S-99").unwrap();
        let err = SprintRepository::load(&repo, &id)
            .await
            .expect_err("missing");
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
        for (id, secs) in [("S-3", 3_000i64), ("S-1", 1_000), ("S-2", 2_000)] {
            let s = Sprint::new(SprintId::new(id).unwrap(), id, ts(secs)).unwrap();
            SprintRepository::save(&repo, &s).await.expect("save");
        }
        let ids: Vec<String> = SprintRepository::list(&repo)
            .await
            .expect("list")
            .into_iter()
            .map(|s| s.id.as_str().to_string())
            .collect();
        assert_eq!(ids, ["S-1", "S-2", "S-3"]);
    }
}
