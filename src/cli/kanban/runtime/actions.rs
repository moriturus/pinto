//! Board mutations triggered from the Kanban event loop.

use super::load_display_board;
use crate::cli::kanban::{BoardView, InputSubmission, InputValidation};
use anyhow::Result;
use pinto::backlog::ItemId;
use pinto::i18n::{Message, current};
use pinto::service::{
    EditOutcome, ItemEdit, MoveOutcome, NewItem, SearchFilter, SearchMode, add_dependency,
    add_item_with_outcome, apply_item_edit, check_wip, edit_item, item_edit_template,
    move_item_with_outcome, remove_dependency, reorder_item,
};
use std::path::Path;
use tokio::runtime::Handle;

/// Submit the active add/relation form through the same services used by the CLI.
pub(super) fn submit_input(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    let submission = match view.submit_input() {
        Ok(submission) => submission,
        Err(InputValidation::EmptyTitle) => {
            view.set_input_error(current().text(Message::KanbanEmptyTitle));
            return Ok(());
        }
        Err(InputValidation::EmptyDependency) => {
            view.set_input_error(current().text(Message::KanbanEmptyDependency));
            return Ok(());
        }
        Err(InputValidation::InvalidItemId(error)) => {
            view.set_input_error(error.localized(current()));
            return Ok(());
        }
    };

    match submission {
        InputSubmission::AddTitle { .. } | InputSubmission::AddStep => Ok(()),
        InputSubmission::Add {
            title,
            body,
            parent,
            depends_on,
        } => {
            let new = NewItem {
                body,
                parent,
                depends_on,
                ..NewItem::default()
            };
            match handle.block_on(add_item_with_outcome(dir, &title, new)) {
                Ok(outcome) => {
                    let item = outcome.item;
                    rebuild(handle, dir, view, &item.id)?;
                    view.end_input();
                    let mut message = current().format(
                        Message::Created,
                        [
                            ("id", item.id.to_string().as_str()),
                            ("title", item.title.as_str()),
                        ],
                    );
                    if outcome.cycle_warning {
                        message.push_str("; ");
                        message.push_str(&current().text(Message::KanbanDependencyCycleWarning));
                    }
                    view.set_status_message(message);
                    Ok(())
                }
                Err(error) if error.is_user_error() => {
                    view.set_input_error(error.localized(current()));
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }
        InputSubmission::Dependency {
            source,
            dependency,
            remove,
        } => {
            let dependency = match dependency.parse::<ItemId>() {
                Ok(dependency) => dependency,
                Err(error) => {
                    view.set_input_error(error.localized(current()));
                    return Ok(());
                }
            };
            if remove {
                match handle.block_on(remove_dependency(dir, &source, &dependency)) {
                    Ok(_) => {
                        rebuild(handle, dir, view, &source)?;
                        view.end_input();
                        view.set_status_message(current().format(
                            Message::KanbanDependencyRemoved,
                            [
                                ("source", source.to_string().as_str()),
                                ("dependency", dependency.to_string().as_str()),
                            ],
                        ));
                        Ok(())
                    }
                    Err(error) if error.is_user_error() => {
                        view.set_input_error(error.localized(current()));
                        Ok(())
                    }
                    Err(error) => Err(error.into()),
                }
            } else {
                match handle.block_on(add_dependency(dir, &source, &dependency)) {
                    Ok(outcome) => {
                        rebuild(handle, dir, view, &source)?;
                        view.end_input();
                        let mut message = current().format(
                            Message::KanbanDependencyAdded,
                            [
                                ("source", source.to_string().as_str()),
                                ("dependency", dependency.to_string().as_str()),
                            ],
                        );
                        if outcome.cycle_warning {
                            message.push_str("; ");
                            message
                                .push_str(&current().text(Message::KanbanDependencyCycleWarning));
                        }
                        view.set_status_message(message);
                        Ok(())
                    }
                    Err(error) if error.is_user_error() => {
                        view.set_input_error(error.localized(current()));
                        Ok(())
                    }
                    Err(error) => Err(error.into()),
                }
            }
        }
        InputSubmission::Parent { source, parent } => {
            let parent = match parent.as_deref().map(str::parse::<ItemId>).transpose() {
                Ok(parent) => parent,
                Err(error) => {
                    view.set_input_error(error.localized(current()));
                    return Ok(());
                }
            };
            let parent_for_message = parent.clone();
            match handle.block_on(edit_item(
                dir,
                &source,
                ItemEdit {
                    parent: Some(parent),
                    ..ItemEdit::default()
                },
            )) {
                Ok(_) => {
                    rebuild(handle, dir, view, &source)?;
                    view.end_input();
                    let message = match parent_for_message {
                        Some(parent) => current().format(
                            Message::KanbanParentSet,
                            [
                                ("source", source.to_string().as_str()),
                                ("parent", parent.to_string().as_str()),
                            ],
                        ),
                        None => current().format(
                            Message::KanbanParentCleared,
                            [("source", source.to_string().as_str())],
                        ),
                    };
                    view.set_status_message(message);
                    Ok(())
                }
                Err(error) if error.is_user_error() => {
                    view.set_input_error(error.localized(current()));
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }
    }
}

/// Transition the selected PBI to the next column and reload it to follow the selection.
///
/// After the transition, check the destination column's WIP limit as the CLI `move` command does.
/// If it is exceeded, keep the warning in the footer so the user can continue working.
pub(super) fn transition(
    handle: &Handle,
    dir: &Path,
    view: &mut BoardView,
    delta: isize,
) -> Result<()> {
    let Some((id, status)) = view.move_target(delta) else {
        return Ok(());
    };
    let outcome = handle.block_on(move_item_with_outcome(dir, &id, &status))?;
    rebuild(handle, dir, view, &id)?;
    let mut warnings = Vec::new();
    if let Some(warning) = acceptance_criteria_warning(&outcome) {
        warnings.push(warning);
    }
    if let Some(v) = handle
        .block_on(check_wip(dir))?
        .into_iter()
        .find(|v| v.column == status)
    {
        warnings.push(format!(
            "{} {} has {} item(s) (limit {})",
            current().text(Message::KanbanWipExceeded),
            v.column,
            v.count,
            v.limit
        ));
    }
    if !warnings.is_empty() {
        view.set_status_message(warnings.join(" | "));
    }
    Ok(())
}

fn acceptance_criteria_warning(outcome: &MoveOutcome) -> Option<String> {
    if !outcome.entered_done_column || !outcome.acceptance_criteria.is_incomplete() {
        return None;
    }

    let progress = outcome.acceptance_criteria.to_string();
    Some(current().format(
        Message::AcceptanceCriteriaIncomplete,
        [
            ("id", outcome.item.id.to_string().as_str()),
            ("progress", progress.as_str()),
        ],
    ))
}

/// Sort selected PBIs within the same column and reload to follow selection.
pub(super) fn reorder(
    handle: &Handle,
    dir: &Path,
    view: &mut BoardView,
    delta: isize,
) -> Result<()> {
    let Some((id, target)) = view.reorder_target(delta) else {
        return Ok(());
    };
    handle.block_on(reorder_item(dir, &id, target))?;
    rebuild(handle, dir, view, &id)
}

/// Open the selected PBI with `$EDITOR`, edit it, and reload it after reflecting.
///
/// While the editor runs, suspend raw mode and the alternate screen, then restore the TUI
/// afterward. Missing editor configuration, launch failures, and invalid content are shown in the
/// footer; the loop remains active unless an internal error must be propagated.
pub(super) fn edit_selected(
    terminal: &mut ratatui::DefaultTerminal,
    handle: &Handle,
    dir: &Path,
    view: &mut BoardView,
) -> Result<()> {
    use ratatui::crossterm::execute;
    use ratatui::crossterm::terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    };

    let Some(id) = view.selected_item().map(|it| it.id.clone()) else {
        return Ok(());
    };
    // If no editor is configured, keep the TUI open and skip editing.
    if crate::cli::editor::resolve_editor().is_none() {
        view.set_status_message(current().text(Message::KanbanNoEditor).to_string());
        return Ok(());
    }

    let template = handle.block_on(item_edit_template(dir, &id))?;

    // Suspend the TUI and give the terminal to the editor. Restore it regardless of the editor result.
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    let edited = crate::cli::editor::edit_in_editor(&template, &id.to_string());
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;

    let edited = match edited {
        Ok(text) => text,
        // Report launch failures in the footer and keep the loop running.
        Err(e) => {
            view.set_status_message(format!(
                "{} {}",
                current().text(Message::KanbanEditorFailed),
                crate::cli::commands::format_anyhow_error(&e, current())
            ));
            return Ok(());
        }
    };

    match handle.block_on(apply_item_edit(dir, &id, &edited)) {
        Ok(EditOutcome::Updated(_)) => rebuild(handle, dir, view, &id),
        Ok(EditOutcome::Unchanged) => {
            view.set_status_message(format!("{} {id}", current().text(Message::KanbanNoChanges)));
            Ok(())
        }
        // Keep user-correctable errors in the footer and preserve the original item.
        Err(e) if e.is_user_error() => {
            view.set_status_message(format!(
                "{} {}",
                current().text(Message::KanbanEditFailed),
                e.localized(current())
            ));
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Reread the board and keep the selected PBI as much as possible.
pub(super) fn reload(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    let selected = view.selected_item().map(|it| it.id.clone());
    let query = view.board_query().clone();
    let display_columns = view.display_statuses().to_vec();
    let loaded = handle.block_on(load_display_board(dir, &query, &display_columns))?;
    view.set_boards(loaded.display, loaded.full);
    if let Some(id) = selected {
        view.select_id(&id);
    }
    Ok(())
}

/// Reread the board and reselect the `keep` PBI (common process after transition/sorting).
/// Retain the expanded state ([`BoardView::set_boards`]).
pub(super) fn rebuild(
    handle: &Handle,
    dir: &Path,
    view: &mut BoardView,
    keep: &ItemId,
) -> Result<()> {
    let query = view.board_query().clone();
    let display_columns = view.display_statuses().to_vec();
    let loaded = handle.block_on(load_display_board(dir, &query, &display_columns))?;
    view.set_boards(loaded.display, loaded.full);
    view.select_id(keep);
    Ok(())
}

/// Reload the board through `filter`, apply it as the active filter, and keep the selection when the
/// selected PBI survives the reload.
pub(super) fn reload_with_filter(
    handle: &Handle,
    dir: &Path,
    view: &mut BoardView,
    filter: Option<SearchFilter>,
) -> Result<()> {
    let selected = view.selected_item().map(|item| item.id.clone());
    let display_columns = view.display_statuses().to_vec();
    let mut query = view.board_query().clone();
    query.search = filter.clone();
    let loaded = handle.block_on(load_display_board(dir, &query, &display_columns))?;
    view.set_search(filter);
    view.set_boards(loaded.display, loaded.full);
    if let Some(selected) = selected {
        view.select_id(&selected);
    }
    Ok(())
}

/// Live-filter the board while a substring query is typed (incremental search).
///
/// Only substring (`Contains`) mode filters as you type; a partial regex is frequently invalid, so
/// regex mode defers to Enter. An empty query shows the whole board.
pub(super) fn apply_incremental_filter(
    handle: &Handle,
    dir: &Path,
    view: &mut BoardView,
) -> Result<()> {
    if view.search_input_mode() != Some(SearchMode::Contains) {
        return Ok(());
    }
    let query = view.search_input_buffer();
    let filter = if query.is_empty() {
        None
    } else {
        // Substring construction never fails; skip silently on the impossible error rather than panic.
        SearchFilter::new(query, false).ok()
    };
    reload_with_filter(handle, dir, view, filter)
}

/// Apply the query typed into the vim-style prompt, reloading the board through the new filter.
///
/// An empty query clears the filter. An invalid regex keeps the prompt open with an inline error so
/// the user can correct it in place. On success the prompt closes and the previously selected PBI is
/// re-selected when it survives the filter.
pub(super) fn commit_search(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    let Some(mode) = view.search_input_mode() else {
        return Ok(());
    };
    let query = view.search_input_buffer();
    let filter = if query.is_empty() {
        None
    } else {
        match SearchFilter::new(query, matches!(mode, SearchMode::Regex)) {
            Ok(filter) => Some(filter),
            Err(error) => {
                // Keep editing: surface the error under the prompt rather than dropping the query.
                view.set_search_input_error(error.localized(current()));
                return Ok(());
            }
        }
    };
    reload_with_filter(handle, dir, view, filter)?;
    view.end_search();
    Ok(())
}

/// Cancel the prompt, rolling the board back to the filter that was active when it opened.
pub(super) fn abort_search(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    let restore = view.take_search_restore();
    reload_with_filter(handle, dir, view, restore)
}

/// Clear the active search filter in one keystroke and show the whole board again.
pub(super) fn clear_filter(handle: &Handle, dir: &Path, view: &mut BoardView) -> Result<()> {
    reload_with_filter(handle, dir, view, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::kanban::{BoardView, InputMode};
    use chrono::Utc;
    use pinto::backlog::{AcceptanceCriteriaProgress, BacklogItem, Status};
    use pinto::rank::Rank;
    use pinto::service::{
        Board, BoardColumn, BoardQuery, NewItem, add_item_with_outcome, init_board,
    };
    use tempfile::TempDir;

    fn move_outcome(body: &str, entered_done_column: bool) -> MoveOutcome {
        MoveOutcome {
            item: BacklogItem::new(
                "T-1".parse().expect("item id"),
                "Task",
                Status::new("todo"),
                Rank::after(None),
                Utc::now(),
            )
            .expect("item"),
            acceptance_criteria: AcceptanceCriteriaProgress::from_markdown(body),
            entered_done_column,
        }
    }

    fn view_with_item() -> BoardView {
        let item = BacklogItem::new(
            "T-1".parse().expect("item id"),
            "Task",
            Status::new("todo"),
            Rank::after(None),
            Utc::now(),
        )
        .expect("item");
        BoardView::new(Board {
            columns: vec![BoardColumn {
                status: Status::new("todo"),
                items: vec![item],
            }],
            orphaned: Vec::new(),
        })
    }

    #[test]
    fn acceptance_warning_requires_done_column_and_incomplete_criteria() {
        assert!(acceptance_criteria_warning(&move_outcome("- [ ] pending", false)).is_none());
        assert!(acceptance_criteria_warning(&move_outcome("- [x] done", true)).is_none());

        let warning = acceptance_criteria_warning(&move_outcome("- [x] done\n- [ ] pending", true))
            .expect("incomplete criteria should warn");
        assert!(warning.contains("T-1"));
        assert!(warning.contains("1/2"));
    }

    #[tokio::test]
    async fn submit_input_keeps_forms_open_for_invalid_user_values() {
        let dir = TempDir::new().expect("temp dir");
        let action_dir = dir.path().to_path_buf();
        let handle = Handle::current();
        tokio::task::spawn_blocking(move || {
            let mut empty_title = view_with_item();
            empty_title.begin_add();
            submit_input(&handle, &action_dir, &mut empty_title)?;
            assert!(empty_title.input_error().is_some());
            empty_title.push_input_char('T');
            submit_input(&handle, &action_dir, &mut empty_title)?;
            assert_eq!(empty_title.input_mode(), Some(InputMode::AddBody));

            let mut empty_dependency = view_with_item();
            empty_dependency.begin_dependency_add();
            submit_input(&handle, &action_dir, &mut empty_dependency)?;
            assert!(empty_dependency.input_error().is_some());

            let mut invalid_dependency = view_with_item();
            invalid_dependency.begin_dependency_add();
            invalid_dependency.push_input_char('x');
            submit_input(&handle, &action_dir, &mut invalid_dependency)?;
            assert!(invalid_dependency.input_error().is_some());

            let mut invalid_parent = view_with_item();
            invalid_parent.begin_parent();
            invalid_parent.push_input_char('x');
            submit_input(&handle, &action_dir, &mut invalid_parent)?;
            assert!(invalid_parent.input_error().is_some());
            Ok::<_, anyhow::Error>(())
        })
        .await
        .expect("action task")
        .expect("invalid values are handled in the form");
    }

    #[tokio::test]
    async fn search_actions_cover_noop_invalid_commit_live_filter_cancel_and_clear() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        for title in ["Alpha", "Beta"] {
            add_item_with_outcome(dir.path(), title, NewItem::default())
                .await
                .expect("add item");
        }

        let display_columns = vec!["todo".to_string()];
        let loaded = load_display_board(dir.path(), &BoardQuery::default(), &display_columns)
            .await
            .expect("load board");
        let mut view = BoardView::new_with_scope_and_query(
            loaded.display,
            loaded.full,
            display_columns,
            BoardQuery::default(),
        );
        let handle = Handle::current();
        let action_dir = dir.path().to_path_buf();

        let view = tokio::task::spawn_blocking(move || {
            // No open prompt is a harmless no-op for both commit and incremental search.
            commit_search(&handle, &action_dir, &mut view)?;
            apply_incremental_filter(&handle, &action_dir, &mut view)?;

            // Invalid regexes keep the prompt open and expose an inline error.
            view.begin_search(SearchMode::Regex);
            view.push_search_char('[');
            commit_search(&handle, &action_dir, &mut view)?;
            assert!(view.is_searching());
            assert!(view.search_input_error().is_some());
            view.end_search();

            // A valid regex commits and closes the prompt while preserving the filter.
            view.begin_search(SearchMode::Regex);
            for character in "^Alpha$".chars() {
                view.push_search_char(character);
            }
            commit_search(&handle, &action_dir, &mut view)?;
            assert!(!view.is_searching());
            assert_eq!(
                view.search_filter().map(SearchFilter::pattern),
                Some("^Alpha$")
            );
            assert_eq!(view.columns()[0].items.len(), 1);

            // Incremental substring search applies as the user types, but regex mode is deferred.
            view.begin_search(SearchMode::Regex);
            apply_incremental_filter(&handle, &action_dir, &mut view)?;
            view.end_search();
            view.begin_search(SearchMode::Contains);
            for character in "Beta".chars() {
                view.push_search_char(character);
            }
            apply_incremental_filter(&handle, &action_dir, &mut view)?;
            assert_eq!(
                view.search_filter().map(SearchFilter::pattern),
                Some("Beta")
            );
            assert_eq!(view.columns()[0].items[0].title, "Beta");

            // Cancel restores the filter captured before the prompt opened.
            abort_search(&handle, &action_dir, &mut view)?;
            assert_eq!(
                view.search_filter().map(SearchFilter::pattern),
                Some("^Alpha$")
            );
            assert_eq!(view.columns()[0].items[0].title, "Alpha");

            // An empty contains query and the explicit clear action show every item.
            view.begin_search(SearchMode::Contains);
            apply_incremental_filter(&handle, &action_dir, &mut view)?;
            view.end_search();
            assert!(view.search_filter().is_none());
            clear_filter(&handle, &action_dir, &mut view)?;
            assert_eq!(view.columns()[0].items.len(), 2);

            // The general reload paths retain a valid selection.
            let selected = view.selected_item().expect("selected item").id.clone();
            reload(&handle, &action_dir, &mut view)?;
            rebuild(&handle, &action_dir, &mut view, &selected)?;
            assert_eq!(view.selected_item().map(|item| &item.id), Some(&selected));
            Ok::<_, anyhow::Error>(view)
        })
        .await
        .expect("action task")
        .expect("actions succeed");

        assert!(!view.is_searching());
        assert_eq!(view.columns()[0].items.len(), 2);
    }
}
