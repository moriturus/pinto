//! Item persistence for the SQLite backend: row mapping, relation loading, and the
//! [`BacklogItemRepository`] implementation.

use super::{SqliteRepository, column, corrupt, dt_from_str, dt_to_str, open_conn, sqlite_err};
use crate::backlog::{BacklogItem, ItemId, Status};
use crate::error::{Error, Result};
use crate::rank::Rank;
use crate::storage::issued_ids::{max_number, record};
use crate::storage::repository::BacklogItemRepository;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

impl SqliteRepository {
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
