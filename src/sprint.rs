//! Sprint model.
//!
//! Pure logic for sprint IDs, goals, schedules, capacity, and state transitions.
//! The model has no I/O dependency; [`crate::storage`] persists it and callers supply timestamps.

use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use std::fmt;
use std::str::FromStr;

/// Stable identifier for the sprint.
///
/// It is also used in file names (`<id>.md`), so it accepts only path-safe ASCII slugs containing
/// alphanumerics, `-`, or `_`. A PBI stores its assignment as this ID string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SprintId(String);

impl SprintId {
    /// Validate a slug and create a sprint ID.
    ///
    /// Return [`Error::InvalidSprintId`] when the value is empty or contains anything other than
    /// ASCII alphanumerics, `-`, or `_`.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !valid {
            return Err(Error::InvalidSprintId(value));
        }
        Ok(Self(value))
    }

    /// Return the ID string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SprintId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SprintId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        SprintId::new(s)
    }
}

/// Sprint state. Transitions are allowed only from `planned` to `active` to `closed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprintState {
    /// Planned and not yet started; the default state after creation.
    Planned,
    /// In progress.
    Active,
    /// Finished and closed.
    Closed,
}

impl SprintState {
    /// Return the lowercase string used for persistence.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            SprintState::Planned => "planned",
            SprintState::Active => "active",
            SprintState::Closed => "closed",
        }
    }
}

impl fmt::Display for SprintState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SprintState {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "planned" => Ok(SprintState::Planned),
            "active" => Ok(SprintState::Active),
            "closed" => Ok(SprintState::Closed),
            other => Err(Error::InvalidSprintState(other.to_string())),
        }
    }
}

/// A sprint that groups PBIs into a time-boxed work period.
///
/// `start` and `end` are planned dates independent of state transitions; editing sets the schedule
/// while [`Sprint::start`] and [`Sprint::close`] advance the state.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprint {
    /// Stable identifier.
    pub id: SprintId,
    /// Display title.
    pub title: String,
    /// Sprint goal (free description, multiple lines allowed).
    pub goal: String,
    /// Planned start date and time, or `None` when unset.
    pub start: Option<DateTime<Utc>>,
    /// Planned end date and time, or `None` when unset.
    pub end: Option<DateTime<Utc>>,
    /// Working hours per day; capacity is unavailable when unset.
    pub daily_work_hours: Option<f64>,
    /// Holidays within the period, deducted from the inclusive calendar-day count.
    pub holiday_days: Option<u32>,
    /// Fraction of daily capacity retained after meetings and interruptions (`0.0..=1.0`).
    pub deduction_factor: Option<f64>,
    /// Current state (`planned`, `active`, or `closed`).
    pub state: SprintState,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

