//! Markdown + TOML frontmatter conversion.

use crate::backlog::{BacklogItem, ItemId, Status};
use crate::error::{Error, Result};
use crate::rank::Rank;
use crate::sprint::{Sprint, SprintId, SprintSpillover, SprintState};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Start/end delimiter for frontmatter.
const DELIMITER: &str = "+++";

/// Structured fields serialized to TOML frontmatter.
///
/// Domain types such as [`ItemId`], [`Status`], and [`Rank`] are stored as strings so the TOML
/// remains simple and easy to edit by hand.
#[derive(Debug, Serialize, Deserialize)]
struct Frontmatter {
    id: String,
    title: String,
    status: String,
    rank: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    points: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    done_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    commits: Vec<String>,
    created: DateTime<Utc>,
    updated: DateTime<Utc>,
}

impl Frontmatter {
    fn from_item(item: &BacklogItem) -> Self {
        Self {
            id: item.id.to_string(),
            title: item.title.clone(),
            status: item.status.to_string(),
            rank: item.rank.to_string(),
            points: item.points,
            labels: item.labels.clone(),
            assignee: item.assignee.clone(),
            sprint: item.sprint.clone(),
            parent: item.parent.as_ref().map(ItemId::to_string),
            depends_on: item.depends_on.iter().map(ItemId::to_string).collect(),
            start_at: item.start_at,
            done_at: item.done_at,
            commits: item.commits.clone(),
            created: item.created,
            updated: item.updated,
        }
    }

    fn into_item(self, body: String, path: &Path) -> Result<BacklogItem> {
        let to_parse = |e: Error| Error::parse(path, e.to_string());
        let id: ItemId = self.id.parse().map_err(to_parse)?;
        let rank = Rank::parse(&self.rank).map_err(to_parse)?;
        let parent = match self.parent {
            Some(p) => Some(p.parse::<ItemId>().map_err(to_parse)?),
            None => None,
        };
        let depends_on = self
            .depends_on
            .iter()
            .map(|d| d.parse::<ItemId>().map_err(to_parse))
            .collect::<Result<Vec<_>>>()?;
        // An empty title violates the constructor invariant even if it is present in the file.
        if self.title.trim().is_empty() {
            return Err(Error::parse(path, Error::EmptyTitle.to_string()));
        }
        Ok(BacklogItem {
            id,
            title: self.title,
            status: Status::new(self.status),
            rank,
            points: self.points,
            labels: self.labels,
            assignee: self.assignee,
            sprint: self.sprint,
            parent,
            depends_on,
            start_at: self.start_at,
            done_at: self.done_at,
            created: self.created,
            updated: self.updated,
            body,
            commits: self.commits,
        })
    }
}

/// Assemble the frontmatter (TOML) and the body into a Markdown string.
///
/// If the body is empty, omit the blank body section. This is shared by PBI and sprint formatting.
fn assemble_markdown(front_toml: &str, body: &str) -> String {
    let mut out = String::new();
    out.push_str(DELIMITER);
    out.push('\n');
    out.push_str(front_toml); // toml::to_string includes a trailing newline.
    out.push_str(DELIMITER);
    out.push('\n');
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
    }
    out
}

/// Format PBI into `+++` frontmatter + body Markdown string.
pub(crate) fn to_markdown(item: &BacklogItem) -> Result<String> {
    let fm = Frontmatter::from_item(item);
    let toml = toml::to_string(&fm)
        .map_err(|e| Error::parse(&PathBuf::from(format!("{}.md", item.id)), e.to_string()))?;
    Ok(assemble_markdown(&toml, &item.body))
}

/// Parse the required PBI frontmatter and the body Markdown.
pub(crate) fn from_markdown(text: &str, path: &Path) -> Result<BacklogItem> {
    let (front, body) = split_frontmatter(text).ok_or_else(|| Error::MissingFrontmatter {
        path: path.to_path_buf(),
    })?;
    let fm: Frontmatter = toml::from_str(front).map_err(|e| Error::parse(path, e.to_string()))?;
    fm.into_item(body.to_string(), path)
}

