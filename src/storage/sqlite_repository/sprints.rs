//! Sprint persistence for the SQLite backend: row mapping and the [`SprintRepository`]
//! implementation.

use super::{SqliteRepository, column, corrupt, dt_from_str, dt_to_str, open_conn, sqlite_err};
use crate::error::{Error, Result};
use crate::sprint::{Sprint, SprintId, SprintSpillover, SprintState};
use crate::storage::repository::SprintRepository;
use rusqlite::{OptionalExtension, Row, params};
use std::path::Path;

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