impl Sprint {
    /// Create a minimal sprint in [`SprintState::Planned`] with an empty goal and no schedule.
    ///
    /// Return [`Error::EmptySprintTitle`] when `title` is empty or only whitespace.
    pub fn new(id: SprintId, title: impl Into<String>, now: DateTime<Utc>) -> Result<Self> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(Error::EmptySprintTitle);
        }
        Ok(Self {
            id,
            title,
            goal: String::new(),
            start: None,
            end: None,
            daily_work_hours: None,
            holiday_days: None,
            deduction_factor: None,
            state: SprintState::Planned,
            created: now,
            updated: now,
        })
    }

    /// Update the editable sprint details and refresh `updated`.
    ///
    /// Fields set to `None` remain unchanged. A blank title or inverted period is rejected before
    /// any field changes are applied. Return [`Error::NothingToUpdate`] when no field is supplied.
    pub fn update_details(
        &mut self,
        title: Option<String>,
        goal: Option<String>,
        period: Option<(DateTime<Utc>, DateTime<Utc>)>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if title.is_none() && goal.is_none() && period.is_none() {
            return Err(Error::NothingToUpdate);
        }
        if let Some(title) = &title
            && title.trim().is_empty()
        {
            return Err(Error::EmptySprintTitle);
        }
        if let Some((start, end)) = period
            && start > end
        {
            return Err(Error::InvalidSprintPeriod {
                start: start.date_naive(),
                end: end.date_naive(),
            });
        }

        if let Some(title) = title {
            self.title = title;
        }
        if let Some(goal) = goal {
            self.goal = goal;
        }
        if let Some((start, end)) = period {
            self.start = Some(start);
            self.end = Some(end);
        }
        self.updated = now;
        Ok(())
    }

    /// Start a sprint (`planned` → `active`) and update `updated`.
    ///
    /// A sprint without a non-blank goal is rejected with [`Error::EmptySprintGoal`].
    /// Return [`Error::InvalidSprintTransition`] for any other starting state without changing it.
    pub fn start(&mut self, now: DateTime<Utc>) -> Result<()> {
        if self.state == SprintState::Planned && self.goal.trim().is_empty() {
            return Err(Error::EmptySprintGoal);
        }
        self.transition(SprintState::Active, now)
    }

    /// Close a sprint (`active` → `closed`) and update `updated`.
    ///
    /// Return [`Error::InvalidSprintTransition`] for any other state without changing it.
    pub fn close(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.transition(SprintState::Closed, now)
    }

    /// Update settings for capacity calculations.
    ///
    /// Count calendar days inclusively, subtract `holiday_days`, and multiply the result by
    /// `daily_work_hours × deduction_factor`. Reject an unset period or invalid value without
    /// changing existing settings.
    pub fn set_capacity(
        &mut self,
        daily_work_hours: f64,
        holiday_days: u32,
        deduction_factor: f64,
    ) -> Result<()> {
        if !daily_work_hours.is_finite() || daily_work_hours < 0.0 {
            return Err(Error::InvalidDailyWorkHours(daily_work_hours.to_string()));
        }
        if !deduction_factor.is_finite() || !(0.0..=1.0).contains(&deduction_factor) {
            return Err(Error::InvalidDeductionFactor(deduction_factor.to_string()));
        }
        let calendar_days = self.calendar_days()?;
        if holiday_days > calendar_days {
            return Err(Error::InvalidSprintHolidays {
                holidays: holiday_days,
                calendar_days,
            });
        }
        self.daily_work_hours = Some(daily_work_hours);
        self.holiday_days = Some(holiday_days);
        self.deduction_factor = Some(deduction_factor);
        Ok(())
    }

    /// Return working days and available hours when the schedule and capacity settings are complete.
    #[must_use]
    pub fn capacity(&self) -> Option<SprintCapacity> {
        let calendar_days = self.calendar_days().ok()?;
        let (daily_work_hours, holiday_days, deduction_factor) = (
            self.daily_work_hours?,
            self.holiday_days?,
            self.deduction_factor?,
        );
        let working_days = calendar_days.checked_sub(holiday_days)?;
        Some(SprintCapacity {
            working_days,
            hours: f64::from(working_days) * daily_work_hours * deduction_factor,
        })
    }

    fn calendar_days(&self) -> Result<u32> {
        let (start, end) = self
            .start
            .zip(self.end)
            .ok_or_else(|| Error::SprintCapacityPeriodUnset(self.id.clone()))?;
        if start > end {
            return Err(Error::InvalidSprintPeriod {
                start: start.date_naive(),
                end: end.date_naive(),
            });
        }
        let days = (end.date_naive() - start.date_naive()).num_days() + 1;
        u32::try_from(days).map_err(|_| Error::InvalidSprintPeriod {
            start: start.date_naive(),
            end: end.date_naive(),
        })
    }

    /// Apply a forward-only state transition: `planned → active → closed`.
    fn transition(&mut self, to: SprintState, now: DateTime<Utc>) -> Result<()> {
        let allowed = matches!(
            (self.state, to),
            (SprintState::Planned, SprintState::Active)
                | (SprintState::Active, SprintState::Closed)
        );
        if !allowed {
            return Err(Error::InvalidSprintTransition {
                from: self.state,
                to,
            });
        }
        self.state = to;
        self.updated = now;
        Ok(())
    }
}