/// Parse a backlog item's `+++` TOML frontmatter and Markdown body.
///
/// The parser validates the serialized [`ItemId`], [`Rank`], and required title before returning
/// a domain item. `path` is included in parse errors so callers can point users to the invalid
/// document.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use pinto::storage::parse_item_markdown;
///
/// let markdown = "+++\n\
/// id = \"T-1\"\n\
/// title = \"Review parser input\"\n\
/// status = \"todo\"\n\
/// rank = \"i\"\n\
/// created = \"1970-01-01T00:00:00Z\"\n\
/// updated = \"1970-01-01T00:00:00Z\"\n\
/// +++\n\
/// body\n";
/// let item = parse_item_markdown(markdown, Path::new("T-1.md")).expect("valid item");
/// assert_eq!(item.id.to_string(), "T-1");
/// assert_eq!(item.body, "body");
/// ```
pub fn parse_item_markdown(text: &str, path: &Path) -> Result<BacklogItem> {
    from_markdown(text, path)
}

/// Sprint frontmatter fields. The title is structured; the goal remains the Markdown body.
///
/// As with item frontmatter, store domain types such as [`SprintId`] and [`SprintState`] as strings
/// so the TOML remains easy to edit by hand.
#[derive(Debug, Serialize, Deserialize)]
struct SprintFrontmatter {
    id: String,
    title: String,
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    daily_work_hours: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    holiday_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deduction_factor: Option<f64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    spillover_points: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    spillover_items: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    unestimated_spillover_items: u32,
    created: DateTime<Utc>,
    updated: DateTime<Utc>,
}

impl SprintFrontmatter {
    fn from_sprint(sprint: &Sprint) -> Self {
        Self {
            id: sprint.id.to_string(),
            title: sprint.title.clone(),
            state: sprint.state.to_string(),
            closed_at: sprint.closed_at,
            start: sprint.start,
            end: sprint.end,
            daily_work_hours: sprint.daily_work_hours,
            holiday_days: sprint.holiday_days,
            deduction_factor: sprint.deduction_factor,
            spillover_points: sprint.spillover.points,
            spillover_items: sprint.spillover.items,
            unestimated_spillover_items: sprint.spillover.unestimated_items,
            created: sprint.created,
            updated: sprint.updated,
        }
    }

    fn into_sprint(self, goal: String, path: &Path) -> Result<Sprint> {
        let to_parse = |e: Error| Error::parse(path, e.to_string());
        let id: SprintId = self.id.parse().map_err(to_parse)?;
        let state: SprintState = self.state.parse().map_err(to_parse)?;
        // An empty title violates the constructor invariant even when the file contains it.
        if self.title.trim().is_empty() {
            return Err(Error::parse(path, Error::EmptySprintTitle.to_string()));
        }
        Ok(Sprint {
            id,
            title: self.title,
            goal,
            start: self.start,
            end: self.end,
            daily_work_hours: self.daily_work_hours,
            holiday_days: self.holiday_days,
            deduction_factor: self.deduction_factor,
            spillover: SprintSpillover {
                points: self.spillover_points,
                items: self.spillover_items,
                unestimated_items: self.unestimated_spillover_items,
            },
            state,
            closed_at: self.closed_at,
            created: self.created,
            updated: self.updated,
        })
    }
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Format the sprint into a Markdown string with a structured title and a Markdown goal body.
pub(super) fn sprint_to_markdown(sprint: &Sprint) -> Result<String> {
    let fm = SprintFrontmatter::from_sprint(sprint);
    let toml = toml::to_string(&fm)
        .map_err(|e| Error::parse(&PathBuf::from(format!("{}.md", sprint.id)), e.to_string()))?;
    Ok(assemble_markdown(&toml, &sprint.goal))
}

/// Parse the Markdown of a sprint with a structured title and a Markdown goal body.
pub(super) fn sprint_from_markdown(text: &str, path: &Path) -> Result<Sprint> {
    let (front, goal) = split_frontmatter(text).ok_or_else(|| Error::MissingFrontmatter {
        path: path.to_path_buf(),
    })?;
    let fm: SprintFrontmatter =
        toml::from_str(front).map_err(|e| Error::parse(path, e.to_string()))?;
    fm.into_sprint(goal.to_string(), path)
}

/// Split a document whose first line starts with `+++` into (frontmatter, body).
///
/// `None` if there is no closing `+++` line. In the main text, remove one blank line and one trailing newline immediately after the delimiter.
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = strip_delimiter_line(text)?;
    let closing = find_delimiter_line(rest)?;
    let front = &rest[..closing];
    // Extract the text from the closing line (`+++\n...` or `+++`).
    let after = &rest[closing..];
    let body = after.split_once('\n').map(|(_, b)| b).unwrap_or("");
    let body = body.strip_prefix('\n').unwrap_or(body); // Separator blank line.
    let body = body.strip_suffix('\n').unwrap_or(body); // Trailing newline.
    Some((front, body))
}

