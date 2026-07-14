//! Product Backlog Item.

use super::{ItemId, Status, Workflow};
use crate::error::{Error, Result};
use crate::rank::Rank;
use chrono::{DateTime, Utc};

/// Product Backlog Item (PBI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogItem {
    pub id: ItemId,
    pub title: String,
    pub status: Status,
    /// Backlog sort order; lexicographically smaller ranks have higher priority.
    pub rank: Rank,
    pub points: Option<u32>,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
    pub sprint: Option<String>,
    /// Parent PBI. Parent-child links are kept acyclic, forming a tree hierarchy.
    ///
    /// This represents a hierarchy such as epic → story → task; the model does not assign a type
    /// such as epic or story. Cycles are rejected by [`crate::backlog::parent_creates_cycle`].
    pub parent: Option<ItemId>,
    /// PBIs that this item depends on and that should be completed first.
    ///
    /// Dependencies are independent of the parent-child hierarchy. Cycles are allowed but
    /// undesirable; callers can detect them with [`crate::backlog::dependency_creates_cycle`].
    pub depends_on: Vec<ItemId>,
    /// Time when work first started, recorded when the item leaves the first workflow column.
    ///
    /// Record it once on the first transition out of the first column and retain it when the item
    /// is reopened. It is `None` when work has not started.
    pub start_at: Option<DateTime<Utc>>,
    /// Time when the item most recently entered the completion column.
    ///
    /// Update it on every entry to the last column and clear it when the item leaves that column.
    /// It is `None` when the item is not complete.
    pub done_at: Option<DateTime<Utc>>,
    /// Git commit SHAs associated with this PBI, in insertion order without duplicates.
    ///
    /// These are plain-text links and do not require Git to be installed. `link` stores the supplied
    /// string; `scan` discovers SHAs from commit messages containing the item ID.
    pub commits: Vec<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub body: String,
}

