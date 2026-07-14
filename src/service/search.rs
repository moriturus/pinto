#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::{BacklogItem, ItemId, Status};
    use crate::rank::Rank;
    use crate::sprint::{Sprint, SprintId};
    use chrono::Utc;

    fn item() -> BacklogItem {
        let mut item = BacklogItem::new(
            ItemId::new("T", 1),
            "Implement parser",
            Status::new("in-progress"),
            Rank::after(None),
            Utc::now(),
        )
        .unwrap();
        item.body = "acceptance: parser handles metadata".to_string();
        item.labels = vec!["backend".to_string()];
        item
    }

    fn sprint() -> Sprint {
        let mut sprint =
            Sprint::new(SprintId::new("S-1").unwrap(), "Release train", Utc::now()).unwrap();
        sprint.goal = "Ship the parser".to_string();
        sprint
    }

    #[test]
    fn contains_search_matches_title_body_and_sprint_goal() {
        let item = item();
        let sprint = sprint();

        assert!(
            SearchFilter::new("parser", false)
                .unwrap()
                .matches(&item, Some(&sprint))
        );
        assert!(
            SearchFilter::new("metadata", false)
                .unwrap()
                .matches(&item, Some(&sprint))
        );
        assert!(
            SearchFilter::new("Ship the parser", false)
                .unwrap()
                .matches(&item, Some(&sprint))
        );
    }

    #[test]
    fn contains_search_matches_metadata_and_rejects_unrelated_items() {
        let item = item();
        let sprint = sprint();

        assert!(
            SearchFilter::new("in-progress", false)
                .unwrap()
                .matches(&item, Some(&sprint))
        );
        assert!(
            !SearchFilter::new("unrelated", false)
                .unwrap()
                .matches(&item, Some(&sprint))
        );
    }

    #[test]
    fn regex_search_uses_the_requested_pattern() {
        let item = item();

        assert!(
            SearchFilter::new(r"^T-\d+$", true)
                .unwrap()
                .matches(&item, None)
        );
        assert!(
            !SearchFilter::new(r"^S-\d+$", true)
                .unwrap()
                .matches(&item, None)
        );
    }

    #[test]
    fn invalid_regex_is_reported_as_a_user_error() {
        let error = SearchFilter::new("[", true).unwrap_err();
        assert!(error.is_user_error());
        assert!(error.to_string().contains("invalid search pattern"));
    }
}
use crate::backlog::BacklogItem;
use crate::error::{Error, Result};
use crate::sprint::Sprint;
use regex::Regex;

/// Search interpretation used by list, board, and Kanban filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Match a literal substring.
    Contains,
    /// Match a regular expression.
    Regex,
}

#[derive(Debug, Clone)]
enum Matcher {
    Contains(String),
    Regex(Regex),
}

/// A validated search filter over a PBI and its assigned sprint metadata.
#[derive(Debug, Clone)]
pub struct SearchFilter {
    pattern: String,
    mode: SearchMode,
    matcher: Matcher,
}

impl SearchFilter {
    /// Create a search filter. `regex = false` performs literal substring matching.
    pub fn new(pattern: impl Into<String>, regex: bool) -> Result<Self> {
        let pattern = pattern.into();
        let mode = if regex {
            SearchMode::Regex
        } else {
            SearchMode::Contains
        };
        let matcher = if regex {
            Matcher::Regex(
                Regex::new(&pattern)
                    .map_err(|error| Error::InvalidSearchPattern(error.to_string()))?,
            )
        } else {
            Matcher::Contains(pattern.clone())
        };
        Ok(Self {
            pattern,
            mode,
            matcher,
        })
    }

    /// Return the original user-supplied pattern.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Return the selected search mode.
    #[must_use]
    pub const fn mode(&self) -> SearchMode {
        self.mode
    }

    /// Return whether the item or its assigned sprint matches this filter.
    #[must_use]
    pub fn matches(&self, item: &BacklogItem, sprint: Option<&Sprint>) -> bool {
        let fields = searchable_fields(item, sprint);
        match &self.matcher {
            Matcher::Contains(pattern) => fields.iter().any(|field| field.contains(pattern)),
            Matcher::Regex(pattern) => fields.iter().any(|field| pattern.is_match(field)),
        }
    }
}

/// Build searchable fields from every persisted PBI field plus the sprint name and goal.
fn searchable_fields(item: &BacklogItem, sprint: Option<&Sprint>) -> Vec<String> {
    let mut fields = vec![
        item.id.to_string(),
        item.title.clone(),
        item.status.to_string(),
        item.rank.to_string(),
        item.points
            .map(|value| value.to_string())
            .unwrap_or_default(),
        item.labels.join(" "),
        item.assignee.clone().unwrap_or_default(),
        item.sprint.clone().unwrap_or_default(),
        item.parent
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        item.depends_on
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        item.start_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_default(),
        item.done_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_default(),
        item.commits.join(" "),
        item.created.to_rfc3339(),
        item.updated.to_rfc3339(),
        item.body.clone(),
    ];
    if let Some(sprint) = sprint {
        fields.push(sprint.id.to_string());
        fields.push(sprint.title.clone());
        fields.push(sprint.goal.clone());
        fields.push(sprint.state.to_string());
        fields.push(
            sprint
                .start
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
        );
        fields.push(
            sprint
                .end
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
        );
        fields.push(
            sprint
                .daily_work_hours
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
        fields.push(
            sprint
                .holiday_days
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
        fields.push(
            sprint
                .deduction_factor
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
        fields.push(sprint.created.to_rfc3339());
        fields.push(sprint.updated.to_rfc3339());
    }
    fields
}