/// Sprint capacity calculation results.
#[derive(Debug, Clone, PartialEq)]
pub struct SprintCapacity {
    /// Inclusive calendar days minus holidays.
    pub working_days: u32,
    /// Available working hours after deductions.
    pub hours: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    fn epoch() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).expect("valid epoch")
    }

    fn sid(s: &str) -> SprintId {
        SprintId::new(s).expect("valid sprint id")
    }

    // --- SprintId ---

    #[test]
    fn sprint_id_accepts_safe_slugs() {
        for ok in ["S-1", "sprint_1", "2026Q3", "A", "release-1-x"] {
            assert!(SprintId::new(ok).is_ok(), "expected {ok:?} to be accepted");
        }
    }

    #[test]
    fn sprint_id_rejects_unsafe_slugs() {
        for bad in ["", "S 1", "a/b", "a.b", "エス", "with\tspace"] {
            assert!(
                SprintId::new(bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn sprint_id_display_and_parse_roundtrip() {
        let id = sid("S-1");
        assert_eq!(id.to_string(), "S-1");
        assert_eq!("S-1".parse::<SprintId>().unwrap(), id);
        assert_eq!(id.as_str(), "S-1");
    }

    // --- SprintState ---

    #[test]
    fn sprint_state_string_roundtrip() {
        for (state, text) in [
            (SprintState::Planned, "planned"),
            (SprintState::Active, "active"),
            (SprintState::Closed, "closed"),
        ] {
            assert_eq!(state.as_str(), text);
            assert_eq!(state.to_string(), text);
            assert_eq!(text.parse::<SprintState>().unwrap(), state);
        }
    }

    #[test]
    fn sprint_state_rejects_unknown() {
        let err = "archived".parse::<SprintState>().unwrap_err();
        assert_eq!(err, Error::InvalidSprintState("archived".to_string()));
    }

    // --- Sprint construction ---

    #[test]
    fn new_sprint_uses_planned_defaults() {
        let s = Sprint::new(sid("S-1"), "Sprint 1", epoch()).unwrap();
        assert_eq!(s.id, sid("S-1"));
        assert_eq!(s.title, "Sprint 1");
        assert_eq!(s.goal, "");
        assert_eq!(s.start, None);
        assert_eq!(s.end, None);
        assert_eq!(s.daily_work_hours, None);
        assert_eq!(s.holiday_days, None);
        assert_eq!(s.deduction_factor, None);
        assert_eq!(s.capacity(), None);
        assert_eq!(s.state, SprintState::Planned);
        assert_eq!(s.created, epoch());
        assert_eq!(s.updated, epoch());
    }

    #[test]
    fn new_sprint_rejects_empty_title() {
        let err = Sprint::new(sid("S-1"), "   ", epoch()).unwrap_err();
        assert_eq!(err, Error::EmptySprintTitle);
    }

    #[test]
    fn update_details_changes_title_goal_period_and_timestamp() {
        let mut sprint = Sprint::new(sid("S-1"), "Planning", epoch()).unwrap();
        let start = epoch() + chrono::Duration::days(1);
        let end = epoch() + chrono::Duration::days(5);
        let updated = epoch() + chrono::Duration::days(10);

        sprint
            .update_details(
                Some("Execution".to_string()),
                Some("Ship the sprint".to_string()),
                Some((start, end)),
                updated,
            )
            .unwrap();

        assert_eq!(sprint.title, "Execution");
        assert_eq!(sprint.goal, "Ship the sprint");
        assert_eq!(sprint.start, Some(start));
        assert_eq!(sprint.end, Some(end));
        assert_eq!(sprint.updated, updated);
        assert_eq!(sprint.created, epoch());
    }

    #[test]
    fn update_details_rejects_invalid_values_without_mutating() {
        let mut sprint = Sprint::new(sid("S-1"), "Planning", epoch()).unwrap();
        let original = sprint.clone();
        let later = epoch() + chrono::Duration::days(1);

        assert_eq!(
            sprint.update_details(Some("   ".to_string()), None, None, later),
            Err(Error::EmptySprintTitle)
        );
        assert_eq!(sprint, original);

        let start = epoch() + chrono::Duration::days(5);
        let end = epoch() + chrono::Duration::days(1);
        assert_eq!(
            sprint.update_details(None, None, Some((start, end)), later),
            Err(Error::InvalidSprintPeriod {
                start: start.date_naive(),
                end: end.date_naive(),
            })
        );
        assert_eq!(sprint, original);

        assert_eq!(
            sprint.update_details(None, None, None, later),
            Err(Error::NothingToUpdate)
        );
        assert_eq!(sprint, original);
    }

    #[test]
    fn capacity_uses_inclusive_calendar_days_and_deductions() {
        let mut sprint = Sprint::new(sid("S-1"), "Sprint 1", epoch()).unwrap();
        sprint.start = Some(
            chrono::NaiveDate::from_ymd_opt(2026, 7, 6)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
        );
        sprint.end = Some(
            chrono::NaiveDate::from_ymd_opt(2026, 7, 10)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
        );

        sprint.set_capacity(8.0, 1, 0.8).unwrap();

        assert_eq!(
            sprint.capacity(),
            Some(SprintCapacity {
                working_days: 4,
                hours: 25.6,
            })
        );
    }

    #[test]
    fn capacity_rejects_invalid_values_and_holidays_outside_period() {
        let mut sprint = Sprint::new(sid("S-1"), "Sprint 1", epoch()).unwrap();
        sprint.start = Some(epoch());
        sprint.end = Some(epoch());

        assert_eq!(
            sprint.set_capacity(-1.0, 0, 1.0),
            Err(Error::InvalidDailyWorkHours("-1".to_string()))
        );
        assert_eq!(
            sprint.set_capacity(8.0, 0, 1.1),
            Err(Error::InvalidDeductionFactor("1.1".to_string()))
        );
        assert_eq!(
            sprint.set_capacity(8.0, 2, 1.0),
            Err(Error::InvalidSprintHolidays {
                holidays: 2,
                calendar_days: 1,
            })
        );
    }

    // --- transitions ---

    #[test]
    fn start_moves_planned_to_active_and_updates_timestamp() {
        let mut s = Sprint::new(sid("S-1"), "Sprint 1", epoch()).unwrap();
        s.goal = "Ship the sprint".to_string();
        let later = epoch() + chrono::Duration::seconds(60);

        s.start(later).unwrap();

        assert_eq!(s.state, SprintState::Active);
        assert_eq!(s.updated, later);
        assert_eq!(s.created, epoch(), "created must not change");
    }

    #[test]
    fn start_requires_a_non_empty_goal() {
        let mut s = Sprint::new(sid("S-1"), "Sprint 1", epoch()).unwrap();

        let err = s.start(epoch()).unwrap_err();

        assert_eq!(err, Error::EmptySprintGoal);
        assert_eq!(s.state, SprintState::Planned);
    }

    #[test]
    fn close_moves_active_to_closed() {
        let mut s = Sprint::new(sid("S-1"), "Sprint 1", epoch()).unwrap();
        s.goal = "Ship the sprint".to_string();
        s.start(epoch()).unwrap();
        let later = epoch() + chrono::Duration::seconds(120);

        s.close(later).unwrap();

        assert_eq!(s.state, SprintState::Closed);
        assert_eq!(s.updated, later);
    }

    #[test]
    fn start_from_non_planned_is_rejected_and_leaves_state() {
        let mut s = Sprint::new(sid("S-1"), "Sprint 1", epoch()).unwrap();
        s.goal = "Ship the sprint".to_string();
        s.start(epoch()).unwrap(); // active

        let err = s.start(epoch()).unwrap_err();
        assert_eq!(
            err,
            Error::InvalidSprintTransition {
                from: SprintState::Active,
                to: SprintState::Active,
            }
        );
        assert_eq!(s.state, SprintState::Active, "state unchanged on failure");
    }

    #[test]
    fn close_from_planned_is_rejected() {
        let mut s = Sprint::new(sid("S-1"), "Sprint 1", epoch()).unwrap();
        let err = s.close(epoch()).unwrap_err();
        assert_eq!(
            err,
            Error::InvalidSprintTransition {
                from: SprintState::Planned,
                to: SprintState::Closed,
            }
        );
        assert_eq!(s.state, SprintState::Planned);
    }
}