impl BacklogItem {
    /// Create a minimally configured PBI with default points, labels, and relationships.
    ///
    /// The caller supplies the backlog [`Rank`]. Return [`Error::EmptyTitle`] if `title` is empty.
    pub fn new(
        id: ItemId,
        title: impl Into<String>,
        status: Status,
        rank: Rank,
        now: DateTime<Utc>,
    ) -> Result<Self> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(Error::EmptyTitle);
        }
        Ok(Self {
            id,
            title,
            status,
            rank,
            points: None,
            labels: Vec::new(),
            assignee: None,
            sprint: None,
            parent: None,
            depends_on: Vec::new(),
            start_at: None,
            done_at: None,
            created: now,
            updated: now,
            body: String::new(),
            commits: Vec::new(),
        })
    }

    /// Canonical backlog order shared by every view (`list`, `board`, `kanban`).
    ///
    /// Ascending [`rank`](Self::rank) first, then a deterministic
    /// `(prefix, number)` tie-break so items with equal ranks never reorder
    /// between views (a stable sort on this comparator preserves that order).
    /// This is the single source of truth for "default" ordering; column-level
    /// overrides (e.g. the terminal column's `done_at` sort) layer on top of it.
    #[must_use]
    pub fn backlog_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank.cmp(&other.rank).then_with(|| {
            (self.id.prefix(), self.id.number()).cmp(&(other.id.prefix(), other.id.number()))
        })
    }

    /// Add related commit `sha`; return `true` when it was added.
    ///
    /// Trim leading and trailing whitespace, ignore blank values, reject duplicates, and preserve
    /// insertion order so `git diff` remains stable.
    pub fn link_commit(&mut self, sha: impl Into<String>) -> bool {
        let sha = sha.into();
        let sha = sha.trim();
        if sha.is_empty() || self.commits.iter().any(|c| c == sha) {
            return false;
        }
        self.commits.push(sha.to_string());
        true
    }

    /// Remove related commit `sha` (`true` if removed, `false` if not).
    pub fn unlink_commit(&mut self, sha: &str) -> bool {
        let before = self.commits.len();
        self.commits.retain(|c| c != sha);
        self.commits.len() != before
    }

    /// Transition the state to `to` and update `updated` and working time (`start_at` / `done_at`).
    ///
    /// If `to` is not in the workflow, return [`Error::UnknownStatus`] without changing the item.
    ///
    /// Determine work timestamps from column position rather than column names, because workflows
    /// can be renamed:
    /// - On the first transition out of the first column, record `now` in `start_at`. Entering the
    ///   last column directly also starts the work, including in a single-column workflow.
    /// - On every entry to the last column, record `now` in `done_at`; clear it when leaving that
    ///   column.
    pub fn transition_to(
        &mut self,
        to: Status,
        workflow: &Workflow,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if !workflow.contains(&to) {
            return Err(Error::UnknownStatus(to.as_str().to_string()));
        }
        let is_first = workflow.default_status() == Some(&to);
        let is_last = workflow.columns().last() == Some(&to);
        // Record the first transition out of the first column only once. In a single-column
        // workflow, entering the only (and completed) column also counts as starting work.
        if (!is_first || is_last) && self.start_at.is_none() {
            self.start_at = Some(now);
        }
        // Record completion time on entry to the last column and clear it when leaving.
        self.done_at = is_last.then_some(now);
        self.status = to;
        self.updated = now;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epoch() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).expect("valid epoch")
    }

    fn workflow() -> Workflow {
        Workflow::new(
            ["todo", "in-progress", "review", "done"]
                .into_iter()
                .map(Status::new),
        )
    }

    fn rank() -> Rank {
        Rank::between(None, None).expect("open bounds produce a rank")
    }

    #[test]
    fn new_item_uses_defaults() {
        let item = BacklogItem::new(
            ItemId::new("T", 1),
            "Write model",
            Status::new("todo"),
            rank(),
            epoch(),
        )
        .unwrap();
        assert_eq!(item.title, "Write model");
        assert_eq!(item.status, Status::new("todo"));
        assert_eq!(item.rank, rank());
        assert!(item.labels.is_empty());
        assert_eq!(item.points, None);
        assert_eq!(item.created, epoch());
        assert_eq!(item.updated, epoch());
    }

    #[test]
    fn new_item_rejects_empty_title() {
        let err = BacklogItem::new(
            ItemId::new("T", 1),
            "   ",
            Status::new("todo"),
            rank(),
            epoch(),
        )
        .unwrap_err();
        assert_eq!(err, Error::EmptyTitle);
    }

    #[test]
    fn transition_updates_status_and_timestamp() {
        let mut item = BacklogItem::new(
            ItemId::new("T", 1),
            "Task",
            Status::new("todo"),
            rank(),
            epoch(),
        )
        .unwrap();
        let later = epoch() + chrono::Duration::seconds(60);

        item.transition_to(Status::new("in-progress"), &workflow(), later)
            .unwrap();

        assert_eq!(item.status, Status::new("in-progress"));
        assert_eq!(item.updated, later);
        assert_eq!(item.created, epoch(), "created must not change");
    }

    #[test]
    fn transition_to_unknown_status_is_rejected_and_leaves_item_unchanged() {
        let mut item = BacklogItem::new(
            ItemId::new("T", 1),
            "Task",
            Status::new("todo"),
            rank(),
            epoch(),
        )
        .unwrap();
        let later = epoch() + chrono::Duration::seconds(60);

        let err = item
            .transition_to(Status::new("archived"), &workflow(), later)
            .unwrap_err();

        assert_eq!(err, Error::UnknownStatus("archived".to_string()));
        assert_eq!(item.status, Status::new("todo"), "status must be unchanged");
        assert_eq!(
            item.updated,
            epoch(),
            "updated must be unchanged on failure"
        );
    }

    fn todo_item() -> BacklogItem {
        BacklogItem::new(
            ItemId::new("T", 1),
            "Task",
            Status::new("todo"),
            rank(),
            epoch(),
        )
        .unwrap()
    }

    #[test]
    fn new_item_has_no_work_timestamps() {
        let item = todo_item();
        assert_eq!(item.start_at, None);
        assert_eq!(item.done_at, None);
    }

    #[test]
    fn new_item_has_no_commits() {
        assert!(todo_item().commits.is_empty());
    }

    #[test]
    fn link_commit_appends_and_deduplicates() {
        let mut item = todo_item();
        assert!(item.link_commit("abc123"), "first link is added");
        assert!(!item.link_commit("abc123"), "duplicate is ignored");
        assert!(item.link_commit("def456"), "distinct sha is added");
        assert_eq!(item.commits, ["abc123", "def456"], "insertion order kept");
    }

    #[test]
    fn link_commit_trims_and_ignores_blank() {
        let mut item = todo_item();
        assert!(item.link_commit("  abc123  "), "surrounding space trimmed");
        assert!(!item.link_commit("   "), "blank sha ignored");
        assert!(!item.link_commit("abc123"), "trimmed value deduplicates");
        assert_eq!(item.commits, ["abc123"]);
    }

    #[test]
    fn unlink_commit_removes_when_present() {
        let mut item = todo_item();
        item.link_commit("abc123");
        item.link_commit("def456");
        assert!(item.unlink_commit("abc123"), "present sha removed");
        assert!(!item.unlink_commit("abc123"), "already gone returns false");
        assert_eq!(item.commits, ["def456"]);
    }

    #[test]
    fn entering_a_working_column_records_start_at() {
        let mut item = todo_item();
        let t = epoch() + chrono::Duration::seconds(60);

        item.transition_to(Status::new("in-progress"), &workflow(), t)
            .unwrap();

        assert_eq!(item.start_at, Some(t), "start_at recorded on first work");
        assert_eq!(item.done_at, None, "not done yet");
    }

    #[test]
    fn start_at_is_kept_on_reentry_not_reset() {
        let mut item = todo_item();
        let first = epoch() + chrono::Duration::seconds(60);
        let later = epoch() + chrono::Duration::seconds(120);

        item.transition_to(Status::new("in-progress"), &workflow(), first)
            .unwrap();
        // Even if you return to todo and start again, the initial start_at will be maintained.
        item.transition_to(Status::new("todo"), &workflow(), later)
            .unwrap();
        item.transition_to(Status::new("review"), &workflow(), later)
            .unwrap();

        assert_eq!(item.start_at, Some(first), "keeps the first start time");
    }

    #[test]
    fn entering_the_terminal_column_records_done_at() {
        let mut item = todo_item();
        let t = epoch() + chrono::Duration::seconds(60);

        item.transition_to(Status::new("done"), &workflow(), t)
            .unwrap();

        assert_eq!(item.done_at, Some(t), "done_at recorded when completed");
        assert_eq!(
            item.start_at,
            Some(t),
            "direct todo→done also marks work started"
        );
    }

    #[test]
    fn leaving_the_terminal_column_clears_done_at() {
        let mut item = todo_item();
        let done_at = epoch() + chrono::Duration::seconds(60);
        let reopened = epoch() + chrono::Duration::seconds(120);

        item.transition_to(Status::new("done"), &workflow(), done_at)
            .unwrap();
        assert_eq!(item.done_at, Some(done_at));

        item.transition_to(Status::new("in-progress"), &workflow(), reopened)
            .unwrap();

        assert_eq!(item.done_at, None, "reopening clears completion time");
        assert_eq!(item.start_at, Some(done_at), "start time is preserved");
    }

    #[test]
    fn single_column_workflow_records_both_start_and_done() {
        // Even in a single-column workflow where the first column = the last column, completion implies that the work has started.
        // Record both start_at / done_at (consistent with multi-column todo→done).
        let single = Workflow::new(std::iter::once(Status::new("done")));
        let mut item = BacklogItem::new(
            ItemId::new("T", 1),
            "Task",
            Status::new("done"),
            rank(),
            epoch(),
        )
        .unwrap();
        let t = epoch() + chrono::Duration::seconds(60);

        item.transition_to(Status::new("done"), &single, t).unwrap();

        assert_eq!(item.done_at, Some(t), "completion recorded");
        assert_eq!(
            item.start_at,
            Some(t),
            "single-column done also marks start"
        );
    }

    #[test]
    fn backlog_cmp_orders_by_rank_then_id() {
        // Canonical backlog order shared by list / board / kanban: ascending rank
        // first, then a deterministic (prefix, number) tie-break so equal ranks
        // never reorder between views.
        let make = |prefix: &str, n: u32, r: &str| {
            BacklogItem::new(
                ItemId::new(prefix, n),
                "x",
                Status::new("todo"),
                Rank::parse(r).expect("valid rank"),
                epoch(),
            )
            .unwrap()
        };
        // Lower rank sorts first regardless of id.
        let low = make("T", 9, "g");
        let high = make("T", 1, "m");
        assert_eq!(low.backlog_cmp(&high), std::cmp::Ordering::Less);
        assert_eq!(high.backlog_cmp(&low), std::cmp::Ordering::Greater);

        // Equal rank: break the tie by (prefix, number).
        let a = make("A", 2, "g");
        let b = make("B", 1, "g");
        assert_eq!(
            a.backlog_cmp(&b),
            std::cmp::Ordering::Less,
            "prefix breaks tie"
        );
        let t2 = make("T", 2, "g");
        let t10 = make("T", 10, "g");
        assert_eq!(
            t2.backlog_cmp(&t10),
            std::cmp::Ordering::Less,
            "numeric (not lexical) number order within a prefix"
        );
    }

    #[test]
    fn re_entering_the_terminal_column_refreshes_done_at() {
        let mut item = todo_item();
        let first_done = epoch() + chrono::Duration::seconds(60);
        let reopened = epoch() + chrono::Duration::seconds(120);
        let second_done = epoch() + chrono::Duration::seconds(180);

        item.transition_to(Status::new("done"), &workflow(), first_done)
            .unwrap();
        item.transition_to(Status::new("review"), &workflow(), reopened)
            .unwrap();
        item.transition_to(Status::new("done"), &workflow(), second_done)
            .unwrap();

        assert_eq!(item.done_at, Some(second_done), "latest completion wins");
    }
}
