//! Markdown Acceptance Criteria progress.

use std::fmt;

/// Completion counts for Markdown task-list items in a PBI body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcceptanceCriteriaProgress {
    completed: usize,
    total: usize,
}

impl AcceptanceCriteriaProgress {
    /// Parse Markdown task-list items from `body`.
    ///
    /// Unordered and ordered list markers, nested items, and blockquotes are accepted. Fenced
    /// code blocks are ignored so examples in a PBI body do not affect its progress.
    #[must_use]
    pub fn from_markdown(body: &str) -> Self {
        let mut progress = Self::default();
        let mut fence: Option<(char, usize)> = None;

        for line in body.lines() {
            if let Some((marker, length)) = fence {
                if is_fence(line, marker, length) {
                    fence = None;
                }
                continue;
            }
            if let Some((marker, length)) = opening_fence(line) {
                fence = Some((marker, length));
                continue;
            }

            if let Some(completed) = checkbox_state(line) {
                progress.total += 1;
                progress.completed += usize::from(completed);
            }
        }

        progress
    }

    /// Number of checked task-list items.
    #[must_use]
    pub const fn completed(self) -> usize {
        self.completed
    }

    /// Total number of task-list items.
    #[must_use]
    pub const fn total(self) -> usize {
        self.total
    }

    /// Whether at least one task-list item remains unchecked.
    #[must_use]
    pub const fn is_incomplete(self) -> bool {
        self.completed < self.total
    }
}

impl fmt::Display for AcceptanceCriteriaProgress {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.total == 0 {
            output.write_str("-")
        } else {
            write!(output, "{}/{}", self.completed, self.total)
        }
    }
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (length >= 3).then_some((marker, length))
}

fn is_fence(line: &str, marker: char, minimum_length: usize) -> bool {
    let Some((candidate, length)) = opening_fence(line) else {
        return false;
    };
    candidate == marker && length >= minimum_length
}

fn checkbox_state(line: &str) -> Option<bool> {
    let mut content = line.trim_start();
    while let Some(rest) = content.strip_prefix('>') {
        content = rest.trim_start();
    }

    let marker_end = if let Some(rest) = content.strip_prefix(['-', '*', '+']) {
        rest.starts_with(char::is_whitespace).then_some(1)
    } else {
        let digits = content
            .char_indices()
            .take_while(|(_, character)| character.is_ascii_digit())
            .last()
            .map_or(0, |(index, character)| index + character.len_utf8());
        if digits == 0 {
            None
        } else {
            let marker = content[digits..].chars().next()?;
            (matches!(marker, '.' | ')')
                && content[digits + marker.len_utf8()..].starts_with(char::is_whitespace))
            .then_some(digits + marker.len_utf8())
        }
    }?;

    let checkbox = content[marker_end..].trim_start();
    let mut characters = checkbox.chars();
    if characters.next()? != '[' {
        return None;
    }
    let state = characters.next()?;
    if !matches!(state, ' ' | 'x' | 'X') || characters.next()? != ']' {
        return None;
    }
    let remainder = characters.next();
    remainder
        .is_none_or(char::is_whitespace)
        .then_some(state != ' ')
}

#[cfg(test)]
mod tests {
    use super::AcceptanceCriteriaProgress;

    #[test]
    fn counts_completed_and_unchecked_task_list_items() {
        let progress = AcceptanceCriteriaProgress::from_markdown(
            "# Acceptance Criteria\n\n- [x] shipped\n- [ ] documented\n- [X] tested",
        );

        assert_eq!(progress.completed(), 2);
        assert_eq!(progress.total(), 3);
        assert_eq!(progress.to_string(), "2/3");
    }

    #[test]
    fn counts_nested_ordered_and_quoted_task_items_but_ignores_fenced_code() {
        let progress = AcceptanceCriteriaProgress::from_markdown(
            "> - [x] root\n>   1. [ ] nested\n\n```markdown\n- [ ] example only\n```\n\n* [ ] actual",
        );

        assert_eq!(progress.completed(), 1);
        assert_eq!(progress.total(), 3);
    }

    #[test]
    fn empty_body_has_no_progress_and_is_not_incomplete() {
        let progress = AcceptanceCriteriaProgress::from_markdown("");

        assert_eq!(progress.completed(), 0);
        assert_eq!(progress.total(), 0);
        assert_eq!(progress.to_string(), "-");
        assert!(!progress.is_incomplete());
    }

    #[test]
    fn ignores_malformed_checkboxes_and_fence_marker_mismatches() {
        let progress = AcceptanceCriteriaProgress::from_markdown(
            "1x [ ] not a task\n- [q] invalid\n- [x]no separator\n\n```\n- [ ] code\n~~~\n- [ ] still code\n```\n\n- [ ] actual\n- [x] complete",
        );

        assert_eq!(progress.completed(), 1);
        assert_eq!(progress.total(), 2);
    }
}