/// Returns the remainder after removing the first `+++` line (`None` if the first line is not a separator line).
///
/// Allows `\r\n` line breaks as well as closing lines ([`find_delimiter_line`]).
fn strip_delimiter_line(text: &str) -> Option<&str> {
    let rest = text.strip_prefix(DELIMITER)?;
    let rest = rest.strip_prefix('\r').unwrap_or(rest);
    match rest.strip_prefix('\n') {
        Some(rest) => Some(rest),
        // Only `+++` and no line break (no body/invalid) → returns empty.
        None if rest.is_empty() => Some(rest),
        None => None,
    }
}

/// Returns the starting byte position of a line consisting only of `+++`.
fn find_delimiter_line(s: &str) -> Option<usize> {
    let mut offset = 0;
    for line in s.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content == DELIMITER {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn epoch() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).expect("valid epoch")
    }

    /// PBI with optional fields filled in (multiple lines of text).
    fn full_item() -> BacklogItem {
        let mut item = BacklogItem::new(
            ItemId::new("T", 7),
            "Full item",
            Status::new("in-progress"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            epoch(),
        )
        .expect("valid item");
        item.points = Some(5);
        item.labels = vec!["backend".to_string(), "urgent".to_string()];
        item.assignee = Some("alice".to_string());
        item.sprint = Some("S-1".to_string());
        item.parent = Some(ItemId::new("T", 1));
        item.depends_on = vec![ItemId::new("T", 2), ItemId::new("T", 3)];
        item.start_at = Some(epoch() + Duration::seconds(30));
        item.done_at = Some(epoch() + Duration::seconds(90));
        item.commits = vec!["abc1234".to_string(), "def5678".to_string()];
        item.body = "Acceptance criteria\n- one\n- two".to_string();
        item.updated = epoch() + Duration::seconds(60);
        item
    }

    #[test]
    fn item_markdown_roundtrips_all_fields() {
        let item = full_item();
        let text = to_markdown(&item).expect("serialize");
        let parsed = from_markdown(&text, Path::new("T-7.md")).expect("parse");
        assert_eq!(parsed, item);
    }

    #[test]
    fn item_markdown_roundtrips_minimal_item_without_body() {
        let item = BacklogItem::new(
            ItemId::new("T", 1),
            "Minimal",
            Status::new("todo"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            epoch(),
        )
        .expect("valid item");
        let text = to_markdown(&item).expect("serialize");
        let parsed = from_markdown(&text, Path::new("T-1.md")).expect("parse");
        assert_eq!(parsed, item);
        assert!(parsed.body.is_empty(), "empty body preserved");
    }

    #[test]
    fn from_markdown_without_work_timestamps_defaults_to_none() {
        // PBIs that have not been started or completed do not have start_at / done_at (they are omitted when exporting).
        let text = "\
+++
id = \"T-1\"
title = \"Unstarted\"
status = \"todo\"
rank = \"n\"
created = \"1970-01-01T00:00:00Z\"
updated = \"1970-01-01T00:00:00Z\"
+++
";
        let parsed = from_markdown(text, Path::new("T-1.md")).expect("parse todo item");
        assert_eq!(parsed.start_at, None);
        assert_eq!(parsed.done_at, None);
    }

    #[test]
    fn sprint_markdown_roundtrips_all_fields() {
        let mut sprint = Sprint::new(
            SprintId::new("sprint-1").expect("valid id"),
            "Sprint One",
            epoch(),
        )
        .expect("valid sprint");
        sprint.goal = "Ship the MVP\nwith tests".to_string();
        sprint.state = SprintState::Closed;
        sprint.closed_at = Some(epoch() + Duration::seconds(20));
        sprint.start = Some(epoch());
        sprint.end = Some(epoch() + Duration::days(14));
        sprint.spillover = crate::sprint::SprintSpillover {
            points: 8,
            items: 2,
            unestimated_items: 1,
        };
        sprint.updated = epoch() + Duration::seconds(30);

        let text = sprint_to_markdown(&sprint).expect("serialize");
        let parsed = sprint_from_markdown(&text, Path::new("sprint-1.md")).expect("parse");
        assert_eq!(parsed, sprint);
    }

    #[test]
    fn sprint_markdown_without_spillover_fields_defaults_to_zero() {
        let text = "\
+++
id = \"S-1\"
title = \"Sprint One\"
state = \"closed\"
created = \"1970-01-01T00:00:00Z\"
updated = \"1970-01-01T00:00:00Z\"
+++
";

        let sprint = sprint_from_markdown(text, Path::new("S-1.md")).expect("parse sprint");

        assert_eq!(sprint.closed_at, None);
        assert_eq!(sprint.spillover, crate::sprint::SprintSpillover::default());
    }

    #[test]
    fn sprint_markdown_writes_title_to_frontmatter_and_goal_to_body() {
        let mut sprint = Sprint::new(
            SprintId::new("sprint-1").expect("valid id"),
            "Sprint One",
            epoch(),
        )
        .expect("valid sprint");
        sprint.goal = "Ship the parser".to_string();

        let text = sprint_to_markdown(&sprint).expect("serialize");

        assert!(text.contains("title = \"Sprint One\""));
        assert!(text.contains("\n\nShip the parser\n"));
    }

    #[test]
    fn item_markdown_accepts_title_without_sprint_goal_field() {
        let text = "\
+++
id = \"T-1\"
title = \"Item\"
status = \"todo\"
rank = \"n\"
created = \"1970-01-01T00:00:00Z\"
updated = \"1970-01-01T00:00:00Z\"
+++
";

        assert!(from_markdown(text, Path::new("T-1.md")).is_ok());
    }

    #[test]
    fn split_frontmatter_separates_front_and_body() {
        let text = "+++\nfoo = 1\n+++\n\nbody line\n";
        let (front, body) = split_frontmatter(text).expect("has frontmatter");
        assert_eq!(front, "foo = 1\n");
        assert_eq!(body, "body line");
    }

    #[test]
    fn split_frontmatter_handles_empty_body() {
        let text = "+++\nfoo = 1\n+++\n";
        let (front, body) = split_frontmatter(text).expect("has frontmatter");
        assert_eq!(front, "foo = 1\n");
        assert_eq!(body, "");
    }

    #[test]
    fn split_frontmatter_without_delimiters_is_none() {
        assert!(split_frontmatter("just some text\nno frontmatter\n").is_none());
    }

    #[test]
    fn split_frontmatter_without_closing_delimiter_is_none() {
        assert!(split_frontmatter("+++\nfoo = 1\nnever closed\n").is_none());
    }

    #[test]
    fn from_markdown_without_frontmatter_errors_without_panic() {
        let err = from_markdown("plain body, no frontmatter", Path::new("bad.md")).unwrap_err();
        assert!(
            matches!(err, Error::MissingFrontmatter { .. }),
            "expected MissingFrontmatter, got {err:?}"
        );
    }
}
